use compiler::{Compiler, golden_snapshot_from_file};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);

    if matches!(args.next().as_deref(), Some("--golden")) {
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
        return;
    }

    println!("faust-rs compiler scaffold v{}", Compiler::version());
}
