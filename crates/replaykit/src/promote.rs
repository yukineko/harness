//! Derive an evalkit golden case from a run's replay fixture.
//!
//! Mirrors curate's emit conventions: a stable `slug_id`, and a golden whose
//! `cmd` re-runs the gate. Here the gate is `replaykit verify <fixture>`: the
//! committed golden points at a committed summary fixture (path *relative to
//! root*, so it travels with the repo), and replaying it recomputes and checks
//! the pinned invariants.

use std::hash::{Hash, Hasher};

use serde_json::{json, Value};

/// Build the evalkit golden case (as JSON) for a run. `fixture_rel_path` is the
/// summary fixture's path relative to the eval root, so the golden is portable.
pub fn derive_golden(run_id: &str, fixture_rel_path: &str) -> Value {
    json!({
        "id": slug_id(run_id),
        "describe": format!("{run_id} trajectory replay"),
        "cmd": ["replaykit", "verify", fixture_rel_path],
        "assert": { "exit": 0 },
    })
}

/// Stable, collision-resistant case id: an ASCII slug of `title` plus a short
/// hash (so non-ASCII titles that slug to the same stem stay distinct).
/// Mirrors curate's `slug_id` byte-for-byte.
pub fn slug_id(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !slug.is_empty() && !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let stem = slug.trim_matches('-');
    let stem = if stem.is_empty() { "case" } else { stem };

    let mut h = std::collections::hash_map::DefaultHasher::new();
    title.hash(&mut h);
    format!("{stem}-{:06x}", (h.finish() as u32) & 0xff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_shape_is_a_verify_gate() {
        let g = derive_golden("my-run", "evals/replay/fixtures/my-run-abc.json");
        assert_eq!(g["cmd"][0], json!("replaykit"));
        assert_eq!(g["cmd"][1], json!("verify"));
        assert_eq!(g["cmd"][2], json!("evals/replay/fixtures/my-run-abc.json"));
        assert_eq!(g["assert"]["exit"], json!(0));
        assert_eq!(g["describe"], json!("my-run trajectory replay"));
        assert_eq!(g["id"], json!(slug_id("my-run")));
    }

    #[test]
    fn slug_id_is_ascii_stable_and_unique() {
        let a = slug_id("condukt run サブ");
        let b = slug_id("condukt run something");
        assert!(a.starts_with("condukt-run"), "{a}");
        assert_ne!(a, b);
        assert_eq!(a, slug_id("condukt run サブ")); // stable
    }

    #[test]
    fn slug_id_has_six_hex_suffix() {
        let id = slug_id("demo");
        let (stem, hex) = id.rsplit_once('-').unwrap();
        assert_eq!(stem, "demo");
        assert_eq!(hex.len(), 6);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()), "{hex}");
    }

    #[test]
    fn pure_non_ascii_run_id_still_gets_an_id() {
        let id = slug_id("実行のみ");
        assert!(id.starts_with("case-"), "{id}");
    }
}
