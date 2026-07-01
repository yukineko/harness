//! Lane / item / spec-classification types.
//!
//! The brief's central discipline: encode the invariants as **types**, not as
//! rules a handler must remember. Two of the six invariants are enforced here at
//! compile time:
//!
//! * **I1 (correctness)** — a `Pinned` item is always present in the final
//!   context. The lane carries that intent; the rehydrator restores it across
//!   compaction.
//! * **I2 (correctness)** — a `Verbatim` item is never lossy-compressed. This is
//!   made *unrepresentable*: the only handler that compresses ([`crate::handlers::ToolResultGroomer`])
//!   accepts an [`Evictable`], and an `Evictable` can only be constructed from a
//!   `Lane::Evictable` item. Code that hands a `Pinned`/`Verbatim` item to the
//!   groomer does not compile.

/// Which discipline an item lives under. The lane is the single source of truth
/// for how the item may be treated by every handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lane {
    /// Must always be present in the final context (I1). Static pins live in the
    /// system prompt / CLAUDE.md; long-lived pins are re-injected at
    /// SessionStart(compact) so they survive compaction. Resident pins *cost
    /// size* (a constant tax) — that is accepted for correctness/cost, not a
    /// size lever.
    Pinned,
    /// Never lossy-compressed (I2). Round-trips losslessly via
    /// snapshot → re-injection. Enforced by [`Evictable`] being unconstructable
    /// from this lane.
    Verbatim,
    /// May be groomed, evicted, or pushed to retrieval. The *only* lane the
    /// groomer is allowed to touch.
    Evictable,
}

/// Classification assigned to a spec document **once, at load time** (§8). It is
/// not a per-turn computation; it decides which lane and storage location a span
/// of the spec lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecClass {
    /// Contracts, invariants, naming rules, acceptance criteria — the norms that
    /// constrain *every* turn. → `Pinned + Verbatim`, persistent layer, outside
    /// compaction. Cannot be retrieved (a violation is only noticed if the norm
    /// is ambient), must not be paraphrased, and is stable enough to cache.
    NormativeCore,
    /// Exhaustive tables, full endpoint lists, examples, appendices — large but
    /// situational. → `Evictable`, external file / retrieval, injected only on
    /// the turns that need it. Made resident, it worsens size and
    /// signal-to-noise and invites lost-in-the-middle.
    ReferenceBody,
}

/// Stable identity of a context item within a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId(pub u64);

/// Handle to a blob in the [`crate::handlers::BackingStore`] (a snapshotted
/// transcript span, an externalized verbatim record, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StoreKey(pub u64);

/// A unit of context the governor reasons about. `tokens` is the measured (or
/// estimated) window cost used by the size invariants (I3/I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextItem {
    pub id: ItemId,
    pub lane: Lane,
    pub tokens: u32,
    pub body: ItemBody,
}

/// Where an item's bytes live: inline in the window, or externalized behind a
/// [`StoreKey`] (recalled on demand). Externalizing is how `Evictable` items
/// leave the window without being lost, and how `Verbatim` items round-trip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemBody {
    Inline(String),
    Ref(StoreKey),
}

impl ContextItem {
    /// True when this item occupies the window right now (its body is inline).
    pub fn is_resident(&self) -> bool {
        matches!(self.body, ItemBody::Inline(_))
    }
}

/// Capability token proving an item is `Evictable`, and therefore safe to groom
/// or compress. The constructor is the *only* way to obtain one, and it rejects
/// every non-`Evictable` lane — this is what makes I2 unrepresentable rather
/// than merely asserted. Borrows the item so a token can never outlive it.
pub struct Evictable<'a>(&'a ContextItem);

impl<'a> Evictable<'a> {
    /// `Some` iff `i.lane == Lane::Evictable`; `None` for `Pinned`/`Verbatim`.
    pub fn new(i: &'a ContextItem) -> Option<Self> {
        matches!(i.lane, Lane::Evictable).then_some(Self(i))
    }

    /// The underlying item (read-only). Grooming reads the body and proposes a
    /// smaller replacement; it never gets a `Pinned`/`Verbatim` item to begin
    /// with, so it cannot violate I1/I2.
    pub fn item(&self) -> &ContextItem {
        self.0
    }
}

/// The constant-tax ceiling: the combined size of everything resident every turn
/// (system prompt + `Pinned`) must stay under `max_resident_tokens` (I3). A spec
/// whose normative core would breach this must shed bulk into a `ReferenceBody`
/// (root cause is over-long resident prose, not the budget).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StandingBudget {
    pub max_resident_tokens: u32,
}

/// Returned when the resident set exceeds [`StandingBudget`] (I3). Carries the
/// numbers so the classifier can report *how much* must move to `ReferenceBody`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overrun {
    pub resident_tokens: u32,
    pub max_resident_tokens: u32,
}

impl Overrun {
    /// Tokens that must be shed from the resident set to satisfy the budget.
    pub fn excess(&self) -> u32 {
        self.resident_tokens
            .saturating_sub(self.max_resident_tokens)
    }
}
