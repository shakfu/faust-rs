use std::ffi::{CStr, CString};

use faust_cranelift::factory::{
    createCCraneliftDSPFactoryFromFile, deleteCCraneliftDSPFactory,
    writeCCraneliftDSPFactoryToBitcodeFile,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect(
        "usage: cargo run -p cranelift-ffi --example export_clif -- <input.dsp> <output.clif>",
    );
    let output = args.next().expect(
        "usage: cargo run -p cranelift-ffi --example export_clif -- <input.dsp> <output.clif>",
    );

    let input_c = CString::new(input.as_bytes()).expect("input path CString");
    let output_c = CString::new(output.as_bytes()).expect("output path CString");
    let mut err = [0_i8; 4096];

    let factory = unsafe {
        createCCraneliftDSPFactoryFromFile(
            input_c.as_ptr(),
            0,
            std::ptr::null(),
            err.as_mut_ptr(),
            1,
        )
    };
    if factory.is_null() {
        let msg = unsafe { CStr::from_ptr(err.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        panic!("createCCraneliftDSPFactoryFromFile failed: {msg}");
    }

    let ok = unsafe { writeCCraneliftDSPFactoryToBitcodeFile(factory, output_c.as_ptr()) };
    if !ok {
        unsafe {
            let _ = deleteCCraneliftDSPFactory(factory);
        }
        panic!("writeCCraneliftDSPFactoryToBitcodeFile failed");
    }

    unsafe {
        let _ = deleteCCraneliftDSPFactory(factory);
    }

    println!("{output}");
}
