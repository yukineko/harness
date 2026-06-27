//! Terminal-friendly display of the full HOTL status.

use crate::budget::BudgetStatus;
use crate::progress::ProgressStatus;
use crate::sessions::SessionSummary;

pub fn print_status(
    today: &str,
    budget: &BudgetStatus,
    sessions: &[SessionSummary],
    progress: &ProgressStatus,
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
) {
    let out = serde_json::json!({
        "date": today,
        "budget": budget,
        "recent_sessions": sessions,
        "progress": progress,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
}
