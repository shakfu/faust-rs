//! Dumps C++ generated from a synthetic FIR module:
//! phasor-based sine oscillator with frequency/gain sliders.

use codegen::backends::cpp::{CppOptions, generate_cpp_module};
use codegen::fixtures::build_sine_phasor_test_module;

fn main() {
    let (store, module) = build_sine_phasor_test_module();
    let cpp = generate_cpp_module(&store, module, &CppOptions::default())
        .expect("synthetic FIR module should generate C++");
    print!("{cpp}");
}
