use std::fs;
use std::path::Path;

/// Find crate names whose src/ is newer than their bin/<name>-linux-x86_64 binary.
///
/// For each `crates/<name>` directory that has BOTH a `src/` dir and a `bin/<name>-linux-x86_64` file,
/// returns `<name>` if the newest mtime under `src/` is more recent than the mtime of the binary.
/// Skips crates missing either `src/` or the binary.
/// On IO error for a crate, silently skips it (fail-soft).
#[allow(dead_code)]
pub fn stale_crates(repo: &Path) -> Vec<String> {
    let crates_dir = repo.join("crates");

    let mut stale = vec![];

    match fs::read_dir(&crates_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(crate_name) = path.file_name().and_then(|n| n.to_str()) {
                        if let Some(true) = check_stale_crate(&path, crate_name) {
                            stale.push(crate_name.to_string());
                        }
                    }
                }
            }
        }
        Err(_) => {
            // Fail-soft: if crates dir doesn't exist or can't be read, return empty
        }
    }

    stale
}

/// Check if a single crate is stale.
/// Returns Some(true) if stale, Some(false) if not stale, None if check couldn't be performed.
#[allow(dead_code)]
fn check_stale_crate(crate_path: &Path, crate_name: &str) -> Option<bool> {
    let src_dir = crate_path.join("src");
    let bin_file = crate_path.join(format!("bin/{}-linux-x86_64", crate_name));

    // Both src/ and bin must exist
    if !src_dir.exists() || !bin_file.exists() {
        return None;
    }

    // Get the newest mtime in src/
    let newest_src_mtime = get_newest_mtime_in_dir(&src_dir).ok()?;

    // Get the mtime of the binary
    let bin_mtime = fs::metadata(&bin_file).ok()?.modified().ok()?;

    // Return true if src is newer than bin
    Some(newest_src_mtime > bin_mtime)
}

/// Get the newest mtime of any file in a directory (recursively).
#[allow(dead_code)]
fn get_newest_mtime_in_dir(dir: &Path) -> std::io::Result<std::time::SystemTime> {
    let mut newest = std::time::SystemTime::UNIX_EPOCH;

    fn walk_dir(dir: &Path, newest: &mut std::time::SystemTime) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::metadata(&path)?;
            if let Ok(modified) = metadata.modified() {
                if modified > *newest {
                    *newest = modified;
                }
            }
            if path.is_dir() {
                walk_dir(&path, newest)?;
            }
        }
        Ok(())
    }

    walk_dir(dir, &mut newest)?;
    Ok(newest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn test_stale_crates_newer_src() {
        let temp_repo = TempDir::new().unwrap();
        let repo_path = temp_repo.path();

        // Create crates/foo directory
        let foo_crate = repo_path.join("crates/foo");
        fs::create_dir_all(&foo_crate).unwrap();

        // Create src/lib.rs
        let src_dir = foo_crate.join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("lib.rs"), "code").unwrap();

        // Create bin/foo-linux-x86_64
        let bin_dir = foo_crate.join("bin");
        fs::create_dir(&bin_dir).unwrap();
        let bin_file = bin_dir.join("foo-linux-x86_64");
        fs::write(&bin_file, "binary").unwrap();

        // Make sure src is newer than bin by sleeping and touching src
        thread::sleep(Duration::from_millis(10));
        fs::write(src_dir.join("lib.rs"), "newer code").unwrap();

        // Check that foo is detected as stale
        let stale = stale_crates(repo_path);
        assert!(stale.contains(&"foo".to_string()));
    }

    #[test]
    fn test_stale_crates_newer_bin() {
        let temp_repo = TempDir::new().unwrap();
        let repo_path = temp_repo.path();

        // Create crates/bar directory
        let bar_crate = repo_path.join("crates/bar");
        fs::create_dir_all(&bar_crate).unwrap();

        // Create src/lib.rs
        let src_dir = bar_crate.join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("lib.rs"), "code").unwrap();

        // Create bin/bar-linux-x86_64
        let bin_dir = bar_crate.join("bin");
        fs::create_dir(&bin_dir).unwrap();
        let bin_file = bin_dir.join("bar-linux-x86_64");
        fs::write(&bin_file, "binary").unwrap();

        // Make sure bin is newer than src by sleeping and touching bin
        thread::sleep(Duration::from_millis(10));
        fs::write(&bin_file, "newer binary").unwrap();

        // Check that bar is NOT detected as stale
        let stale = stale_crates(repo_path);
        assert!(!stale.contains(&"bar".to_string()));
    }

    #[test]
    fn test_stale_crates_missing_src() {
        let temp_repo = TempDir::new().unwrap();
        let repo_path = temp_repo.path();

        // Create crates/baz directory without src
        let baz_crate = repo_path.join("crates/baz");
        fs::create_dir_all(&baz_crate).unwrap();

        // Create bin/baz-linux-x86_64
        let bin_dir = baz_crate.join("bin");
        fs::create_dir(&bin_dir).unwrap();
        fs::write(bin_dir.join("baz-linux-x86_64"), "binary").unwrap();

        // Check that baz is NOT detected (missing src)
        let stale = stale_crates(repo_path);
        assert!(!stale.contains(&"baz".to_string()));
    }

    #[test]
    fn test_stale_crates_missing_bin() {
        let temp_repo = TempDir::new().unwrap();
        let repo_path = temp_repo.path();

        // Create crates/qux directory without bin
        let qux_crate = repo_path.join("crates/qux");
        fs::create_dir_all(&qux_crate).unwrap();

        // Create src/lib.rs
        let src_dir = qux_crate.join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::write(src_dir.join("lib.rs"), "code").unwrap();

        // Check that qux is NOT detected (missing bin)
        let stale = stale_crates(repo_path);
        assert!(!stale.contains(&"qux".to_string()));
    }

    #[test]
    fn test_stale_crates_empty_repo() {
        let temp_repo = TempDir::new().unwrap();
        let repo_path = temp_repo.path();

        // Check that empty repo returns empty vec
        let stale = stale_crates(repo_path);
        assert!(stale.is_empty());
    }
}
