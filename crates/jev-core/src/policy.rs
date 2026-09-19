//! Code-side routing on the certainty Jev reports.
//!
//! Every verdict comes back with a distribution behind it, and a caller that
//! only reads the argmax throws that away. The pattern System One's docs
//! recommend is to route on it in code: act on a confident answer, and send an
//! answer the model was not sure of to a person. A [`ReviewPolicy`] is that
//! rule, stated once per domain. It never changes an action — the gate's verdict
//! stands — it flags the decision for a second look, which is a different thing
//! and is reported separately.

use crate::primitives::CheckOut;

/// When a decision should be flagged for review, in terms of the certainty
/// behind its judgments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewPolicy {
    /// A classification below this confidence is flagged. Confidence is how
    /// concentrated the distribution was, 0..=1.
    pub min_classify_confidence: f32,
    /// A judged check whose probability falls strictly inside this band is
    /// flagged: it neither passed nor failed by a margin anyone should lean on.
    pub undecided_check_band: (f32, f32),
}

impl Default for ReviewPolicy {
    fn default() -> Self {
        Self { min_classify_confidence: 0.25, undecided_check_band: (0.4, 0.6) }
    }
}

impl ReviewPolicy {
    /// Why this decision should be reviewed, if it should. The first reason
    /// found is returned; a reviewer reads the whole record anyway.
    pub fn review(&self, classify_confidence: f32, checks: &CheckOut) -> Option<String> {
        if classify_confidence < self.min_classify_confidence {
            return Some(format!(
                "classification confidence {classify_confidence:.2} is below {:.2}",
                self.min_classify_confidence
            ));
        }
        let (lo, hi) = self.undecided_check_band;
        let undecided = checks.undecided(lo, hi);
        if let Some(first) = undecided.first() {
            return Some(format!(
                "{} is undecided at p={:.2} (inside {lo:.1}..{hi:.1})",
                first.name, first.p
            ));
        }
        None
    }
}
