//! Config primitives shared by every plugin: home/base-dir resolution, tilde
//! expansion, and env-var parsing. Each plugin defines its OWN `Config` struct
//! with its own fields and load/merge, composing these helpers — only the common
//! primitives live here.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The `~/.<plugin>` base directory for a plugin's config/store/state.
pub fn base_dir(plugin: &str) -> PathBuf {
    home().join(format!(".{plugin}"))
}

/// Expand a leading `~` to the home directory.
pub fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        home().join(rest)
    } else if s == "~" {
        home()
    } else {
        PathBuf::from(s)
    }
}

/// Parse a `u64` env var, or None when unset/empty/unparseable.
pub fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key).ok()?.trim().parse::<u64>().ok()
}

/// Parse a boolean-ish env var: `0`/`false`/`no`/`off`/empty → false, else true.
pub fn env_bool(key: &str) -> Option<bool> {
    let v = std::env::var(key).ok()?;
    let v = v.trim().to_ascii_lowercase();
    Some(!matches!(v.as_str(), "" | "0" | "false" | "no" | "off"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_dir_is_dotprefixed_under_home() {
        assert_eq!(base_dir("ctxrot"), home().join(".ctxrot"));
    }

    #[test]
    fn expand_tilde_handles_home_forms() {
        assert_eq!(expand_tilde("~"), home());
        assert_eq!(expand_tilde("~/store"), home().join("store"));
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
    }

    #[test]
    fn env_parsers_are_lenient() {
        std::env::set_var("HARNESS_TEST_U64", " 42 ");
        std::env::set_var("HARNESS_TEST_BOOL", "off");
        assert_eq!(env_u64("HARNESS_TEST_U64"), Some(42));
        assert_eq!(env_bool("HARNESS_TEST_BOOL"), Some(false));
        assert_eq!(env_u64("HARNESS_TEST_UNSET_XYZ"), None);
        std::env::remove_var("HARNESS_TEST_U64");
        std::env::remove_var("HARNESS_TEST_BOOL");
    }
}
