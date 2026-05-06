//! Runtime regressions for reverse-mode AD (`rad`) on the feed-forward
//! subset.
//!
//! Two complementary checks per case:
//! - **RAD vs FAD parity.** For the same expression and seed list, the
//!   gradient lanes of `rad(expr, seeds)` must agree with the matching
//!   tangent lanes of `fad(expr, seeds)` lane by lane. This is the
//!   strongest invariant because both use the same underlying
//!   differentiable signal subset.
//! - **RAD vs central finite differences.** Each gradient lane is also
//!   checked against the central difference of the primal under
//!   per-seed perturbation. This catches any drift that would slip past
//!   FAD itself if both sides shared a bug.
//!
//! The tests use the interpreter fast lane through the public compiler
//! facade, so they exercise propagation, transform, FIR lowering and the
//! interp backend together.

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use codegen::backends::interp::{FbcDspInstance, InterpOptions, read_fbc};
use compiler::{Compiler, SignalFirLane};

fn corpus_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("corpus")
        .join(file)
}

fn run_interp_corpus_inner(stem: &str, frame_count: usize) -> Vec<Vec<f32>> {
    let path = corpus_path(&format!("{stem}.dsp"));
    let compiler = Compiler::new();
    let fbc = compiler
        .compile_file_default_to_interp_with_lane(
            &path,
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

/// Same 64 MB-stack worker pattern as `run_interp_temp_source`, but reads
/// fixtures from `tests/corpus/` so they can be inspected directly and
/// reused across the broader corpus tooling.
fn run_interp_corpus(stem: &'static str, frame_count: usize) -> Vec<Vec<f32>> {
    std::thread::Builder::new()
        .name(format!("rad-runtime-corpus-{stem}"))
        .stack_size(64 * 1024 * 1024)
        .spawn(move || run_interp_corpus_inner(stem, frame_count))
        .expect("spawn rad-runtime-corpus worker")
        .join()
        .expect("rad-runtime-corpus worker thread should finish")
}

static NEXT_TEMP_DSP_ID: AtomicUsize = AtomicUsize::new(0);

fn run_interp_temp_source(stem: &str, source: &str, frame_count: usize) -> Vec<Vec<f32>> {
    let stem = stem.to_owned();
    let source = source.to_owned();
    // Spawn on a 64 MB stack: pipelines that drag `stdfaust.lib` produce
    // deep evaluation trees (same pattern used by `signal_pipeline.rs` and
    // `zita_pipeline.rs`) and overflow the default 2 MB test-thread stack.
    std::thread::Builder::new()
        .name(format!("rad-runtime-{stem}"))
        .stack_size(64 * 1024 * 1024)
        .spawn(move || run_interp_temp_source_inner(&stem, &source, frame_count))
        .expect("spawn rad-runtime worker")
        .join()
        .expect("rad-runtime worker thread should finish")
}

fn run_interp_temp_source_inner(stem: &str, source: &str, frame_count: usize) -> Vec<Vec<f32>> {
    run_interp_temp_source_with_inputs_inner(stem, source, &[], frame_count)
}

fn run_interp_temp_source_with_inputs(
    stem: &str,
    source: &str,
    inputs: &[Vec<f32>],
    frame_count: usize,
) -> Vec<Vec<f32>> {
    let stem = stem.to_owned();
    let source = source.to_owned();
    let inputs = inputs.to_vec();
    std::thread::Builder::new()
        .name(format!("rad-runtime-{stem}"))
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            run_interp_temp_source_with_inputs_inner(&stem, &source, &inputs, frame_count)
        })
        .expect("spawn rad-runtime worker")
        .join()
        .expect("rad-runtime worker thread should finish")
}

fn run_interp_temp_source_with_inputs_inner(
    stem: &str,
    source: &str,
    inputs: &[Vec<f32>],
    frame_count: usize,
) -> Vec<Vec<f32>> {
    let unique_id = NEXT_TEMP_DSP_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "faust-rs-rad-{stem}-{}-{unique_id}.dsp",
        std::process::id()
    ));
    fs::write(&path, source)
        .unwrap_or_else(|e| panic!("failed to write temporary DSP {}: {e}", path.display()));
    let compiler = Compiler::new();
    let fbc = compiler
        .compile_file_default_to_interp_with_lane(
            &path,
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
    let num_inputs = usize::try_from(instance.get_num_inputs()).expect("non-negative inputs");
    assert_eq!(
        inputs.len(),
        num_inputs,
        "{stem}: input fixture arity must match compiled DSP input count"
    );
    for (index, input) in inputs.iter().enumerate() {
        assert_eq!(
            input.len(),
            frame_count,
            "{stem}: input lane {index} length must match frame count"
        );
    }
    let mut outputs = vec![vec![0.0_f32; frame_count]; num_outputs];
    let input_slices: Vec<&[f32]> = inputs.iter().map(Vec::as_slice).collect();
    let mut output_slices: Vec<&mut [f32]> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
    instance
        .try_compute(frame_count as i32, &input_slices, &mut output_slices)
        .unwrap_or_else(|e| panic!("{} interp execution failed: {e}", path.display()));
    let _ = fs::remove_file(&path);
    outputs
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

/// Asserts RAD output bundle layout `[primals…, grad(seeds)…]` matches:
/// - the primal for each output via direct evaluation,
/// - each gradient lane via central finite difference on the primal source.
///
/// `build_rad_source(seeds)` must yield a `rad(expr, seeds)` program.
/// `build_primal_source(seeds)` builds the same `expr` without `rad(...)`.
#[allow(clippy::too_many_arguments)]
fn assert_rad_matches_central_difference<BuildRad, BuildPrimal>(
    stem: &str,
    primal_outputs: usize,
    frame_count: usize,
    base_seeds: &[f32],
    epsilons: &[f32],
    abs_tol: f32,
    build_rad_source: BuildRad,
    build_primal_source: BuildPrimal,
) where
    BuildRad: Fn(&[f32]) -> String,
    BuildPrimal: Fn(&[f32]) -> String,
{
    assert_eq!(
        base_seeds.len(),
        epsilons.len(),
        "seed/epsilon arity must match"
    );
    let n = base_seeds.len();

    let rad_outputs = run_interp_temp_source(
        &format!("{stem}-rad"),
        &build_rad_source(base_seeds),
        frame_count,
    );
    let primal_base = run_interp_temp_source(
        &format!("{stem}-primal"),
        &build_primal_source(base_seeds),
        frame_count,
    );
    assert_eq!(
        rad_outputs.len(),
        primal_outputs + n,
        "{stem}: rad output bundle layout = [primals…, gradients…]"
    );
    assert_eq!(primal_base.len(), primal_outputs);

    let mut primal_plus = Vec::with_capacity(n);
    let mut primal_minus = Vec::with_capacity(n);
    for j in 0..n {
        let mut up = base_seeds.to_vec();
        up[j] += epsilons[j];
        let mut dn = base_seeds.to_vec();
        dn[j] -= epsilons[j];
        primal_plus.push(run_interp_temp_source(
            &format!("{stem}-plus-{j}"),
            &build_primal_source(&up),
            frame_count,
        ));
        primal_minus.push(run_interp_temp_source(
            &format!("{stem}-minus-{j}"),
            &build_primal_source(&dn),
            frame_count,
        ));
    }

    for primal_index in 0..primal_outputs {
        for frame in 0..frame_count {
            assert_close(
                rad_outputs[primal_index][frame],
                primal_base[primal_index][frame],
                abs_tol,
                &format!("{stem} primal[{primal_index}] frame {frame}"),
            );
        }
    }

    // Gradient lanes are at indices [primal_outputs, primal_outputs + n).
    // Each gradient is the partial of `sum(primals)` w.r.t. seed j.
    for j in 0..n {
        for frame in 0..frame_count {
            let mut expected = 0.0_f32;
            for primal_index in 0..primal_outputs {
                expected += (primal_plus[j][primal_index][frame]
                    - primal_minus[j][primal_index][frame])
                    / (2.0 * epsilons[j]);
            }
            let actual = rad_outputs[primal_outputs + j][frame];
            assert_close(
                actual,
                expected,
                abs_tol,
                &format!("{stem} gradient[seed {j}] frame {frame}"),
            );
        }
    }
}

/// Asserts RAD gradients agree with the matching FAD tangent lanes for a
/// scalar primal. For multi-output primals the FAD lanes are summed, since
/// RAD's implicit cotangent is all-ones.
fn assert_rad_matches_fad<BuildRad, BuildFad>(
    stem: &str,
    primal_outputs: usize,
    seed_count: usize,
    frame_count: usize,
    abs_tol: f32,
    build_rad_source: BuildRad,
    build_fad_source: BuildFad,
) where
    BuildRad: Fn() -> String,
    BuildFad: Fn() -> String,
{
    let rad_outputs = run_interp_temp_source(
        &format!("{stem}-rad-vs-fad-rad"),
        &build_rad_source(),
        frame_count,
    );
    let fad_outputs = run_interp_temp_source(
        &format!("{stem}-rad-vs-fad-fad"),
        &build_fad_source(),
        frame_count,
    );
    // RAD layout: [p_0, …, p_{M-1}, grad_s0, …, grad_s{N-1}]
    // FAD layout: [p_0, t_0_s0, …, t_0_s{N-1}, p_1, t_1_s0, …]
    assert_eq!(rad_outputs.len(), primal_outputs + seed_count);
    assert_eq!(fad_outputs.len(), primal_outputs * (1 + seed_count));

    for primal_index in 0..primal_outputs {
        for frame in 0..frame_count {
            assert_close(
                rad_outputs[primal_index][frame],
                fad_outputs[primal_index * (1 + seed_count)][frame],
                abs_tol,
                &format!("{stem} primal[{primal_index}] RAD vs FAD frame {frame}"),
            );
        }
    }

    for j in 0..seed_count {
        for frame in 0..frame_count {
            let mut fad_sum = 0.0_f32;
            for primal_index in 0..primal_outputs {
                fad_sum += fad_outputs[primal_index * (1 + seed_count) + 1 + j][frame];
            }
            assert_close(
                rad_outputs[primal_outputs + j][frame],
                fad_sum,
                abs_tol,
                &format!("{stem} gradient[seed {j}] RAD vs FAD frame {frame}"),
            );
        }
    }
}

#[test]
fn fastlane_interp_lti_recursive_rad_feedback_coeff_matches_closed_form_contributions() {
    let frame_count = 6;
    let outputs = run_interp_temp_source(
        "rad-lti-recursive-feedback-coeff",
        r#"
p = 0.5;
process = rad((2 : + ~ *(p)), p);
"#,
        frame_count,
    );
    assert_eq!(
        outputs.len(),
        2,
        "RAD recursive fixture layout must be [primal, gradient]"
    );

    let p = 0.5_f32;
    let mut primals = Vec::with_capacity(frame_count);
    let mut previous_primal = 0.0_f32;
    for _ in 0..frame_count {
        let primal = 2.0 + p * previous_primal;
        primals.push(primal);
        previous_primal = primal;
    }

    let mut cotangents = vec![0.0_f32; frame_count];
    let mut next_cotangent = 0.0_f32;
    for frame in (0..frame_count).rev() {
        let cotangent = 1.0 + p * next_cotangent;
        cotangents[frame] = cotangent;
        next_cotangent = cotangent;
    }

    for frame in 0..frame_count {
        assert_close(
            outputs[0][frame],
            primals[frame],
            1.0e-6,
            &format!("rad_lti_recursive_feedback_coeff primal frame {frame}"),
        );

        let previous_state = if frame == 0 { 0.0 } else { primals[frame - 1] };
        let gradient_contribution = cotangents[frame] * previous_state;
        assert_close(
            outputs[1][frame],
            gradient_contribution,
            1.0e-6,
            &format!("rad_lti_recursive_feedback_coeff gradient frame {frame}"),
        );
    }
}

#[test]
fn fastlane_interp_audio_one_pole_lti_recursive_rad_matches_closed_form_contributions() {
    let input = vec![0.25, -0.5, 1.0, 0.75, -0.25, 0.5];
    let frame_count = input.len();
    let source = fs::read_to_string(corpus_path("rad_lti_recursive_one_pole.dsp"))
        .expect("read rad_lti_recursive_one_pole fixture");
    let outputs = run_interp_temp_source_with_inputs(
        "rad-audio-one-pole-lti-recursive-feedback-coeff",
        &source,
        std::slice::from_ref(&input),
        frame_count,
    );
    assert_eq!(
        outputs.len(),
        2,
        "RAD audio one-pole fixture layout must be [y, dp]"
    );

    let p = 0.5_f32;
    let mut primals = Vec::with_capacity(frame_count);
    let mut previous_primal = 0.0_f32;
    for x in &input {
        let primal = x + p * previous_primal;
        primals.push(primal);
        previous_primal = primal;
    }

    let mut cotangents = vec![0.0_f32; frame_count];
    let mut next_cotangent = 0.0_f32;
    for frame in (0..frame_count).rev() {
        let cotangent = 1.0 + p * next_cotangent;
        cotangents[frame] = cotangent;
        next_cotangent = cotangent;
    }

    for frame in 0..frame_count {
        assert_close(
            outputs[0][frame],
            primals[frame],
            1.0e-6,
            &format!("rad_audio_one_pole_lti_recursive primal frame {frame}"),
        );

        let previous_state = if frame == 0 { 0.0 } else { primals[frame - 1] };
        assert_close(
            outputs[1][frame],
            cotangents[frame] * previous_state,
            1.0e-6,
            &format!("rad_audio_one_pole_lti_recursive dp frame {frame}"),
        );
    }
}

#[test]
fn fastlane_interp_multi_output_lti_recursive_rad_matches_closed_form_contributions() {
    let frame_count = 6;
    let outputs = run_interp_corpus("rad_lti_recursive_multi_output", frame_count);
    assert_eq!(
        outputs.len(),
        4,
        "RAD recursive fixture layout must be [y0, y1, dp, dq]"
    );

    let cases = [
        (0usize, 2usize, 0.5_f32, 2.0_f32, "p"),
        (1usize, 3usize, 0.25_f32, 3.0_f32, "q"),
    ];
    for (primal_lane, gradient_lane, coeff, drive, label) in cases {
        let mut primals = Vec::with_capacity(frame_count);
        let mut previous_primal = 0.0_f32;
        for _ in 0..frame_count {
            let primal = drive + coeff * previous_primal;
            primals.push(primal);
            previous_primal = primal;
        }

        let mut cotangents = vec![0.0_f32; frame_count];
        let mut next_cotangent = 0.0_f32;
        for frame in (0..frame_count).rev() {
            let cotangent = 1.0 + coeff * next_cotangent;
            cotangents[frame] = cotangent;
            next_cotangent = cotangent;
        }

        for frame in 0..frame_count {
            assert_close(
                outputs[primal_lane][frame],
                primals[frame],
                1.0e-6,
                &format!("rad_multi_output_lti_recursive {label} primal frame {frame}"),
            );

            let previous_state = if frame == 0 { 0.0 } else { primals[frame - 1] };
            let gradient_contribution = cotangents[frame] * previous_state;
            assert_close(
                outputs[gradient_lane][frame],
                gradient_contribution,
                1.0e-6,
                &format!("rad_multi_output_lti_recursive d{label} frame {frame}"),
            );
        }
    }
}

#[test]
fn fastlane_interp_audio_state_space_lti_recursive_rad_matches_closed_form_contributions() {
    let drive = vec![0.25, -0.5, 1.0, 0.75, -0.25, 0.5];
    let zero = vec![0.0; drive.len()];
    let frame_count = drive.len();
    let source = fs::read_to_string(corpus_path("rad_lti_recursive_state_space.dsp"))
        .expect("read rad_lti_recursive_state_space fixture");
    let outputs = run_interp_temp_source_with_inputs(
        "rad-audio-state-space-lti-recursive-feedback-coeff",
        &source,
        &[drive.clone(), zero],
        frame_count,
    );
    assert_eq!(
        outputs.len(),
        4,
        "RAD audio state-space fixture layout must be [y0, y1, dp, dq]"
    );

    let p = 0.5_f32;
    let q = 0.25_f32;
    let mut y0 = vec![0.0_f32; frame_count];
    let mut y1 = vec![0.0_f32; frame_count];
    let mut prev0 = 0.0_f32;
    let mut prev1 = 0.0_f32;
    for frame in 0..frame_count {
        y0[frame] = drive[frame] + q * prev1;
        y1[frame] = p * prev0;
        prev0 = y0[frame];
        prev1 = y1[frame];
    }

    let mut lambda0 = vec![0.0_f32; frame_count];
    let mut lambda1 = vec![0.0_f32; frame_count];
    let mut next0 = 0.0_f32;
    let mut next1 = 0.0_f32;
    for frame in (0..frame_count).rev() {
        lambda0[frame] = 1.0 + p * next1;
        lambda1[frame] = 1.0 + q * next0;
        next0 = lambda0[frame];
        next1 = lambda1[frame];
    }

    for frame in 0..frame_count {
        assert_close(
            outputs[0][frame],
            y0[frame],
            1.0e-6,
            &format!("rad_audio_state_space_lti_recursive y0 frame {frame}"),
        );
        assert_close(
            outputs[1][frame],
            y1[frame],
            1.0e-6,
            &format!("rad_audio_state_space_lti_recursive y1 frame {frame}"),
        );

        let prev0 = if frame == 0 { 0.0 } else { y0[frame - 1] };
        let prev1 = if frame == 0 { 0.0 } else { y1[frame - 1] };
        assert_close(
            outputs[2][frame],
            lambda1[frame] * prev0,
            1.0e-6,
            &format!("rad_audio_state_space_lti_recursive dp frame {frame}"),
        );
        assert_close(
            outputs[3][frame],
            lambda0[frame] * prev1,
            1.0e-6,
            &format!("rad_audio_state_space_lti_recursive dq frame {frame}"),
        );
    }
}

#[test]
fn fastlane_interp_coupled_lti_recursive_rad_matches_closed_form_contributions() {
    let frame_count = 6;
    let outputs = run_interp_temp_source(
        "rad-coupled-lti-recursive-feedback-coeff",
        r#"
import("stdfaust.lib");
p = 0.5;
q = 0.25;
core = (ro.interleave(2, 2) : (+, +)) ~ ((*(p), *(q)) : ro.cross(2));
process = rad((2, 3) : core, (p, q));
"#,
        frame_count,
    );
    assert_eq!(
        outputs.len(),
        4,
        "RAD coupled recursive fixture layout must be [y0, y1, dp, dq]"
    );

    let p = 0.5_f32;
    let q = 0.25_f32;
    let mut y0 = vec![0.0_f32; frame_count];
    let mut y1 = vec![0.0_f32; frame_count];
    let mut prev0 = 0.0_f32;
    let mut prev1 = 0.0_f32;
    for frame in 0..frame_count {
        y0[frame] = 2.0 + q * prev1;
        y1[frame] = 3.0 + p * prev0;
        prev0 = y0[frame];
        prev1 = y1[frame];
    }

    let mut lambda0 = vec![0.0_f32; frame_count];
    let mut lambda1 = vec![0.0_f32; frame_count];
    let mut next0 = 0.0_f32;
    let mut next1 = 0.0_f32;
    for frame in (0..frame_count).rev() {
        lambda0[frame] = 1.0 + p * next1;
        lambda1[frame] = 1.0 + q * next0;
        next0 = lambda0[frame];
        next1 = lambda1[frame];
    }

    for frame in 0..frame_count {
        assert_close(
            outputs[0][frame],
            y0[frame],
            1.0e-6,
            &format!("rad_coupled_lti_recursive y0 frame {frame}"),
        );
        assert_close(
            outputs[1][frame],
            y1[frame],
            1.0e-6,
            &format!("rad_coupled_lti_recursive y1 frame {frame}"),
        );

        let prev0 = if frame == 0 { 0.0 } else { y0[frame - 1] };
        let prev1 = if frame == 0 { 0.0 } else { y1[frame - 1] };
        assert_close(
            outputs[2][frame],
            lambda1[frame] * prev0,
            1.0e-6,
            &format!("rad_coupled_lti_recursive dp frame {frame}"),
        );
        assert_close(
            outputs[3][frame],
            lambda0[frame] * prev1,
            1.0e-6,
            &format!("rad_coupled_lti_recursive dq frame {frame}"),
        );
    }
}

#[test]
fn rad_polynomial_two_seeds_matches_central_difference() {
    fn rad_source(seeds: &[f32]) -> String {
        let (a, b) = (seeds[0], seeds[1]);
        format!(
            r#"
a = hslider("a", {a}, -2.0, 2.0, 0.001);
b = hslider("b", {b}, -2.0, 2.0, 0.001);
process = rad(a*a*b + b, (a, b));
"#
        )
    }
    fn primal_source(seeds: &[f32]) -> String {
        let (a, b) = (seeds[0], seeds[1]);
        format!(
            r#"
a = hslider("a", {a}, -2.0, 2.0, 0.001);
b = hslider("b", {b}, -2.0, 2.0, 0.001);
process = a*a*b + b;
"#
        )
    }
    assert_rad_matches_central_difference(
        "polynomial-two-seeds",
        1,
        4,
        &[1.5, -0.7],
        &[1.0e-3, 1.0e-3],
        5.0e-3,
        rad_source,
        primal_source,
    );
}

#[test]
fn rad_trig_composition_matches_central_difference() {
    fn rad_source(seeds: &[f32]) -> String {
        let (a, b) = (seeds[0], seeds[1]);
        format!(
            r#"
a = hslider("a", {a}, -2.0, 2.0, 0.001);
b = hslider("b", {b}, -2.0, 2.0, 0.001);
process = rad(sin(a*b), (a, b));
"#
        )
    }
    fn primal_source(seeds: &[f32]) -> String {
        let (a, b) = (seeds[0], seeds[1]);
        format!(
            r#"
a = hslider("a", {a}, -2.0, 2.0, 0.001);
b = hslider("b", {b}, -2.0, 2.0, 0.001);
process = sin(a*b);
"#
        )
    }
    assert_rad_matches_central_difference(
        "trig-composition",
        1,
        4,
        &[0.6, 0.4],
        &[1.0e-3, 1.0e-3],
        5.0e-3,
        rad_source,
        primal_source,
    );
}

#[test]
fn rad_repeated_seed_duplicates_gradient_lane() {
    // rad(a*b, (a, a)) must produce the same gradient on lane 0 and lane 1
    // since both seed lanes refer to the same `SigId`.
    fn rad_source(seeds: &[f32]) -> String {
        let a = seeds[0];
        format!(
            r#"
a = hslider("a", {a}, -2.0, 2.0, 0.001);
b = hslider("b", 0.7, -2.0, 2.0, 0.001);
process = rad(a*b, (a, a));
"#
        )
    }
    fn primal_source(seeds: &[f32]) -> String {
        let a = seeds[0];
        format!(
            r#"
a = hslider("a", {a}, -2.0, 2.0, 0.001);
b = hslider("b", 0.7, -2.0, 2.0, 0.001);
process = a*b;
"#
        )
    }
    let frame_count = 2;
    let base = [0.5_f32, 0.5_f32];
    let eps = [1.0e-3_f32, 1.0e-3_f32];
    let outs = run_interp_temp_source("rad-repeated-seed-rad", &rad_source(&base), frame_count);
    let primal_plus = run_interp_temp_source(
        "rad-repeated-seed-plus",
        &primal_source(&[base[0] + eps[0]]),
        frame_count,
    );
    let primal_minus = run_interp_temp_source(
        "rad-repeated-seed-minus",
        &primal_source(&[base[0] - eps[0]]),
        frame_count,
    );
    assert_eq!(outs.len(), 3, "1 primal + 2 (repeated) gradient lanes");
    for frame in 0..frame_count {
        let expected = (primal_plus[0][frame] - primal_minus[0][frame]) / (2.0 * eps[0]);
        assert_close(
            outs[1][frame],
            expected,
            5.0e-3,
            &format!("rad-repeated-seed lane 0 frame {frame}"),
        );
        assert_close(
            outs[2][frame],
            outs[1][frame],
            1.0e-6,
            &format!("rad-repeated-seed lane 1 must equal lane 0 frame {frame}"),
        );
    }
}

#[test]
fn rad_absent_seed_yields_zero_gradient() {
    // rad(sin(x), y): y does not appear in sin(x), so its gradient must be
    // exactly zero even though x is a UI control.
    let source = r#"
x = hslider("x", 0.3, -1.0, 1.0, 0.01);
y = hslider("y", 0.7, -1.0, 1.0, 0.01);
process = rad(sin(x), y);
"#;
    let outs = run_interp_temp_source("rad-absent-seed", source, 4);
    assert_eq!(outs.len(), 2, "1 primal + 1 absent-seed gradient");
    for (frame, sample) in outs[1].iter().copied().enumerate().take(4) {
        assert_close(sample, 0.0, 1.0e-6, &format!("absent-seed frame {frame}"));
    }
}

#[test]
fn rad_multi_output_uses_implicit_all_ones_cotangent() {
    // rad((a*b, sin(a)), (a, b)) must produce gradients of (a*b + sin(a))
    // w.r.t. each seed: d/da = b + cos(a); d/db = a.
    fn rad_source(seeds: &[f32]) -> String {
        let (a, b) = (seeds[0], seeds[1]);
        format!(
            r#"
a = hslider("a", {a}, -2.0, 2.0, 0.001);
b = hslider("b", {b}, -2.0, 2.0, 0.001);
process = rad((a*b, sin(a)), (a, b));
"#
        )
    }
    fn primal_source(seeds: &[f32]) -> String {
        let (a, b) = (seeds[0], seeds[1]);
        format!(
            r#"
a = hslider("a", {a}, -2.0, 2.0, 0.001);
b = hslider("b", {b}, -2.0, 2.0, 0.001);
process = (a*b, sin(a));
"#
        )
    }
    assert_rad_matches_central_difference(
        "multi-output-implicit-ones",
        2,
        3,
        &[0.4, 0.3],
        &[1.0e-3, 1.0e-3],
        5.0e-3,
        rad_source,
        primal_source,
    );
}

#[test]
fn rad_vs_fad_parity_on_polynomial() {
    let rad = || {
        r#"
a = hslider("a", 0.5, -2.0, 2.0, 0.001);
b = hslider("b", 0.7, -2.0, 2.0, 0.001);
process = rad(a*a*b + b, (a, b));
"#
        .to_string()
    };
    let fad = || {
        r#"
a = hslider("a", 0.5, -2.0, 2.0, 0.001);
b = hslider("b", 0.7, -2.0, 2.0, 0.001);
process = fad(a*a*b + b, (a, b));
"#
        .to_string()
    };
    assert_rad_matches_fad("rad-vs-fad-polynomial", 1, 2, 4, 1.0e-5, rad, fad);
}

#[test]
fn rad_vs_fad_parity_on_trig_with_min_max() {
    let rad = || {
        r#"
a = hslider("a", 0.5, -2.0, 2.0, 0.001);
b = hslider("b", 0.7, -2.0, 2.0, 0.001);
process = rad(min(sin(a*b), max(a, b)), (a, b));
"#
        .to_string()
    };
    let fad = || {
        r#"
a = hslider("a", 0.5, -2.0, 2.0, 0.001);
b = hslider("b", 0.7, -2.0, 2.0, 0.001);
process = fad(min(sin(a*b), max(a, b)), (a, b));
"#
        .to_string()
    };
    assert_rad_matches_fad("rad-vs-fad-min-max", 1, 2, 4, 1.0e-5, rad, fad);
}

#[test]
fn rad_vs_fad_parity_on_tanh_ffun() {
    // tanh is a unary foreign function in Faust; phase C wires the same
    // chain rule used by FAD.
    let rad = || {
        r#"
import("stdfaust.lib");
a = hslider("a", 0.5, -2.0, 2.0, 0.001);
process = rad(ma.tanh(a*a), a);
"#
        .to_string()
    };
    let fad = || {
        r#"
import("stdfaust.lib");
a = hslider("a", 0.5, -2.0, 2.0, 0.001);
process = fad(ma.tanh(a*a), a);
"#
        .to_string()
    };
    assert_rad_matches_fad("rad-vs-fad-tanh-ffun", 1, 1, 4, 5.0e-5, rad, fad);
}

#[test]
fn rad_vs_fad_parity_on_readonly_table_index() {
    // Read-only `rdtable(waveform{...}, idx)` is differentiable through
    // the read index via the symmetric finite-difference slope.
    let rad = || {
        r#"
k = hslider("k", 3.0, 1, 6, 1);
process = rad(rdtable(waveform{0, 1, 4, 9, 16, 25, 36, 49}, k), k);
"#
        .to_string()
    };
    let fad = || {
        r#"
k = hslider("k", 3.0, 1, 6, 1);
process = fad(rdtable(waveform{0, 1, 4, 9, 16, 25, 36, 49}, k), k);
"#
        .to_string()
    };
    assert_rad_matches_fad("rad-vs-fad-rdtbl", 1, 1, 4, 1.0e-5, rad, fad);
}

#[test]
fn rad_vs_fad_parity_on_pow_select2() {
    let rad = || {
        r#"
a = hslider("a", 0.5, 0.1, 2.0, 0.001);
b = hslider("b", 0.7, -2.0, 2.0, 0.001);
process = rad(select2(a > 0.0, pow(a, b), b), (a, b));
"#
        .to_string()
    };
    let fad = || {
        r#"
a = hslider("a", 0.5, 0.1, 2.0, 0.001);
b = hslider("b", 0.7, -2.0, 2.0, 0.001);
process = fad(select2(a > 0.0, pow(a, b), b), (a, b));
"#
        .to_string()
    };
    assert_rad_matches_fad("rad-vs-fad-pow-select2", 1, 2, 4, 1.0e-5, rad, fad);
}

// -----------------------------------------------------------------------
// Corpus-driven smoke tests
// -----------------------------------------------------------------------
//
// Each fixture lives in `tests/corpus/`. Validating them through the
// compiler+interp pipeline guarantees parser/eval/propagate/transform/FIR
// all stay aligned with the documented RAD contract. Where the gradient
// has a closed form we also assert the numeric value on frame 0.

#[test]
fn corpus_rad_basic_compiles_and_emits_two_lanes() {
    let outs = run_interp_corpus("rad_basic", 2);
    assert_eq!(outs.len(), 2, "rad_basic = [primal, gradient]");
}

#[test]
fn corpus_rad_product_multi_seed_emits_expected_gradients() {
    // process = rad(a*b, (a, b)) at a=1, b=2 → [a*b, b, a] = [2, 2, 1]
    let outs = run_interp_corpus("rad_product_multi_seed", 1);
    assert_eq!(outs.len(), 3);
    assert_close(outs[0][0], 2.0, 1.0e-5, "primal a*b");
    assert_close(outs[1][0], 2.0, 1.0e-5, "d/da (a*b) = b");
    assert_close(outs[2][0], 1.0, 1.0e-5, "d/db (a*b) = a");
}

#[test]
fn corpus_rad_trig_composition_compiles_with_three_outputs() {
    // process = rad(sin(a*b), (a, b)) → 3 outputs.
    let outs = run_interp_corpus("rad_trig_composition", 1);
    assert_eq!(outs.len(), 3);
}

#[test]
fn corpus_rad_absent_seed_produces_zero_gradient_for_unreachable_seed() {
    // process = rad(sin(x), y) → [sin(x), 0.0]
    let outs = run_interp_corpus("rad_absent_seed", 2);
    assert_eq!(outs.len(), 2);
    for (frame, sample) in outs[1].iter().copied().enumerate().take(2) {
        assert_close(
            sample,
            0.0,
            1.0e-6,
            &format!("absent seed gradient frame {frame}"),
        );
    }
}

#[test]
fn corpus_rad_repeated_seed_duplicates_gradient_lane_verbatim() {
    // process = rad(a*b, (a, a)) → both gradient lanes equal d/da (a*b) = b.
    let outs = run_interp_corpus("rad_repeated_seed", 1);
    assert_eq!(outs.len(), 3);
    assert_close(
        outs[1][0],
        outs[2][0],
        1.0e-6,
        "repeated-seed lanes must alias",
    );
    assert_close(outs[1][0], 0.7, 1.0e-5, "d/da (a*b) at b=0.7");
}

#[test]
fn corpus_rad_multi_output_sum_cotangent_emits_expected_gradients() {
    // process = rad((a*b, sin(a)), (a, b)) at a=0.4, b=0.3
    // primals  = [a*b, sin(a)]                  ≈ [0.12, 0.3894]
    // grad/da  = b + cos(a)                     ≈ 0.3 + cos(0.4) ≈ 1.2211
    // grad/db  = a                              = 0.4
    let outs = run_interp_corpus("rad_multi_output_sum_cotangent", 1);
    assert_eq!(outs.len(), 4);
    assert_close(outs[0][0], 0.4 * 0.3, 5.0e-5, "primal a*b");
    let sin_a = (0.4_f32).sin();
    assert_close(outs[1][0], sin_a, 5.0e-5, "primal sin(a)");
    let cos_a = (0.4_f32).cos();
    assert_close(outs[2][0], 0.3 + cos_a, 5.0e-5, "d/da sum = b + cos(a)");
    assert_close(outs[3][0], 0.4, 5.0e-5, "d/db sum = a");
}

#[test]
fn corpus_rad_rdtbl_index_basic_emits_two_outputs() {
    // process = rad(rdtable(waveform{0,1,4,9,16,25,36,49}, k), k)
    // The slope is the symmetric finite difference. We just confirm the
    // arity here; numeric parity vs FAD is exercised in
    // `rad_vs_fad_parity_on_readonly_table_index`.
    let outs = run_interp_corpus("rad_rdtbl_index_basic", 2);
    assert_eq!(outs.len(), 2);
}

#[test]
fn corpus_err_rad_zero_body_surfaces_rad_body_arity_diagnostic() {
    let path = corpus_path("err_rad_zero_body.dsp");
    let source = std::fs::read_to_string(&path).expect("read err_rad_zero_body fixture");
    let compiler = Compiler::new();
    let err = compiler
        .compile_source_to_signals("err_rad_zero_body.dsp", &source)
        .expect_err("zero-output body must fail at propagate stage");
    let diagnostics = err.diagnostics().expect("diagnostics on rad body arity");
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|d| d.message.contains("rad body")),
        "diagnostic must name rad body arity: {diagnostics:?}"
    );
}

#[test]
fn corpus_err_rad_zero_seed_surfaces_rad_seed_arity_diagnostic() {
    let path = corpus_path("err_rad_zero_seed.dsp");
    let source = std::fs::read_to_string(&path).expect("read err_rad_zero_seed fixture");
    let compiler = Compiler::new();
    let err = compiler
        .compile_source_to_signals("err_rad_zero_seed.dsp", &source)
        .expect_err("zero-output seeds must fail at propagate stage");
    let diagnostics = err.diagnostics().expect("diagnostics on rad seed arity");
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|d| d.message.contains("rad seeds")),
        "diagnostic must name rad seeds arity: {diagnostics:?}"
    );
}

// -----------------------------------------------------------------------
// Nested AD combinations
// -----------------------------------------------------------------------
//
// FAD and RAD have the same two-child surface, so they can be nested in
// either order. The tests here pin two contracts:
//
// 1. The output bundle layouts compose multiplicatively (FAD) or
//    additively (RAD) as documented in `docs/fad-...md` and
//    `docs/rad-note-en.md`.
// 2. Second-order derivatives computed two different ways agree
//    numerically. Specifically:
//    - `fad(rad(f, s), s)` — outer FAD over RAD: the third lane is the
//      second derivative `f''(s)`.
//    - `rad(fad(f, s), s)` — outer RAD over FAD: the only gradient lane
//      is `d/ds (f + f')(s) = f'(s) + f''(s)` (sum cotangent over the
//      two-output FAD bundle).
//
// Both rely on the feed-forward subset only; nested temporal cases are
// covered by the temporal-rejection tests further up.

#[test]
fn nested_fad_rad_on_quadratic_matches_second_derivative() {
    // f(x) = x*x  ⇒  f'(x) = 2x, f''(x) = 2
    // inner rad(x*x, x)         → [x*x,  2x]
    // outer fad([x*x, 2x], x)   → [x*x, 2x, 2x, 2]                 (4 lanes)
    let source = r#"
x = hslider("x", 1.5, -2.0, 2.0, 0.001);
process = fad(rad(x*x, x), x);
"#;
    let outs = run_interp_temp_source("nested-fad-rad-quadratic", source, 2);
    assert_eq!(
        outs.len(),
        4,
        "fad(rad(f, s), s) layout = primal+tangent for each of rad's 2 outputs"
    );
    let x = 1.5_f32;
    assert_close(outs[0][0], x * x, 1.0e-5, "primal x*x");
    assert_close(outs[1][0], 2.0 * x, 1.0e-5, "tangent of x*x w.r.t. x");
    assert_close(outs[2][0], 2.0 * x, 1.0e-5, "rad first-order primal");
    assert_close(outs[3][0], 2.0, 1.0e-5, "second derivative f''(x) = 2");
}

#[test]
fn nested_rad_fad_on_quadratic_matches_first_plus_second_derivative() {
    // inner fad(x*x, x)         → [x*x, 2x]
    // outer rad([x*x, 2x], x)   → [x*x, 2x, d/dx(x*x + 2x) = 2x + 2]
    let source = r#"
x = hslider("x", 1.5, -2.0, 2.0, 0.001);
process = rad(fad(x*x, x), x);
"#;
    let outs = run_interp_temp_source("nested-rad-fad-quadratic", source, 2);
    assert_eq!(
        outs.len(),
        3,
        "rad(fad(f, s), s) layout = [primals…, gradient(s)]"
    );
    let x = 1.5_f32;
    assert_close(outs[0][0], x * x, 1.0e-5, "fad primal x*x");
    assert_close(outs[1][0], 2.0 * x, 1.0e-5, "fad tangent 2x");
    assert_close(
        outs[2][0],
        2.0 * x + 2.0,
        1.0e-5,
        "rad sum-cotangent gradient = f'(x) + f''(x)",
    );
}

#[test]
fn nested_fad_rad_on_trig_matches_second_derivative_via_finite_difference() {
    // f(x) = sin(x*x). Inner rad gives [sin(x*x), 2x*cos(x*x)]; outer
    // fad against x gives a 4-output bundle whose last lane is f''(x).
    // We compare that lane against a central finite difference on f' to
    // catch any second-order index-arithmetic regression.
    fn outer_source(x: f32) -> String {
        format!(
            r#"
x = hslider("x", {x}, -2.0, 2.0, 0.001);
process = fad(rad(sin(x*x), x), x);
"#
        )
    }
    fn inner_grad_source(x: f32) -> String {
        // Just rad(sin(x*x), x): first-order gradient as a primal.
        format!(
            r#"
x = hslider("x", {x}, -2.0, 2.0, 0.001);
process = rad(sin(x*x), x);
"#
        )
    }
    let base = 0.7_f32;
    let eps = 1.0e-3_f32;
    let outer = run_interp_temp_source("nested-fad-rad-trig-outer", &outer_source(base), 2);
    let grad_plus = run_interp_temp_source(
        "nested-fad-rad-trig-grad-plus",
        &inner_grad_source(base + eps),
        2,
    );
    let grad_minus = run_interp_temp_source(
        "nested-fad-rad-trig-grad-minus",
        &inner_grad_source(base - eps),
        2,
    );
    // outer layout: [f, df/dx, g = f', dg/dx = f''] across 4 lanes.
    assert_eq!(outer.len(), 4);
    // Frame-0 second-derivative lane vs. central difference on the
    // first-order gradient (lane 1 of inner rad output = the 2nd output).
    let expected_second = (grad_plus[1][0] - grad_minus[1][0]) / (2.0 * eps);
    assert_close(
        outer[3][0],
        expected_second,
        2.0e-3,
        "f''(x) from fad(rad(...)) vs. central diff of rad gradient",
    );
}

#[test]
fn nested_rad_fad_multi_seed_routes_implicit_cotangent_through_inner_lanes() {
    // f(x, y) = x*y ; inner fad against (x, y) → [x*y, y, x] (3 outputs)
    // Outer rad against (x, y) sums the inner lanes:
    //   primals  = [x*y, y, x]
    //   d/dx sum = d/dx (x*y + y + x) = y + 1
    //   d/dy sum = d/dy (x*y + y + x) = x + 1
    // Final bundle: [x*y, y, x, y+1, x+1] (5 outputs).
    let source = r#"
x = hslider("x", 0.6, -2.0, 2.0, 0.001);
y = hslider("y", 0.4, -2.0, 2.0, 0.001);
process = rad(fad(x*y, (x, y)), (x, y));
"#;
    let outs = run_interp_temp_source("nested-rad-fad-multi-seed", source, 1);
    assert_eq!(
        outs.len(),
        5,
        "outer rad bundle = [primals (3), grad/dx, grad/dy]"
    );
    let x = 0.6_f32;
    let y = 0.4_f32;
    assert_close(outs[0][0], x * y, 1.0e-5, "fad primal x*y");
    assert_close(outs[1][0], y, 1.0e-5, "fad tangent w.r.t. x = y");
    assert_close(outs[2][0], x, 1.0e-5, "fad tangent w.r.t. y = x");
    assert_close(outs[3][0], y + 1.0, 1.0e-5, "rad d/dx sum = y + 1");
    assert_close(outs[4][0], x + 1.0, 1.0e-5, "rad d/dy sum = x + 1");
}

// Corpus-driven mixed AD tests. The inline tests above already cover
// the same shapes; these reach the fixtures in tests/corpus/ so the
// source-level surface (parser + eval + propagate + transform + interp)
// is exercised end-to-end against committed DSP files.

#[test]
fn corpus_fad_rad_quadratic_emits_second_derivative_lane() {
    // tests/corpus/fad_rad_quadratic.dsp
    //   process = fad(rad(x*x, x), x);  x = 1.5
    // Expected lanes: [x*x, 2x, 2x, 2].
    let outs = run_interp_corpus("fad_rad_quadratic", 1);
    assert_eq!(outs.len(), 4);
    let x = 1.5_f32;
    assert_close(outs[0][0], x * x, 1.0e-5, "primal x*x");
    assert_close(outs[1][0], 2.0 * x, 1.0e-5, "fad tangent 2x");
    assert_close(outs[2][0], 2.0 * x, 1.0e-5, "rad first-order primal");
    assert_close(outs[3][0], 2.0, 1.0e-5, "f''(x) = 2");
}

#[test]
fn corpus_rad_fad_quadratic_sums_first_and_second_derivative() {
    // tests/corpus/rad_fad_quadratic.dsp
    //   process = rad(fad(x*x, x), x);  x = 1.5
    // Expected lanes: [x*x, 2x, 2x + 2].
    let outs = run_interp_corpus("rad_fad_quadratic", 1);
    assert_eq!(outs.len(), 3);
    let x = 1.5_f32;
    assert_close(outs[0][0], x * x, 1.0e-5, "fad primal x*x");
    assert_close(outs[1][0], 2.0 * x, 1.0e-5, "fad tangent 2x");
    assert_close(
        outs[2][0],
        2.0 * x + 2.0,
        1.0e-5,
        "rad sum-cotangent gradient = f'(x) + f''(x)",
    );
}

#[test]
fn corpus_fad_rad_trig_second_derivative_compiles_and_has_four_lanes() {
    // tests/corpus/fad_rad_trig_second_derivative.dsp
    //   process = fad(rad(sin(x*x), x), x);
    // We just check arity here; numeric parity for the second-derivative
    // lane against a central finite difference is exercised by the
    // inline `nested_fad_rad_on_trig_...` test, which can perturb the
    // seed without needing a separate fixture.
    let outs = run_interp_corpus("fad_rad_trig_second_derivative", 1);
    assert_eq!(outs.len(), 4);
}

#[test]
fn corpus_rad_fad_multi_seed_routes_implicit_cotangent_through_inner_lanes() {
    // tests/corpus/rad_fad_multi_seed.dsp
    //   process = rad(fad(x*y, (x, y)), (x, y));  x = 0.6, y = 0.4
    // Expected lanes: [x*y, y, x, y+1, x+1].
    let outs = run_interp_corpus("rad_fad_multi_seed", 1);
    assert_eq!(outs.len(), 5);
    let x = 0.6_f32;
    let y = 0.4_f32;
    assert_close(outs[0][0], x * y, 1.0e-5, "fad primal x*y");
    assert_close(outs[1][0], y, 1.0e-5, "fad tangent w.r.t. x = y");
    assert_close(outs[2][0], x, 1.0e-5, "fad tangent w.r.t. y = x");
    assert_close(outs[3][0], y + 1.0, 1.0e-5, "rad d/dx sum = y + 1");
    assert_close(outs[4][0], x + 1.0, 1.0e-5, "rad d/dy sum = x + 1");
}

#[test]
fn corpus_err_fad_rad_temporal_surfaces_rad_diagnostic() {
    // tests/corpus/err_fad_rad_temporal.dsp
    //   process = fad(rad(x', x), x);
    // The inner RAD must reject the delay even when wrapped in FAD.
    let path = corpus_path("err_fad_rad_temporal.dsp");
    let source = std::fs::read_to_string(&path).expect("read err_fad_rad_temporal fixture");
    let compiler = Compiler::new();
    let err = compiler
        .compile_source_to_signals("err_fad_rad_temporal.dsp", &source)
        .expect_err("inner rad over delay1 must fail when wrapped in fad");
    let diagnostics = err
        .diagnostics()
        .expect("propagate error should expose diagnostics");
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|d| d.message.contains("rad")),
        "fad-wrapped rad temporal diagnostic must mention `rad`: {diagnostics:?}"
    );
    // Confirm it specifically came from the temporal kind, not a
    // generic UnsupportedBox.
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|d| d.message.contains("delay-or-prefix")
                || d.notes.iter().any(|n| n.contains("non-causal"))),
        "fad-wrapped rad temporal diagnostic must surface the temporal kind: {diagnostics:?}"
    );
}

#[test]
fn nested_rad_in_fad_temporal_inner_still_rejects_with_diagnostic() {
    // Sanity check: the temporal rejection is preserved when RAD is the
    // inner pass. Compiling `fad(rad(x', x), x)` must still fail with the
    // RAD temporal diagnostic, not silently produce a misleading double
    // gradient.
    use compiler::Compiler;
    let source = r#"
x = hslider("x", 0.0, -1.0, 1.0, 0.01);
process = fad(rad(x', x), x);
"#;
    let compiler = Compiler::new();
    let err = compiler
        .compile_source_to_signals("nested-rad-in-fad-temporal.dsp", source)
        .expect_err("inner rad over delay1 must fail even when wrapped in fad");
    let diagnostics = err
        .diagnostics()
        .expect("propagate error should expose diagnostics");
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|d| d.message.contains("rad")),
        "nested rad-in-fad temporal diagnostic must mention `rad`: {diagnostics:?}"
    );
}
