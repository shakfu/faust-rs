//! Integration tests for cpp_fir_sine_phasor.rs.

use codegen::backends::cpp::{CppOptions, generate_cpp_module};
use codegen::fixtures::build_sine_phasor_test_module;

#[test]
fn fir_module_sine_phasor_with_freq_and_gain_sliders_generates_cpp() {
    let (store, module) = build_sine_phasor_test_module();
    let cpp = generate_cpp_module(&store, module, &CppOptions::default())
        .expect("synthetic FIR module should generate C++");

    assert!(cpp.contains("class mydsp : public dsp"));
    assert!(cpp.contains("FAUSTFLOAT fFreq = 440.0;"));
    assert!(cpp.contains("FAUSTFLOAT fGain = 0.2;"));
    assert!(cpp.contains("double fPhase = 0.0;"));
    assert!(cpp.contains("void buildUserInterface(UI* ui_interface)"));
    assert!(cpp.contains(
        "ui_interface->addHorizontalSlider(\"freq\", &fFreq, 440.0, 20.0, 3000.0, 1.0);"
    ));
    assert!(
        cpp.contains("ui_interface->addHorizontalSlider(\"gain\", &fGain, 0.2, 0.0, 1.0, 0.001);")
    );
    assert!(cpp.contains(
        "void compute(int count, FAUSTFLOAT** RESTRICT inputs, FAUSTFLOAT** RESTRICT outputs)"
    ));
    assert!(cpp.contains("fPhase = "));
    assert!(cpp.contains("(fFreq / 48000.0)"));
    assert!(cpp.contains("for (int i0 = 0; i0 < count; i0 = i0 + 1) {"));
    assert!(cpp.contains("output0[i0] = FAUSTFLOAT("));
    assert!(cpp.contains(" ? "));
    assert!(cpp.contains("std::sin"));
    assert!(cpp.contains("(fGain * std::sin"));
}
