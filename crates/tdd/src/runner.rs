//! Thin adapter over `harness_core::gate::runner`: run a test command as a
//! subprocess and map the raw outcome onto tdd's `Outcome`. Shared by `tdd red`
//! and `tdd green`. The dangerous spawn/timeout/bounded-tail logic lives in
//! harness-core; this only fixes the log path and maps types.

use std::path::Path;

use harness_core::gate::runner;

pub struct Outcome {
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub spawn_error: Option<String>,
    pub output_tail: String,
}

/// Run `cmd` in `root` with a timeout, returning the outcome and a bounded tail.
pub fn run_cmd(cmd: &str, root: &Path, timeout: u64, tail_lines: usize, tmp_dir: &Path) -> Outcome {
    let log_path = tmp_dir.join("tdd-run.log");
    let raw = runner::run(cmd, root, timeout, tail_lines, &log_path);
    Outcome {
        passed: raw.passed,
        exit_code: raw.exit_code,
        timed_out: raw.timed_out,
        spawn_error: raw.spawn_error,
        output_tail: raw.output_tail,
    }
}
