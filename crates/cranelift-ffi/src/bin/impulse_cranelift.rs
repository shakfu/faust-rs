//! Cranelift impulse runner — the Cranelift-backend counterpart of
//! `impulse-runner` (interpreter) for the `tests/impulse-tests` harness.
//!
//! It JIT-compiles one DSP through the Cranelift backend C-API and runs the
//! scalar impulse pass (SR 44100, block 64, impulse on frame 0), emitting the
//! reference `.ir` text format with the same `normalize()` zero-clamp.
//!
//! Limitations (documented as known failures in the harness):
//! - the Cranelift backend buffer type is `f32` only, so output is compared
//!   against an `f32` (`-single`) reference;
//! - button/checkbox zones are not driven yet (no zone-setter in the C-API
//!   surface used here), so button-gated DSPs diverge.
//!
//! Usage: `impulse_cranelift <file.dsp> [-n <frames>] [-I <dir>]...`
//! (`-single`/`-double` are accepted and ignored: the backend is f32-only.)

use std::ffi::{CStr, CString, c_char, c_int};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;

use cranelift_ffi::factory::{createCCraneliftDSPFactoryFromFile, deleteCCraneliftDSPFactory};
use cranelift_ffi::instance::{
    computeCCraneliftDSPInstance, createCCraneliftDSPInstance, deleteCCraneliftDSPInstance,
    getNumInputsCCraneliftDSPInstance, getNumOutputsCCraneliftDSPInstance,
    initCCraneliftDSPInstance,
};
use cranelift_ffi::types::FaustFloat;

const SAMPLE_RATE: i32 = 44100;
const BLOCK_SIZE: usize = 64;
const DEFAULT_FRAMES: usize = 15000;

fn main() -> ExitCode {
    // Cranelift JIT compilation plus the faust-rs front-end can recurse deeply;
    // run on a large stack like the crate's differential tests do.
    let result = thread::Builder::new()
        .name("impulse-cranelift".to_owned())
        .stack_size(256 * 1024 * 1024)
        .spawn(run)
        .expect("spawn worker thread")
        .join()
        .expect("join worker thread");
    match result {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("impulse-cranelift: {err}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    dsp: String,
    frames: usize,
    double: bool,
    import_dirs: Vec<String>,
}

fn parse_args() -> Result<Options, String> {
    let mut dsp: Option<String> = None;
    let mut frames = DEFAULT_FRAMES;
    let mut double = false;
    let mut import_dirs = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-double" => double = true,
            "-single" => double = false,
            "-n" => {
                frames = args
                    .next()
                    .ok_or("missing value after -n")?
                    .parse::<usize>()
                    .map_err(|e| format!("bad -n value: {e}"))?;
            }
            "-I" => import_dirs.push(args.next().ok_or("missing value after -I")?),
            other if other.starts_with('-') => return Err(format!("unknown option: {other}")),
            other => {
                if dsp.is_some() {
                    return Err(format!("unexpected extra argument: {other}"));
                }
                dsp = Some(other.to_owned());
            }
        }
    }
    Ok(Options {
        dsp: dsp.ok_or("missing <file.dsp> argument")?,
        frames,
        double,
        import_dirs,
    })
}

fn run() -> Result<String, String> {
    let options = parse_args()?;

    // Search paths: explicit -I, then the DSP's own dir, then system libs.
    let mut search = options.import_dirs.clone();
    if let Some(parent) = PathBuf::from(&options.dsp).parent()
        && !parent.as_os_str().is_empty()
    {
        search.push(parent.to_string_lossy().into_owned());
    }
    if PathBuf::from("/usr/local/share/faust").is_dir() {
        search.push("/usr/local/share/faust".to_owned());
    }

    let mut argv_storage: Vec<CString> = Vec::new();
    if options.double {
        argv_storage.push(CString::new("-double").map_err(|e| e.to_string())?);
    }
    for dir in &search {
        argv_storage.push(CString::new("-I").map_err(|e| e.to_string())?);
        argv_storage.push(CString::new(dir.as_str()).map_err(|e| e.to_string())?);
    }
    let argv_ptrs: Vec<*const c_char> = argv_storage.iter().map(|s| s.as_ptr()).collect();
    let c_path = CString::new(options.dsp.as_str()).map_err(|e| e.to_string())?;
    let mut err = [0_i8; 4096];

    let factory = unsafe {
        createCCraneliftDSPFactoryFromFile(
            c_path.as_ptr(),
            c_int::try_from(argv_ptrs.len()).map_err(|_| "too many -I args")?,
            if argv_ptrs.is_empty() {
                std::ptr::null()
            } else {
                argv_ptrs.as_ptr()
            },
            err.as_mut_ptr(),
            1,
        )
    };
    if factory.is_null() {
        return Err(format!(
            "Cranelift factory creation failed: {}",
            unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy()
        ));
    }

    // The factory's concrete type is private to the crate; keep it inferred by
    // doing the run inline, and free it before returning.
    let frames = options.frames;
    let dsp = unsafe { createCCraneliftDSPInstance(factory) };
    if dsp.is_null() {
        unsafe {
            let _ = deleteCCraneliftDSPFactory(factory);
        }
        return Err("Cranelift instance creation failed".to_owned());
    }
    unsafe { initCCraneliftDSPInstance(dsp, SAMPLE_RATE) };

    let num_inputs = usize::try_from(unsafe { getNumInputsCCraneliftDSPInstance(dsp) })
        .map_err(|_| "negative input arity".to_string())?;
    let num_outputs = usize::try_from(unsafe { getNumOutputsCCraneliftDSPInstance(dsp) })
        .map_err(|_| "negative output arity".to_string())?;

    let mut out = String::new();
    out.push_str(&format!("number_of_inputs  : {num_inputs:3}\n"));
    out.push_str(&format!("number_of_outputs : {num_outputs:3}\n"));
    out.push_str(&format!("number_of_frames  : {frames:6}\n"));

    // The JIT reads/writes I/O buffers at the compiled width (`f64` under
    // `-double`, `f32` otherwise). `computeCCraneliftDSPInstance` only forwards
    // the pointers, so the buffer element type is the caller's responsibility.
    // The element type differs but the loop is identical, hence the macro.
    macro_rules! run_pass {
        ($elem:ty) => {{
            let mut in_buffer = vec![vec![<$elem>::default(); BLOCK_SIZE]; num_inputs];
            let mut out_buffer = vec![vec![<$elem>::default(); BLOCK_SIZE]; num_outputs];
            let mut written = 0usize;
            while written < frames {
                let n = BLOCK_SIZE.min(frames - written);
                for channel in &mut in_buffer {
                    for sample in channel.iter_mut() {
                        *sample = <$elem>::default();
                    }
                    if written == 0 && !channel.is_empty() {
                        channel[0] = 1.0;
                    }
                }
                let mut in_ptrs: Vec<*mut FaustFloat> = in_buffer
                    .iter_mut()
                    .map(|c| c.as_mut_ptr().cast::<FaustFloat>())
                    .collect();
                let mut out_ptrs: Vec<*mut FaustFloat> = out_buffer
                    .iter_mut()
                    .map(|c| c.as_mut_ptr().cast::<FaustFloat>())
                    .collect();
                unsafe {
                    computeCCraneliftDSPInstance(
                        dsp,
                        n as i32,
                        in_ptrs.as_mut_ptr(),
                        out_ptrs.as_mut_ptr(),
                    );
                }
                for j in 0..n {
                    out.push_str(&format!("{written:6} : "));
                    for channel in out_buffer.iter().take(num_outputs) {
                        let value = normalize(channel[j] as f64);
                        out.push_str(&format!(" {value:8.6}"));
                    }
                    out.push('\n');
                    written += 1;
                }
            }
        }};
    }

    if options.double {
        run_pass!(f64);
    } else {
        run_pass!(f32);
    }

    unsafe {
        deleteCCraneliftDSPInstance(dsp);
        let _ = deleteCCraneliftDSPFactory(factory);
    }
    Ok(out)
}

/// Zero-clamps tiny magnitudes exactly like `controlTools.h::normalize`.
fn normalize(value: f64) -> f64 {
    if value.is_nan() || value.is_infinite() {
        value
    } else if value.abs() < 0.000_001 {
        0.0
    } else {
        value
    }
}
