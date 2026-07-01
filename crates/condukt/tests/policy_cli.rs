//! Integration coverage for `condukt policy decide` — the graded autonomy
//! policy engine's CLI surface and its exit-code contract. Spawns the built
//! binary so it exercises argument parsing + the process exit codes that
//! callers (skills, `state autonomy-check`) branch on. This test fails before
//! the `policy` subcommand exists (unrecognized subcommand -> clap exit 2 with
//! no `auto`/`block` stdout) and passes once it is wired = a genuine
//! Fail->Pass reproduction oracle for the wiring task.

use std::process::Command;

fn decide(risk: &str, reversible: &str, confidence: &str) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_condukt"))
        .args([
            "policy",
            "decide",
            "--risk",
            risk,
            "--reversible",
            reversible,
            "--confidence",
            confidence,
        ])
        .output()
        .expect("spawn condukt");
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let code = out.status.code().expect("exited with a code");
    (stdout, code)
}

#[test]
fn high_risk_irreversible_blocks_exit_3() {
    let (stdout, code) = decide("high", "low", "high");
    assert_eq!(stdout, "block");
    assert_eq!(code, 3);
}

#[test]
fn low_risk_reversible_confident_autos_exit_0() {
    let (stdout, code) = decide("low", "high", "high");
    assert_eq!(stdout, "auto");
    assert_eq!(code, 0);
}

#[test]
fn ambiguous_middle_escalates_exit_2() {
    let (stdout, code) = decide("medium", "medium", "medium");
    assert_eq!(stdout, "escalate");
    assert_eq!(code, 2);
}

#[test]
fn unparseable_level_exits_1_without_panic() {
    let (stdout, code) = decide("catastrophic", "low", "high");
    // Invalid input: no decision word on stdout, nonzero-but-not-a-verdict exit.
    assert!(stdout.is_empty(), "expected no verdict, got {stdout:?}");
    assert_eq!(code, 1);
}
