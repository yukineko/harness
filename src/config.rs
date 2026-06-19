//! Configuration: defaults, `~/.ctxrot/config.toml`, and env overrides.
//!
//! Env overrides keep compatibility with the Python v1 hook:
//!   GUARD_DISABLE, CLAUDE_CONTEXT_WINDOW, GUARD_LARGE_FILE_BYTES

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Config {
    pub store_dir: PathBuf,
    pub state_dir: PathBuf,
    pub context_window: u64,
    pub large_file_bytes: u64,
    pub huge_tool_output_bytes: u64,
    /// PreToolUse hard gate: a `Read` of an unbounded (no `limit`) local file at
    /// or above this many bytes is denied, steering the model to a sub-agent.
    /// 0 disables the gate entirely.
    pub gate_file_bytes: u64,
    /// Append one JSONL metrics line per hook event to `<state_dir>/metrics.jsonl`
    /// (budget trajectory, band crossings, note sizes, gate denies). Local only.
    pub metrics: bool,
    /// ascending fractions of the window that trigger escalating advice
    pub bands: Vec<f64>,
}

/// On-disk form (`~/.ctxrot/config.toml`); every field optional.
#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    store_dir: Option<String>,
    state_dir: Option<String>,
    context_window: Option<u64>,
    large_file_bytes: Option<u64>,
    huge_tool_output_bytes: Option<u64>,
    gate_file_bytes: Option<u64>,
    metrics: Option<bool>,
    bands: Option<Vec<f64>>,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The `~/.ctxrot` base directory.
pub fn base_dir() -> PathBuf {
    home().join(".ctxrot")
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

impl Default for Config {
    fn default() -> Self {
        let base = base_dir();
        Config {
            store_dir: base.join("store"),
            state_dir: base.join("state"),
            context_window: 200_000,
            large_file_bytes: 50_000,
            huge_tool_output_bytes: 50_000,
            gate_file_bytes: 1_000_000,
            metrics: true,
            bands: vec![0.50, 0.75, 0.90],
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        base_dir().join("config.toml")
    }

    /// Load config from disk (if present) layered over defaults, then apply env
    /// overrides. Any read/parse error silently falls back to defaults.
    pub fn load() -> Self {
        let mut cfg = Config::default();

        if let Ok(text) = std::fs::read_to_string(Self::config_path()) {
            if let Ok(fc) = toml::from_str::<FileConfig>(&text) {
                if let Some(v) = fc.store_dir {
                    cfg.store_dir = expand_tilde(&v);
                }
                if let Some(v) = fc.state_dir {
                    cfg.state_dir = expand_tilde(&v);
                }
                if let Some(v) = fc.context_window {
                    cfg.context_window = v;
                }
                if let Some(v) = fc.large_file_bytes {
                    cfg.large_file_bytes = v;
                }
                if let Some(v) = fc.huge_tool_output_bytes {
                    cfg.huge_tool_output_bytes = v;
                }
                if let Some(v) = fc.gate_file_bytes {
                    cfg.gate_file_bytes = v;
                }
                if let Some(v) = fc.metrics {
                    cfg.metrics = v;
                }
                if let Some(v) = fc.bands {
                    if !v.is_empty() {
                        cfg.bands = v;
                    }
                }
            }
        }

        // env overrides (v1 compatibility)
        if let Some(v) = env_u64("CLAUDE_CONTEXT_WINDOW") {
            cfg.context_window = v;
        }
        if let Some(v) = env_u64("GUARD_LARGE_FILE_BYTES") {
            cfg.large_file_bytes = v;
        }
        if let Some(v) = env_u64("GUARD_GATE_FILE_BYTES") {
            cfg.gate_file_bytes = v;
        }
        if let Some(v) = env_bool("GUARD_METRICS") {
            cfg.metrics = v;
        }

        // bands must be ascending and within (0,1]; sanitize defensively
        cfg.bands.retain(|b| *b > 0.0 && *b <= 1.0);
        cfg.bands
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if cfg.bands.is_empty() {
            cfg.bands = vec![0.50, 0.75, 0.90];
        }
        if cfg.context_window == 0 {
            cfg.context_window = 200_000;
        }
        cfg
    }

    /// Hook is globally disabled via env.
    pub fn disabled() -> bool {
        std::env::var("GUARD_DISABLE")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    /// Band index (1-based) for a usage fraction; 0 means "below the lowest band".
    pub fn band_for(&self, frac: f64) -> usize {
        let mut band = 0;
        for (i, lo) in self.bands.iter().enumerate() {
            if frac >= *lo {
                band = i + 1;
            }
        }
        band
    }
}

fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key).ok()?.trim().parse::<u64>().ok()
}

/// Parse a boolean-ish env var: `0`/`false`/`no`/`off`/empty → false, else true.
fn env_bool(key: &str) -> Option<bool> {
    let v = std::env::var(key).ok()?;
    let v = v.trim().to_ascii_lowercase();
    Some(!matches!(v.as_str(), "" | "0" | "false" | "no" | "off"))
}
