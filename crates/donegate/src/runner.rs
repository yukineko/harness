//! Thin adapter over `harness_core::gate::runner`: run one [`Check`] as a
//! subprocess and decorate the raw outcome with donegate's metadata (check name,
//! command line, duration, optionality). The dangerous spawn/timeout/bounded-tail
//! logic lives in harness-core; this only maps types and computes the per-check
//! log path.

use std::path::{Path, PathBuf};
use std::time::Instant;

use harness_core::gate::runner;

use crate::config::Check;

pub struct Outcome {
    pub name: String,
    pub cmd: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub spawn_error: Option<String>,
    pub duration_secs: f64,
    pub output_tail: String,
    pub optional: bool,
}

impl Outcome {
    /// A short status verb for reports.
    pub fn status(&self) -> &'static str {
        if self.passed {
            "ok"
        } else if self.timed_out {
            "timeout"
        } else if self.spawn_error.is_some() {
            "spawn-error"
        } else {
            "fail"
        }
    }
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn run_check(
    check: &Check,
    root: &Path,
    default_timeout: u64,
    tail_lines: usize,
    tmp_dir: &Path,
) -> Outcome {
    let timeout = check.timeout_secs.unwrap_or(default_timeout).max(1);
    let workdir = match &check.workdir {
        Some(w) => root.join(w),
        None => root.to_path_buf(),
    };
    let log_path: PathBuf = tmp_dir.join(format!("{}.log", sanitize(&check.name)));

    let start = Instant::now();
    let raw = runner::run(&check.cmd, &workdir, timeout, tail_lines, &log_path);

    Outcome {
        name: check.name.clone(),
        cmd: check.cmd.clone(),
        passed: raw.passed,
        exit_code: raw.exit_code,
        timed_out: raw.timed_out,
        spawn_error: raw.spawn_error,
        duration_secs: start.elapsed().as_secs_f64(),
        output_tail: raw.output_tail,
        optional: check.optional,
    }
}
