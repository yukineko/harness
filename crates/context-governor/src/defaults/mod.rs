//! Default handler implementations + the constructor seams the bin dispatches
//! through. Phase 1 ships the contract: every body is `todo!()` and the
//! constructors return the default type. Phase 2 fills the bodies in
//! force-priority order (groomer first).
//!
//! The bin never names a concrete handler — it calls `groomer()` / `injector()`
//! / … so Phase 3 can swap the default for an existing-plugin wrapper behind the
//! same trait without touching dispatch.

pub mod checkpointer;
pub mod classifier;
pub mod groomer;
pub mod guard;
pub mod injector;
pub mod rehydrator;

pub use checkpointer::DefaultCheckpointer;
pub use classifier::DefaultClassifier;
pub use groomer::DefaultGroomer;
pub use guard::DefaultGuard;
pub use injector::DefaultInjector;
pub use rehydrator::DefaultRehydrator;

/// The active groomer (★ primary size lever).
pub fn groomer() -> DefaultGroomer {
    DefaultGroomer
}
/// The active retrieval / reference-body injector.
pub fn injector() -> DefaultInjector {
    DefaultInjector
}
/// The active load-time spec classifier + resident-budget check.
pub fn classifier() -> DefaultClassifier {
    DefaultClassifier
}
/// The active PreCompact backstop guard.
pub fn guard() -> DefaultGuard {
    DefaultGuard
}
/// The active SessionStart rehydrator.
pub fn rehydrator() -> DefaultRehydrator {
    DefaultRehydrator
}
/// The active Stop/SubagentStop checkpointer.
pub fn checkpointer() -> DefaultCheckpointer {
    DefaultCheckpointer
}
