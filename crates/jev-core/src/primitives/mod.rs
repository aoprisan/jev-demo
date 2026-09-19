//! The six primitives.
//!
//! Each is a generic typed call: a domain input struct goes in, a fixed output
//! struct comes out, and the output is validated before it is returned. Jev
//! never produces a quantity the solver owns — it classifies, scores, gates,
//! checks, ranks or explains.
//!
//! ## What Jev actually returns
//!
//! The System One API answers only three kinds of question: a probability, a
//! label drawn from a set you defined, and a position on a rubric you defined.
//! It does not emit free text. So the *judgment* fields of every output here —
//! the action, the label, the score, each check's boolean, the ranking order —
//! come from Jev, while the `reason` / `note` / `rationale` / `summary` strings
//! are composed deterministically by this crate from those typed verdicts and
//! the caller's own vocabulary. A reason can therefore never say anything the
//! schema did not already carry, which is the point: the prose is a rendering
//! of the decision, not a second, unchecked channel of it.

mod check;
mod classify;
mod explain;
mod gate;
mod rank;
mod score;

pub use check::{Check, CheckItem, CheckOut, CheckResult, CheckSpec};
pub use classify::{Classify, ClassifyOut, ClassifySpec, Label};
pub use explain::{Audience, Explain, ExplainOut, ExplainSpec, FactSpec, Framing};
pub use gate::{Action, Gate, GateOut, GateSpec};
pub use rank::{Candidate, CandidateId, Rank, RankOut, RankSpec, Ranked};
pub use score::{DriverSpec, Score, ScoreOut, ScoreSpec};

use crate::ask::Verdict;
use crate::client::JevReply;
use crate::error::{JevError, Result};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Anything that can be handed to Jev as the state of a call.
///
/// Implementors are responsible for exposing *observable* features only. The
/// mock backend reads nothing but this serialised state, so a field that leaks
/// ground truth (a synthetic generator's true regime, a future price) would
/// quietly invalidate any comparison drawn against it.
pub trait JevInput: Serialize + Send + Sync {
    /// Domain framing for this call, injected into the primitive's prompt as a
    /// context block rather than replacing it.
    fn context_block(&self) -> String;

    /// The state as Jev sees it.
    ///
    /// Always `{"input": …, "prior_judgments": […]}`, whether or not the call
    /// came through a [`crate::Pipeline`], so that everything reading a state —
    /// the mock, the audit log, `priors_cue` — sees one shape.
    /// [`crate::pipeline::Staged`] overrides this to fill in the priors.
    fn to_state(&self) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "input": serde_json::to_value(self)
                .map_err(|e| JevError::InvalidCall(format!("state is not serialisable: {e}")))?,
            "prior_judgments": [],
        }))
    }
}

/// One label and the weight Jev gave it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Weight {
    /// The label.
    pub label: String,
    /// Its probability, 0..=1.
    pub p: f32,
}

/// The typed verdict behind a composed output: what Jev actually said, kept
/// alongside the prose so a report can show the distribution a decision rests on.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    /// Certainty Jev reported, 0..=1.
    pub confidence: f32,
    /// The full distribution, highest first.
    pub distribution: Vec<Weight>,
}

impl Evidence {
    /// The runner-up, when there is one.
    pub fn runner_up(&self) -> Option<&Weight> {
        self.distribution.get(1)
    }
}

// ---- extraction helpers --------------------------------------------------------------------

pub(crate) fn need<'a>(
    reply: &'a JevReply,
    name: &str,
    primitive: &'static str,
) -> Result<&'a Verdict> {
    reply.verdicts.get(name).ok_or_else(|| JevError::MissingAnswer {
        name: name.to_owned(),
        primitive,
        asked: reply.verdicts.len(),
    })
}

pub(crate) fn need_noul(reply: &JevReply, name: &str, primitive: &'static str) -> Result<f64> {
    let v = need(reply, name, primitive)?;
    v.as_noul().ok_or_else(|| JevError::AnswerKind {
        name: name.to_owned(),
        got: v.kind(),
        want: "noul",
        primitive,
    })
}

pub(crate) fn need_choice<'a>(
    reply: &'a JevReply,
    name: &str,
    primitive: &'static str,
) -> Result<(&'a str, &'a IndexMap<String, f64>, f64)> {
    let v = need(reply, name, primitive)?;
    v.as_choice().ok_or_else(|| JevError::AnswerKind {
        name: name.to_owned(),
        got: v.kind(),
        want: "choice",
        primitive,
    })
}

pub(crate) fn need_score(
    reply: &JevReply,
    name: &str,
    primitive: &'static str,
) -> Result<(f64, f64)> {
    let v = need(reply, name, primitive)?;
    let (_, _, confidence) = v.as_score().ok_or_else(|| JevError::AnswerKind {
        name: name.to_owned(),
        got: v.kind(),
        want: "score",
        primitive,
    })?;
    let fraction = v.score_fraction().unwrap_or(0.0);
    Ok((fraction, confidence))
}

pub(crate) fn evidence_from(probabilities: &IndexMap<String, f64>, confidence: f64) -> Evidence {
    let mut distribution: Vec<Weight> = probabilities
        .iter()
        .map(|(label, p)| Weight { label: label.clone(), p: *p as f32 })
        .collect();
    distribution.sort_by(|a, b| b.p.total_cmp(&a.p));
    Evidence { confidence: confidence as f32, distribution }
}

// ---- validation helpers --------------------------------------------------------------------

pub(crate) fn in_range(
    primitive: &'static str,
    field: &'static str,
    value: f64,
    min: f64,
    max: f64,
) -> Result<()> {
    if !value.is_finite() || value < min || value > max {
        return Err(JevError::OutOfRange { primitive, field, value, min, max });
    }
    Ok(())
}

pub(crate) fn at_most_chars(
    primitive: &'static str,
    field: &'static str,
    text: &str,
    max: usize,
) -> Result<()> {
    let len = text.chars().count();
    if len > max {
        return Err(JevError::TooLong { primitive, field, len, max });
    }
    Ok(())
}

pub(crate) fn at_most_items<T>(
    primitive: &'static str,
    field: &'static str,
    items: &[T],
    max: usize,
) -> Result<()> {
    if items.len() > max {
        return Err(JevError::TooLong { primitive, field, len: items.len(), max });
    }
    Ok(())
}

/// Truncate to `max` characters on a character boundary, marking the cut.
///
/// Composed prose is built to fit well inside its schema bound; this is the
/// belt to `at_most_chars`'s braces, so that a caller supplying an unusually
/// long subject degrades the *prose* rather than failing the whole decision.
/// The bound itself is still enforced afterwards.
pub(crate) fn fit(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push('\u{2026}');
    out
}

/// A short cue drawn from the prior judgments already in the state, so a gate's
/// composed reason can name what moved it. Deterministic; reads only the log.
pub(crate) fn priors_cue(state: &serde_json::Value) -> Option<String> {
    let priors = state.get("prior_judgments")?.as_array()?;

    // A high risk score outranks a failed check: when both are present it is
    // the score that drove the verdict, and naming the check instead would
    // point a reader at the wrong thing.
    for entry in priors {
        let Some(output) = entry.get("output") else { continue };
        if let Some(score) = output.get("score").and_then(serde_json::Value::as_u64) {
            if score >= 75 {
                let driver = output
                    .get("drivers")
                    .and_then(|d| d.as_array())
                    .and_then(|d| d.first())
                    .and_then(serde_json::Value::as_str);
                return Some(match driver {
                    Some(d) => format!("risk {score}, {d}"),
                    None => format!("risk {score}"),
                });
            }
        }
    }
    for entry in priors {
        let Some(output) = entry.get("output") else { continue };
        if let Some(checks) = output.get("checks").and_then(|c| c.as_array()) {
            if let Some(failed) = checks
                .iter()
                .find(|c| c.get("ok").and_then(serde_json::Value::as_bool) == Some(false))
                .and_then(|c| c.get("name"))
                .and_then(serde_json::Value::as_str)
            {
                return Some(format!("{failed} failed"));
            }
        }
    }
    for entry in priors {
        let Some(output) = entry.get("output") else { continue };
        if let Some(score) = output.get("score").and_then(serde_json::Value::as_u64) {
            if score >= 60 {
                return Some(format!("risk {score}"));
            }
        }
    }
    None
}
