//! Side-by-side compile-and-run comparison of `rad(expr, seeds)` versus
//! `fad(expr, seeds)` on representative trainable shapes. Designed to
//! surface, in concrete numbers, the adjoint-sum growth risk identified
//! in the RAD plan §17 (risk #2).
//!
//! For each shape this binary:
//!
//! 1. compiles both the `fad(...)` and `rad(...)` programs through the
//!    public `Compiler` facade (interp fast lane);
//! 2. records the compilation wall-clock and the produced bytecode size;
//! 3. instantiates each on the interp backend and measures the per-frame
//!    compute time over a fixed-length buffer, averaged over many cycles.
//!
//! The comparison is structural rather than rigorous: it is meant to
//! make growth and timing trends visible to a developer reading the
//! output, not to replace a Criterion benchmark suite.
//!
//! Run with:
//!
//! ```bash
//! cargo run --release --example rad_vs_fad_perf -p compiler
//! ```

use std::io::Cursor;
use std::time::{Duration, Instant};

use codegen::backends::interp::{FbcDspInstance, InterpOptions, read_fbc};
use compiler::{Compiler, SignalFirLane};

const SAMPLE_RATE: i32 = 48_000;
const BUFFER_LEN: usize = 4_096;
const COMPUTE_REPS: usize = 200;

/// One side of a benchmark pair: the source DSP and the metrics gathered
/// after compiling and running it. `_label` and `_source` are retained
/// for diagnostic-friendly debugging when a future run prints the failing
/// case; suppressed `dead_code` since they are not consumed today.
#[allow(dead_code)]
struct Run {
    label: &'static str,
    source: String,
    compile_time: Duration,
    bytecode_bytes: usize,
    inputs: i32,
    outputs: i32,
    avg_compute_per_frame_ns: f64,
}

fn run_once(label: &'static str, source: String, num_inputs: usize) -> Run {
    let compiler = Compiler::new();
    let path = std::env::temp_dir().join(format!(
        "faust-rs-rad-vs-fad-perf-{}-{}.dsp",
        label,
        std::process::id()
    ));
    std::fs::write(&path, &source).expect("temporary DSP file write must succeed");

    let start = Instant::now();
    let fbc = compiler
        .compile_file_default_to_interp_with_lane(
            &path,
            &InterpOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .unwrap_or_else(|e| panic!("`{label}` compilation failed: {e}"));
    let compile_time = start.elapsed();
    let bytecode_bytes = fbc.len();

    let mut reader = Cursor::new(fbc);
    let mut factory = read_fbc::<f32>(&mut reader).expect("interp bytecode must parse cleanly");
    let mut instance = FbcDspInstance::new(&mut factory);
    instance.init(SAMPLE_RATE);
    let inputs = instance.get_num_inputs();
    let outputs = instance.get_num_outputs();

    // Allocate IO buffers once, reuse across reps.
    let zero_input = vec![0.5_f32; BUFFER_LEN];
    let inputs_storage: Vec<&[f32]> = (0..num_inputs).map(|_| zero_input.as_slice()).collect();
    let mut outputs_owned: Vec<Vec<f32>> = (0..outputs as usize)
        .map(|_| vec![0.0_f32; BUFFER_LEN])
        .collect();

    // Warmup, then time COMPUTE_REPS calls. Sum durations to keep noise low.
    {
        let mut output_slices: Vec<&mut [f32]> =
            outputs_owned.iter_mut().map(Vec::as_mut_slice).collect();
        instance
            .try_compute(BUFFER_LEN as i32, &inputs_storage, &mut output_slices)
            .expect("warmup compute must succeed");
    }
    let mut total = Duration::ZERO;
    for _ in 0..COMPUTE_REPS {
        let mut output_slices: Vec<&mut [f32]> =
            outputs_owned.iter_mut().map(Vec::as_mut_slice).collect();
        let start = Instant::now();
        instance
            .try_compute(BUFFER_LEN as i32, &inputs_storage, &mut output_slices)
            .expect("compute must succeed");
        total += start.elapsed();
    }
    let total_frames = COMPUTE_REPS * BUFFER_LEN;
    let avg_compute_per_frame_ns = total.as_nanos() as f64 / total_frames as f64;

    let _ = std::fs::remove_file(&path);

    let _ = num_inputs; // currently inputs is read from the instance instead
    Run {
        label,
        source,
        compile_time,
        bytecode_bytes,
        inputs,
        outputs,
        avg_compute_per_frame_ns,
    }
}

fn print_pair(name: &str, fad: &Run, rad: &Run) {
    println!(
        "\n## {name}\n  inputs = {fad_in}/{rad_in}, outputs FAD={fad_out}, RAD={rad_out}",
        fad_in = fad.inputs,
        rad_in = rad.inputs,
        fad_out = fad.outputs,
        rad_out = rad.outputs,
    );
    println!(
        "  compile time : FAD = {:>7.2} ms   RAD = {:>7.2} ms   ratio = {:>5.2}",
        fad.compile_time.as_secs_f64() * 1000.0,
        rad.compile_time.as_secs_f64() * 1000.0,
        rad.compile_time.as_secs_f64() / fad.compile_time.as_secs_f64().max(1e-9),
    );
    println!(
        "  bytecode size: FAD = {:>7} B   RAD = {:>7} B   ratio = {:>5.2}",
        fad.bytecode_bytes,
        rad.bytecode_bytes,
        rad.bytecode_bytes as f64 / fad.bytecode_bytes.max(1) as f64,
    );
    println!(
        "  compute / frame: FAD = {:>7.1} ns   RAD = {:>7.1} ns   ratio = {:>5.2}",
        fad.avg_compute_per_frame_ns,
        rad.avg_compute_per_frame_ns,
        rad.avg_compute_per_frame_ns / fad.avg_compute_per_frame_ns.max(1e-9),
    );
}

fn shape_gain_bias() -> (Run, Run) {
    let common = r#"
gain = hslider("gain", 1.0, -4.0, 4.0, 0.001);
bias = hslider("bias", 0.0, -4.0, 4.0, 0.001);
"#;
    let fad = run_once(
        "fad-gain-bias",
        format!("{common}\nprocess = fad(gain * _ + bias, (gain, bias));\n"),
        1,
    );
    let rad = run_once(
        "rad-gain-bias",
        format!("{common}\nprocess = rad(gain * _ + bias, (gain, bias));\n"),
        1,
    );
    (fad, rad)
}

fn shape_polynomial_4_seeds() -> (Run, Run) {
    let common = r#"
c0 = hslider("c0", 0.0, -2.0, 2.0, 0.001);
c1 = hslider("c1", 1.0, -2.0, 2.0, 0.001);
c2 = hslider("c2", 0.0, -2.0, 2.0, 0.001);
c3 = hslider("c3", 0.0, -2.0, 2.0, 0.001);
shape(x, xx, xxx) = c0 + c1 * x + c2 * xx + c3 * xxx;
"#;
    let fad = run_once(
        "fad-polynomial-4-seeds",
        format!("{common}\nprocess = fad(shape, (c0, c1, c2, c3));\n"),
        3,
    );
    let rad = run_once(
        "rad-polynomial-4-seeds",
        format!("{common}\nprocess = rad(shape, (c0, c1, c2, c3));\n"),
        3,
    );
    (fad, rad)
}

fn shape_static_softclip() -> (Run, Run) {
    let common = r#"
import("stdfaust.lib");
drive = hslider("drive", 1.5, 0.1, 8.0, 0.001);
bias = hslider("bias", 0.0, -2.0, 2.0, 0.001);
clip(x) = ma.tanh(drive * x + bias) / drive;
"#;
    let fad = run_once(
        "fad-static-softclip",
        format!("{common}\nprocess = fad(clip(_), (drive, bias));\n"),
        1,
    );
    let rad = run_once(
        "rad-static-softclip",
        format!("{common}\nprocess = rad(clip(_), (drive, bias));\n"),
        1,
    );
    (fad, rad)
}

fn shape_fir_4_taps() -> (Run, Run) {
    let common = r#"
c0 = hslider("c0", 0.25, -2.0, 2.0, 0.001);
c1 = hslider("c1", 0.25, -2.0, 2.0, 0.001);
c2 = hslider("c2", 0.25, -2.0, 2.0, 0.001);
c3 = hslider("c3", 0.25, -2.0, 2.0, 0.001);
kernel(x0, x1, x2, x3) = c0 * x0 + c1 * x1 + c2 * x2 + c3 * x3;
"#;
    let fad = run_once(
        "fad-fir-4-taps",
        format!("{common}\nprocess = fad(kernel, (c0, c1, c2, c3));\n"),
        4,
    );
    let rad = run_once(
        "rad-fir-4-taps",
        format!("{common}\nprocess = rad(kernel, (c0, c1, c2, c3));\n"),
        4,
    );
    (fad, rad)
}

/// Stress case: a chain `((((c0+c1)*c2+c3)*c4+c5)*x` with 6 seeds. RAD
/// must accumulate adjoint contributions through every multiplicative
/// fold; FAD reuses tangent values via DAG sharing.
fn shape_deep_chain_6_seeds() -> (Run, Run) {
    let common = r#"
c0 = hslider("c0", 0.0, -2.0, 2.0, 0.001);
c1 = hslider("c1", 0.0, -2.0, 2.0, 0.001);
c2 = hslider("c2", 0.5, -2.0, 2.0, 0.001);
c3 = hslider("c3", 0.0, -2.0, 2.0, 0.001);
c4 = hslider("c4", 0.5, -2.0, 2.0, 0.001);
c5 = hslider("c5", 0.0, -2.0, 2.0, 0.001);
chain(x) = ((((c0 + c1) * c2 + c3) * c4 + c5) * x);
"#;
    let fad = run_once(
        "fad-deep-chain-6",
        format!("{common}\nprocess = fad(chain(_), (c0, c1, c2, c3, c4, c5));\n"),
        1,
    );
    let rad = run_once(
        "rad-deep-chain-6",
        format!("{common}\nprocess = rad(chain(_), (c0, c1, c2, c3, c4, c5));\n"),
        1,
    );
    (fad, rad)
}

fn shape_lti_one_pole_recursive() -> (Run, Run) {
    let common = r#"
p = 0.5;
core = _ : + ~ *(p);
"#;
    let fad = run_once(
        "fad-lti-one-pole-recursive",
        format!("{common}\nprocess = fad(core, p);\n"),
        1,
    );
    let rad = run_once(
        "rad-lti-one-pole-recursive",
        format!("{common}\nprocess = rad(core, p);\n"),
        1,
    );
    (fad, rad)
}

fn shape_lti_state_space_recursive() -> (Run, Run) {
    let common = r#"
import("stdfaust.lib");
p = 0.5;
q = 0.25;
core = (ro.interleave(2, 2) : (+, +)) ~ ((*(p), *(q)) : ro.cross(2));
"#;
    let fad = run_once(
        "fad-lti-state-space-recursive",
        format!("{common}\nprocess = fad((_, _) : core, (p, q));\n"),
        2,
    );
    let rad = run_once(
        "rad-lti-state-space-recursive",
        format!("{common}\nprocess = rad((_, _) : core, (p, q));\n"),
        2,
    );
    (fad, rad)
}

fn main() {
    println!("# RAD vs FAD performance comparison");
    println!(
        "(buffer = {BUFFER_LEN} samples, compute reps = {COMPUTE_REPS}, sample rate = {SAMPLE_RATE} Hz)\n"
    );
    println!(
        "Read out: ratios > 1 show RAD is heavier than FAD on this metric, < 1 show RAD is lighter.\n"
    );

    let (fad, rad) = shape_gain_bias();
    print_pair("gain * x + bias  (1 input, 2 seeds)", &fad, &rad);

    let (fad, rad) = shape_polynomial_4_seeds();
    print_pair(
        "polynomial waveshaper  (3 inputs, 4 seeds, 1 primal)",
        &fad,
        &rad,
    );

    let (fad, rad) = shape_static_softclip();
    print_pair("static softclip with tanh  (1 input, 2 seeds)", &fad, &rad);

    let (fad, rad) = shape_fir_4_taps();
    print_pair(
        "FIR taps with host-fed delays  (4 inputs, 4 seeds)",
        &fad,
        &rad,
    );

    let (fad, rad) = shape_deep_chain_6_seeds();
    print_pair(
        "deep multiplicative chain  (1 input, 6 seeds — adjoint-sum stress)",
        &fad,
        &rad,
    );

    let (fad, rad) = shape_lti_one_pole_recursive();
    print_pair(
        "strict-LTI one-pole recursion  (1 input, 1 literal seed)",
        &fad,
        &rad,
    );

    let (fad, rad) = shape_lti_state_space_recursive();
    print_pair(
        "strict-LTI coupled state-space recursion  (2 inputs, 2 literal seeds)",
        &fad,
        &rad,
    );

    println!("\n## Notes");
    println!("- FAD output bundle is [primal, t_seed0, …, t_seed{{N-1}}] interleaved per primal.");
    println!("- RAD output bundle is [primals…, gradient(seed_0), …, gradient(seed_{{N-1}})].");
    println!(
        "- The deep multiplicative chain is the canonical adjoint-sum-growth\n  stress case (plan §17 risk #2). RAD has to accumulate one chain rule\n  contribution per seed at every fold; without simplification, the\n  emitted signal IR can grow super-linearly in the seed count."
    );
    println!(
        "- The recursive cases use literal strict-LTI coefficients, which are\n  accepted by the current E1 transpose. UI/input-controlled recursive\n  coefficients are LTV and remain outside this phase."
    );
}
