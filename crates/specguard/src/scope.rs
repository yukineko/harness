//! Change-triggered scope resolution.
//!
//! The audit only looks at areas touched since a baseline ref (plus invariants,
//! which run every time). This keeps each run cheap and bounded instead of
//! re-auditing the whole tree. Git interaction is isolated in [`changed_files`]
//! so the area-classification logic ([`classify`]) stays pure and unit-testable.

use crate::config::{Area, Config};
use anyhow::{Context, Result};
use globset::{Glob, GlobSetBuilder};
use std::path::Path;
use std::process::Command;

/// The resolved scope for one audit run.
#[derive(Debug)]
pub struct Scope {
    /// The baseline ref actually used (after fallback resolution).
    pub baseline: String,
    /// Whether the configured/recorded baseline failed and we fell back.
    pub fell_back: bool,
    /// All files changed between `baseline` and HEAD.
    pub changed_files: Vec<String>,
    /// Indices into `config.areas` that are in scope, each with the changed
    /// files that landed in it (for prompt context).
    pub in_scope: Vec<AreaHit>,
    /// Names of areas with no changed files (reported as explicitly skipped).
    pub skipped_areas: Vec<String>,
    /// Decision record files (absolute paths) for the D3 audit; empty disables it.
    pub decision_files: Vec<String>,
}

#[derive(Debug)]
pub struct AreaHit {
    pub area_index: usize,
    /// Changed files matching the area's globs (implementation changes).
    pub matched_files: Vec<String>,
    /// Changed files that are this area's canon (spec changed → check the
    /// implementation still follows). An area is in scope if EITHER list is
    /// non-empty, so a pure-canon edit re-triggers the audit.
    pub changed_canon: Vec<String>,
}

/// Decide the baseline ref: explicit override > recorded last-ref > fallback.
pub fn resolve_baseline(cfg: &Config, override_ref: Option<&str>, last_ref: Option<&str>) -> String {
    if let Some(r) = override_ref {
        if !r.trim().is_empty() {
            return r.trim().to_string();
        }
    }
    if !cfg.scope.baseline_ref.trim().is_empty() {
        return cfg.scope.baseline_ref.trim().to_string();
    }
    if let Some(r) = last_ref {
        if !r.trim().is_empty() {
            return r.trim().to_string();
        }
    }
    cfg.scope.fallback_ref.clone()
}

/// Label used when the audit falls all the way back to "every tracked file"
/// (a young/shallow repo where neither baseline nor fallback ref resolves).
pub const ALL_TRACKED: &str = "(all tracked files)";

/// Resolve the set of changed files via a 3-tier fallback so a first run on a
/// young repo never hard-errors:
///   1. the requested `baseline` (`baseline..HEAD`),
///   2. the configured `fallback` ref,
///   3. all tracked files (`git ls-tree HEAD`).
///
/// Uses a two-dot diff (`baseline..HEAD`), i.e. "what HEAD changed relative to
/// baseline" — NOT three-dot, which diffs from the merge-base and would miss
/// changes that came in on the baseline side. Only committed state is audited;
/// uncommitted working-tree edits are out of scope by design.
///
/// Returns (files, ref-actually-used, fell_back).
pub fn changed_files(
    repo_root: &Path,
    baseline: &str,
    fallback: &str,
) -> Result<(Vec<String>, String, bool)> {
    if let Ok(files) = git_diff_names(repo_root, baseline) {
        return Ok((files, baseline.to_string(), false));
    }
    if fallback != baseline {
        if let Ok(files) = git_diff_names(repo_root, fallback) {
            return Ok((files, fallback.to_string(), true));
        }
    }
    let files = all_tracked_files(repo_root).with_context(|| {
        format!("could not resolve baseline '{baseline}' or fallback '{fallback}', and listing all tracked files failed")
    })?;
    Ok((files, ALL_TRACKED.to_string(), true))
}

fn git_diff_names(repo_root: &Path, baseline: &str) -> Result<Vec<String>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("diff")
        .arg("--name-only")
        .arg(format!("{baseline}..HEAD"))
        .output()
        .context("spawning git")?;
    if !out.status.success() {
        anyhow::bail!(
            "git diff {baseline}..HEAD failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(parse_name_list(&out.stdout))
}

/// All files tracked at HEAD. Used as the final fallback ("audit everything").
fn all_tracked_files(repo_root: &Path) -> Result<Vec<String>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .output()
        .context("spawning git ls-tree")?;
    if !out.status.success() {
        anyhow::bail!(
            "git ls-tree HEAD failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(parse_name_list(&out.stdout))
}

fn parse_name_list(stdout: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Record HEAD of `repo_root`, used to advance the baseline after a run.
pub fn current_head(repo_root: &Path) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .context("spawning git rev-parse")?;
    if !out.status.success() {
        anyhow::bail!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The file part of a canon pointer (`file` or `file:section`).
fn canon_file(pointer: &str) -> &str {
    pointer.split(':').next().unwrap_or(pointer).trim()
}

/// Pure: map changed files onto configured areas. Returns (in-scope hits,
/// skipped area names). An area is in scope iff a changed file matches its globs
/// OR one of its canon pointers' files changed (so a pure-spec edit re-triggers
/// the audit). Errors only on an invalid glob pattern in the config.
pub fn classify(changed: &[String], areas: &[Area]) -> Result<(Vec<AreaHit>, Vec<String>)> {
    let mut in_scope = Vec::new();
    let mut skipped = Vec::new();
    let changed_set: std::collections::HashSet<&str> =
        changed.iter().map(|s| s.as_str()).collect();

    for (idx, area) in areas.iter().enumerate() {
        let mut builder = GlobSetBuilder::new();
        for g in &area.globs {
            builder.add(
                Glob::new(g).with_context(|| format!("invalid glob '{g}' in area '{}'", area.name))?,
            );
        }
        let set = builder.build().context("building glob set")?;

        let matched: Vec<String> = changed
            .iter()
            .filter(|f| set.is_match(f.as_str()))
            .cloned()
            .collect();

        // Canon pointers whose file changed since the baseline (deduped).
        let mut changed_canon: Vec<String> = Vec::new();
        for c in &area.canon {
            let f = canon_file(c);
            if changed_set.contains(f) && !changed_canon.iter().any(|x| x == f) {
                changed_canon.push(f.to_string());
            }
        }

        if matched.is_empty() && changed_canon.is_empty() {
            skipped.push(area.name.clone());
        } else {
            in_scope.push(AreaHit {
                area_index: idx,
                matched_files: matched,
                changed_canon,
            });
        }
    }
    Ok((in_scope, skipped))
}

/// Full resolution: baseline -> diff -> classification.
pub fn resolve(
    cfg: &Config,
    repo_root: &Path,
    override_ref: Option<&str>,
    last_ref: Option<&str>,
) -> Result<Scope> {
    let requested = resolve_baseline(cfg, override_ref, last_ref);
    let (changed_files, baseline, fell_back) =
        changed_files(repo_root, &requested, &cfg.scope.fallback_ref)?;
    let (in_scope, skipped_areas) = classify(&changed_files, &cfg.areas)?;
    let decision_files = crate::decision::list_files(repo_root, &cfg.decisions.dir);
    Ok(Scope {
        baseline,
        fell_back,
        changed_files,
        in_scope,
        skipped_areas,
        decision_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Area;

    fn area(name: &str, globs: &[&str]) -> Area {
        Area {
            name: name.to_string(),
            globs: globs.iter().map(|s| s.to_string()).collect(),
            canon: vec![],
        }
    }

    fn area_with_canon(name: &str, globs: &[&str], canon: &[&str]) -> Area {
        Area {
            name: name.to_string(),
            globs: globs.iter().map(|s| s.to_string()).collect(),
            canon: canon.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn classify_matches_recursive_glob() {
        let areas = vec![area("logging", &["aegis_logging/**"]), area("web", &["web/**"])];
        let changed = vec![
            "aegis_logging/signature.py".to_string(),
            "aegis_logging/vector/cfg.toml".to_string(),
            "README.md".to_string(),
        ];
        let (hits, skipped) = classify(&changed, &areas).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].area_index, 0);
        assert_eq!(hits[0].matched_files.len(), 2);
        assert!(hits[0].changed_canon.is_empty());
        assert_eq!(skipped, vec!["web".to_string()]);
    }

    #[test]
    fn classify_triggers_on_canon_change_only() {
        // No code change in the area, but its canon doc changed -> in scope, via
        // changed_canon (the file part of a `file:section` pointer matches).
        let areas = vec![area_with_canon(
            "logging",
            &["aegis_logging/**"],
            &["docs/logging.md:HMAC", "docs/glossary.md"],
        )];
        let changed = vec!["docs/logging.md".to_string()];
        let (hits, skipped) = classify(&changed, &areas).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].matched_files.is_empty(), "no impl change");
        assert_eq!(hits[0].changed_canon, vec!["docs/logging.md".to_string()]);
        assert!(skipped.is_empty());
    }

    #[test]
    fn classify_empty_diff_skips_all() {
        let areas = vec![area("a", &["a/**"])];
        let (hits, skipped) = classify(&[], &areas).unwrap();
        assert!(hits.is_empty());
        assert_eq!(skipped, vec!["a".to_string()]);
    }

    #[test]
    fn classify_multiple_globs_per_area() {
        let areas = vec![area("api", &["admin/**", "web/api/**"])];
        let changed = vec!["web/api/users.py".to_string()];
        let (hits, _) = classify(&changed, &areas).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn resolve_baseline_precedence() {
        let mut cfg: Config = toml::from_str(
            r#"
            [project]
            name = "x"
            [[area]]
            name = "a"
            globs = ["a/**"]
            "#,
        )
        .unwrap();
        // override wins
        assert_eq!(resolve_baseline(&cfg, Some("abc"), Some("zzz")), "abc");
        // then config.baseline_ref
        cfg.scope.baseline_ref = "cfgref".to_string();
        assert_eq!(resolve_baseline(&cfg, None, Some("zzz")), "cfgref");
        // then last_ref
        cfg.scope.baseline_ref = "".to_string();
        assert_eq!(resolve_baseline(&cfg, None, Some("zzz")), "zzz");
        // then fallback
        assert_eq!(resolve_baseline(&cfg, None, None), "HEAD~20");
    }
}
