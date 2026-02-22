// build.rs for interp-ffi
//
// cbindgen does not yet support Rust edition 2024's `#[unsafe(no_mangle)]`
// attribute, so automatic C header generation is disabled.
// The C header `include/interpreter-dsp-c.h` is maintained manually.
//
// This build script only ensures the include directory exists.

fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let include_dir = format!("{crate_dir}/include");

    std::fs::create_dir_all(&include_dir).expect("Failed to create include directory");

    // Rerun if any source file changes.
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=include/");
}
