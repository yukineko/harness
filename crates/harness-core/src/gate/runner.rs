//! Run one command as a subprocess with a timeout, capturing a bounded tail of
//! its combined output. Output is streamed to a log file (not a pipe) so a
//! chatty command can never deadlock us by filling a pipe buffer, and we only
//! ever read back the last few KB — never the whole log.
//!
//! This is the single home for the dangerous subprocess code that donegate's
//! `run_check` and tdd's `run_cmd` previously duplicated verbatim. Each plugin
//! now wraps [`run`] in a thin adapter that maps [`RawOutcome`] onto its own
//! richer outcome type (names, durations, optional flags, …).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

/// At most this many bytes are read back from the end of a run's log. Bounded by
/// design — we never load a whole build log into memory.
pub const TAIL_CAP_BYTES: u64 = 256 * 1024;

/// The raw result of running one command. Callers decorate this with their own
/// metadata (check name, command line, duration, optionality, …).
pub struct RawOutcome {
    /// True iff the process exited with a success status (code 0).
    pub passed: bool,
    /// The exit code, when the process exited normally and reported one.
    pub exit_code: Option<i32>,
    /// True iff we killed the process because it exceeded the timeout.
    pub timed_out: bool,
    /// `Some(msg)` iff we could not even spawn / wait on the process.
    pub spawn_error: Option<String>,
    /// The last `tail_lines` lines of the combined stdout+stderr (bounded).
    pub output_tail: String,
}

/// Run `cmd` through the platform shell in `workdir`, with a timeout, streaming
/// combined stdout+stderr to `log_path` (created/truncated). Returns the outcome
/// plus a bounded tail of the last `tail_lines` lines.
///
/// A timeout (≥ 1s; `timeout_secs` is clamped up to 1) kills the child. We never
/// pipe the child's output — it goes to the log file — so a noisy command can't
/// deadlock us, and only the tail is read back.
pub fn run(
    cmd: &str,
    workdir: &Path,
    timeout_secs: u64,
    tail_lines: usize,
    log_path: &Path,
) -> RawOutcome {
    let timeout = timeout_secs.max(1);

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = match File::create(log_path) {
        Ok(f) => f,
        Err(e) => return spawn_failed(format!("could not open log file: {e}")),
    };
    let file2 = match file.try_clone() {
        Ok(f) => f,
        Err(e) => return spawn_failed(format!("could not dup log file: {e}")),
    };

    let (prog, flag) = crate::shell::shell();
    let mut child = match Command::new(prog)
        .arg(flag)
        .arg(cmd)
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(file2))
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return spawn_failed(format!("failed to spawn: {e}")),
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

    RawOutcome {
        passed,
        exit_code,
        timed_out,
        spawn_error: None,
        output_tail: read_tail(log_path, tail_lines),
    }
}

fn spawn_failed(msg: String) -> RawOutcome {
    RawOutcome {
        passed: false,
        exit_code: None,
        timed_out: false,
        spawn_error: Some(msg.clone()),
        output_tail: msg,
    }
}

/// Read at most the last [`TAIL_CAP_BYTES`] of the file, then keep the final
/// `lines` lines. Bounded by design — we never load a whole build log.
pub fn read_tail(path: &Path, lines: usize) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_log(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("hc-gate-runner-{}-{name}.log", std::process::id()))
    }

    #[test]
    fn success_sets_passed() {
        let log = tmp_log("ok");
        let cwd = std::env::current_dir().unwrap();
        let o = run("exit 0", &cwd, 5, 10, &log);
        assert!(o.passed);
        assert_eq!(o.exit_code, Some(0));
        assert!(!o.timed_out);
        assert!(o.spawn_error.is_none());
        let _ = std::fs::remove_file(&log);
    }

    #[test]
    fn failure_clears_passed() {
        let log = tmp_log("fail");
        let cwd = std::env::current_dir().unwrap();
        let o = run("exit 3", &cwd, 5, 10, &log);
        assert!(!o.passed);
        assert_eq!(o.exit_code, Some(3));
        assert!(!o.timed_out);
        assert!(o.spawn_error.is_none());
        let _ = std::fs::remove_file(&log);
    }

    #[test]
    fn sleep_past_timeout_is_flagged() {
        let log = tmp_log("timeout");
        let cwd = std::env::current_dir().unwrap();
        let o = run("sleep 5", &cwd, 1, 10, &log);
        assert!(!o.passed);
        assert!(o.timed_out);
        assert!(o.exit_code.is_none());
        let _ = std::fs::remove_file(&log);
    }

    #[test]
    fn spawn_error_when_workdir_missing() {
        let log = tmp_log("spawn");
        let missing = Path::new("/no/such/dir/condukt-gate-xyzzy-12345");
        let o = run("echo hi", missing, 5, 10, &log);
        assert!(!o.passed);
        assert!(o.spawn_error.is_some());
        let _ = std::fs::remove_file(&log);
    }

    #[test]
    fn read_tail_keeps_last_lines() {
        let p = tmp_log("tail");
        std::fs::write(&p, "a\nb\nc\nd\ne\n").unwrap();
        assert_eq!(read_tail(&p, 2), "d\ne");
        assert_eq!(read_tail(&p, 99), "a\nb\nc\nd\ne");
        let _ = std::fs::remove_file(&p);
    }
}
