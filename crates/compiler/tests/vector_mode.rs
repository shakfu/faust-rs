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
    Compiler, ComputeMode, SchedulingStrategy, SignalFirLane, VectorEffectiveMode,
    VectorFallbackReason, VectorPipelineStatus,
};

const PULSE_COUNTUP_LOOP_SOURCE: &str =
    "ba = library(\"basics.lib\"); process = ba.pulse_countup_loop(4, 1) + 0.001;";
const PULSE_COUNTDOWN_LOOP_SOURCE: &str =
    "ba = library(\"basics.lib\"); process = ba.pulse_countdown_loop(4, 1) + 0.001;";

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

#[test]
fn scalar_scheduling_strategies_are_bit_exact() {
    let source = "process = _,_ <: (_ * 2.0 + 1.0), (_ * 3.0 + 4.0) :> _;";
    let inputs = [ramp(67), ramp(67).into_iter().rev().collect()];
    let expected = run_channels_with_strategy(
        source,
        ComputeMode::Scalar,
        &inputs,
        SchedulingStrategy::DepthFirst,
    );
    for strategy in scheduling_strategies().into_iter().skip(1) {
        assert_eq!(
            run_channels_with_strategy(source, ComputeMode::Scalar, &inputs, strategy),
            expected,
            "scalar execution changed under {strategy:?}"
        );
    }
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

fn assert_vector_pipeline_certified(name: &str, source: &str, vec_size: u32) {
    for strategy in scheduling_strategies() {
        for loop_variant in [0_u8, 1] {
            let path = std::env::temp_dir().join(format!(
                "faust-rs-vecmode-status-{}-{:?}.dsp",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::write(&path, source).expect("write temp dsp");
            let result = Compiler::new()
                .with_compute_mode(ComputeMode::Vector {
                    vec_size,
                    loop_variant,
                })
                .with_scheduling_strategy(strategy)
                .compile_file_default_to_fir_with_lane(&path, SignalFirLane::TransformFastLane);
            let _ = std::fs::remove_file(&path);
            let output = result.unwrap_or_else(|error| {
                panic!("{name} vector FIR (-lv {loop_variant}, {strategy:?}): {error}")
            });
            assert_eq!(
                output.vector_pipeline_status,
                VectorPipelineStatus::Certified,
                "{name} must use the checked vector path under -lv {loop_variant} and {strategy:?}"
            );
        }
    }
}

#[test]
fn stateless_gain_is_bit_exact() {
    assert_scalar_vector_bit_exact("gain", "process = _ * 0.5;", 32);
}

#[test]
fn ui_slider_and_bargraph_are_certified_and_bit_exact() {
    let source =
        "process = _ * hslider(\"gain\", 0.5, 0.0, 1.0, 0.01) : hbargraph(\"level\", -10.0, 10.0);";
    assert_vector_pipeline_certified("ui_slider_bargraph", source, 24);
    assert_scalar_vector_bit_exact("ui_slider_bargraph", source, 24);
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
fn recursive_short_delay_transport_reads_after_copy_in() {
    assert_scalar_vector_bit_exact("pulse_countup_loop", PULSE_COUNTUP_LOOP_SOURCE, 32);
    assert_scalar_vector_bit_exact("pulse_countdown_loop", PULSE_COUNTDOWN_LOOP_SOURCE, 32);
    // 64 samples with vec_size 24 exercises two full chunks and a 16-sample tail.
    assert_scalar_vector_bit_exact("pulse_countup_loop_tail", PULSE_COUNTUP_LOOP_SOURCE, 24);
    // vec_size exceeds the 64-sample block, covering count < vec_size.
    assert_scalar_vector_bit_exact("pulse_countup_loop_short", PULSE_COUNTUP_LOOP_SOURCE, 96);
}

#[test]
fn recursive_short_delay_transport_uses_certified_vector_pipeline() {
    assert_vector_pipeline_certified("pulse_countup_loop", PULSE_COUNTUP_LOOP_SOURCE, 32);
}

#[test]
fn recursive_short_delay_cpp_has_one_fused_read_compute_write_loop() {
    let path = std::env::temp_dir().join(format!(
        "faust-rs-vec-fused-cpp-{}-{:?}.dsp",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, PULSE_COUNTUP_LOOP_SOURCE).expect("write fused vector DSP");
    let cpp = Compiler::new()
        .with_compute_mode(ComputeMode::Vector {
            vec_size: 32,
            loop_variant: 1,
        })
        .with_scheduling_strategy(SchedulingStrategy::ReverseBreadthFirst)
        .compile_file_default_to_cpp_with_lane(
            &path,
            &codegen::backends::cpp::CppOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .expect("compile fused vector C++");
    let _ = std::fs::remove_file(path);

    assert!(cpp.contains("float transport_s23_l2_l1;"));
    assert!(!cpp.contains("float transport_s23_l2_l1[32];"));
    let first_loop = cpp
        .find("for (int i0 = vindex;")
        .expect("fused serial sample loop");
    let second_loop = cpp[first_loop + 1..]
        .find("for (int i0 = vindex;")
        .map(|offset| first_loop + 1 + offset)
        .expect("safe pure tail loop");
    let fused = &cpp[first_loop..second_loop];
    let read = fused
        .find("transport_s23_l2_l1 = vstate_s22_tmp")
        .expect("recursive delayed read in fused loop");
    let write = fused
        .find("vstate_s22_tmp[(4 + (i0 - vindex))] =")
        .expect("recursive state write in fused loop");
    assert!(read < write, "delayed read must precede the state write");
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
    assert_channels_bit_exact(
        "clock_recursion",
        "process = ((_ != 0), _) : ondemand(+ ~ _);",
        &clock_inputs,
        24,
    );
    assert_channels_bit_exact(
        "clock_delay",
        "process = ((_ != 0), _) : ondemand(_ <: _, @(3) :> +);",
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
fn bounded_variable_delay_is_bit_exact() {
    let amount = (0..64)
        .map(|index| (index as f32 * 0.31).sin() * 0.9)
        .collect::<Vec<_>>();
    assert_channels_bit_exact(
        "variable_delay",
        "process(carrier, amount) = carrier @ int(amount + 10);",
        &[ramp(64), amount],
        24,
    );
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
    assert_eq!(
        pure.vector_effective_mode,
        VectorEffectiveMode::CertifiedVector
    );
    assert_eq!(pure.vector_pipeline_detail, None);

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
            ("clock-state", "process = ((_ != 0), _) : ondemand(+ ~ _);"),
            ("variable-delay", "process = @(int(_ + 10));"),
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
    assert_eq!(ui.vector_pipeline_status, VectorPipelineStatus::Certified);
    assert_eq!(
        ui.vector_effective_mode,
        VectorEffectiveMode::CertifiedVector
    );
    assert_eq!(ui.vector_pipeline_detail, None);

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
    assert_eq!(rad.vector_effective_mode, VectorEffectiveMode::Scalar);
    assert!(rad.vector_pipeline_detail.is_some());
    assert_eq!(VectorFallbackReason::ReverseAd.code(), "FRS-VEC-RAD-SCALAR");
}

#[test]
fn phase2_plan_accepts_multi_projection_recursion_and_table_value_transports() {
    std::thread::Builder::new()
        .name("phase2-vector-plan-corpus".to_owned())
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let compiler = Compiler::new().with_compute_mode(ComputeMode::Vector {
                vec_size: 32,
                loop_variant: 0,
            });
            let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/impulse-tests/dsp");
            for name in ["APF.dsp", "pow.dsp"] {
                let path = corpus.join(name);
                let output = compiler
                    .compile_file_default_to_fir_with_lane(&path, SignalFirLane::TransformFastLane)
                    .unwrap_or_else(|error| panic!("{name} vector FIR: {error}"));
                assert_ne!(
                    output.vector_pipeline_status,
                    VectorPipelineStatus::Fallback(VectorFallbackReason::VectorPlan),
                    "{name} must pass the checked phase-2 vector plan"
                );
            }
        })
        .expect("spawn large-stack phase-2 test")
        .join()
        .expect("phase-2 test thread");
}

#[test]
fn vector_copy_delay_loops_reach_c_family_backends() {
    let source = include_str!("../../../tests/impulse-tests/dsp/noiseabs.dsp");
    for loop_variant in [0_u8, 1] {
        let compiler = Compiler::new()
            .with_compute_mode(ComputeMode::Vector {
                vec_size: 32,
                loop_variant,
            })
            .with_scheduling_strategy(SchedulingStrategy::DepthFirst);
        let cpp = compiler
            .compile_source_to_cpp_with_lane(
                "noiseabs.dsp",
                source,
                &codegen::backends::cpp::CppOptions::default(),
                SignalFirLane::TransformFastLane,
            )
            .unwrap_or_else(|error| {
                panic!("noiseabs C++ -vec -lv {loop_variant} lowering failed: {error}")
            });
        assert!(cpp.contains("vstate_s21_perm"));

        let c = compiler
            .compile_source_to_c_with_lane(
                "noiseabs.dsp",
                source,
                &codegen::backends::c::COptions::default(),
                SignalFirLane::TransformFastLane,
            )
            .unwrap_or_else(|error| {
                panic!("noiseabs C -vec -lv {loop_variant} lowering failed: {error}")
            });
        assert!(c.contains("vstate_s21_perm"));
    }
}
