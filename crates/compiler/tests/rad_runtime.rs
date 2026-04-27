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
use std::sync::atomic::{AtomicUsize, Ordering};

use codegen::backends::interp::{FbcDspInstance, InterpOptions, read_fbc};
use compiler::{Compiler, SignalFirLane};

static NEXT_TEMP_DSP_ID: AtomicUsize = AtomicUsize::new(0);

fn run_interp_temp_source(stem: &str, source: &str, frame_count: usize) -> Vec<Vec<f32>> {
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
    let mut outputs = vec![vec![0.0_f32; frame_count]; num_outputs];
    let mut output_slices: Vec<&mut [f32]> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
    instance
        .try_compute(frame_count as i32, &[], &mut output_slices)
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
    assert_eq!(base_seeds.len(), epsilons.len(), "seed/epsilon arity must match");
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
    let primal_plus =
        run_interp_temp_source("rad-repeated-seed-plus", &primal_source(&[base[0] + eps[0]]), frame_count);
    let primal_minus =
        run_interp_temp_source("rad-repeated-seed-minus", &primal_source(&[base[0] - eps[0]]), frame_count);
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
    for frame in 0..4 {
        assert_close(outs[1][frame], 0.0, 1.0e-6, &format!("absent-seed frame {frame}"));
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
