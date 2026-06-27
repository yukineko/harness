//! Shared Stop-gate infrastructure for the completion gates (donegate,
//! reviewgate, tdd).
//!
//! These three plugins each ship a Stop hook that runs project commands as
//! subprocesses, counts consecutive blocks per session, and consumes a one-shot
//! skip marker. The dangerous bits — spawning a subprocess with a timeout and a
//! bounded-tail log, persisting per-session attempt state, and the
//! never-break-a-turn panic guard — were duplicated verbatim across all three.
//! Centralizing them here keeps the risky subprocess code in exactly one place
//! while each plugin keeps its own config keys, messages, skip-marker filename
//! and log fields in a thin adapter.
//!
//! * [`runner`] — bounded-tail subprocess runner (`run`).
//! * [`state`] — per-session attempt counter (`load`/`save`/`reset`/`bump`).
//! * [`run`] — the panic guard (`run_guarded`) and skip-marker consumer
//!   (`consume_skip`).

pub mod run;
pub mod runner;
pub mod state;
