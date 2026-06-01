//! `faust-rs` CLI launcher.

mod cli;

fn main() {
    // The evaluator's structural-lowering pass (`a2sb`) can recurse deeply for
    // large programs (e.g. auto-panning with many channels). 64 MiB is the CLI
    // stack contract for the evaluator's guarded recursion budgets; library
    // embedders that run the compiler on their own threads must provide
    // comparable stack headroom or use a lower evaluator depth budget.
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(cli::runner::run_main)
        .expect("failed to spawn compiler thread")
        .join()
        .expect("compiler thread panicked");
}
