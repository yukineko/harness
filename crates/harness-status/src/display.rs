//! Terminal-friendly display of the full HOTL status.

use crate::budget::BudgetStatus;
use crate::hooks::{sess8, HookLatencyReport};
use crate::inject::{key8, InjectReport};
use crate::progress::ProgressStatus;
use crate::sessions::SessionSummary;

pub fn print_status(
    today: &str,
    budget: &BudgetStatus,
    sessions: &[SessionSummary],
    progress: &ProgressStatus,
    hooks: &HookLatencyReport,
    inject: &InjectReport,
    cwd_display: &str,
) {
    println!("╔══════════════════════════════════════════════╗");
    println!("║         harness-status  ({today})         ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();

    // Budget section
    println!("── Budget (budgetguard) ──────────────────────────");
    if budget.ledger_present {
        println!(
            "  Today spend:  ${:.4}  ({} session(s))",
            budget.today_usd, budget.session_count_today
        );
    } else {
        println!("  ledger.json not found — budgetguard not installed?");
    }
    println!();

    // Recent sessions
    println!("── Recent sessions (gauge) ───────────────────────");
    if sessions.is_empty() {
        println!("  No session records found — gauge not installed?");
    } else {
        println!(
            "  {:<16} {:<20} {:>6} {:>12} {:>9}",
            "Session", "Project", "Turns", "Tokens", "Cost USD"
        );
        println!("  {}", "-".repeat(70));
        for s in sessions {
            let id8 = s.session_id.get(..8).unwrap_or(&s.session_id);
            let proj = truncate(&s.project, 20);
            println!(
                "  {:<16} {:<20} {:>6} {:>12} {:>9.4}",
                id8, proj, s.turns, s.total_tokens, s.cost_usd
            );
        }
    }
    println!();

    // Progress file
    println!("── Progress file (taskprog) ──────────────────────");
    println!("  cwd: {cwd_display}");
    if progress.exists {
        println!("  {}", progress.path);
        if let Some(preview) = &progress.preview {
            println!();
            for line in preview.lines() {
                println!("  │ {line}");
            }
        }
    } else {
        println!("  [not found] {}", progress.path);
        println!("  Run `taskprog init` or `/taskprog` to create one.");
    }
    println!();

    // Stop-hook latency (only the 3 heavy 600s gates record; see the contract).
    println!("── Stop-hook latency (donegate/reviewgate/propguard) ──");
    if hooks.sessions.is_empty() {
        println!("  [no Stop-hook latency recorded]");
    } else {
        println!("  budget: {}ms", hooks.budget_ms);
        for s in &hooks.sessions {
            let flag = if s.over_budget {
                "  ⚠ OVER BUDGET"
            } else {
                ""
            };
            println!(
                "  {} | {}ms across {} hooks{}",
                sess8(&s.session),
                s.total_ms,
                s.per_hook.len(),
                flag
            );
        }
    }
    println!();

    // UserPromptSubmit injection size (ADR 0001 Phase 2): the five injectors
    // (playbook/run-book/ctxrot/context-governor/fugu-router) record post-cap
    // injected char size per turn; warn when the combined size exceeds budget.
    println!("── UserPromptSubmit injection (aggregate budget) ──");
    if inject.turns.is_empty() {
        println!("  [no UserPromptSubmit injections recorded]");
    } else {
        println!("  budget: {} chars", inject.budget_chars);
        for t in &inject.turns {
            let flag = if t.over_budget {
                "  ⚠ OVER BUDGET"
            } else {
                ""
            };
            println!(
                "  {} | {} chars across {} injectors{}",
                key8(&t.turn_key),
                t.total_chars,
                t.per_plugin.len(),
                flag
            );
        }
    }
    println!();
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n.saturating_sub(1)])
    }
}

pub fn print_json(
    today: &str,
    budget: &BudgetStatus,
    sessions: &[SessionSummary],
    progress: &ProgressStatus,
    hooks: &HookLatencyReport,
    inject: &InjectReport,
) {
    let out = serde_json::json!({
        "date": today,
        "budget": budget,
        "recent_sessions": sessions,
        "progress": progress,
        "hook_latency": hooks,
        "inject": inject,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
}
