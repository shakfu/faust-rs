//! Host-driven adaptive notch filter using `rad(...)` for the gradient
//! flow. This is a recognisable adaptive-filtering problem (LMS-style
//! convergence on output-power minimisation) that fits inside phase-1
//! RAD: the filter itself is a 3-tap FIR with zeros on the unit circle,
//! and the delay line is buffered by the host so the differentiated
//! body remains feed-forward.
//!
//! Setup
//! -----
//!
//! - The DSP is `tests/corpus/rad_adaptive_notch_omega.dsp`:
//!
//!   ```faust
//!   omega = hslider("omega", 1.0, 0.01, 3.0, 0.0001);
//!   notch(xn, xn1, xn2) = xn - 2.0 * cos(omega) * xn1 + xn2;
//!   process = rad(notch, omega);
//!   ```
//!
//!   It places a pair of zeros at `e^(±j·omega)` so a sinusoid at
//!   `omega` is fully suppressed. The output bundle exposes
//!   `[y, ∂y/∂omega = 2·sin(omega)·x_{n-1}]`.
//!
//! - The host builds the input as a sinusoid at `OMEGA_TARGET` plus
//!   small white noise, buffers `x[n], x[n-1], x[n-2]` over a moving
//!   window, and runs the rad-compiled DSP block-by-block.
//!
//! - Loss `J(ω) = E[y²]`. Gradient `∂J/∂ω = E[2·y·∂y/∂ω]`.
//!
//! - Update: plain SGD `ω ← ω − η · ∂J/∂ω`.
//!
//! Adaptation drives `ω` toward `OMEGA_TARGET` because the only stable
//! minimum of the output power is the frequency at which the notch
//! exactly cancels the strong input tone.
//!
//! Run with:
//!
//! ```bash
//! cargo run --release -p compiler --example rad_adaptive_notch
//! ```

use std::io::Cursor;
use std::path::PathBuf;

use codegen::backends::interp::bytecode::FbcUiInstruction;
use codegen::backends::interp::opcode::FbcOpcode;
use codegen::backends::interp::{FbcDspInstance, InterpOptions, read_fbc};
use compiler::{Compiler, SignalFirLane};

const SAMPLE_RATE: i32 = 48_000;
const BLOCK_LEN: usize = 512;
const ITERATIONS: usize = 600;
const LEARNING_RATE: f32 = 0.05;
const OMEGA_INIT: f32 = 0.4;
const OMEGA_TARGET: f32 = 1.3;
const NOISE_STD: f32 = 0.02;
const TONE_AMPLITUDE: f32 = 1.0;

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
        .find(|i| {
            matches!(
                i.opcode,
                FbcOpcode::AddHorizontalSlider
                    | FbcOpcode::AddVerticalSlider
                    | FbcOpcode::AddNumEntry
            ) && i.label == label
        })
        .map(|i| i.offset)
        .unwrap_or_else(|| panic!("slider `{label}` not found in UI block"))
}

/// Tiny LCG so the demo is deterministic without dragging `rand` into
/// the dependency list.
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
        (bits as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
    /// Approximate Gaussian via 12-sample sum trick (Bates), scaled to
    /// unit variance.
    fn next_gaussian(&mut self) -> f32 {
        let mut s = 0.0_f32;
        for _ in 0..12 {
            s += self.next_uniform();
        }
        // Sum of 12 uniforms in [-1, 1] has variance 12 · (1/3) = 4 → std 2.
        s * 0.5
    }
}

fn main() {
    let path = corpus_path("rad_adaptive_notch_omega");
    let compiler = Compiler::new();
    let fbc = compiler
        .compile_file_default_to_interp_with_lane(
            &path,
            &InterpOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .expect("rad_adaptive_notch_omega must compile");
    let mut reader = Cursor::new(fbc);
    let mut factory = read_fbc::<f32>(&mut reader).expect("interp bytecode must parse");
    let mut instance = FbcDspInstance::new(&mut factory);
    instance.init(SAMPLE_RATE);

    let ui = instance.ui_instructions().to_vec();
    let omega_offset = slider_offset(&ui, "omega");

    let mut omega = OMEGA_INIT;
    instance.set_real_zone(omega_offset, omega);

    let mut rng = Lcg::new(0xADA0_70F1);

    // Persistent moving-window state for x[n-1] and x[n-2] across blocks.
    let mut x_prev1 = 0.0_f32;
    let mut x_prev2 = 0.0_f32;
    // Phase accumulator for the target tone.
    let mut tone_phase = 0.0_f32;

    println!(
        "adaptive notch:  init ω = {OMEGA_INIT:.4}  target ω = {OMEGA_TARGET:.4}  block = {BLOCK_LEN}  η = {LEARNING_RATE}"
    );
    println!(
        "input = {:.2}·sin(ω·n) + N(0, σ²) with σ = {:.4}\n",
        TONE_AMPLITUDE, NOISE_STD
    );

    for iter in 0..ITERATIONS {
        // Synthesise the input block: tone at OMEGA_TARGET + small
        // Gaussian noise.
        let mut x_n = vec![0.0_f32; BLOCK_LEN];
        let mut x_n1 = vec![0.0_f32; BLOCK_LEN];
        let mut x_n2 = vec![0.0_f32; BLOCK_LEN];
        for k in 0..BLOCK_LEN {
            let sample = TONE_AMPLITUDE * tone_phase.sin() + NOISE_STD * rng.next_gaussian();
            tone_phase += OMEGA_TARGET;
            // Wrap to keep the phase well-conditioned over long runs.
            if tone_phase > std::f32::consts::TAU {
                tone_phase -= std::f32::consts::TAU;
            }
            // Buffer the moving window.
            x_n2[k] = x_prev2;
            x_n1[k] = x_prev1;
            x_n[k] = sample;
            x_prev2 = x_prev1;
            x_prev1 = sample;
        }

        let inputs: [&[f32]; 3] = [&x_n, &x_n1, &x_n2];
        let mut y = vec![0.0_f32; BLOCK_LEN];
        let mut dy_domega = vec![0.0_f32; BLOCK_LEN];
        let mut outs: [&mut [f32]; 2] = [&mut y, &mut dy_domega];
        instance
            .try_compute(BLOCK_LEN as i32, &inputs, &mut outs)
            .expect("interp compute must succeed");

        // Loss = E[y²], gradient = E[2·y·∂y/∂ω].
        let mut loss = 0.0_f32;
        let mut grad = 0.0_f32;
        for k in 0..BLOCK_LEN {
            loss += y[k] * y[k];
            grad += 2.0 * y[k] * dy_domega[k];
        }
        let n = BLOCK_LEN as f32;
        loss /= n;
        grad /= n;

        omega -= LEARNING_RATE * grad;
        // Project back into the slider's declared range.
        omega = omega.clamp(0.01, 3.0);
        instance.set_real_zone(omega_offset, omega);

        if iter < 5 || iter % 50 == 0 || iter + 1 == ITERATIONS {
            let omega_err = (omega - OMEGA_TARGET).abs();
            println!(
                "iter {iter:>4}  loss = {loss:.6e}  ω = {omega:.4}  |ω − ω*| = {omega_err:.4e}  ∂J/∂ω = {grad:+.4e}"
            );
        }
    }

    let omega_err = (omega - OMEGA_TARGET).abs();
    println!("\nfinal  ω = {omega:.6}  target {OMEGA_TARGET:.6}  |Δω| = {omega_err:.4e}");
    assert!(
        omega_err < 1.0e-2,
        "expected adaptive notch to converge within 0.01 of target ω; got |Δω| = {omega_err:.4e}"
    );
    println!("rad_adaptive_notch: ω converged to within 0.01 rad/sample of target.");
}
