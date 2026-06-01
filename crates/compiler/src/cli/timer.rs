//! CLI timing and timeout support.
//!
//! [`CompilationTimer`] tracks total wall-clock time for one CLI invocation.
//! It complements the compiler library's internal timing sink by enforcing the
//! user-facing `--timeout` limit and printing the aggregate `[total]` line when
//! `--compilation-time` is enabled.

use std::time::{Duration, Instant};

/// Tracks elapsed time across compilation phases and enforces a global timeout.
pub struct CompilationTimer {
    /// Absolute start time of the compilation run.
    start: Instant,
    /// Maximum total compilation duration.  Exceeded → `process::exit(1)`.
    timeout: Duration,
    /// When `true`, the total timing is printed to stderr. Internal compiler
    /// phase timings are reported through `compiler::Compiler::with_timing_sink`.
    display: bool,
}

impl CompilationTimer {
    /// Creates a new timer. `timeout_secs` sets the hard limit; `display`
    /// controls whether the total timing is printed to stderr.
    pub fn new(timeout_secs: u64, display: bool) -> Self {
        let now = Instant::now();
        Self {
            start: now,
            timeout: Duration::from_secs(timeout_secs),
            display,
        }
    }

    /// Mark the end of a compilation phase and abort if the global timeout has
    /// been exceeded.
    pub fn phase(&mut self, name: &str) {
        let now = Instant::now();
        let total_elapsed = now.duration_since(self.start);
        if total_elapsed > self.timeout {
            eprintln!(
                "ERROR: compilation timeout ({:.1}s > {}s limit) after phase '{name}'",
                total_elapsed.as_secs_f64(),
                self.timeout.as_secs(),
            );
            std::process::exit(1);
        }
    }

    /// Print the total compilation time (only when `--compilation-time` is active).
    pub fn total(&self) {
        if self.display {
            let elapsed = self.start.elapsed();
            eprintln!("[total] {:.1}ms", elapsed.as_secs_f64() * 1000.0);
        }
    }
}
