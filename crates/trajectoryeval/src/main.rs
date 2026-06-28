//! trajectoryeval — a trajectory-match verifier, the sibling of an output verifier.
//!
//! condukt's online verifier checks a task's OUTPUT (its done_criteria);
//! trajectoryeval checks the PATH the worker took — its ordered tool-call
//! sequence — against an expected trajectory spec. Inspired by the trajectory
//! matchers in langchain-ai/agentevals.
//!
//! Exit codes (mirrors evalkit/schemaguard's 0/1/2 gate policy):
//!   0  — trajectory matched the spec (pass)
//!   1  — a deviation (missing / unexpected / out-of-order steps)
//!   2  — harness error (unreadable or unparseable input)
//!
//! This is a plain CLI gate, NOT a lifecycle hook — do not wrap in `run_hook`;
//! let real errors surface as exit 2.

mod extract;
mod match_traj;

use std::path::PathBuf;
use std::process::exit;

use clap::{Parser, Subcommand};

use match_traj::{evaluate, MatchResult, Spec};

#[derive(Parser)]
#[command(
    name = "trajectoryeval",
    version,
    about = "Trajectory-match verifier: check an actual tool-call path against an expected spec."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compare an actual ordered tool sequence against an expected trajectory spec.
    Check(CheckArgs),
    /// Stream a transcript and print its ordered tool_use names as a JSON array.
    Extract(ExtractArgs),
}

#[derive(clap::Args)]
struct CheckArgs {
    /// Path to the expected spec JSON ({mode, steps:[{tool,optional}]}).
    #[arg(long)]
    expected: PathBuf,
    /// Path to the actual trajectory JSON (an array of tool-name strings).
    #[arg(long)]
    actual: PathBuf,
    /// Emit the serialized MatchResult as JSON instead of a human report.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct ExtractArgs {
    /// Path to the JSONL transcript to stream.
    #[arg(long)]
    transcript: PathBuf,
}

// ── command handlers ──────────────────────────────────────────────────────────

fn cmd_check(args: CheckArgs) -> i32 {
    // Read + parse the expected spec.
    let spec_raw = match std::fs::read_to_string(&args.expected) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "trajectoryeval: cannot read expected spec {}: {}",
                args.expected.display(),
                e
            );
            return 2;
        }
    };
    let spec: Spec = match serde_json::from_str(&spec_raw) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("trajectoryeval: invalid expected spec JSON: {}", e);
            return 2;
        }
    };

    // Read + parse the actual trajectory (array of tool-name strings).
    let actual_raw = match std::fs::read_to_string(&args.actual) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "trajectoryeval: cannot read actual trajectory {}: {}",
                args.actual.display(),
                e
            );
            return 2;
        }
    };
    let actual: Vec<String> = match serde_json::from_str(&actual_raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("trajectoryeval: invalid actual trajectory JSON (expected an array of tool-name strings): {}", e);
            return 2;
        }
    };

    let result = evaluate(&spec, &actual);

    if args.json {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        print_report(&result);
    }

    if result.pass {
        0
    } else {
        1
    }
}

fn print_report(r: &MatchResult) {
    if r.pass {
        println!("trajectory matched (pass)");
        return;
    }
    println!("trajectory deviated (fail)");
    if !r.missing.is_empty() {
        println!("  missing:     {}", r.missing.join(", "));
    }
    if !r.unexpected.is_empty() {
        println!("  unexpected:  {}", r.unexpected.join(", "));
    }
    if r.out_of_order {
        println!("  out of order: the right set of tools appeared in the wrong order");
    }
}

fn cmd_extract(args: ExtractArgs) -> i32 {
    match extract::extract_tools(&args.transcript) {
        Ok(tools) => {
            println!("{}", serde_json::to_string(&tools).unwrap());
            0
        }
        Err(e) => {
            eprintln!(
                "trajectoryeval: cannot read transcript {}: {}",
                args.transcript.display(),
                e
            );
            2
        }
    }
}

// ── entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Check(args) => cmd_check(args),
        Command::Extract(args) => cmd_extract(args),
    };
    exit(code);
}
