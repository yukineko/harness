//! ① gather — the deterministic intake stage (DESIGN-INTAKE.md §1–§3).
//!
//! Cross-source requirement composition WITHOUT an LLM: walk three sources
//! (Obsidian decisions/sessions, repo canon docs, past Claude Code prompts),
//! attach provenance + authority, score each fragment against the topic by
//! lexical overlap (no embeddings), and bundle the top matches for the later
//! pre-flight / interrogate stages to chew on.
//!
//! Authority and score are ORTHOGONAL (DESIGN-INTAKE.md §3): authority is the
//! source's *strength* (Obsidian decision > repo canon > past prompt), score is
//! topic *relevance*. Presentation order is (authority desc, score desc).
//!
//! INVARIANT (DESIGN-INTAKE.md §3.1, principle 5): gather does NOT resolve
//! conflicts and does NOT use authority to drop fragments. Authority only
//! affects ordering — the machine never decides "the higher authority wins".
//! Conflicts are surfaced to the human later; here we only collect and rank.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Stage marker for the §8 contract (DESIGN-INTAKE.md §8). The trailer carries
/// `bundle_path:` / `fragment_count:`; emitted by the `gather` subcommand.
pub const MARKER: &str = "<<<SPEC_GATHER>>>";

/// Source strength (DESIGN-INTAKE.md §3 table). Maps directly onto the three
/// sources: obsidian-decision (High) / repo-canon (Mid) / past-prompt (Low).
/// `Ord` is derived so High > Mid > Low, used ONLY for ordering (never to drop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Authority {
    /// Obsidian AEGIS decisions / sessions — confirmed decisions and rationale.
    High,
    /// repo canon docs — the spec closest to the code.
    Mid,
    /// past Claude Code prompts — weak intent hints; never grounded on alone.
    Low,
}

/// One material fragment with provenance (DESIGN-INTAKE.md §3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Fragment {
    /// The collected text snippet.
    pub text: String,
    /// Where it came from (file path, or transcript path).
    pub source_path: String,
    /// Source strength.
    pub authority: Authority,
    /// Topic relevance (lexical overlap count).
    pub score: i64,
    /// A short locator within the source (heading line / line number / "prompt").
    pub anchor: String,
}

/// The gathered material bundle, persisted as JSON (matching the forge
/// precedent for persisted artifacts: impl results / evidence are serde_json).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bundle {
    pub topic: String,
    pub fragments: Vec<Fragment>,
}

/// Knobs the gather stage needs (DESIGN-INTAKE.md §7 `[sources]` / `[gather]`).
/// Resolved/absolute paths are passed in so this module is filesystem-pure given
/// its inputs and easy to test.
pub struct GatherInput {
    /// `<vault>` — `<vault>/AEGIS/{decisions,sessions}` are walked (authority High).
    pub obsidian_vault: Option<PathBuf>,
    /// repo canon globs (e.g. `docs/**/*.md`), resolved relative to repo root
    /// by the caller (authority Mid).
    pub canon_root: PathBuf,
    pub canon_globs: Vec<String>,
    /// `<transcripts>/<enc-cwd>/` — past-prompt jsonl dir (authority Low).
    pub transcripts_dir: Option<PathBuf>,
    pub top_k: usize,
    pub min_score: i64,
}

// ── source walking ─────────────────────────────────────────────────────────

/// Encode a cwd the way Claude Code names its transcript directory: every run of
/// non-alphanumeric characters collapses to a single `-`, with no leading dash
/// kept beyond the one a leading `/` produces. e.g.
/// `/mnt/c/Users/hiroyuki_nakayama/src/harness`
///   → `-mnt-c-Users-hiroyuki-nakayama-src-harness`.
pub fn encode_cwd(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out
}

/// Walk a directory recursively, collecting files whose name ends in `suffix`.
/// A small std-only walk so we don't add a glob/walkdir dependency for the
/// (simple) directory patterns gather needs (DESIGN-INTAKE.md §7 note).
fn walk_suffix(dir: &Path, suffix: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    // Sort for deterministic traversal order.
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let path = e.path();
        if path.is_dir() {
            walk_suffix(&path, suffix, out);
        } else if path.to_string_lossy().ends_with(suffix) {
            out.push(path);
        }
    }
}

/// Read one markdown file into per-heading fragments. We split on top-level `#`
/// headings so each fragment is a coherent unit with a heading anchor; a file
/// without headings becomes one fragment.
fn md_fragments(path: &Path, authority: Authority) -> Vec<RawFragment> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let source = path.to_string_lossy().to_string();
    let mut out = Vec::new();
    let mut anchor = String::from("(top)");
    let mut buf = String::new();
    let flush = |anchor: &str, buf: &str, out: &mut Vec<RawFragment>| {
        if !buf.trim().is_empty() {
            out.push(RawFragment {
                text: buf.trim().to_string(),
                source_path: String::new(), // filled by caller
                authority,
                anchor: anchor.to_string(),
            });
        }
    };
    for line in text.lines() {
        if line.trim_start().starts_with('#') {
            flush(&anchor, &buf, &mut out);
            buf.clear();
            anchor = line.trim_start_matches('#').trim().to_string();
            if anchor.is_empty() {
                anchor = "(heading)".to_string();
            }
        }
        buf.push_str(line);
        buf.push('\n');
    }
    flush(&anchor, &buf, &mut out);
    for f in &mut out {
        f.source_path = source.clone();
    }
    out
}

/// A fragment before scoring (no score yet).
struct RawFragment {
    text: String,
    source_path: String,
    authority: Authority,
    anchor: String,
}

/// Collect Obsidian fragments (authority High): `<vault>/AEGIS/{decisions,sessions}/**/*.md`.
fn gather_obsidian(vault: &Path) -> Vec<RawFragment> {
    let mut out = Vec::new();
    for sub in ["AEGIS/decisions", "AEGIS/sessions"] {
        let dir = vault.join(sub);
        let mut files = Vec::new();
        walk_suffix(&dir, ".md", &mut files);
        for f in files {
            out.extend(md_fragments(&f, Authority::High));
        }
    }
    out
}

/// Collect repo-canon fragments (authority Mid) from the `canon` globs. We only
/// honor the leaf suffix of each glob (e.g. `docs/**/*.md` → walk `docs` for
/// `*.md`) so a simple recursive walk suffices — no glob crate (DESIGN §7).
fn gather_canon(root: &Path, globs: &[String]) -> Vec<RawFragment> {
    let mut out = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for glob in globs {
        let (base, suffix) = split_glob(glob);
        let dir = root.join(base);
        let mut files = Vec::new();
        walk_suffix(&dir, &suffix, &mut files);
        for f in files {
            if seen.insert(f.clone()) {
                out.extend(md_fragments(&f, Authority::Mid));
            }
        }
    }
    out
}

/// Split a simple glob into (base dir before any wildcard, file suffix). e.g.
/// `docs/**/*.md` → (`docs`, `.md`); `docs/*.md` → (`docs`, `.md`);
/// `README.md` → (``, `README.md`). Only the leaf suffix is matched (DESIGN §7
/// allows a simple recursive walk instead of a glob dependency).
fn split_glob(glob: &str) -> (String, String) {
    let parts: Vec<&str> = glob.split('/').collect();
    let last = parts.last().copied().unwrap_or("");
    // base = leading directory components (everything but the final filename
    // pattern), stopping at the first one that contains a wildcard.
    let mut base = Vec::new();
    for p in &parts[..parts.len().saturating_sub(1)] {
        if p.contains('*') {
            break;
        }
        base.push(*p);
    }
    // suffix: the part of the last component after the final '*', else the whole.
    let suffix = match last.rsplit_once('*') {
        Some((_, s)) => s.to_string(),
        None => last.to_string(),
    };
    (base.join("/"), suffix)
}

/// Collect past-prompt fragments (authority Low) from Claude Code transcripts:
/// every `*.jsonl` under `<transcripts>/<enc-cwd>/`. Each line is best-effort
/// JSON; we pull only user-prompt text and skip anything that doesn't parse.
fn gather_transcripts(dir: &Path) -> Vec<RawFragment> {
    let mut files = Vec::new();
    walk_suffix(dir, ".jsonl", &mut files);
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let source = f.to_string_lossy().to_string();
        for line in text.lines() {
            if let Some(prompt) = extract_prompt(line) {
                if !prompt.trim().is_empty() {
                    out.push(RawFragment {
                        text: prompt,
                        source_path: source.clone(),
                        authority: Authority::Low,
                        anchor: "prompt".to_string(),
                    });
                }
            }
        }
    }
    out
}

/// Best-effort user-prompt extraction from one transcript jsonl line. Returns
/// `None` for lines that don't parse or carry no user-prompt text. Transcripts
/// are the weakest source (DESIGN §3) so we are conservative: only user turns.
pub fn extract_prompt(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;

    // Claude Code transcript entries vary; cover the common shapes:
    //   { "type": "user", "message": { "role": "user", "content": "..." } }
    //   { "type": "user", "message": { "content": [ {"type":"text","text":".."} ] } }
    //   { "role": "user", "content": "..." }
    //   { "prompt": "..." }  (UserPromptSubmit-style hook payloads)
    if let Some(p) = v.get("prompt").and_then(|p| p.as_str()) {
        return Some(p.to_string());
    }

    let is_user = v.get("type").and_then(|t| t.as_str()) == Some("user")
        || v.get("role").and_then(|r| r.as_str()) == Some("user")
        || v.get("message")
            .and_then(|m| m.get("role"))
            .and_then(|r| r.as_str())
            == Some("user");
    if !is_user {
        return None;
    }

    // content lives either at top level or under `message`.
    let content = v
        .get("message")
        .and_then(|m| m.get("content"))
        .or_else(|| v.get("content"))?;
    Some(content_text(content)).filter(|s| !s.is_empty())
}

/// Flatten a transcript `content` field (string or array of text parts) to plain
/// text. Non-text parts (tool_use, images, …) are ignored.
fn content_text(content: &serde_json::Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                    parts.push(t.to_string());
                }
            }
        }
        return parts.join("\n");
    }
    String::new()
}

// ── lexical scoring (DESIGN-INTAKE.md §3: topic × fragment overlap, CJK per-char) ──

const STOP: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "you", "your", "are", "was", "from", "have",
    "してください", "を", "に", "は", "が", "の", "で", "と", "も", "して", "する", "した",
];

/// Tokenize to a lowercase set: ASCII alphanumeric words (len ≥ 2) plus CJK
/// per-character. Mirrors the playbook/specguard idiom (reimplemented locally to
/// avoid a cross-crate dependency, DESIGN brief).
pub fn tokenize(s: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, set: &mut HashSet<String>| {
        if cur.chars().count() >= 2 && !STOP.contains(&cur.as_str()) {
            set.insert(cur.clone());
        }
        cur.clear();
    };
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            cur.extend(c.to_lowercase());
        } else if is_cjk(c) {
            if !cur.is_empty() {
                flush(&mut cur, &mut set);
            }
            if !STOP.contains(&c.to_string().as_str()) {
                set.insert(c.to_string());
            }
        } else {
            flush(&mut cur, &mut set);
        }
    }
    flush(&mut cur, &mut set);
    set
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30ff |   // hiragana + katakana
        0x4e00..=0x9fff |   // CJK unified
        0xff66..=0xff9d) // halfwidth katakana
}

/// Relevance = count of topic tokens that also appear in the fragment (simple
/// overlap, matching the sibling idiom). authority is NOT consulted here — score
/// and authority are orthogonal (DESIGN-INTAKE.md §3).
fn score(topic_toks: &HashSet<String>, text: &str) -> i64 {
    let frag = tokenize(text);
    topic_toks.iter().filter(|t| frag.contains(*t)).count() as i64
}

// ── driver ─────────────────────────────────────────────────────────────────

/// Run the deterministic gather: walk all three sources, score against `topic`,
/// drop fragments below `min_score`, and keep the top_k ordered by
/// (authority desc, score desc). No LLM, no conflict resolution.
pub fn gather(topic: &str, input: &GatherInput) -> Bundle {
    let mut raws: Vec<RawFragment> = Vec::new();
    if let Some(vault) = &input.obsidian_vault {
        raws.extend(gather_obsidian(vault));
    }
    raws.extend(gather_canon(&input.canon_root, &input.canon_globs));
    if let Some(tdir) = &input.transcripts_dir {
        raws.extend(gather_transcripts(tdir));
    }
    Bundle {
        topic: topic.to_string(),
        fragments: rank(topic, raws, input.min_score, input.top_k),
    }
}

/// Score, threshold, and rank raw fragments. Split out so tests can drive
/// ranking/ordering without touching the filesystem.
fn rank(topic: &str, raws: Vec<RawFragment>, min_score: i64, top_k: usize) -> Vec<Fragment> {
    let topic_toks = tokenize(topic);
    let mut scored: Vec<Fragment> = raws
        .into_iter()
        .map(|r| {
            let s = score(&topic_toks, &r.text);
            Fragment {
                text: r.text,
                source_path: r.source_path,
                authority: r.authority,
                score: s,
                anchor: r.anchor,
            }
        })
        // min_score is a *relevance* floor; authority is NEVER used to drop
        // (DESIGN-INTAKE.md §3.1 / principle 5).
        .filter(|f| f.score >= min_score)
        .collect();

    // Order: authority desc (High first), then score desc, then a deterministic
    // tiebreak on source_path + anchor for stable output.
    scored.sort_by(|a, b| {
        a.authority
            .cmp(&b.authority) // High < Mid < Low in derive order → ascending = High first
            .then(b.score.cmp(&a.score))
            .then(a.source_path.cmp(&b.source_path))
            .then(a.anchor.cmp(&b.anchor))
    });
    scored.truncate(top_k);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str, auth: Authority, src: &str) -> RawFragment {
        RawFragment {
            text: text.to_string(),
            source_path: src.to_string(),
            authority: auth,
            anchor: "a".to_string(),
        }
    }

    #[test]
    fn encode_cwd_matches_claude_code() {
        assert_eq!(
            encode_cwd(Path::new("/mnt/c/Users/hiroyuki_nakayama/src/harness")),
            "-mnt-c-Users-hiroyuki-nakayama-src-harness"
        );
        // dots and other punctuation collapse to a single dash too.
        assert_eq!(encode_cwd(Path::new("/a/b.c/d")), "-a-b-c-d");
        assert_eq!(encode_cwd(Path::new("relative/path")), "relative-path");
    }

    #[test]
    fn split_glob_handles_common_shapes() {
        assert_eq!(split_glob("docs/**/*.md"), ("docs".into(), ".md".into()));
        assert_eq!(split_glob("docs/*.md"), ("docs".into(), ".md".into()));
        assert_eq!(split_glob("README.md"), ("".into(), "README.md".into()));
    }

    #[test]
    fn tokenize_english_and_cjk() {
        let t = tokenize("Rate limit ログイン 試行");
        assert!(t.contains("rate"));
        assert!(t.contains("limit"));
        // CJK indexed per char.
        assert!(t.contains("ロ"));
        assert!(t.contains("試"));
        assert!(t.contains("行"));
        // stopword particle dropped.
        assert!(!t.contains("を"));
    }

    #[test]
    fn score_matches_japanese_topic_to_japanese_fragment() {
        let topic = tokenize("レート制限 ログイン");
        let hit = score(&topic, "ログイン試行のレート制限を実装する");
        let miss = score(&topic, "全く無関係な英語 text only");
        assert!(hit > 0, "Japanese topic should match Japanese fragment");
        assert_eq!(miss, 0, "unrelated fragment scores zero");
    }

    #[test]
    fn authority_orders_but_never_drops() {
        // A Low fragment with a HIGH score must still sort AFTER a High fragment
        // with a lower score, and BOTH must survive (authority never drops).
        let topic = "alpha beta gamma";
        let raws = vec![
            raw("alpha beta gamma extra", Authority::Low, "prompt.jsonl"), // score 3
            raw("alpha only", Authority::High, "decision.md"),             // score 1
        ];
        let out = rank(topic, raws, 1, 10);
        assert_eq!(out.len(), 2, "authority must not drop the lower-authority fragment");
        assert_eq!(out[0].authority, Authority::High, "High sorts first despite lower score");
        assert_eq!(out[1].authority, Authority::Low);
        assert!(out[1].score > out[0].score, "the Low one is actually more relevant");
    }

    #[test]
    fn min_score_drops_only_on_relevance() {
        let topic = "needle";
        let raws = vec![
            raw("needle present", Authority::Low, "a"),  // score 1
            raw("nothing matching", Authority::High, "b"), // score 0 → dropped
        ];
        let out = rank(topic, raws, 1, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].authority, Authority::Low);
    }

    #[test]
    fn top_k_caps_after_ranking() {
        let topic = "needle";
        let raws = vec![
            raw("needle", Authority::High, "a"),
            raw("needle", Authority::Mid, "b"),
            raw("needle", Authority::Low, "c"),
        ];
        let out = rank(topic, raws, 1, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].authority, Authority::High);
        assert_eq!(out[1].authority, Authority::Mid);
    }

    #[test]
    fn extract_prompt_pulls_user_text_only() {
        let user = r#"{"type":"user","message":{"role":"user","content":"レート制限を実装"}}"#;
        assert_eq!(extract_prompt(user).as_deref(), Some("レート制限を実装"));

        let user_parts = r#"{"role":"user","content":[{"type":"text","text":"hello"},{"type":"tool_result","content":"ignored"}]}"#;
        assert_eq!(extract_prompt(user_parts).as_deref(), Some("hello"));

        let hook = r#"{"prompt":"direct prompt"}"#;
        assert_eq!(extract_prompt(hook).as_deref(), Some("direct prompt"));

        // assistant turns and garbage produce nothing.
        assert_eq!(extract_prompt(r#"{"type":"assistant","message":{"content":"hi"}}"#), None);
        assert_eq!(extract_prompt("not json at all"), None);
    }

    #[test]
    fn gather_walks_sources_and_bundles() {
        // Filesystem-backed end-to-end gather using a temp vault + canon dir.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Obsidian decision (High).
        let dec = root.join("vault/AEGIS/decisions");
        std::fs::create_dir_all(&dec).unwrap();
        std::fs::write(
            dec.join("d1.md"),
            "# レート制限の決定\n429 を返す。レート制限の閾値は 5。\n",
        )
        .unwrap();

        // repo canon (Mid).
        let docs = root.join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("auth.md"), "# Auth\nrate limit policy here\n").unwrap();
        std::fs::write(docs.join("unrelated.md"), "# Misc\nnothing relevant\n").unwrap();

        // transcripts (Low) under <transcripts>/<enc-cwd>/.
        let tdir = root.join("transcripts").join(encode_cwd(Path::new("/some/cwd")));
        std::fs::create_dir_all(&tdir).unwrap();
        std::fs::write(
            tdir.join("s.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"レート制限を入れたい\"}}\nnot-json\n",
        )
        .unwrap();

        let input = GatherInput {
            obsidian_vault: Some(root.join("vault")),
            canon_root: root.to_path_buf(),
            canon_globs: vec!["docs/**/*.md".to_string()],
            transcripts_dir: Some(tdir.clone()),
            top_k: 24,
            min_score: 1,
        };
        let bundle = gather("レート制限", &input);

        assert_eq!(bundle.topic, "レート制限");
        assert!(!bundle.fragments.is_empty());
        // The unrelated canon doc scored 0 and was dropped.
        assert!(bundle
            .fragments
            .iter()
            .all(|f| !f.source_path.contains("unrelated")));
        // High authority (Obsidian) sorts first.
        assert_eq!(bundle.fragments[0].authority, Authority::High);
        // JSON round-trips.
        let json = serde_json::to_string_pretty(&bundle).unwrap();
        let back: Bundle = serde_json::from_str(&json).unwrap();
        assert_eq!(bundle, back);
    }
}
