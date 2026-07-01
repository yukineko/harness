//! The handler trait set — one trait per hook role from the lever × timing
//! table (§3). Each trait is a *seam*: Phase 2 ships a default impl, Phase 3
//! wraps an existing plugin (`toolguard`/`playbook`/`ctxrot`/`budgetguard`)
//! behind the same trait without changing this contract.
//!
//! The traits are ordered by the brief's force-priority:
//! groomer ＞ classifier+budget ＞ injector ＞ rehydrator+guard ＞ checkpointer.

use crate::io::HookOutput;
use crate::types::{ContextItem, Evictable, SpecClass, StandingBudget, StoreKey};
use harness_core::hook::HookInput;

/// **Primary size lever** (PostToolUse). Trims/summary-replaces a bloated tool
/// result — the dominant growth term in an agent loop. Takes an [`Evictable`],
/// so by construction it can never be handed a `Pinned`/`Verbatim` item (I2),
/// and its output must be smaller than the input (I4, checked at the call site
/// / in tests). Returns `None` to leave the result untouched.
pub trait ToolResultGroomer {
    fn groom(&self, tool_output: Evictable<'_>, budget: u32) -> Option<serde_json::Value>;
}

/// Retrieval / reference-body injection (UserPromptSubmit, SessionStart). Emits
/// an `additionalContext` envelope beside the prompt — reduce-*before* the model
/// reads, never a prompt replacement.
pub trait ContextInjector {
    fn inject(&self, input: &HookInput) -> HookOutput;
}

/// Load-time spec handling (§8). Splits a spec doc into `NormativeCore`
/// (Pinned+Verbatim, resident) and `ReferenceBody` (Evictable, retrieval), then
/// verifies the resident set fits the standing budget (I3). Run **once** at
/// load, never per turn.
pub trait SpecClassifier {
    fn classify(&self, doc: &str) -> Vec<(SpecClass, ContextItem)>;

    /// `Err(Overrun)` when the resident (system + Pinned) total exceeds the
    /// budget — the caller must then move bulk into `ReferenceBody`.
    fn check_resident(
        &self,
        items: &[ContextItem],
        budget: &StandingBudget,
    ) -> Result<(), crate::types::Overrun>;
}

/// PreCompact backstop. Snapshots the transcript + records verbatim spans to the
/// backing store, then decides whether to let compaction proceed. The default is
/// `Proceed`: compression is delegated to built-in compaction, never duplicated
/// here (no self-summarization).
pub trait CompactionGuard {
    fn on_pre_compact(&mut self, i: &HookInput, s: &mut dyn BackingStore) -> CompactDecision;
}

/// Outcome of the PreCompact guard. `Block` maps to hook exit 2 in the bin;
/// `Proceed` to exit 0. Blocking is rare — the backstop exists to *guard* the
/// snapshot, not to prevent compaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactDecision {
    Proceed,
    Block { reason: String },
}

/// SessionStart restore. Re-injects normative core / verbatim from the store so
/// pins survive compaction (I1) and resume reseeds durably. Reads the store; the
/// returned [`HookOutput`] carries the `additionalContext`.
pub trait StateRehydrator {
    fn rehydrate(&self, i: &HookInput, s: &dyn BackingStore) -> HookOutput;
}

/// Stop / SubagentStop externalization. Writes completed work to the backing
/// store under a threshold gate. **Side effects only** — it must never return a
/// blocking decision (the bin discards its output and exits 0), because the
/// per-session block cap short-circuits the session after repeated blocks.
pub trait Checkpointer {
    fn checkpoint(&mut self, i: &HookInput, s: &mut dyn BackingStore);
}

/// Lossless backing for externalized context. The snapshot source is the
/// transcript (always supplied to every hook); `put`/`recall` round-trip
/// individual items. Object-safe so handlers take `&mut dyn BackingStore`.
pub trait BackingStore {
    /// Snapshot the transcript at `transcript_path` and return a handle to it.
    fn snapshot_transcript(&mut self, transcript_path: &str) -> StoreKey;
    /// Externalize an item, returning its handle. The lossless half of I2.
    fn put(&mut self, item: &ContextItem) -> StoreKey;
    /// Recall an externalized item by handle; `None` if absent.
    fn recall(&self, key: &StoreKey) -> Option<ContextItem>;
}

/// Marker: compression is *delegated* to Claude Code's built-in compaction. Its
/// presence in a handler signals "this path does not summarize" — the
/// no-self-summarization invariant (§14.5) is a property of the design, made
/// legible by never wiring a summarizer to a [`CompactionGuard`].
pub struct DelegateToBuiltinCompaction;
