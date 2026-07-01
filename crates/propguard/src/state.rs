//! Per-session state — a thin re-export of the shared gate state in
//! `harness_core::gate::state`. propguard uses `SessionState` (incl. the
//! `last_hash` of the diff it last forced a property check of) with
//! `load`/`save`/`reset` directly; the round counter is driven inline by
//! `property::evaluate`.

pub use harness_core::gate::state::{load, reset, save, SessionState};
