//! Runtime regressions for forward-mode AD across recursive structures.
//!
//! These tests complement the structural `signal_pipeline` coverage with
//! numeric checks against either closed-form recurrences or central finite
//! differences executed through the interpreter fast lane.

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use codegen::backends::interp::{FbcDspInstance, InterpOptions, read_fbc};
use compiler::{Compiler, SignalFirLane};

static NEXT_TEMP_DSP_ID: AtomicUsize = AtomicUsize::new(0);

struct CentralDifferenceCase<BuildFad, BuildPrimal> {
    stem: &'static str,
    primal_outputs: usize,
    frame_count: usize,
    base_param: f32,
    epsilon: f32,
    abs_tol: f32,
    build_fad_source: BuildFad,
    build_primal_source: BuildPrimal,
}

fn corpus_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("corpus")
        .join(file)
}

fn run_interp_file(path: &std::path::Path, frame_count: usize) -> Vec<Vec<f32>> {
    let compiler = Compiler::new();
    let fbc = compiler
        .compile_file_default_to_interp_with_lane(
            path,
            &InterpOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .unwrap_or_else(|e| panic!("{} interp compilation failed: {e}", path.display()));

    let mut reader = Cursor::new(fbc);
    let mut factory = read_fbc::<f32>(&mut reader)
        .unwrap_or_else(|e| panic!("{} interp bytecode parse failed: {e}", path.display()));
    let mut instance = FbcDspInstance::new(&mut factory);
    instance.init(48_000);

    let num_outputs = usize::try_from(instance.get_num_outputs()).expect("non-negative outputs");
    let mut outputs = vec![vec![0.0_f32; frame_count]; num_outputs];
    let mut output_slices: Vec<&mut [f32]> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
    instance
        .try_compute(frame_count as i32, &[], &mut output_slices)
        .unwrap_or_else(|e| panic!("{} interp execution failed: {e}", path.display()));
    outputs
}

fn run_interp_temp_source(stem: &str, source: &str, frame_count: usize) -> Vec<Vec<f32>> {
    let unique_id = NEXT_TEMP_DSP_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "faust-rs-{stem}-{}-{unique_id}.dsp",
        std::process::id()
    ));
    fs::write(&path, source)
        .unwrap_or_else(|e| panic!("failed to write temporary DSP {}: {e}", path.display()));
    let result = run_interp_file(&path, frame_count);
    let _ = fs::remove_file(&path);
    result
}

fn assert_close(actual: f32, expected: f32, abs_tol: f32, label: &str) {
    let diff = (actual - expected).abs();
    let rel_tol = 1.0e-5_f32 * actual.abs().max(expected.abs());
    let allowed = abs_tol.max(rel_tol);
    assert!(
        diff <= allowed,
        "{label}: expected {expected}, got {actual}, abs diff {diff}, allowed {allowed}"
    );
}

fn assert_single_seed_fad_matches_central_difference(
    case: CentralDifferenceCase<impl Fn(f32) -> String, impl Fn(f32) -> String>,
) {
    let fad_outputs = run_interp_temp_source(
        &format!("{}-fad", case.stem),
        &(case.build_fad_source)(case.base_param),
        case.frame_count,
    );
    let primal_outputs_base = run_interp_temp_source(
        &format!("{}-primal", case.stem),
        &(case.build_primal_source)(case.base_param),
        case.frame_count,
    );
    let primal_outputs_plus = run_interp_temp_source(
        &format!("{}-plus", case.stem),
        &(case.build_primal_source)(case.base_param + case.epsilon),
        case.frame_count,
    );
    let primal_outputs_minus = run_interp_temp_source(
        &format!("{}-minus", case.stem),
        &(case.build_primal_source)(case.base_param - case.epsilon),
        case.frame_count,
    );

    assert_eq!(
        fad_outputs.len(),
        case.primal_outputs * 2,
        "{}: one-seed FAD layout must be [p0, t0, p1, t1, ...]",
        case.stem
    );
    assert_eq!(primal_outputs_base.len(), case.primal_outputs);
    assert_eq!(primal_outputs_plus.len(), case.primal_outputs);
    assert_eq!(primal_outputs_minus.len(), case.primal_outputs);

    for primal_index in 0..case.primal_outputs {
        for frame in 0..case.frame_count {
            let actual_primal = fad_outputs[primal_index * 2][frame];
            let expected_primal = primal_outputs_base[primal_index][frame];
            assert_close(
                actual_primal,
                expected_primal,
                case.abs_tol,
                &format!("{} primal[{primal_index}] frame {frame}", case.stem),
            );

            let expected_tangent = (primal_outputs_plus[primal_index][frame]
                - primal_outputs_minus[primal_index][frame])
                / (2.0 * case.epsilon);
            let actual_tangent = fad_outputs[primal_index * 2 + 1][frame];
            assert_close(
                actual_tangent,
                expected_tangent,
                case.abs_tol,
                &format!("{} tangent[{primal_index}] frame {frame}", case.stem),
            );
        }
    }
}

#[test]
fn fastlane_interp_self_recursive_fad_matches_closed_form_recurrence() {
    let outputs = run_interp_file(&corpus_path("fad_recursive_parametric_self.dsp"), 6);
    assert_eq!(outputs.len(), 2);

    let p = 0.2_f32;
    let mut primal_prev = 0.0_f32;
    let mut tangent_prev = 0.0_f32;
    for (frame, (&actual_primal, &actual_tangent)) in
        outputs[0].iter().zip(outputs[1].iter()).enumerate().take(6)
    {
        let primal = p * primal_prev + 2.0;
        let tangent = primal_prev + p * tangent_prev;

        assert_close(
            actual_primal,
            primal,
            1.0e-6,
            &format!("fad_recursive_parametric_self primal frame {frame}"),
        );
        assert_close(
            actual_tangent,
            tangent,
            1.0e-6,
            &format!("fad_recursive_parametric_self tangent frame {frame}"),
        );

        primal_prev = primal;
        tangent_prev = tangent;
    }
}

#[test]
fn fastlane_interp_nested_recursive_fad_matches_central_difference() {
    fn fad_source(p: f32) -> String {
        format!(
            r#"
p = hslider("p", {p}, -0.9, 0.9, 0.001);
inner = 2 : + ~ *(p);
outer = 1 : + ~ *(inner);
process = fad(outer, p);
"#
        )
    }

    fn primal_source(p: f32) -> String {
        format!(
            r#"
p = hslider("p", {p}, -0.9, 0.9, 0.001);
inner = 2 : + ~ *(p);
outer = 1 : + ~ *(inner);
process = outer;
"#
        )
    }

    assert_single_seed_fad_matches_central_difference(CentralDifferenceCase {
        stem: "fad-nested-recursive",
        primal_outputs: 1,
        frame_count: 8,
        base_param: 0.2,
        epsilon: 1.0e-3,
        abs_tol: 5.0e-3,
        build_fad_source: fad_source,
        build_primal_source: primal_source,
    });
}

#[test]
fn fastlane_interp_multi_output_recursive_fad_matches_central_difference() {
    fn fad_source(p: f32) -> String {
        format!(
            r#"
import("stdfaust.lib");
p = hslider("p", {p}, -0.9, 0.9, 0.001);
process = fad(si.bus(2) ~ (*(p), *(0.25)), p);
"#
        )
    }

    fn primal_source(p: f32) -> String {
        format!(
            r#"
import("stdfaust.lib");
p = hslider("p", {p}, -0.9, 0.9, 0.001);
process = si.bus(2) ~ (*(p), *(0.25));
"#
        )
    }

    assert_single_seed_fad_matches_central_difference(CentralDifferenceCase {
        stem: "fad-multi-output-recursive",
        primal_outputs: 2,
        frame_count: 8,
        base_param: 0.2,
        epsilon: 1.0e-3,
        abs_tol: 2.0e-3,
        build_fad_source: fad_source,
        build_primal_source: primal_source,
    });
}

#[test]
fn fastlane_interp_mutual_recursive_fad_matches_central_difference() {
    fn fad_source(p: f32) -> String {
        format!(
            r#"
import("stdfaust.lib");
p = hslider("p", {p}, -0.9, 0.9, 0.001);
process = fad(si.bus(2) ~ ((*(p), *(0.25)) : ro.cross(2)), p);
"#
        )
    }

    fn primal_source(p: f32) -> String {
        format!(
            r#"
import("stdfaust.lib");
p = hslider("p", {p}, -0.9, 0.9, 0.001);
process = si.bus(2) ~ ((*(p), *(0.25)) : ro.cross(2));
"#
        )
    }

    assert_single_seed_fad_matches_central_difference(CentralDifferenceCase {
        stem: "fad-mutual-recursive",
        primal_outputs: 2,
        frame_count: 8,
        base_param: 0.2,
        epsilon: 1.0e-3,
        abs_tol: 2.0e-3,
        build_fad_source: fad_source,
        build_primal_source: primal_source,
    });
}

#[test]
fn fastlane_interp_multi_seed_recursive_fad_matches_central_difference_per_seed() {
    fn fad_source(a: f32, b: f32) -> String {
        format!(
            r#"
a = hslider("a", {a}, -0.9, 0.9, 0.001);
b = hslider("b", {b}, -2.0, 2.0, 0.001);
process = fad((b : + ~ *(a)), (a, b));
"#
        )
    }

    fn primal_source(a: f32, b: f32) -> String {
        format!(
            r#"
a = hslider("a", {a}, -0.9, 0.9, 0.001);
b = hslider("b", {b}, -2.0, 2.0, 0.001);
process = (b : + ~ *(a));
"#
        )
    }

    let base_a = 0.2_f32;
    let base_b = 1.0_f32;
    let epsilon = 1.0e-3_f32;
    let frame_count = 8;

    let fad_outputs = run_interp_temp_source(
        "fad-multi-seed-recursive-fad",
        &fad_source(base_a, base_b),
        frame_count,
    );
    let primal_outputs = run_interp_temp_source(
        "fad-multi-seed-recursive-primal",
        &primal_source(base_a, base_b),
        frame_count,
    );
    let primal_plus_a = run_interp_temp_source(
        "fad-multi-seed-recursive-plus-a",
        &primal_source(base_a + epsilon, base_b),
        frame_count,
    );
    let primal_minus_a = run_interp_temp_source(
        "fad-multi-seed-recursive-minus-a",
        &primal_source(base_a - epsilon, base_b),
        frame_count,
    );
    let primal_plus_b = run_interp_temp_source(
        "fad-multi-seed-recursive-plus-b",
        &primal_source(base_a, base_b + epsilon),
        frame_count,
    );
    let primal_minus_b = run_interp_temp_source(
        "fad-multi-seed-recursive-minus-b",
        &primal_source(base_a, base_b - epsilon),
        frame_count,
    );

    assert_eq!(fad_outputs.len(), 3);
    assert_eq!(primal_outputs.len(), 1);

    for frame in 0..frame_count {
        assert_close(
            fad_outputs[0][frame],
            primal_outputs[0][frame],
            2.0e-3,
            &format!("fad-multi-seed-recursive primal frame {frame}"),
        );

        let expected_da = (primal_plus_a[0][frame] - primal_minus_a[0][frame]) / (2.0 * epsilon);
        assert_close(
            fad_outputs[1][frame],
            expected_da,
            2.0e-3,
            &format!("fad-multi-seed-recursive da frame {frame}"),
        );

        let expected_db = (primal_plus_b[0][frame] - primal_minus_b[0][frame]) / (2.0 * epsilon);
        assert_close(
            fad_outputs[2][frame],
            expected_db,
            2.0e-3,
            &format!("fad-multi-seed-recursive db frame {frame}"),
        );
    }
}
