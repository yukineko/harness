//! Configuration: project `reviewgate.toml` (preferred) layered over a
//! home-level `~/.reviewgate/config.toml`, over built-in defaults.
//!
//! Safe by default: with no config the gate reviews ordinary source changes,
//! but if nothing reviewable changed (or there is no git repo) it lets every
//! stop through. Installing the hook can never *trap* a turn on its own.

use std::path::{Path, PathBuf};

use serde::Deserialize;

// Re-exported so existing `crate::config::expand_tilde` call sites keep working.
pub use harness_core::config::expand_tilde;

/// How the review is actually performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Block the stop and inject a rubric; the running (subscription) agent
    /// reviews its own diff. No API key, no extra process.
    Inject,
    /// Spawn `reviewer_cmd` as an independent reviewer over the diff and inject
    /// only its findings (block only when issues are reported).
    Subprocess,
}

impl Mode {
    fn parse(s: &str) -> Mode {
        match s.trim().to_ascii_lowercase().as_str() {
            "subprocess" | "reviewer" | "independent" => Mode::Subprocess,
            _ => Mode::Inject,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Inject => "inject",
            Mode::Subprocess => "subprocess",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    pub mode: Mode,
    /// After this many consecutive review rounds in one session (the diff kept
    /// changing), give up and allow the stop so the agent isn't trapped.
    pub max_attempts: u32,
    /// The per-session counter resets if this many seconds pass between stops.
    pub reset_after_secs: i64,
    /// Don't bother reviewing fewer than this many reviewable files.
    pub min_changed_files: usize,
    /// Cap the diff fed to the hasher / reviewer (bytes). Larger diffs are
    /// truncated with a marker.
    pub max_diff_bytes: usize,
    /// Globs of files worth reviewing. A changed file must match one of these.
    pub include: Vec<String>,
    /// Globs that are never reviewed (lockfiles, vendored, generated…), applied
    /// after `include`.
    pub exclude: Vec<String>,
    /// The review checklist injected into the model / handed to the reviewer.
    pub rubric: String,
    /// subprocess mode: command line that receives the review prompt on stdin
    /// and prints findings on stdout. "LGTM" (or empty) = no issues.
    pub reviewer_cmd: String,
    pub reviewer_timeout_secs: u64,
    pub state_dir: PathBuf,
}

/// On-disk form; every field optional.
#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    enabled: Option<bool>,
    mode: Option<String>,
    max_attempts: Option<u32>,
    reset_after_secs: Option<i64>,
    min_changed_files: Option<usize>,
    max_diff_bytes: Option<usize>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    rubric: Option<String>,
    reviewer_cmd: Option<String>,
    reviewer_timeout_secs: Option<u64>,
    state_dir: Option<String>,
}

/// The `~/.reviewgate` base directory. Thin wrapper over the shared helper.
pub fn base_dir() -> PathBuf {
    harness_core::config::base_dir("reviewgate")
}

pub const DEFAULT_RUBRIC: &str = "\
- 正しさ: ロジックの誤り、境界・エッジケース、off-by-one、null/None、型の取り違え
- エラー処理: 失敗パスの握りつぶし、過剰な unwrap/expect、リソースリーク
- セキュリティ: 入力検証、インジェクション、機密の露出、安全でないデフォルト
- 並行性: データ競合、デッドロック、共有可変状態
- テスト: 新規ロジックのカバレッジ、回帰の防止
- 設計/簡潔さ: 重複、過剰な複雑さ、既存ユーティリティの再利用漏れ、命名";

fn default_include() -> Vec<String> {
    [
        "**/*.rs",
        "**/*.ts",
        "**/*.tsx",
        "**/*.js",
        "**/*.jsx",
        "**/*.py",
        "**/*.go",
        "**/*.java",
        "**/*.kt",
        "**/*.rb",
        "**/*.php",
        "**/*.c",
        "**/*.h",
        "**/*.cc",
        "**/*.cpp",
        "**/*.hpp",
        "**/*.cs",
        "**/*.swift",
        "**/*.scala",
        "**/*.sh",
        "**/*.sql",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_exclude() -> Vec<String> {
    [
        "**/*.lock",
        "**/*.min.js",
        "**/*.min.css",
        "**/Cargo.lock",
        "**/package-lock.json",
        "**/pnpm-lock.yaml",
        "**/yarn.lock",
        "**/node_modules/**",
        "**/target/**",
        "**/dist/**",
        "**/build/**",
        "**/vendor/**",
        "**/.venv/**",
        "**/*_pb2.py",
        "**/*.generated.*",
        "**/*.snap",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            mode: Mode::Inject,
            max_attempts: 2,
            reset_after_secs: 600,
            min_changed_files: 1,
            max_diff_bytes: 200_000,
            include: default_include(),
            exclude: default_exclude(),
            rubric: DEFAULT_RUBRIC.to_string(),
            reviewer_cmd: "claude -p".to_string(),
            reviewer_timeout_secs: 300,
            state_dir: base_dir().join("state"),
        }
    }
}

impl Config {
    pub fn project_path(root: &Path) -> PathBuf {
        root.join("reviewgate.toml")
    }

    pub fn home_path() -> PathBuf {
        base_dir().join("config.toml")
    }

    /// Load config for a project root. A project `reviewgate.toml` wins outright;
    /// otherwise the home config; otherwise built-in defaults. Any parse error
    /// silently falls back (the gate must never crash a turn).
    pub fn load(root: &Path) -> Self {
        let mut cfg = Config::default();

        let chosen = {
            let p = Config::project_path(root);
            if p.exists() {
                Some(p)
            } else {
                let h = Config::home_path();
                if h.exists() {
                    Some(h)
                } else {
                    None
                }
            }
        };

        if let Some(path) = chosen {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(fc) = toml::from_str::<FileConfig>(&text) {
                    if let Some(v) = fc.enabled {
                        cfg.enabled = v;
                    }
                    if let Some(v) = fc.mode {
                        cfg.mode = Mode::parse(&v);
                    }
                    if let Some(v) = fc.max_attempts {
                        cfg.max_attempts = v;
                    }
                    if let Some(v) = fc.reset_after_secs {
                        cfg.reset_after_secs = v;
                    }
                    if let Some(v) = fc.min_changed_files {
                        cfg.min_changed_files = v;
                    }
                    if let Some(v) = fc.max_diff_bytes {
                        cfg.max_diff_bytes = v;
                    }
                    if let Some(v) = fc.include {
                        cfg.include = v;
                    }
                    if let Some(v) = fc.exclude {
                        cfg.exclude = v;
                    }
                    if let Some(v) = fc.rubric {
                        if !v.trim().is_empty() {
                            cfg.rubric = v;
                        }
                    }
                    if let Some(v) = fc.reviewer_cmd {
                        cfg.reviewer_cmd = v;
                    }
                    if let Some(v) = fc.reviewer_timeout_secs {
                        cfg.reviewer_timeout_secs = v;
                    }
                    if let Some(v) = fc.state_dir {
                        cfg.state_dir = expand_tilde(&v);
                    }
                }
            }
        }

        // sanitize
        if cfg.max_attempts == 0 {
            cfg.max_attempts = 1;
        }
        if cfg.min_changed_files == 0 {
            cfg.min_changed_files = 1;
        }
        if cfg.max_diff_bytes == 0 {
            cfg.max_diff_bytes = 200_000;
        }
        if cfg.reviewer_timeout_secs == 0 {
            cfg.reviewer_timeout_secs = 300;
        }
        cfg
    }

    /// Globally disabled via env.
    pub fn disabled_env() -> bool {
        std::env::var("REVIEWGATE_DISABLE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    }
}
