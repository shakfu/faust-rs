use compiler::{Compiler, golden_snapshot_from_file};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--golden") => {
            let Some(input) = args.next() else {
                eprintln!("Usage: cargo run -p compiler -- --golden <input.dsp>");
                std::process::exit(2);
            };

            if args.next().is_some() {
                eprintln!("Usage: cargo run -p compiler -- --golden <input.dsp>");
                std::process::exit(2);
            }

            let input_path = PathBuf::from(input);
            match golden_snapshot_from_file(&input_path) {
                Ok(snapshot) => {
                    print!("{snapshot}");
                }
                Err(err) => {
                    eprintln!("Failed to create golden snapshot: {err}");
                    std::process::exit(1);
                }
            }
        }
        Some("--parse") => {
            let Some(input) = args.next() else {
                eprintln!("Usage: cargo run -p compiler -- --parse <input.dsp> [-I <dir> ...]");
                std::process::exit(2);
            };

            let mut search_paths = Vec::new();
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "-I" | "--import-dir" => {
                        let Some(dir) = args.next() else {
                            eprintln!(
                                "Usage: cargo run -p compiler -- --parse <input.dsp> [-I <dir> ...]"
                            );
                            std::process::exit(2);
                        };
                        search_paths.push(PathBuf::from(dir));
                    }
                    _ => {
                        eprintln!(
                            "Usage: cargo run -p compiler -- --parse <input.dsp> [-I <dir> ...]"
                        );
                        std::process::exit(2);
                    }
                }
            }

            let input_path = PathBuf::from(input);
            let compiler = Compiler::new();
            let result = if search_paths.is_empty() {
                compiler.compile_file_default(&input_path)
            } else {
                compiler.compile_file(&input_path, &search_paths)
            };

            match result {
                Ok(out) => {
                    println!(
                        "Parsed OK: root={:?} parse_errors={} recoveries={}",
                        out.root,
                        out.errors.len(),
                        out.state.ctx.recovery_count()
                    );
                }
                Err(err) => {
                    eprintln!("Parse failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        None => {
            println!("faust-rs compiler scaffold v{}", Compiler::version());
        }
        Some(_) => {
            eprintln!("Usage:");
            eprintln!("  cargo run -p compiler -- --golden <input.dsp>");
            eprintln!("  cargo run -p compiler -- --parse <input.dsp> [-I <dir> ...]");
            std::process::exit(2);
        }
    }
}
