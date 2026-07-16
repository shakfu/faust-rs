//! Native SIMD evidence gate for signal-level lockstep vectorization.
//!
//! Section 8 of the vector-mode port plan deliberately lowers each certified
//! bundle as one C++ sample loop containing unchanged scalar lane expressions.
//! This workflow first requires [`compiler::VectorPipelineStatus::Certified`]
//! and [`compiler::VectorEffectiveMode::CertifiedVector`], then checks the
//! final part of that adapted contract: Clang at
//! `-O3`, without fast-math or FMA contraction, must turn the representative
//! four-lane recursive expressions into LLVM vector floating-point operations.
//! It complements the FIR certificate and bit-exact interpreter tests; it does
//! not make a target-specific instruction-set assumption and cannot accept
//! SIMD emitted from a scalar fallback module.

use super::*;

const SIMD_CASES: [(&str, usize); 3] = [
    ("tests/corpus/vector_lockstep_simd_quad.dsp", 10),
    ("tests/corpus/vector_lockstep_mixed_reduce.dsp", 10),
    ("tests/corpus/vector_lockstep_mixed_branch.dsp", 10),
];

fn require_checked_vector_status(
    relative: &str,
    status: compiler::VectorPipelineStatus,
    effective: compiler::VectorEffectiveMode,
    detail: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    if status != compiler::VectorPipelineStatus::Certified
        || effective != compiler::VectorEffectiveMode::CertifiedVector
    {
        return Err(format!(
            "{relative}: lockstep SIMD evidence requires checked vector FIR; status={status:?}, effective={effective:?}, detail={}",
            detail.unwrap_or("none")
        )
        .into());
    }
    if let Some(detail) = detail {
        return Err(format!(
            "{relative}: certified vector FIR unexpectedly retained fallback detail: {detail}"
        )
        .into());
    }
    Ok(())
}

/// Compiles the complex lockstep corpus through vector C++, then asks Clang for
/// optimized LLVM IR and requires profitable four-wide floating-point SLP.
pub(crate) fn lockstep_simd_check(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(option) = args.next() {
        return Err(format!("unknown lockstep-simd-check option: {option}").into());
    }

    let root = workspace_root();
    let driver = root.join("tests/bench/faust_cpp_compute_driver.cpp");
    let clang = std::env::var_os("CLANGXX").unwrap_or_else(|| "clang++".into());
    let temp_root =
        std::env::temp_dir().join(format!("faust-rs-lockstep-simd-{}", std::process::id()));
    fs::create_dir_all(&temp_root)?;

    let result = (|| {
        for (relative, minimum_vector_ops) in SIMD_CASES {
            let source_path = root.join(relative);
            let source = fs::read_to_string(&source_path)?;
            let stem = source_path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("invalid corpus file name: {}", source_path.display()))?;
            let fir = compiler::Compiler::new()
                .with_compute_mode(compiler::ComputeMode::Vector {
                    vec_size: 32,
                    loop_variant: 1,
                })
                .compile_source_to_fir_with_lane(
                    relative,
                    &source,
                    compiler::SignalFirLane::TransformFastLane,
                )?;
            require_checked_vector_status(
                relative,
                fir.vector_pipeline_status,
                fir.vector_effective_mode,
                fir.vector_pipeline_detail.as_deref(),
            )?;
            let cpp = codegen::backends::cpp::generate_cpp_module(
                &fir.store,
                fir.module,
                &codegen::backends::cpp::CppOptions::default(),
            )?;
            let cpp_path = temp_root.join(format!("{stem}.cpp"));
            let llvm_path = temp_root.join(format!("{stem}.ll"));
            fs::write(&cpp_path, cpp)?;

            // Preprocessor include paths use forward slashes on every host,
            // including Windows Clang, so the macro remains one C++ string.
            let include_path = cpp_path.to_string_lossy().replace('\\', "/");
            let output = Command::new(&clang)
                .arg("-std=c++17")
                .arg("-O3")
                .arg("-ffp-contract=off")
                .arg(format!("-DFAUST_TEST_DSP=\"{include_path}\""))
                .arg("-S")
                .arg("-emit-llvm")
                .arg(&driver)
                .arg("-o")
                .arg(&llvm_path)
                .output()
                .map_err(|error| {
                    format!(
                        "failed to execute {} for {relative}: {error}",
                        Path::new(&clang).display()
                    )
                })?;
            if !output.status.success() {
                return Err(format!(
                    "{} failed for {relative}:\n{}",
                    Path::new(&clang).display(),
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }

            let llvm = fs::read_to_string(&llvm_path)?;
            let vector_ops = llvm
                .lines()
                .filter(|line| {
                    ["fadd <4 x float>", "fsub <4 x float>", "fmul <4 x float>"]
                        .iter()
                        .any(|operation| line.contains(operation))
                })
                .count();
            if vector_ops < minimum_vector_ops {
                return Err(format!(
                    "{relative}: expected at least {minimum_vector_ops} four-wide LLVM floating-point operations, found {vector_ops}"
                )
                .into());
            }
            println!("SIMD {relative}: {vector_ops} four-wide LLVM FP operations");
        }
        Ok(())
    })();

    let _ = fs::remove_dir_all(&temp_root);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_fallback_cannot_count_as_lockstep_simd_evidence() {
        let error = require_checked_vector_status(
            "fallback.dsp",
            compiler::VectorPipelineStatus::Fallback(
                compiler::VectorFallbackReason::EventCertificate,
            ),
            compiler::VectorEffectiveMode::Scalar,
            Some("bounded event table"),
        )
        .expect_err("scalar fallback must be rejected");
        assert!(error.to_string().contains("requires checked vector FIR"));
    }

    #[test]
    fn certified_vector_without_fallback_detail_is_accepted() {
        require_checked_vector_status(
            "certified.dsp",
            compiler::VectorPipelineStatus::Certified,
            compiler::VectorEffectiveMode::CertifiedVector,
            None,
        )
        .expect("checked vector status");
    }
}
