//! Integration tests for zita-related signal/FIR pipeline regressions.

use compiler::Compiler;
use std::path::PathBuf;
use std::thread;
use transform::signal_fir::{RealType, SignalFirOptions, compile_signals_to_fir_fastlane_with_ui};
use transform::signal_prepare::prepare_signals_for_fir;
use ui::UiProgram;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("zita")
}

fn fixture_dsp(file: &str) -> PathBuf {
    fixture_root().join(file)
}

fn run_with_large_stack<T>(f: impl FnOnce() -> T + Send + 'static) -> T
where
    T: Send + 'static,
{
    thread::Builder::new()
        .name("zita-pipeline".to_owned())
        .stack_size(64 * 1024 * 1024)
        .spawn(f)
        .expect("spawn zita worker thread")
        .join()
        .expect("zita worker thread should finish")
}

#[test]
fn zita_min_preparation_preserves_multi_output_recursion_groups() {
    run_with_large_stack(|| {
        let compiler = Compiler::new();
        let path = fixture_dsp("zita_min.dsp");
        let output = compiler
            .compile_file_default_to_signals(&path)
            .expect("signal compilation should succeed");
        let prepared = prepare_signals_for_fir(
            &output.parse.state.arena,
            &output.signals,
            &UiProgram::empty(),
        )
        .expect("signal preparation should succeed for zita_min");
        assert_eq!(prepared.outputs.len(), 2);
    });
}

#[test]
fn zita_min_fastlane_fir_lowering_completes() {
    run_with_large_stack(|| {
        let compiler = Compiler::new();
        let path = fixture_dsp("zita_min.dsp");
        let output = compiler
            .compile_file_default_to_signals(&path)
            .expect("signal compilation should succeed");
        let fir = compile_signals_to_fir_fastlane_with_ui(
            &output.parse.state.arena,
            &output.signals,
            output.process_arity.inputs,
            output.process_arity.outputs,
            &UiProgram::empty(),
            &SignalFirOptions {
                module_name: "mydsp".to_owned(),
                strict_mode: true,
                real_type: RealType::Float32,
            },
        )
        .expect("fast-lane FIR lowering should succeed for zita_min");
        assert!(fir.module.as_u32() > 0);
    });
}
