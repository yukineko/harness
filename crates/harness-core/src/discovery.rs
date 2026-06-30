//! Machine-scope shared "discovery record" store. Append-only JSONL, fully fail-soft.
//!
//! Concurrent compass/scout sessions on one machine use this to avoid duplicating
//! discovered tasks. Records are keyed by task fingerprint (content hash of title)
//! and annotated with the discovering session and current status (Discovered or Selected).
//!
//! All operations are fail-soft: on any IO/lock/parse error, operations degrade
//! silently and never panic. Missing/corrupt files yield sensible defaults. This is
//! load-bearing — discovery may be called from hooks.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Status of a discovered task: either newly Discovered or Selected by a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Discovered,
    Selected,
}

/// A single discovery record: fingerprint, session, status, timestamp, and title.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryRecord {
    pub fingerprint: String,
    pub session_id: String,
    pub status: Status,
    pub created_at: u64,
    pub title: String,
}

/// Path to the discovery.jsonl store for a given cwd.
/// Returns `~/.compass/<project_key>/discovery.jsonl`.
pub fn record_path(cwd: &Path) -> PathBuf {
    crate::config::base_dir("compass")
        .join(crate::store::project_key(cwd))
        .join("discovery.jsonl")
}

/// Deterministic content hash of a title. Returns a hex string of the FNV-1a 64-bit
/// hash, formatted as 16 lowercase hex digits (64 bits).
pub fn fingerprint(title: &str) -> String {
    let h = crate::hash::fnv1a64(title.as_bytes());
    format!("{:016x}", h)
}

/// Append a discovery record to the store. Fails soft: on any IO or serialization
/// error, the record is silently dropped and discovery continues.
pub fn append(cwd: &Path, rec: &DiscoveryRecord) {
    append_at(&record_path(cwd), rec);
}

/// Internal: append to an explicit path. Used by append() and by tests.
fn append_at(path: &Path, rec: &DiscoveryRecord) {
    use std::io::Write;

    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Open or create the file in append mode.
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };

    // Serialize the record and write as a single JSON line.
    let Ok(json) = serde_json::to_string(rec) else {
        return;
    };

    // Write JSON line + newline. Swallow any write error.
    let _ = writeln!(file, "{}", json);
}

/// Load all discovery records from the store, returning only those that parsed
/// successfully. Missing file, empty file, or blank lines are silently skipped.
/// Corrupt JSON lines are also skipped. Never panics.
pub fn load(cwd: &Path) -> Vec<DiscoveryRecord> {
    load_at(&record_path(cwd))
}

/// Internal: load from an explicit path. Used by load() and by tests.
fn load_at(path: &Path) -> Vec<DiscoveryRecord> {
    let mut records = Vec::new();

    let Ok(contents) = std::fs::read_to_string(path) else {
        return records;
    };

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Skip corrupt/partial lines silently.
        if let Ok(rec) = serde_json::from_str::<DiscoveryRecord>(line) {
            records.push(rec);
        }
    }

    records
}

/// Check if another session has already discovered a task (and it hasn't been
/// superseded by selection yet). Returns true iff the store contains a record
/// with the same `fingerprint` whose `session_id != my_session` AND `status == Discovered`.
pub fn already_discovered_by_other(cwd: &Path, fingerprint: &str, my_session: &str) -> bool {
    let records = load(cwd);
    records.iter().any(|r| {
        r.fingerprint == fingerprint && r.session_id != my_session && r.status == Status::Discovered
    })
}

/// Mark a task as selected, rewriting the store atomically. All records matching
/// the fingerprint have their status set to Selected; all others remain unchanged.
/// Fails soft: on any error, leaves the file as-is and returns silently.
pub fn mark_selected(cwd: &Path, fingerprint: &str) {
    mark_selected_at(&record_path(cwd), fingerprint);
}

/// Internal: mark selected on an explicit path. Used by mark_selected() and by tests.
fn mark_selected_at(path: &Path, fingerprint: &str) {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    // Load all records.
    let mut records = load_at(path);

    // Update matching records to Selected.
    for r in &mut records {
        if r.fingerprint == fingerprint {
            r.status = Status::Selected;
        }
    }

    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Atomic write: write to a temp file, then rename.
    // Mirrors the style of store::save_json's atomic write pattern.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = path.with_extension(format!("jsonl.tmp.{}.{}", std::process::id(), seq));

    // Write all records to the temp file.
    let Ok(mut tmp_file) = std::fs::File::create(&tmp_path) else {
        return;
    };

    let mut wrote_ok = true;
    for rec in records {
        let Ok(json) = serde_json::to_string(&rec) else {
            wrote_ok = false;
            break;
        };
        if writeln!(tmp_file, "{}", json).is_err() {
            wrote_ok = false;
            break;
        }
    }

    // If write failed, clean up the temp file and return.
    if !wrote_ok {
        let _ = std::fs::remove_file(&tmp_path);
        return;
    }

    // Drop the file handle so the rename can succeed on Windows.
    drop(tmp_file);

    // Atomically rename the temp file to the target.
    let _ = std::fs::rename(&tmp_path, path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_transition_mark_selected_at() {
        // Create a temp directory and file.
        let tempdir = tempfile::tempdir().expect("tempfile::tempdir");
        let path = tempdir.path().join("discovery.jsonl");

        // Append 3 records from 2 sessions with different fingerprints.
        let rec_a = DiscoveryRecord {
            fingerprint: "aaa".to_string(),
            session_id: "session1".to_string(),
            status: Status::Discovered,
            created_at: 1000,
            title: "Task A".to_string(),
        };
        let rec_b = DiscoveryRecord {
            fingerprint: "bbb".to_string(),
            session_id: "session2".to_string(),
            status: Status::Discovered,
            created_at: 1001,
            title: "Task B".to_string(),
        };
        let rec_c = DiscoveryRecord {
            fingerprint: "ccc".to_string(),
            session_id: "session1".to_string(),
            status: Status::Discovered,
            created_at: 1002,
            title: "Task C".to_string(),
        };

        append_at(&path, &rec_a);
        append_at(&path, &rec_b);
        append_at(&path, &rec_c);

        // Mark C as selected.
        mark_selected_at(&path, "ccc");

        // Reload and verify: only C should be Selected, A and B remain Discovered.
        let records = load_at(&path);
        assert_eq!(records.len(), 3);

        let rec_a_loaded = &records[0];
        assert_eq!(rec_a_loaded.fingerprint, "aaa");
        assert_eq!(rec_a_loaded.status, Status::Discovered);

        let rec_b_loaded = &records[1];
        assert_eq!(rec_b_loaded.fingerprint, "bbb");
        assert_eq!(rec_b_loaded.status, Status::Discovered);

        let rec_c_loaded = &records[2];
        assert_eq!(rec_c_loaded.fingerprint, "ccc");
        assert_eq!(rec_c_loaded.status, Status::Selected);
    }

    #[test]
    fn fail_soft_missing_path() {
        // load_at on a missing path should return an empty Vec, not panic.
        let records = load_at(Path::new("/nonexistent/discovery.jsonl"));
        assert_eq!(records.len(), 0);

        // mark_selected_at on a missing path should not panic.
        mark_selected_at(Path::new("/nonexistent/discovery.jsonl"), "some-fp");
        // If we reach here, it didn't panic.
    }

    #[test]
    fn fail_soft_corrupt_lines() {
        // Create a temp file with a mix of valid and corrupt JSON lines.
        let tempdir = tempfile::tempdir().expect("tempfile::tempdir");
        let path = tempdir.path().join("discovery.jsonl");

        let valid_rec = DiscoveryRecord {
            fingerprint: "valid".to_string(),
            session_id: "s1".to_string(),
            status: Status::Discovered,
            created_at: 1000,
            title: "Valid".to_string(),
        };

        // Write manually: valid line, corrupt line, valid line.
        let valid_json = serde_json::to_string(&valid_rec).expect("to_string");
        let corrupt_line = r#"{"not": "valid json""#;
        let content = format!("{}\n{}\n{}\n", valid_json, corrupt_line, valid_json);
        std::fs::write(&path, content).expect("write");

        // load_at should skip the corrupt line and return 2 records.
        let records = load_at(&path);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].fingerprint, "valid");
        assert_eq!(records[1].fingerprint, "valid");
    }

    #[test]
    fn dedup_signal_already_discovered_by_other() {
        // Create a temp directory with two records: same fingerprint, different sessions.
        let tempdir = tempfile::tempdir().expect("tempfile::tempdir");
        let path = tempdir.path().join("discovery.jsonl");

        let rec_fp_s1 = DiscoveryRecord {
            fingerprint: "same-fp".to_string(),
            session_id: "session1".to_string(),
            status: Status::Discovered,
            created_at: 1000,
            title: "Task X".to_string(),
        };
        let rec_fp_s2 = DiscoveryRecord {
            fingerprint: "same-fp".to_string(),
            session_id: "session2".to_string(),
            status: Status::Discovered,
            created_at: 1001,
            title: "Task X".to_string(),
        };

        append_at(&path, &rec_fp_s1);
        append_at(&path, &rec_fp_s2);

        // Helper: simulate the already_discovered_by_other check on loaded records.
        let records = load_at(&path);
        let my_session = "session3";
        let found = records.iter().any(|r| {
            r.fingerprint == "same-fp"
                && r.session_id != my_session
                && r.status == Status::Discovered
        });
        assert!(
            found,
            "should find a record with same fp from a different session"
        );

        // Also test that it returns false if the fingerprint is not found.
        let found_other = records.iter().any(|r| {
            r.fingerprint == "different-fp"
                && r.session_id != my_session
                && r.status == Status::Discovered
        });
        assert!(!found_other, "should not find a non-existent fingerprint");

        // Test that it returns false if the status is Selected (not Discovered).
        mark_selected_at(&path, "same-fp");
        let records_after = load_at(&path);
        let found_selected = records_after.iter().any(|r| {
            r.fingerprint == "same-fp"
                && r.session_id != my_session
                && r.status == Status::Discovered
        });
        assert!(
            !found_selected,
            "should not find a record if it's already Selected"
        );
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let fp1 = fingerprint("hello");
        let fp2 = fingerprint("hello");
        assert_eq!(fp1, fp2, "same title should produce same fingerprint");

        let fp3 = fingerprint("world");
        assert_ne!(
            fp1, fp3,
            "different title should produce different fingerprint"
        );

        // Check that it's 16 hex digits (64-bit hash).
        assert_eq!(fp1.len(), 16, "fingerprint should be 16 hex digits");
        assert!(
            fp1.chars().all(|c| c.is_ascii_hexdigit()),
            "fingerprint should be all hex digits"
        );
    }

    #[test]
    fn record_path_includes_project_key() {
        let cwd = Path::new("/some/project/dir");
        let path = record_path(cwd);
        // Should be ~/.compass/<project_key>/discovery.jsonl
        assert!(path.to_string_lossy().contains(".compass"));
        assert!(path.to_string_lossy().ends_with("discovery.jsonl"));
    }
}
