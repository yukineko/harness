//! Default [`SpecClassifier`] — load-time spec split + resident-budget check
//! (§8, I3). Splits a spec into NormativeCore (resident) and ReferenceBody
//! (retrieval), then verifies the resident set fits the standing budget.

use crate::handlers::SpecClassifier;
use crate::types::{ContextItem, Overrun, SpecClass, StandingBudget};

pub struct DefaultClassifier;

impl SpecClassifier for DefaultClassifier {
    fn classify(&self, doc: &str) -> Vec<(SpecClass, ContextItem)> {
        let _ = doc;
        todo!("Phase 2: classify spans into NormativeCore / ReferenceBody")
    }

    fn check_resident(
        &self,
        items: &[ContextItem],
        budget: &StandingBudget,
    ) -> Result<(), Overrun> {
        let _ = (items, budget);
        todo!("Phase 2: sum resident (system + Pinned) tokens, compare to budget (I3)")
    }
}
