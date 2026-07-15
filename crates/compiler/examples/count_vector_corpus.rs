//! Count impulse-test DSPs whose requested vector pipeline remains certified.
//!
//! Usage:
//! `cargo run -p compiler --example count_vector_corpus -- [lv] [ss]`
//!
//! The default is `-vec -lv 0 -ss 0`. A DSP is counted as vector-capable only
//! when the FIR result reports `VectorPipelineStatus::Certified`; a generated
//! artifact alone is not sufficient because unsupported vector shapes may use
//! the scalar fallback path.

use std::path::PathBuf;

use compiler::{
    Compiler, ComputeMode, RealType, SchedulingStrategy, SignalFirLane, VectorPipelineStatus,
};

fn parse_arg(args: &[String], index: usize, default: u8, name: &str) -> u8 {
    args.get(index)
        .map_or(Ok(default), |value| {
            value
                .parse::<u8>()
                .map_err(|_| format!("invalid {name}: {value}"))
        })
        .unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2);
        })
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let loop_variant = parse_arg(&args, 1, 0, "loop variant");
    let strategy = parse_arg(&args, 2, 0, "scheduling strategy");
    let root = PathBuf::from("tests/impulse-tests/dsp");

    let mut files = std::fs::read_dir(&root)
        .expect("read impulse DSP directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "dsp"))
        .collect::<Vec<_>>();
    files.sort();

    let mut certified = Vec::new();
    let mut fallback = Vec::new();
    let mut errors = Vec::new();

    for path in files {
        let compiler = Compiler::new()
            .with_real_type(RealType::Float64)
            .with_compute_mode(ComputeMode::Vector {
                vec_size: ComputeMode::DEFAULT_VEC_SIZE,
                loop_variant,
            })
            .with_scheduling_strategy(match strategy {
                0 => SchedulingStrategy::DepthFirst,
                1 => SchedulingStrategy::BreadthFirst,
                2 => SchedulingStrategy::Special,
                _ => SchedulingStrategy::ReverseBreadthFirst,
            });

        match compiler.compile_file_to_fir_with_lane(
            &path,
            &[root.clone(), PathBuf::from("/usr/local/share/faust")],
            SignalFirLane::TransformFastLane,
        ) {
            Ok(output) => match output.vector_pipeline_status {
                VectorPipelineStatus::Certified => certified.push(path),
                status => fallback.push((path, status)),
            },
            Err(error) => errors.push((path, error.to_string())),
        }
    }

    println!("MODE -vec -lv {loop_variant} -ss {strategy}");
    println!("TOTAL {}", certified.len() + fallback.len() + errors.len());
    println!("CERTIFIED {}", certified.len());
    println!("FALLBACK {}", fallback.len());
    println!("ERROR {}", errors.len());
    println!("CERTIFIED_FILES");
    for path in certified {
        println!("{}", path.display());
    }
    println!("FALLBACK_FILES");
    for (path, status) in fallback {
        println!("{}: {status:?}", path.display());
    }
    println!("ERROR_FILES");
    for (path, error) in errors {
        println!("{}: {error}", path.display());
    }
}
