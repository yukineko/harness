//! Stop hook: read the transcript, compute cost, check against limits.
//!
//! A budget violation emits `{"decision":"block","reason":"…"}` so Claude
//! receives the overage notice and can wind down gracefully. A warn-only
//! crossing emits `{"additionalContext":"…"}` (advisory, no block). Harness
//! errors always exit 0 and allow the stop.

use harness_core::{pricing, usage};
use serde_json::json;

use crate::config::Config;
use crate::state::Ledger;

pub struct GateResult {
    pub session_usd: f64,
    pub day_usd: f64,
    pub verdict: Verdict,
}

pub enum Verdict {
    Allow,
    Warn(String),
    Block(String),
}

/// Run the budget gate. Returns a GateResult (or None on data errors).
pub fn evaluate(
    cfg: &Config,
    session_id: &str,
    transcript_path: &str,
    today: &str,
) -> Option<GateResult> {
    // Aggregate this session's transcript to compute USD cost.
    let agg = usage::aggregate(transcript_path)?;
    let session_usd = pricing::session_cost(agg.models.iter(), &cfg.price_overrides);

    // Update the daily ledger with this session's latest cost.
    let mut ledger = Ledger::load(&cfg.state_dir);
    let day_usd = ledger.record(session_id, today, session_usd);
    let _ = ledger.save(&cfg.state_dir);

    let verdict = verdict(cfg, session_usd, day_usd);
    Some(GateResult { session_usd, day_usd, verdict })
}

fn verdict(cfg: &Config, session_usd: f64, day_usd: f64) -> Verdict {
    // Check block limits first (higher priority than warn).
    if cfg.session_block_usd > 0.0 && session_usd >= cfg.session_block_usd {
        return Verdict::Block(format!(
            "budgetguard: セッション予算超過 ${:.4} / ${:.2} (上限)。\n\
             作業を保存し、コミットして終了してください。",
            session_usd, cfg.session_block_usd
        ));
    }
    if cfg.daily_block_usd > 0.0 && day_usd >= cfg.daily_block_usd {
        return Verdict::Block(format!(
            "budgetguard: 日次予算超過 ${:.4} / ${:.2} (上限)。\n\
             作業を保存し、コミットして終了してください。",
            day_usd, cfg.daily_block_usd
        ));
    }

    // Warn limits.
    let mut warns = Vec::new();
    if cfg.session_warn_usd > 0.0 && session_usd >= cfg.session_warn_usd {
        warns.push(format!(
            "セッション費用 ${:.4} が警告閾値 ${:.2} を超えています",
            session_usd, cfg.session_warn_usd
        ));
    }
    if cfg.daily_warn_usd > 0.0 && day_usd >= cfg.daily_warn_usd {
        warns.push(format!(
            "本日累計 ${:.4} が警告閾値 ${:.2} を超えています",
            day_usd, cfg.daily_warn_usd
        ));
    }

    if warns.is_empty() {
        Verdict::Allow
    } else {
        Verdict::Warn(format!("⚠ budgetguard:\n{}", warns.join("\n")))
    }
}

/// Emit the Stop hook output and exit.
pub fn emit_and_exit(result: Option<GateResult>) -> ! {
    match result {
        None => {
            // Data error or no transcript yet — allow silently.
            std::process::exit(0);
        }
        Some(r) => {
            // Running totals to stderr (operator-visible log; never touches the
            // stdout JSON the hook protocol parses).
            eprintln!(
                "budgetguard: session ${:.4} / day ${:.4}",
                r.session_usd, r.day_usd
            );
            match r.verdict {
                Verdict::Allow => std::process::exit(0),
                Verdict::Warn(msg) => {
                    println!("{}", json!({ "additionalContext": msg }));
                    std::process::exit(0);
                }
                Verdict::Block(reason) => {
                    println!("{}", json!({ "decision": "block", "reason": reason }));
                    std::process::exit(0);
                }
            }
        }
    }
}
