//! Run one check as a subprocess with a timeout, capturing a bounded tail of its
//! combined output. Output is streamed to a temp file (not a pipe) so a chatty
//! command can never deadlock us by filling a pipe buffer, and we only ever read
//! back the last few KB — never the whole log.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

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

fn shell() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
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

/// At most this many bytes are read back from the end of a check's log.
const TAIL_CAP_BYTES: u64 = 256 * 1024;

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

    let _ = std::fs::create_dir_all(tmp_dir);
    let file = match File::create(&log_path) {
        Ok(f) => f,
        Err(e) => return spawn_failed(check, format!("could not open log file: {e}")),
    };
    let file2 = match file.try_clone() {
        Ok(f) => f,
        Err(e) => return spawn_failed(check, format!("could not dup log file: {e}")),
    };

    let (prog, flag) = shell();
    let start = Instant::now();
    let mut child = match Command::new(prog)
        .arg(flag)
        .arg(&check.cmd)
        .current_dir(&workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(file2))
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return spawn_failed(check, format!("failed to spawn: {e}")),
    };

    let (passed, exit_code, timed_out) = match child.wait_timeout(Duration::from_secs(timeout)) {
        Ok(Some(status)) => (status.success(), status.code(), false),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            (false, None, true)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            (false, None, false)
        }
    };

    Outcome {
        name: check.name.clone(),
        cmd: check.cmd.clone(),
        passed,
        exit_code,
        timed_out,
        spawn_error: None,
        duration_secs: start.elapsed().as_secs_f64(),
        output_tail: read_tail(&log_path, tail_lines),
        optional: check.optional,
    }
}

fn spawn_failed(check: &Check, msg: String) -> Outcome {
    Outcome {
        name: check.name.clone(),
        cmd: check.cmd.clone(),
        passed: false,
        exit_code: None,
        timed_out: false,
        spawn_error: Some(msg.clone()),
        duration_secs: 0.0,
        output_tail: msg,
        optional: check.optional,
    }
}

/// Read at most the last `TAIL_CAP_BYTES` of the file, then keep the final
/// `lines` lines. Bounded by design — we never load a whole build log.
fn read_tail(path: &Path, lines: usize) -> String {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(TAIL_CAP_BYTES);
    if start > 0 && f.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        // non-UTF8 logs: re-read as bytes and lossily convert the tail.
        let mut bytes = Vec::new();
        let mut f = match File::open(path) {
            Ok(f) => f,
            Err(_) => return String::new(),
        };
        let _ = f.seek(SeekFrom::Start(start));
        let _ = f.read_to_end(&mut bytes);
        buf = String::from_utf8_lossy(&bytes).into_owned();
    }
    let collected: Vec<&str> = buf.lines().collect();
    let tail = if collected.len() > lines {
        &collected[collected.len() - lines..]
    } else {
        &collected[..]
    };
    tail.join("\n")
}
