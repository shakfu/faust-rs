//! Host-side gradient-descent loop driving a `rad(...)` Faust program.
//!
//! This example fits the `rad_gain_bias_train` corpus fixture against a
//! synthetic target signal `target = TRUE_GAIN * x + TRUE_BIAS`. The DSP
//! exposes two seeds (`gain`, `bias`) as `hslider` controls; the host:
//!
//! 1. compiles the corpus fixture through the public `Compiler` facade;
//! 2. discovers the slider heap offsets from the interp UI block;
//! 3. for each batch:
//!    - generates an input block `x[n]` (white noise),
//!    - generates the target block `target[n]` from the true parameters,
//!    - runs the rad-compiled DSP over the block to read
//!      `[primal[n], ∂out/∂gain[n], ∂out/∂bias[n]]`,
//!    - accumulates the loss `Σ (primal - target)²` and the gradient via
//!      the chain rule `∇loss = Σ 2·(primal - target)·∂out/∂param`,
//!    - updates `gain` and `bias` by plain SGD: `θ ← θ − η ∇loss`,
//!    - writes the updated values back into the slider heap zones.
//!
//! The loop demonstrates the phase-1 `rad(...)` contract end-to-end, on a
//! real (if minimal) trainable feed-forward map. The same pattern scales
//! to the polynomial waveshaper, soft-clip, and FIR-taps fixtures with
//! more inputs.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example rad_gradient_descent -p compiler
//! ```

use std::io::Cursor;
use std::path::PathBuf;

use codegen::backends::interp::bytecode::FbcUiInstruction;
use codegen::backends::interp::opcode::FbcOpcode;
use codegen::backends::interp::{FbcDspInstance, InterpOptions, read_fbc};
use compiler::{Compiler, SignalFirLane};

const SAMPLE_RATE: i32 = 48_000;
const BLOCK_LEN: usize = 512;
const ITERATIONS: usize = 400;
const LEARNING_RATE: f32 = 0.05;
const TRUE_GAIN: f32 = 1.7;
const TRUE_BIAS: f32 = -0.3;

fn corpus_path(stem: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("corpus")
        .join(format!("{stem}.dsp"))
}

fn slider_offset(ui: &[FbcUiInstruction<f32>], label: &str) -> i32 {
    ui.iter()
        .find(|instr| {
            matches!(
                instr.opcode,
                FbcOpcode::AddHorizontalSlider
                    | FbcOpcode::AddVerticalSlider
                    | FbcOpcode::AddNumEntry
            ) && instr.label == label
        })
        .map(|instr| instr.offset)
        .unwrap_or_else(|| panic!("slider `{label}` not present in UI block"))
}

/// Tiny linear-congruential generator for deterministic inputs without
/// pulling a `rand` dependency into the compiler crate's example surface.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1))
    }
    fn next_uniform(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let bits = (self.0 >> 32) as u32;
        // map to [-1.0, 1.0]
        (bits as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn main() {
    let path = corpus_path("rad_gain_bias_train");
    let compiler = Compiler::new();
    let fbc = compiler
        .compile_file_default_to_interp_with_lane(
            &path,
            &InterpOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .expect("rad_gain_bias_train must compile through the interp fast lane");

    let mut reader = Cursor::new(fbc);
    let mut factory =
        read_fbc::<f32>(&mut reader).expect("interp bytecode must parse cleanly");
    let mut instance = FbcDspInstance::new(&mut factory);
    instance.init(SAMPLE_RATE);

    // Discover slider offsets after init (init resets the heap to slider
    // initial values, so any host-side mutation must come after).
    let ui = instance.ui_instructions().to_vec();
    let gain_offset = slider_offset(&ui, "gain");
    let bias_offset = slider_offset(&ui, "bias");

    // Trainable parameters, started off the true values on purpose.
    let mut gain = 0.5_f32;
    let mut bias = 0.0_f32;
    instance.set_real_zone(gain_offset, gain);
    instance.set_real_zone(bias_offset, bias);

    let mut rng = Lcg::new(0xC0DE_FACE);

    println!(
        "starting fit: gain = {:.4}, bias = {:.4} (true gain = {:.4}, true bias = {:.4})",
        gain, bias, TRUE_GAIN, TRUE_BIAS
    );

    for iter in 0..ITERATIONS {
        // Generate a fresh batch of inputs and the corresponding targets.
        let mut x = vec![0.0_f32; BLOCK_LEN];
        let mut target = vec![0.0_f32; BLOCK_LEN];
        for k in 0..BLOCK_LEN {
            let xn = rng.next_uniform();
            x[k] = xn;
            target[k] = TRUE_GAIN * xn + TRUE_BIAS;
        }

        // The fixture has 1 audio input and 3 outputs:
        //   [out, ∂out/∂gain, ∂out/∂bias]
        let inputs: [&[f32]; 1] = [&x];
        let mut out_primal = vec![0.0_f32; BLOCK_LEN];
        let mut out_dgain = vec![0.0_f32; BLOCK_LEN];
        let mut out_dbias = vec![0.0_f32; BLOCK_LEN];
        let mut outputs: [&mut [f32]; 3] = [&mut out_primal, &mut out_dgain, &mut out_dbias];
        instance
            .try_compute(BLOCK_LEN as i32, &inputs, &mut outputs)
            .expect("interp compute must succeed");

        // Aggregate loss and gradient over the block.
        //   loss = Σ (primal - target)²
        //   ∇loss = Σ 2 · (primal - target) · ∂out/∂param
        let mut loss = 0.0_f32;
        let mut grad_gain = 0.0_f32;
        let mut grad_bias = 0.0_f32;
        for k in 0..BLOCK_LEN {
            let err = out_primal[k] - target[k];
            loss += err * err;
            grad_gain += 2.0 * err * out_dgain[k];
            grad_bias += 2.0 * err * out_dbias[k];
        }
        loss /= BLOCK_LEN as f32;
        grad_gain /= BLOCK_LEN as f32;
        grad_bias /= BLOCK_LEN as f32;

        // Plain SGD update.
        gain -= LEARNING_RATE * grad_gain;
        bias -= LEARNING_RATE * grad_bias;
        instance.set_real_zone(gain_offset, gain);
        instance.set_real_zone(bias_offset, bias);

        if iter % 20 == 0 || iter + 1 == ITERATIONS {
            println!(
                "iter {iter:>4}  loss = {loss:.6e}  gain = {gain:.4}  bias = {bias:.4}  ∇gain = {grad_gain:+.4e}  ∇bias = {grad_bias:+.4e}"
            );
        }
    }

    let final_gain_err = (gain - TRUE_GAIN).abs();
    let final_bias_err = (bias - TRUE_BIAS).abs();
    println!(
        "\nfinal gain err = {final_gain_err:.4e}, final bias err = {final_bias_err:.4e}"
    );
    assert!(
        final_gain_err < 5.0e-2,
        "expected the host loop to recover gain to within 0.05; got {final_gain_err:.4e}"
    );
    assert!(
        final_bias_err < 5.0e-2,
        "expected the host loop to recover bias to within 0.05; got {final_bias_err:.4e}"
    );
    println!("rad_gradient_descent: gain & bias recovered to within 0.05.");
}
