//! Vector mode (`-vec`) bit-exactness oracle — roadmap P6, vector doc V6.
//!
//! Vector mode only changes *storage/loop structure*, not the per-sample
//! arithmetic, so its output must be **bit-identical** to scalar. These tests
//! compile the same DSP scalar and with `ComputeMode::Vector`, run both through
//! the interpreter over a block larger than the vector size (so state crosses a
//! chunk boundary), and assert the outputs are exactly equal.

use std::io::Cursor;

use codegen::backends::interp::{FbcDspInstance, InterpOptions, read_fbc};
use compiler::{
    Compiler, ComputeMode, SchedulingStrategy, SignalFirLane, VectorFallbackReason,
    VectorPipelineStatus,
};

/// Compiles `source` to interpreter bytecode with the given compute and
/// scheduling modes, then runs one block with the provided input channels.
fn run_channels_with_strategy(
    source: &str,
    mode: ComputeMode,
    inputs: &[Vec<f32>],
    strategy: SchedulingStrategy,
) -> Vec<Vec<f32>> {
    let path = std::env::temp_dir().join(format!(
        "faust-rs-vecmode-{}-{:?}.dsp",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, source).expect("write temp dsp");
    let fbc = Compiler::new()
        .with_compute_mode(mode)
        .with_scheduling_strategy(strategy)
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
    let frames = inputs.first().map_or(0, Vec::len);
    assert!(inputs.iter().all(|input| input.len() == frames));
    let mut outputs = vec![vec![0.0_f32; frames]; num_outputs];
    let mut slices: Vec<&mut [f32]> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
    let input_slices = inputs.iter().map(Vec::as_slice).collect::<Vec<_>>();
    instance
        .try_compute(frames as i32, &input_slices, &mut slices)
        .expect("compute");
    outputs
}

fn assert_channels_bit_exact(name: &str, source: &str, inputs: &[Vec<f32>], vec_size: u32) {
    for strategy in scheduling_strategies() {
        let scalar = run_channels_with_strategy(source, ComputeMode::Scalar, inputs, strategy);
        for loop_variant in [0_u8, 1] {
            let vector = run_channels_with_strategy(
                source,
                ComputeMode::Vector {
                    vec_size,
                    loop_variant,
                },
                inputs,
                strategy,
            );
            assert_eq!(
                scalar, vector,
                "{name}: scalar differs from -lv {loop_variant} under {strategy:?}"
            );
        }
    }
}

fn scheduling_strategies() -> [SchedulingStrategy; 4] {
    [
        SchedulingStrategy::DepthFirst,
        SchedulingStrategy::BreadthFirst,
        SchedulingStrategy::Special,
        SchedulingStrategy::ReverseBreadthFirst,
    ]
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
    for strategy in scheduling_strategies() {
        let scalar = run_channels_with_strategy(
            source,
            ComputeMode::Scalar,
            std::slice::from_ref(&input),
            strategy,
        );
        for loop_variant in [0_u8, 1] {
            let vector = run_channels_with_strategy(
                source,
                ComputeMode::Vector {
                    vec_size,
                    loop_variant,
                },
                std::slice::from_ref(&input),
                strategy,
            );
            assert_eq!(
                scalar.len(),
                vector.len(),
                "{name}: output channel count differs (-lv {loop_variant}, {strategy:?})"
            );
            for (ch, (s, v)) in scalar.iter().zip(vector.iter()).enumerate() {
                assert_eq!(
                    s, v,
                    "{name}: channel {ch} differs, scalar vs -vec (-vs {vec_size} -lv {loop_variant} {strategy:?})"
                );
            }
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

#[test]
fn clock_island_and_expanded_fad_are_bit_exact() {
    let input = (0..64)
        .map(|index| if index % 3 == 0 { 1.0 } else { 0.0 })
        .collect::<Vec<_>>();
    let clock_inputs = vec![input, ramp(64)];
    assert_channels_bit_exact(
        "clock",
        "process = ((_ != 0), _) : ondemand(*(2));",
        &clock_inputs,
        24,
    );

    let inputs = vec![
        ramp(64),
        (0..64).map(|index| 0.25 + index as f32 * 0.01).collect(),
        (0..64).map(|index| 1.0 - index as f32 * 0.02).collect(),
    ];
    assert_channels_bit_exact("fad", "process = fad(*, (_,_,_));", &inputs, 24);
}

#[test]
fn production_fir_reports_certified_and_named_fallback_paths() {
    let compiler = Compiler::new().with_compute_mode(ComputeMode::Vector {
        vec_size: 8,
        loop_variant: 0,
    });
    let pure = compiler
        .compile_source_to_fir_with_lane(
            "pure.dsp",
            "process = _ * 0.5;",
            SignalFirLane::TransformFastLane,
        )
        .expect("pure vector FIR");
    assert_eq!(pure.vector_pipeline_status, VectorPipelineStatus::Certified);

    let stateful = compiler
        .compile_source_to_fir_with_lane(
            "stateful.dsp",
            "process = _ : mem;",
            SignalFirLane::TransformFastLane,
        )
        .expect("stateful transitional vector FIR");
    assert_eq!(
        stateful.vector_pipeline_status,
        VectorPipelineStatus::Certified
    );

    for strategy in scheduling_strategies() {
        let compiler = Compiler::new()
            .with_compute_mode(ComputeMode::Vector {
                vec_size: 8,
                loop_variant: 0,
            })
            .with_scheduling_strategy(strategy);
        for (name, source) in [
            ("recursive", "process = (_ : + ~ _) * 0.5;"),
            ("fad", "process = fad(*, (_,_,_));"),
            ("clock", "process = ((_ != 0), _) : ondemand(*(2));"),
        ] {
            let output = compiler
                .compile_source_to_fir_with_lane(
                    &format!("{name}.dsp"),
                    source,
                    SignalFirLane::TransformFastLane,
                )
                .unwrap_or_else(|error| panic!("{name} vector FIR: {error}"));
            assert_eq!(
                output.vector_pipeline_status,
                VectorPipelineStatus::Certified,
                "{name} must use the checked vector path under {strategy:?}"
            );
        }
    }

    let ui = compiler
        .compile_source_to_fir_with_lane(
            "ui.dsp",
            "process = _ * hslider(\"gain\", 0.5, 0.0, 1.0, 0.01);",
            SignalFirLane::TransformFastLane,
        )
        .expect("UI transitional vector FIR");
    assert_eq!(
        ui.vector_pipeline_status,
        VectorPipelineStatus::Fallback(VectorFallbackReason::UiProgram)
    );

    let clock_state = compiler
        .compile_source_to_fir_with_lane(
            "clock-state.dsp",
            "process = ((_ != 0), _) : ondemand(+ ~ _);",
            SignalFirLane::TransformFastLane,
        )
        .expect("clock-local state transitional vector FIR");
    assert_eq!(
        clock_state.vector_pipeline_status,
        VectorPipelineStatus::Fallback(VectorFallbackReason::StatePlan)
    );

    let rad = compiler
        .compile_source_to_fir_with_lane(
            "rad.dsp",
            "process = rad(_', _);",
            SignalFirLane::TransformFastLane,
        )
        .expect("RAD transitional vector FIR");
    assert_eq!(
        rad.vector_pipeline_status,
        VectorPipelineStatus::Fallback(VectorFallbackReason::ReverseAd)
    );
    assert_eq!(VectorFallbackReason::ReverseAd.code(), "FRS-VEC-RAD-SCALAR");
}
