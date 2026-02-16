use boxes::dump_box;
use compiler::{Compiler, golden_snapshot_from_file};
use signals::dump_sig_readable;
use std::path::PathBuf;

fn parse_input_with_import_dirs(
    mut args: impl Iterator<Item = String>,
    usage: &str,
) -> (PathBuf, Vec<PathBuf>) {
    let Some(input) = args.next() else {
        eprintln!("{usage}");
        std::process::exit(2);
    };

    let mut search_paths = Vec::new();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "-I" | "--import-dir" => {
                let Some(dir) = args.next() else {
                    eprintln!("{usage}");
                    std::process::exit(2);
                };
                search_paths.push(PathBuf::from(dir));
            }
            _ => {
                eprintln!("{usage}");
                std::process::exit(2);
            }
        }
    }

    (PathBuf::from(input), search_paths)
}

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
            let usage = "Usage: cargo run -p compiler -- --parse <input.dsp> [-I <dir> ...]";
            let (input_path, search_paths) = parse_input_with_import_dirs(args, usage);
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
        Some("--dump-box") => {
            let usage = "Usage: cargo run -p compiler -- --dump-box <input.dsp> [-I <dir> ...]";
            let (input_path, search_paths) = parse_input_with_import_dirs(args, usage);
            let compiler = Compiler::new();
            let result = if search_paths.is_empty() {
                compiler.compile_file_default(&input_path)
            } else {
                compiler.compile_file(&input_path, &search_paths)
            };

            match result {
                Ok(out) => {
                    let Some(root) = out.root else {
                        eprintln!("Parse failed: no root node produced");
                        std::process::exit(1);
                    };
                    println!("{}", dump_box(&out.state.arena, root));
                }
                Err(err) => {
                    eprintln!("Parse failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Some("--dump-sig") => {
            let usage = "Usage: cargo run -p compiler -- --dump-sig <input.dsp> [-I <dir> ...]";
            let (input_path, search_paths) = parse_input_with_import_dirs(args, usage);
            let compiler = Compiler::new();
            let result = if search_paths.is_empty() {
                compiler.compile_file_default_to_signals(&input_path)
            } else {
                compiler.compile_file_to_signals(&input_path, &search_paths)
            };

            match result {
                Ok(out) => {
                    println!(
                        "Signals OK: inputs={} outputs={}",
                        out.process_arity.inputs, out.process_arity.outputs
                    );
                    for (index, sig) in out.signals.iter().enumerate() {
                        println!(
                            "[{index}] {}",
                            dump_sig_readable(&out.parse.state.arena, *sig)
                        );
                    }
                }
                Err(err) => {
                    eprintln!("Signal pipeline failed: {err}");
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
            eprintln!("  cargo run -p compiler -- --dump-box <input.dsp> [-I <dir> ...]");
            eprintln!("  cargo run -p compiler -- --dump-sig <input.dsp> [-I <dir> ...]");
            std::process::exit(2);
        }
    }
}
