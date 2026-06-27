//! Per-session state — a thin re-export of the shared gate state in
//! `harness_core::gate::state`. reviewgate uses `SessionState` (incl. the
//! `last_hash` of the diff it last forced a review of) with `load`/`save`/`reset`
//! directly; the round counter is driven inline by `review::evaluate`.

pub use harness_core::gate::state::{load, reset, save, SessionState};
