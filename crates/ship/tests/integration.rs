//! Integration tests for the `ship` crate.
//!
//! The crux test is `binary_never_mutates_git`: a hard, textual safety scan
//! of this crate's own non-test source that asserts no state-mutating git
//! command (commit/merge/push/add/checkout/reset) is ever invoked. `ship`
//! only *detects* and *reports* unshipped state; the only thing it is
//! allowed to run is `scripts/rebuild-plugins.sh` via `--run-safe`.

use std::path::Path;

/// Subcommand args that would mutate repo state if passed to `git`.
const MUTATING_GIT_ARGS: &[&str] = &["commit", "merge", "push", "add", "checkout", "reset"];

/// Return the non-test portion of a source file: everything before the
/// first `#[cfg(test)]` marker. All of this crate's `mod tests` blocks live
/// at the end of their file, so this cleanly excludes test-only code (which
/// legitimately spins up tempdir git repos and calls `git add`/`git commit`
/// as test fixtures) while still scanning all production code.
fn non_test_source(contents: &str) -> &str {
    match contents.find("#[cfg(test)]") {
        Some(idx) => &contents[..idx],
        None => contents,
    }
}

/// Scan a chunk of source for a `.arg("<mutating-subcommand>")` immediately
/// preceded (within the file, not necessarily same line) by evidence of a
/// `Command::new("git")` builder. To keep this simple, robust, and hermetic
/// (per the task spec) we take the conservative approach: ANY occurrence of
/// `.arg("<mutating>")` in non-test production code is treated as a
/// violation, since this crate's git module only ever talks to `git` via
/// `Command::new("git")` builders — there is no other `.arg(...)` call site
/// that would legitimately use these exact literal subcommand strings.
fn find_mutating_git_args(source: &str) -> Vec<String> {
    let mut violations = vec![];
    for (lineno, line) in source.lines().enumerate() {
        for arg in MUTATING_GIT_ARGS {
            let needle = format!(".arg(\"{arg}\")");
            if line.contains(&needle) {
                violations.push(format!("line {}: {}", lineno + 1, line.trim()));
            }
        }
    }
    violations
}

#[test]
fn binary_never_mutates_git() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = [
        "src/main.rs",
        "src/checklist.rs",
        "src/git.rs",
        "src/stale.rs",
    ];

    let mut all_violations = vec![];

    for file in files {
        let path = manifest_dir.join(file);
        let contents = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let production_src = non_test_source(&contents);
        let violations = find_mutating_git_args(production_src);
        for v in violations {
            all_violations.push(format!("{}: {}", file, v));
        }
    }

    assert!(
        all_violations.is_empty(),
        "found state-mutating git command(s) in non-test ship source:\n{}",
        all_violations.join("\n")
    );
}

#[test]
fn non_test_source_excludes_test_module() {
    let sample = r#"
fn production() {
    // nothing mutating here
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    #[test]
    fn setup() {
        Command::new("git").arg("commit").output().unwrap();
    }
}
"#;
    let scanned = non_test_source(sample);
    assert!(!scanned.contains("mod tests"));
    assert!(find_mutating_git_args(scanned).is_empty());

    // Sanity: the full (unfiltered) sample WOULD have flagged it — proving
    // the detector logic itself works and isn't vacuously passing.
    assert!(!find_mutating_git_args(sample).is_empty());
}
