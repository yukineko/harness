//! Run a test command as a subprocess with a timeout, capturing a bounded tail
//! of its combined output. Output streams to a temp file (not a pipe) so a
//! chatty command can never deadlock us; we only ever read back the last few KB.
//! Shared by `tdd red` and `tdd green`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

pub struct Outcome {
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub spawn_error: Option<String>,
    pub output_tail: String,
}

fn shell() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    }
}

const TAIL_CAP_BYTES: u64 = 256 * 1024;

/// Run `cmd` in `root` with a timeout, returning the outcome and a bounded tail.
pub fn run_cmd(cmd: &str, root: &Path, timeout: u64, tail_lines: usize, tmp_dir: &Path) -> Outcome {
    let timeout = timeout.max(1);
    let _ = std::fs::create_dir_all(tmp_dir);
    let log_path: PathBuf = tmp_dir.join("tdd-run.log");

    let file = match File::create(&log_path) {
        Ok(f) => f,
        Err(e) => return spawn_failed(cmd, format!("could not open log file: {e}")),
    };
    let file2 = match file.try_clone() {
        Ok(f) => f,
        Err(e) => return spawn_failed(cmd, format!("could not dup log file: {e}")),
    };

    let (prog, flag) = shell();
    let mut child = match Command::new(prog)
        .arg(flag)
        .arg(cmd)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(file2))
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return spawn_failed(cmd, format!("failed to spawn: {e}")),
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
        passed,
        exit_code,
        timed_out,
        spawn_error: None,
        output_tail: read_tail(&log_path, tail_lines),
    }
}

fn spawn_failed(_cmd: &str, msg: String) -> Outcome {
    Outcome {
        passed: false,
        exit_code: None,
        timed_out: false,
        spawn_error: Some(msg.clone()),
        output_tail: msg,
    }
}

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
