//! Cross-platform shell invocation — the single source of truth.
//!
//! Plugins that run a configured command line need a shell. Several hardcoded
//! `Command::new("sh")`, which silently no-ops on Windows (no `sh`), while a
//! couple branched on `cfg!(windows)` themselves. This centralizes the choice so
//! every plugin runs `cmd /C` on Windows and `sh -c` elsewhere, consistently.

use std::process::Command;

/// The platform shell program and its "run this command string" flag:
/// `("cmd", "/C")` on Windows, `("sh", "-c")` everywhere else.
pub fn shell() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    }
}

/// A `Command` that runs `cmdline` through the platform shell. The caller adds
/// stdio / env / current_dir as needed.
pub fn command(cmdline: &str) -> Command {
    let (prog, flag) = shell();
    let mut c = Command::new(prog);
    c.arg(flag).arg(cmdline);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_matches_platform() {
        let (prog, flag) = shell();
        if cfg!(windows) {
            assert_eq!((prog, flag), ("cmd", "/C"));
        } else {
            assert_eq!((prog, flag), ("sh", "-c"));
        }
    }

    #[test]
    fn command_runs_a_trivial_line() {
        // `exit 0` on sh, also valid on cmd. Just confirm it spawns and succeeds.
        let ok = command("exit 0").status().map(|s| s.success()).unwrap_or(false);
        assert!(ok);
    }
}
