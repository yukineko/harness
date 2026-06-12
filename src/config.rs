//! Configuration schema for precommit-audit.
//!
//! All knobs live in a single TOML file (default `.precommit-audit.toml` at the
//! repo root). Everything has a sensible built-in default, so a project with no
//! config file still gets the generic checks. Project-specific policy (e.g. an
//! API-namespace split or canonical `.env` paths) is expressed as data-driven
//! `[[rules]]`, never hard-coded into the binary.

use serde::Deserialize;
use std::path::Path;

/// Top-level configuration. Field defaults reproduce the generic behaviour of
/// the original PowerShell hook; project specifics come from the TOML file.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Directory holding hook state files (skip marker, block marker, audit
    /// log, review artifact). Claude Code convention is `.claude`.
    pub audit_dir: String,

    /// File classification (which extensions are source, what counts as a test,
    /// which paths are excluded from all scanning).
    pub classify: Classify,

    /// Enable/disable individual built-in checks.
    pub checks: Checks,

    pub hardcoded_ip: HardcodedIp,
    pub hardcoded_secret: HardcodedSecret,
    pub swallowed_error: SwallowedError,
    pub duplicate_function: DuplicateFunction,
    pub local_capture: LocalCapture,
    pub line_endings: LineEndings,
    pub file_length: FileLength,
    pub linters: Linters,

    /// Optional subagent-review contract (Claude Code specific). Disabled by
    /// default; opt in per project.
    pub review_contract: ReviewContract,

    /// Project-specific regex rules over added diff lines.
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            audit_dir: ".claude".into(),
            classify: Classify::default(),
            checks: Checks::default(),
            hardcoded_ip: HardcodedIp::default(),
            hardcoded_secret: HardcodedSecret::default(),
            swallowed_error: SwallowedError::default(),
            duplicate_function: DuplicateFunction::default(),
            local_capture: LocalCapture::default(),
            line_endings: LineEndings::default(),
            file_length: FileLength::default(),
            linters: Linters::default(),
            review_contract: ReviewContract::default(),
            rules: Vec::new(),
        }
    }
}

impl Config {
    /// Load config from `path`. Missing file => all defaults (Ok).
    pub fn load(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| format!("failed to parse {}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("failed to read {}: {e}", path.display())),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Classify {
    /// Extensions treated as auditable source (drives the missing-test check).
    pub source_exts: Vec<String>,
    /// Extensions scanned for red-flag added-line patterns / custom rules.
    pub scan_exts: Vec<String>,
    /// Substring patterns; any path containing one is excluded from everything.
    pub exclude: Vec<String>,
}

impl Default for Classify {
    fn default() -> Self {
        Classify {
            source_exts: strs(&[".py", ".ts", ".tsx", ".js", ".jsx", ".sh", ".ps1"]),
            scan_exts: strs(&[
                ".py", ".ts", ".tsx", ".js", ".jsx", ".sh", ".ps1", ".yml", ".yaml", ".toml",
                ".conf",
            ]),
            exclude: strs(&[
                "node_modules/",
                ".next/",
                "__pycache__/",
                "dist/",
                ".venv/",
                ".git/",
            ]),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Checks {
    pub missing_test: bool,
    pub hardcoded_ip: bool,
    pub hardcoded_secret: bool,
    pub swallowed_error: bool,
    pub duplicate_function: bool,
    pub local_capture: bool,
    pub markdown_links: bool,
    pub line_endings: bool,
    pub file_length: bool,
    pub custom_rules: bool,
    pub linters: bool,
}

impl Default for Checks {
    fn default() -> Self {
        // duplicate_function defaults OFF: it is heuristic and noisy without a
        // tuned allowlist. Projects opt in.
        Checks {
            missing_test: true,
            hardcoded_ip: true,
            hardcoded_secret: true,
            swallowed_error: true,
            duplicate_function: false,
            local_capture: true,
            markdown_links: true,
            line_endings: true,
            file_length: true,
            custom_rules: true,
            linters: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HardcodedIp {
    /// Prefixes treated as benign (RFC 5737 test-nets, loopback, public DNS).
    pub benign: Vec<String>,
}

impl Default for HardcodedIp {
    fn default() -> Self {
        HardcodedIp {
            benign: strs(&[
                "127.0.0.1",
                "0.0.0.0",
                "255.255.255",
                "8.8.8.8",
                "1.1.1.1",
                "192.0.2.",
                "198.51.100.",
                "203.0.113.",
            ]),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HardcodedSecret {
    /// If an otherwise-matching line contains any of these substrings it is
    /// treated as an env getter / placeholder, not a hard-coded secret.
    pub allow: Vec<String>,
}

impl Default for HardcodedSecret {
    fn default() -> Self {
        HardcodedSecret {
            allow: strs(&[
                "os.environ",
                "getenv",
                "process.env",
                "${",
                "<REDACTED>",
                "EXAMPLE",
                "CHANGEME",
            ]),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SwallowedError {
    /// Extra regexes (matched against the raw `+`-prefixed added line) that
    /// count as a swallowed error / fall-through, on top of the built-ins.
    pub extra_patterns: Vec<String>,
}

impl Default for SwallowedError {
    fn default() -> Self {
        SwallowedError {
            extra_patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DuplicateFunction {
    /// Function names that are routinely redefined per file and never flagged.
    pub common_names: Vec<String>,
}

impl Default for DuplicateFunction {
    fn default() -> Self {
        DuplicateFunction {
            common_names: strs(&[
                "__init__", "__main__", "setUp", "tearDown", "get", "post", "put", "delete",
                "list", "create", "update", "read", "write", "load", "save", "main", "init",
                "setup", "start", "stop", "run", "close", "open", "shutdown", "cleanup",
                "connect", "disconnect", "enable", "disable", "handle", "process", "transform",
                "wrap", "unwrap", "compute", "calculate", "check", "verify", "validate", "parse",
                "format", "encode", "decode", "serialize", "deserialize", "test",
            ]),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalCapture {
    /// Reason link surfaced in the message (project doc reference).
    pub doc_ref: String,
}

impl Default for LocalCapture {
    fn default() -> Self {
        LocalCapture {
            doc_ref: String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LineEndings {
    /// Extensions that must be CRLF (Windows scripts).
    pub crlf_exts: Vec<String>,
    /// Extensions that must be LF (POSIX shebang scripts).
    pub lf_exts: Vec<String>,
}

impl Default for LineEndings {
    fn default() -> Self {
        LineEndings {
            crlf_exts: strs(&[".ps1", ".cmd", ".bat"]),
            lf_exts: strs(&[".sh"]),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileLength {
    pub limit: usize,
}

impl Default for FileLength {
    fn default() -> Self {
        FileLength { limit: 500 }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Linters {
    pub py_compile: bool,
    pub ruff: bool,
    pub bash_n: bool,
    pub eslint: bool,
    pub tsc: bool,
    pub radon: bool,
    pub semgrep: bool,
    pub gitleaks: bool,
    /// Node project roots (relative to repo root) that own an eslint/tsc.
    pub node_projects: Vec<String>,
    /// Per-tool execution timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for Linters {
    fn default() -> Self {
        Linters {
            py_compile: true,
            ruff: true,
            bash_n: true,
            eslint: true,
            tsc: true,
            radon: true,
            semgrep: true,
            gitleaks: true,
            node_projects: Vec::new(),
            timeout_secs: 25,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewContract {
    pub enabled: bool,
    /// Path (relative to repo root) of the review artifact.
    pub path: String,
}

impl Default for ReviewContract {
    fn default() -> Self {
        ReviewContract {
            enabled: false,
            path: ".claude/last-review.json".into(),
        }
    }
}

/// Severity of a finding. `block` affects the exit code; `warn` never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Block,
    Warn,
}

fn default_severity() -> Severity {
    Severity::Block
}

/// A project-specific regex rule applied to added diff lines.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Short identifier used in the audit log category.
    pub id: String,
    /// Regex matched against the raw `+`-prefixed added line.
    pub pattern: String,
    /// If the line also matches any of these, it is exempt (allowlist).
    #[serde(default)]
    pub unless: Vec<String>,
    /// Only apply to files matching these globs (empty = all files).
    #[serde(default)]
    pub include_globs: Vec<String>,
    /// Never apply to files matching these globs.
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    /// Skip lines that look like comments (`#`, `//`, `--`, `*`, `;`).
    #[serde(default)]
    pub skip_comments: bool,
    #[serde(default = "default_severity")]
    pub severity: Severity,
    /// Human-facing message shown when the rule fires.
    pub message: String,
}

fn strs(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}
