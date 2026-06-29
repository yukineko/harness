//! `ctxrot eval` — offline measurement of whether re-anchor actually *helps*,
//! and at what added-token cost.
//!
//! Re-anchor re-injects already-known decisions at the window tail — extra tokens,
//! which is the very thing ctxrot fights. So it only earns its keep if the recall
//! gain outweighs the cost. The hook NEVER calls an LLM (hard invariant), so this
//! measurement lives entirely outside the hook path:
//!
//!   1. `eval gen`  — write deterministic recall cases. Each plants an arbitrary,
//!      un-guessable decision (a port, a codename, a count) early in a long
//!      transcript, buries it under filler so it sinks into the lost-in-the-middle
//!      zone, then asks about it at the very end. Two prompt variants per case:
//!      `*.off.txt` (no anchor) and `*.on.txt` (the decision re-surfaced at the
//!      tail, exactly as guard would inject it).
//!   2. a thin driver (`eval/run-recall.sh`) feeds each variant to `claude -p`
//!      (subscription, no API key) and records the answers.
//!   3. `eval score` — tally accuracy per variant and the added-token cost
//!      (Σ anchor bytes), so the net benefit is one table.
//!
//! Generation and scoring are deterministic and unit-tested; only step 2 needs a
//! model, and it stays out-of-process by design.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One recall case: a planted decision and the question that probes it.
#[derive(Clone, Debug)]
pub struct Case {
    pub id: String,
    pub decision: String,
    pub question: String,
    /// Substring the model's answer must contain to count as correct.
    pub expected: String,
}

impl Case {
    /// The re-anchor block the ON variant appends at the tail — byte length is the
    /// added-token proxy (Σ over cases = re-anchor's cost), mirroring guard's
    /// `anchor` metric `bytes`.
    pub fn anchor_block(&self) -> String {
        format!(
            "[ctxrot anchor] 直近の確定事項（再掲・末尾再浮上）:\n\n■ 決定事項:\n- {}\n",
            self.decision
        )
    }
}

const CODENAMES: &[&str] = &[
    "Tindalos", "Aurora", "Carcosa", "Vespera", "Hadal", "Borealis", "Nyx", "Erebus",
];

/// Deterministically generate `n` cases. Answers are arbitrary numbers/codenames
/// the model cannot guess from priors, so a correct answer means genuine recall.
pub fn generate_cases(n: usize) -> Vec<Case> {
    (0..n)
        .map(|i| {
            let id = format!("case-{i:02}");
            match i % 3 {
                0 => {
                    let port = 7000 + i * 37;
                    Case {
                        id,
                        decision: format!("本番ポートは {port} 番に固定すると決定した。"),
                        question: "本番ポート番号は？ 数字のみで答えよ。".into(),
                        expected: port.to_string(),
                    }
                }
                1 => {
                    let name = CODENAMES[i % CODENAMES.len()];
                    Case {
                        id,
                        decision: format!("リリースのコードネームは {name} とすると決定した。"),
                        question: "リリースのコードネームは？ 一語で答えよ。".into(),
                        expected: name.to_string(),
                    }
                }
                _ => {
                    let retries = 3 + (i % 9);
                    Case {
                        id,
                        decision: format!("リトライ上限は {retries} 回に決定した。"),
                        question: "リトライ上限の回数は？ 数字のみで答えよ。".into(),
                        expected: retries.to_string(),
                    }
                }
            }
        })
        .collect()
}

/// Neutral filler to push the planted decision out of the high-attention head and
/// into the lost-in-the-middle zone. Sized by char count (CJK-safe).
fn filler(chars: usize) -> String {
    const UNIT: &str =
        "この区間は本筋と無関係な作業ログである。各種ファイルを編集し、テストを実行し、出力を確認し、次の作業へ移った。\n";
    let mut s = String::new();
    while s.chars().count() < chars {
        s.push_str(UNIT);
    }
    s
}

/// Render one variant's prompt. The decision is planted early, buried under
/// `filler_chars` of filler; the ON variant re-surfaces it at the tail (as guard
/// would); both close with the question.
pub fn render_prompt(case: &Case, with_anchor: bool, filler_chars: usize) -> String {
    let mut s = String::new();
    s.push_str("以下は長い作業セッションの記録の抜粋です。\n\n");
    s.push_str(&format!("【序盤の決定】{}\n\n", case.decision));
    s.push_str(&filler(filler_chars));
    if with_anchor {
        s.push('\n');
        s.push_str(&case.anchor_block());
    }
    s.push_str(&format!(
        "\n【質問】{} セッションの記録だけに基づいて、答えだけを返してください。\n",
        case.question
    ));
    s
}

// --------------------------------------------------------------- manifest / io

#[derive(Serialize, Deserialize)]
struct ManifestEntry {
    id: String,
    expected: String,
    anchor_bytes: usize,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    cases: Vec<ManifestEntry>,
}

/// One recorded model answer (a line of the driver's results.jsonl).
#[derive(Deserialize)]
pub struct ResultRec {
    pub id: String,
    pub variant: String,
    pub answer: String,
}

/// Write cases + both prompt variants + a manifest into `out`. Returns the case
/// count written.
pub fn run_gen(out: &Path, n: usize, filler_chars: usize) -> anyhow::Result<usize> {
    std::fs::create_dir_all(out)?;
    let cases = generate_cases(n);
    let mut entries = Vec::with_capacity(cases.len());
    for c in &cases {
        std::fs::write(
            out.join(format!("{}.off.txt", c.id)),
            render_prompt(c, false, filler_chars),
        )?;
        std::fs::write(
            out.join(format!("{}.on.txt", c.id)),
            render_prompt(c, true, filler_chars),
        )?;
        entries.push(ManifestEntry {
            id: c.id.clone(),
            expected: c.expected.clone(),
            anchor_bytes: c.anchor_block().len(),
        });
    }
    let manifest = Manifest { cases: entries };
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(cases.len())
}

/// Accuracy + cost tally for the two variants.
#[derive(Default, Debug, PartialEq)]
pub struct Report {
    pub off_total: usize,
    pub off_correct: usize,
    pub on_total: usize,
    pub on_correct: usize,
    /// Σ anchor bytes over the ON cases that were actually answered.
    pub added_bytes: usize,
}

impl Report {
    pub fn off_pct(&self) -> f64 {
        pct(self.off_correct, self.off_total)
    }
    pub fn on_pct(&self) -> f64 {
        pct(self.on_correct, self.on_total)
    }
}

fn pct(num: usize, den: usize) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 * 100.0 / den as f64
    }
}

/// A correct answer contains the expected token (case-insensitive substring).
fn judge(answer: &str, expected: &str) -> bool {
    answer.to_lowercase().contains(&expected.to_lowercase())
}

/// Pure scoring: tally `results` against the cases' expected answers + anchor
/// costs. Unknown ids and unknown variants are ignored.
fn score(cases: &[Case], anchor_bytes: &[(String, usize)], results: &[ResultRec]) -> Report {
    use std::collections::HashMap;
    let expected: HashMap<&str, &str> = cases
        .iter()
        .map(|c| (c.id.as_str(), c.expected.as_str()))
        .collect();
    let bytes: HashMap<&str, usize> = anchor_bytes.iter().map(|(k, v)| (k.as_str(), *v)).collect();

    let mut r = Report::default();
    for rec in results {
        let Some(exp) = expected.get(rec.id.as_str()) else {
            continue;
        };
        let ok = judge(&rec.answer, exp);
        match rec.variant.as_str() {
            "off" => {
                r.off_total += 1;
                r.off_correct += ok as usize;
            }
            "on" => {
                r.on_total += 1;
                r.on_correct += ok as usize;
                r.added_bytes += bytes.get(rec.id.as_str()).copied().unwrap_or(0);
            }
            _ => {}
        }
    }
    r
}

/// Read a manifest + a results.jsonl, score, and print the comparison table.
pub fn run_score(manifest_path: &Path, results_path: &Path) -> anyhow::Result<Report> {
    let manifest: Manifest = serde_json::from_str(&std::fs::read_to_string(manifest_path)?)?;
    // Reconstruct minimal Cases (id + expected) for scoring; anchor bytes come
    // straight from the manifest so the cost matches what was generated.
    let cases: Vec<Case> = manifest
        .cases
        .iter()
        .map(|e| Case {
            id: e.id.clone(),
            decision: String::new(),
            question: String::new(),
            expected: e.expected.clone(),
        })
        .collect();
    let anchor_bytes: Vec<(String, usize)> = manifest
        .cases
        .iter()
        .map(|e| (e.id.clone(), e.anchor_bytes))
        .collect();

    let mut results = Vec::new();
    for line in std::fs::read_to_string(results_path)?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<ResultRec>(line) {
            results.push(rec);
        }
    }

    let report = score(&cases, &anchor_bytes, &results);
    print_report(&report);
    Ok(report)
}

fn print_report(r: &Report) {
    println!(
        "{:<10} {:>6} {:>8} {:>9}",
        "variant", "cases", "correct", "accuracy"
    );
    println!(
        "{:<10} {:>6} {:>8} {:>8.0}%",
        "off",
        r.off_total,
        r.off_correct,
        r.off_pct()
    );
    println!(
        "{:<10} {:>6} {:>8} {:>8.0}%",
        "on",
        r.on_total,
        r.on_correct,
        r.on_pct()
    );
    let added_tok = r.added_bytes / 4; // same ~4 bytes/token estimate as transcript
    println!(
        "\nre-anchor 追加注入: {} bytes (~{} tok, /4) over {} ON case(s)",
        r.added_bytes, added_tok, r.on_total
    );
    println!(
        "Δ accuracy (on − off): {:+.0} pts",
        r.on_pct() - r.off_pct()
    );
    println!(
        "→ Δ が大きく正なら re-anchor は追加トークン分の価値あり。0付近〜負なら reanchor_min_band を上げる/無効化を検討。"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases_are_deterministic_and_unique() {
        let a = generate_cases(9);
        let b = generate_cases(9);
        assert_eq!(a.len(), 9);
        // deterministic
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.id, y.id);
            assert_eq!(x.decision, y.decision);
            assert_eq!(x.expected, y.expected);
        }
        // unique ids
        let mut ids: Vec<&str> = a.iter().map(|c| c.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 9);
    }

    #[test]
    fn on_variant_resurfaces_decision_off_does_not() {
        let c = &generate_cases(1)[0];
        let off = render_prompt(c, false, 200);
        let on = render_prompt(c, true, 200);
        assert!(
            !off.contains("[ctxrot anchor]"),
            "off must not carry the anchor"
        );
        assert!(on.contains("[ctxrot anchor]"), "on must carry the anchor");
        // The decision appears once in off (early) and twice in on (early + tail).
        assert_eq!(off.matches(&c.decision).count(), 1);
        assert_eq!(on.matches(&c.decision).count(), 2);
        // Filler actually buries the decision.
        assert!(on.chars().count() > 200);
    }

    #[test]
    fn scoring_tallies_accuracy_and_cost() {
        let cases = generate_cases(2);
        let anchor_bytes: Vec<(String, usize)> = cases
            .iter()
            .map(|c| (c.id.clone(), c.anchor_block().len()))
            .collect();
        let exp0 = cases[0].expected.clone();
        let exp1 = cases[1].expected.clone();
        let results = vec![
            // off: one right, one wrong
            ResultRec {
                id: cases[0].id.clone(),
                variant: "off".into(),
                answer: exp0.clone(),
            },
            ResultRec {
                id: cases[1].id.clone(),
                variant: "off".into(),
                answer: "わかりません".into(),
            },
            // on: both right
            ResultRec {
                id: cases[0].id.clone(),
                variant: "on".into(),
                answer: format!("答えは {exp0}"),
            },
            ResultRec {
                id: cases[1].id.clone(),
                variant: "on".into(),
                answer: exp1,
            },
            // unknown id / variant are ignored
            ResultRec {
                id: "case-99".into(),
                variant: "on".into(),
                answer: "x".into(),
            },
            ResultRec {
                id: cases[0].id.clone(),
                variant: "weird".into(),
                answer: "x".into(),
            },
        ];
        let r = score(&cases, &anchor_bytes, &results);
        assert_eq!(r.off_total, 2);
        assert_eq!(r.off_correct, 1);
        assert_eq!(r.on_total, 2);
        assert_eq!(r.on_correct, 2);
        let expected_bytes: usize = cases.iter().map(|c| c.anchor_block().len()).sum();
        assert_eq!(r.added_bytes, expected_bytes);
        assert_eq!(r.off_pct() as usize, 50);
        assert_eq!(r.on_pct() as usize, 100);
    }

    #[test]
    fn gen_writes_files_and_parseable_manifest() {
        // Auto-cleaned unique temp dir (atomic mkdtemp, no pid-collision TOCTOU).
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let n = run_gen(dir, 3, 100).unwrap();
        assert_eq!(n, 3);
        assert!(dir.join("case-00.on.txt").exists());
        assert!(dir.join("case-00.off.txt").exists());
        let m: Manifest =
            serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(m.cases.len(), 3);
        assert!(m.cases[0].anchor_bytes > 0);
    }
}
