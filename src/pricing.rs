//! Cost estimation. A built-in table maps a model id to USD-per-1M-token input
//! and output rates; cache writes bill at 1.25x input (5-minute TTL) or 2x
//! (1-hour TTL), cache reads at 0.1x input — the standard Claude cache
//! economics. Config can override or extend the table per model.
//!
//! Built-in rates (USD per 1M tokens, input/output):
//!   Fable / Mythos  10 / 50
//!   Opus            5 / 25
//!   Sonnet          3 / 15
//!   Haiku           1 / 5
//! An unrecognized model contributes 0 (so an unknown id never invents cost).

use crate::config::PriceOverride;
use crate::store::Usage;

const CACHE_WRITE_5M_MULT: f64 = 1.25;
const CACHE_WRITE_1H_MULT: f64 = 2.0;
const CACHE_READ_MULT: f64 = 0.1;

#[derive(Debug, Clone, Copy)]
pub struct Rate {
    /// USD per 1M input tokens.
    pub input: f64,
    /// USD per 1M output tokens.
    pub output: f64,
}

fn builtin_rate(model: &str) -> Rate {
    let m = model.to_lowercase();
    if m.contains("fable") || m.contains("mythos") {
        Rate { input: 10.0, output: 50.0 }
    } else if m.contains("opus") {
        Rate { input: 5.0, output: 25.0 }
    } else if m.contains("sonnet") {
        Rate { input: 3.0, output: 15.0 }
    } else if m.contains("haiku") {
        Rate { input: 1.0, output: 5.0 }
    } else {
        Rate { input: 0.0, output: 0.0 }
    }
}

/// Resolve the rate for a model: the first matching override (substring), else
/// the built-in table.
pub fn rate_for(model: &str, overrides: &[PriceOverride]) -> Rate {
    let m = model.to_lowercase();
    for o in overrides {
        if m.contains(&o.pattern) {
            return Rate {
                input: o.input,
                output: o.output,
            };
        }
    }
    builtin_rate(model)
}

/// Estimated USD cost of one model's usage within a session.
pub fn cost(model: &str, u: &Usage, overrides: &[PriceOverride]) -> f64 {
    let r = rate_for(model, overrides);
    let per_token_in = r.input / 1_000_000.0;
    let per_token_out = r.output / 1_000_000.0;
    u.input as f64 * per_token_in
        + u.output as f64 * per_token_out
        + u.cache_write_5m as f64 * per_token_in * CACHE_WRITE_5M_MULT
        + u.cache_write_1h as f64 * per_token_in * CACHE_WRITE_1H_MULT
        + u.cache_read as f64 * per_token_in * CACHE_READ_MULT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_input_output() {
        let u = Usage {
            input: 1_000_000,
            output: 1_000_000,
            ..Default::default()
        };
        // 5 + 25
        assert!((cost("claude-opus-4-8", &u, &[]) - 30.0).abs() < 1e-9);
    }

    #[test]
    fn cache_read_is_cheap() {
        let u = Usage {
            cache_read: 1_000_000,
            ..Default::default()
        };
        // 5 * 0.1
        assert!((cost("claude-opus-4-8", &u, &[]) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn override_wins() {
        let ov = vec![PriceOverride {
            pattern: "opus".into(),
            input: 1.0,
            output: 1.0,
        }];
        let u = Usage {
            input: 1_000_000,
            ..Default::default()
        };
        assert!((cost("claude-opus-4-8", &u, &ov) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unknown_model_is_free() {
        let u = Usage {
            input: 1_000_000,
            ..Default::default()
        };
        assert_eq!(cost("some-local-model", &u, &[]), 0.0);
    }
}
