//! Vector mode (`-vec`) bit-exactness oracle — roadmap P6, vector doc V6.
//!
//! Vector mode only changes *storage/loop structure*, not the per-sample
//! arithmetic, so its output must be **bit-identical** to scalar. These tests
//! compile the same DSP scalar and with `ComputeMode::Vector`, run both through
//! the interpreter over a block larger than the vector size (so state crosses a
//! chunk boundary), and assert the outputs are exactly equal.

use std::io::Cursor;

use codegen::backends::interp::{FbcDspInstance, InterpOptions, read_fbc};
use compiler::{Compiler, ComputeMode, SignalFirLane};

/// Compiles `source` to interpreter bytecode with the given compute mode and
/// runs one `frames`-sample block with the provided single-channel input.
fn run(source: &str, mode: ComputeMode, input: &[f32]) -> Vec<Vec<f32>> {
    let path = std::env::temp_dir().join(format!(
        "faust-rs-vecmode-{}-{:?}.dsp",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, source).expect("write temp dsp");
    let fbc = Compiler::new()
        .with_compute_mode(mode)
        .compile_file_default_to_interp_with_lane(
            &path,
            &InterpOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .unwrap_or_else(|e| panic!("compile failed ({mode:?}): {e}"));
    let _ = std::fs::remove_file(&path);

    let mut reader = Cursor::new(fbc);
    let mut factory = read_fbc::<f32>(&mut reader).expect("fbc parse");
    let mut instance = FbcDspInstance::new(&mut factory);
    instance.init(48_000);
    let num_outputs = usize::try_from(instance.get_num_outputs()).expect("outputs");
    let frames = input.len();
    let mut outputs = vec![vec![0.0_f32; frames]; num_outputs];
    let mut slices: Vec<&mut [f32]> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
    instance
        .try_compute(frames as i32, &[input], &mut slices)
        .expect("compute");
    outputs
}

/// A `frames`-sample deterministic ramp with a non-integer step.
fn ramp(frames: usize) -> Vec<f32> {
    (0..frames).map(|k| 0.13 * k as f32 - 1.0).collect()
}

fn assert_scalar_vector_bit_exact(name: &str, source: &str, vec_size: u32) {
    // 64-sample block: with vec_size 32 → two full chunks + a boundary crossing;
    // with a non-dividing vec_size → a short remainder / tail chunk. Both loop
    // variants (`-lv 0` fastest, `-lv 1` simple) must match scalar bit-for-bit.
    let frames = 64;
    let input = ramp(frames);
    let scalar = run(source, ComputeMode::Scalar, &input);
    for loop_variant in [0_u8, 1] {
        let vector = run(
            source,
            ComputeMode::Vector {
                vec_size,
                loop_variant,
            },
            &input,
        );
        assert_eq!(
            scalar.len(),
            vector.len(),
            "{name}: output channel count differs (-lv {loop_variant})"
        );
        for (ch, (s, v)) in scalar.iter().zip(vector.iter()).enumerate() {
            assert_eq!(
                s, v,
                "{name}: channel {ch} differs, scalar vs -vec (-vs {vec_size} -lv {loop_variant})"
            );
        }
    }
}

#[test]
fn stateless_gain_is_bit_exact() {
    assert_scalar_vector_bit_exact("gain", "process = _ * 0.5;", 32);
}

#[test]
fn recursion_and_delay_cross_chunk_boundary_bit_exact() {
    // Integrator (loop-carried state) plus a 5-sample delay: both the recursive
    // carrier and the delay line must survive the chunk boundary unchanged.
    assert_scalar_vector_bit_exact(
        "rec_delay",
        "process = _ <: (+ ~ _), (@(5) * 0.5) : + ;",
        32,
    );
}

#[test]
fn vec_size_not_dividing_the_block_bit_exact() {
    // vec_size = 24 does not divide 64 → a short tail chunk (16) exercises the
    // `min(vindex + vs, count)` clamp.
    assert_scalar_vector_bit_exact("tail_chunk", "process = (_ : + ~ _) * 0.25;", 24);
}

#[test]
fn recursive_split_pure_tail_is_bit_exact() {
    // `(_ : + ~ _) * 0.5` is the S-D split case: the integrator is the serial
    // core, the `* 0.5` output scaling is hoisted into a second vectorizable inner
    // loop fed by a chunk buffer. The split must stay bit-exact vs scalar.
    assert_scalar_vector_bit_exact("split_tail", "process = (_ : + ~ _) * 0.5;", 32);
    // Tail chunk (vec_size ∤ block) through the split path too.
    assert_scalar_vector_bit_exact("split_tail_odd", "process = (_ : + ~ _) * 0.5;", 24);
}

#[test]
fn two_pole_filter_is_bit_exact() {
    // A second-order recurrence — deeper loop-carried state across the boundary.
    assert_scalar_vector_bit_exact(
        "biquad_like",
        "process = _ : + ~ (_ <: 0.5 * _' , -0.2 * _'' :> _);",
        32,
    );
}
