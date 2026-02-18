//! Dumps C generated from a synthetic FIR module:
//! phasor-based sine oscillator with frequency/gain sliders.

use codegen::backends::c::{COptions, generate_c_module};
use codegen::fixtures::build_sine_phasor_test_module;

fn main() {
    let (store, module) = build_sine_phasor_test_module();
    let c = generate_c_module(
        &store,
        module,
        &COptions {
            num_outputs: 1,
            ..COptions::default()
        },
    )
    .expect("synthetic FIR module should generate C");
    print!("{c}");
}
