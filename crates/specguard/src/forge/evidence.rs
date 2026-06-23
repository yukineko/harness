//! ⑥ evidence+agreement gate: collect impl results, present for human consent.
//!
//! The machine collects typed evidence (test results, D1 findings). The human
//! decides with `specforge agree`. Only then can the worktrees be merged
//! (DESIGN.md §6, §6.1). A failed requirement stays in its worktree for retry
//! or human override — the harness never silently discards work.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::implement::ImplResult;

/// Summary of the evidence gate outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceReport {
    pub spec_id: String,
    pub date: String,
    /// `pending` → human must run `specforge agree`; `agreed` → merges allowed.
    pub gate_status: String,
    pub total: usize,
    pub passed: usize,
    pub partial: usize,
    pub failed: usize,
    pub no_marker: usize,
    pub items: Vec<EvidenceItem>,
    /// Why the human agreed (set by `specforge agree`).
    pub agreement_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub req_id: String,
    pub status: String,
    pub test_result: Option<String>,
    pub evidence_note: Option<String>,
    pub worktree: Option<String>,
}

impl EvidenceReport {
    pub fn is_gate_open(&self) -> bool {
        self.gate_status == "agreed"
    }
}

impl EvidenceItem {
    /// A requirement whose impl agent reported full success — eligible for merge.
    pub fn is_done(&self) -> bool {
        self.status == "done"
    }
}

/// Build an evidence report from impl results.
pub fn build(spec_id: &str, date: &str, results: &[ImplResult]) -> EvidenceReport {
    let total = results.len();
    let passed = results.iter().filter(|r| r.is_success()).count();
    let partial = results.iter().filter(|r| r.status == "partial").count();
    let failed = results.iter().filter(|r| r.status == "failed").count();
    let no_marker = results.iter().filter(|r| r.status == "no-marker").count();
    let items = results
        .iter()
        .map(|r| EvidenceItem {
            req_id: r.req_id.clone(),
            status: r.status.clone(),
            test_result: r.test_result.clone(),
            evidence_note: r.evidence_note.clone(),
            worktree: r.worktree.clone(),
        })
        .collect();
    EvidenceReport {
        spec_id: spec_id.to_string(),
        date: date.to_string(),
        gate_status: "pending".to_string(),
        total,
        passed,
        partial,
        failed,
        no_marker,
        items,
        agreement_reason: None,
    }
}

/// Persist evidence report to `<dir>/<spec_id>-evidence.json`.
pub fn write(dir: &Path, report: &EvidenceReport) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).context("creating impl dir")?;
    let path = dir.join(format!("{}-evidence.json", report.spec_id));
    let json = serde_json::to_string_pretty(report).context("serializing evidence")?;
    std::fs::write(&path, json).context("writing evidence")?;
    Ok(path)
}

/// Load a persisted evidence report.
pub fn load(dir: &Path, spec_id: &str) -> Result<EvidenceReport> {
    let path = dir.join(format!("{spec_id}-evidence.json"));
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading evidence {}", path.display()))?;
    serde_json::from_str(&text).context("parsing evidence")
}

/// Set `gate_status = agreed` and record the human's reason.
pub fn record_agreement(dir: &Path, spec_id: &str, reason: &str) -> Result<EvidenceReport> {
    let mut report = load(dir, spec_id)?;
    if reason.trim().is_empty() {
        anyhow::bail!("合意には理由が必要です (-m \"...\")");
    }
    report.gate_status = "agreed".to_string();
    report.agreement_reason = Some(reason.to_string());
    write(dir, &report)?;
    Ok(report)
}

/// Print a human-readable summary of the evidence gate to stdout.
pub fn print_summary(report: &EvidenceReport) {
    println!("╔══════════════════════════════════════════════╗");
    println!("║  ⑥ 実装証拠ゲート  ({})  ", report.spec_id);
    println!("╚══════════════════════════════════════════════╝");
    println!();
    println!("合計 {total}  ✔ done {passed}  ⚠ partial {partial}  ✗ failed {failed}  ? no-marker {no_marker}",
        total = report.total, passed = report.passed, partial = report.partial,
        failed = report.failed, no_marker = report.no_marker);
    println!();
    for item in &report.items {
        let icon = match item.status.as_str() {
            "done" => "✔",
            "partial" => "⚠",
            "failed" => "✗",
            _ => "?",
        };
        println!("  {icon} {req_id} [{status}]", req_id = item.req_id, status = item.status);
        if let Some(tr) = &item.test_result {
            println!("     test: {tr}");
        }
        if let Some(note) = &item.evidence_note {
            println!("     note: {note}");
        }
        if let Some(wt) = &item.worktree {
            println!("     worktree: {wt}");
        }
    }
    println!();
    match report.gate_status.as_str() {
        "agreed" => {
            println!("Gate: AGREED ({})", report.agreement_reason.as_deref().unwrap_or(""));
            println!("次: `specforge merge --id {}` でワークツリーを統合できます。", report.spec_id);
        }
        _ => {
            println!("Gate: PENDING (未合意)");
            println!("証拠を確認し、合意できるなら:");
            println!("  specforge agree --id {} -m \"理由\"", report.spec_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implement::ImplResult;

    fn result(req_id: &str, status: &str) -> ImplResult {
        ImplResult {
            spec_id: "S".into(),
            req_id: req_id.into(),
            status: status.into(),
            test_cmd: None,
            test_result: None,
            evidence_note: None,
            worktree: None,
            agent_exit: 0,
        }
    }

    #[test]
    fn build_counts_each_status_and_starts_pending() {
        let results = vec![
            result("R1", "done"),
            result("R2", "done"),
            result("R3", "partial"),
            result("R4", "failed"),
            result("R5", "no-marker"),
        ];
        let r = build("S", "2026-06-23", &results);
        assert_eq!(r.total, 5);
        assert_eq!(r.passed, 2);
        assert_eq!(r.partial, 1);
        assert_eq!(r.failed, 1);
        assert_eq!(r.no_marker, 1);
        assert_eq!(r.gate_status, "pending");
        assert!(!r.is_gate_open());
    }

    #[test]
    fn only_done_items_are_mergeable() {
        let results = vec![result("R1", "done"), result("R2", "partial")];
        let r = build("S", "d", &results);
        let mergeable: Vec<_> = r.items.iter().filter(|i| i.is_done()).collect();
        assert_eq!(mergeable.len(), 1);
        assert_eq!(mergeable[0].req_id, "R1");
    }
}
