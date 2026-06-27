//! Per-session attempt counter — a thin re-export of the shared gate state in
//! `harness_core::gate::state`. donegate only uses `bump`/`reset` (the
//! `last_hash` field of the shared `SessionState` stays empty for this gate).

pub use harness_core::gate::state::{bump, reset};
