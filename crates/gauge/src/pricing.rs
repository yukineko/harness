//! Cost estimation.
//!
//! The rate table and cost function now live in `harness_core::pricing` (shared
//! with other plugins). This module re-exports them so gauge's call sites
//! (`pricing::cost`, `pricing::rate_for`, `pricing::Rate`) are unchanged. The
//! built-in rates, cache TTL multipliers, and substring-override semantics are
//! identical to before the move.

#[allow(unused_imports)]
pub use harness_core::pricing::{cost, rate_for, Rate};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PriceOverride;
    use crate::store::Usage;

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
