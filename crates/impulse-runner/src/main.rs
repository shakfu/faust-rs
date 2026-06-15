//! `impulse-runner` — faust-rs analogue of the C++ `tools/impulseinterp.cpp`.
//!
//! It compiles one DSP file through the faust-rs library to interpreter
//! bytecode and runs the **scalar impulse pass** of the reference impulse-test
//! protocol (see `controlTools.h::runDSP` in the C++ test suite):
//!
//! - sample rate 44100, block size 64 (`kFrames`),
//! - first frame of every input channel = 1.0 (impulse), all other inputs 0.0,
//! - every `button`/`checkbox` zone held at 1.0 during the first block then 0.0,
//! - output samples printed as `"%6d :  %8.6f ..."` after the same
//!   `normalize()` zero-clamp (|x| < 1e-6 → 0) the C++ harness applies.
//!
//! The faust-rs interpreter runtime has no polyphonic / MIDI / soundfile
//! wrapper, so this runner only reproduces the scalar pass (the first 15000
//! reference frames). The generated `.ir` is therefore compared against the
//! genuine 4-pass C++ reference with `filesCompare -part`, which compares only
//! the produced prefix — exactly how the C++ suite's own `Make.rust` tests a
//! scalar-only Rust architecture against the full reference.
//!
//! Usage:
//! ```text
//! impulse-runner <file.dsp> [-double] [-n <frames>] [-I <dir>]...
//! ```
//! The `.ir` text is written to stdout (the Makefile redirects it to a file).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use codegen::backends::interp::{
    FbcDspInstance, FbcOpcode, FbcReal, InterpOptions, generate_interp_module,
};
use compiler::{Compiler, FirVerifyOptions, RealType, SignalFirLane};
use fir::{FirId, FirStore};

/// Reference protocol constants (mirrors `controlTools.h`).
const SAMPLE_RATE: i32 = 44100;
const BLOCK_SIZE: usize = 64;
/// Default produced frame count: the scalar pass length of the C++ reference
/// (`nbsamples / 4` with `nbsamples == 60000`).
const DEFAULT_FRAMES: usize = 15000;

/// Parsed command-line options.
struct Options {
    dsp: PathBuf,
    double: bool,
    frames: usize,
    import_dirs: Vec<PathBuf>,
}

fn main() -> ExitCode {
    match real_main() {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("impulse-runner: {err}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<String, String> {
    let options = parse_args()?;
    let real_type = if options.double {
        RealType::Float64
    } else {
        RealType::Float32
    };

    let search_paths = resolve_search_paths(&options);

    let compiler = Compiler::new()
        .with_real_type(real_type)
        .with_fir_verify_options(FirVerifyOptions {
            enabled: true,
            strict: false,
        });

    let fir = compiler
        .compile_file_to_fir_with_lane(&options.dsp, &search_paths, SignalFirLane::TransformFastLane)
        .map_err(|e| format!("compilation failed for {}: {e}", options.dsp.display()))?;

    if options.double {
        run::<f64>(&fir.store, fir.module, options.frames)
    } else {
        run::<f32>(&fir.store, fir.module, options.frames)
    }
}

/// Parses argv into [`Options`], accepting the Faust-style flags the Makefile
/// passes through (`-double`, `-I <dir>`), plus the runner-specific `-n`.
fn parse_args() -> Result<Options, String> {
    let mut dsp: Option<PathBuf> = None;
    let mut double = false;
    let mut frames = DEFAULT_FRAMES;
    let mut import_dirs = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-double" => double = true,
            "-single" => double = false,
            "-n" => {
                let value = args.next().ok_or("missing value after -n")?;
                frames = value.parse::<usize>().map_err(|e| format!("bad -n value: {e}"))?;
            }
            "-I" => {
                let value = args.next().ok_or("missing value after -I")?;
                import_dirs.push(PathBuf::from(value));
            }
            // Accept and ignore other Faust options the Makefile may pass so the
            // runner can be a near drop-in for the C++ `impulseinterp` binary.
            other if other.starts_with('-') => {
                // Options taking an argument we do not model would desync parsing;
                // none are passed today, so reject unknown flags loudly instead.
                return Err(format!("unknown option: {other}"));
            }
            other => {
                if dsp.is_some() {
                    return Err(format!("unexpected extra argument: {other}"));
                }
                dsp = Some(PathBuf::from(other));
            }
        }
    }

    Ok(Options {
        dsp: dsp.ok_or("missing <file.dsp> argument")?,
        double,
        frames,
        import_dirs,
    })
}

/// Builds the import search path list: explicit `-I` dirs first, then the DSP's
/// own directory, then the system faust libraries when present.
fn resolve_search_paths(options: &Options) -> Vec<PathBuf> {
    let mut paths = options.import_dirs.clone();
    if let Some(parent) = options.dsp.parent()
        && !parent.as_os_str().is_empty()
    {
        paths.push(parent.to_path_buf());
    }
    let system_libs = PathBuf::from("/usr/local/share/faust");
    if system_libs.is_dir() {
        paths.push(system_libs);
    }
    paths
}

/// Runs the scalar impulse pass for one precision and renders the `.ir` text.
fn run<R: FbcReal>(store: &FirStore, module: FirId, frames: usize) -> Result<String, String> {
    let options = InterpOptions {
        opt_level: 0,
        module_name: None,
    };
    let mut factory = generate_interp_module::<R>(store, module, &options)
        .map_err(|e| format!("interp codegen failed: {e}"))?;
    let mut instance = FbcDspInstance::new(&mut factory);
    instance.init(SAMPLE_RATE);

    let num_inputs = usize::try_from(instance.get_num_inputs())
        .map_err(|_| "negative input arity".to_string())?;
    let num_outputs = usize::try_from(instance.get_num_outputs())
        .map_err(|_| "negative output arity".to_string())?;

    // Discover button / checkbox zones to drive like `FUI::setButtons`.
    let button_zones: Vec<i32> = instance
        .ui_instructions()
        .iter()
        .filter(|ui| matches!(ui.opcode, FbcOpcode::AddButton | FbcOpcode::AddCheckButton))
        .map(|ui| ui.offset)
        .collect();

    let mut out = String::new();
    out.push_str(&format!("number_of_inputs  : {num_inputs:3}\n"));
    out.push_str(&format!("number_of_outputs : {num_outputs:3}\n"));
    out.push_str(&format!("number_of_frames  : {frames:6}\n"));

    let mut in_buffer = vec![vec![R::default(); BLOCK_SIZE]; num_inputs];
    let mut out_buffer = vec![vec![R::default(); BLOCK_SIZE]; num_outputs];

    let zero = R::default();
    let one = R::from_f64(1.0);

    let mut written = 0usize;
    let mut cycle = 0usize;
    while written < frames {
        let n = BLOCK_SIZE.min(frames - written);

        // Impulse: first frame of every input channel is 1.0 on the very first
        // block, everything else is silence.
        for channel in &mut in_buffer {
            for sample in channel.iter_mut() {
                *sample = zero;
            }
            if written == 0 && !channel.is_empty() {
                channel[0] = one;
            }
        }

        // Buttons held high during the first block then released.
        let button_value = if cycle == 0 { one } else { zero };
        for &offset in &button_zones {
            instance.set_real_zone(offset, button_value);
        }

        let input_refs: Vec<&[R]> = in_buffer.iter().map(|c| &c[..n]).collect();
        let mut output_refs: Vec<&mut [R]> =
            out_buffer.iter_mut().map(|c| &mut c[..n]).collect();
        instance
            .try_compute(n as i32, &input_refs, &mut output_refs)
            .map_err(|e| format!("compute failed at frame {written}: {e}"))?;

        for j in 0..n {
            out.push_str(&format!("{written:6} : "));
            for channel in out_buffer.iter().take(num_outputs) {
                let value = normalize(channel[j].to_f64());
                out.push_str(&format!(" {value:8.6}"));
            }
            out.push('\n');
            written += 1;
        }
        cycle += 1;
    }

    Ok(out)
}

/// Zero-clamps tiny magnitudes exactly like `controlTools.h::normalize`.
///
/// The C++ harness aborts on NaN/Inf; here they are passed through so the
/// downstream `filesCompare` reports a concrete sample mismatch instead.
fn normalize(value: f64) -> f64 {
    if value.is_nan() || value.is_infinite() {
        value
    } else if value.abs() < 0.000_001 {
        0.0
    } else {
        value
    }
}

/// Kept to document the runner's contract against a known-good reference path.
#[allow(dead_code)]
fn _reference_protocol_note(_: &Path) {}
