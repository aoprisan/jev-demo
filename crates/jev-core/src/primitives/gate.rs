//! `Gate<I>` — the last judgment before a deterministic system acts.

use super::{
    at_most_chars, evidence_from, fit, in_range, need_choice, need_score, priors_cue, Evidence,
    JevInput,
};
use crate::ask::{Ask, Asks};
use crate::client::Primitive;
use crate::error::{JevError, Result};
use crate::jev::Jev;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const P: &str = "gate";
const MAX_REASON: usize = 160;

/// What to do with the solver's proposed action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Proceed at the size the solver chose.
    Execute,
    /// Proceed, but smaller.
    Reduce,
    /// Do not act now; the blocking condition is expected to pass.
    Hold,
    /// Do not act; a human should look.
    Escalate,
}

impl Action {
    /// All four, in escalating order of caution.
    pub const ALL: [Action; 4] = [Action::Execute, Action::Reduce, Action::Hold, Action::Escalate];

    /// Lower-case name, as offered to Jev.
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Execute => "execute",
            Action::Reduce => "reduce",
            Action::Hold => "hold",
            Action::Escalate => "escalate",
        }
    }

    /// Whether the action lets anything through at all.
    pub fn acts(&self) -> bool {
        matches!(self, Action::Execute | Action::Reduce)
    }

    fn describe(&self) -> &'static str {
        match self {
            Action::Execute => {
                "Proceed exactly as sized. Nothing in the state argues against acting."
            }
            Action::Reduce => {
                "The action is sound but conditions are worse than the solver can see. \
                 Proceed at a smaller fraction."
            }
            Action::Hold => {
                "Do not act now. The blocking condition is expected to pass on its own \
                 (an event window, a stale input, a transient)."
            }
            Action::Escalate => {
                "Do not act, and a human should look. The state is inconsistent, \
                 implausible, or outside what the system handles."
            }
        }
    }

    fn parse(label: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.as_str() == label)
    }
}

/// What the gate is being asked about, and the size rubric it should use.
#[derive(Debug, Clone)]
pub struct GateSpec {
    /// A short name for what is being gated, used in the composed reason.
    pub subject: String,
    /// The question put to Jev alongside the standing gate instructions.
    pub question: String,
    /// The rubric for `size_factor`, lowest first. Level `i` maps to `i/(n-1)`.
    pub size_levels: Vec<String>,
}

impl GateSpec {
    /// A gate with the default five-level size rubric (none / quarter / half /
    /// most / full).
    pub fn new(subject: impl Into<String>, question: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            question: question.into(),
            size_levels: vec![
                "None of it. Conditions do not support acting at all.".into(),
                "A quarter. Act, but the conditions are poor.".into(),
                "Half. Real reservations, but the action stands.".into(),
                "Most of it. Minor reservations only.".into(),
                "All of it. Conditions fully support the solver's size.".into(),
            ],
        }
    }

    /// Replace the size rubric.
    pub fn size_levels<I: IntoIterator<Item = S>, S: Into<String>>(mut self, levels: I) -> Self {
        self.size_levels = levels.into_iter().map(Into::into).collect();
        self
    }
}

/// The gate's verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GateOut {
    /// What to do.
    pub action: Action,
    /// Fraction of the solver's intended size to put on, 0..=1.
    pub size_factor: f32,
    /// Composed from the verdict; at most 160 characters.
    pub reason: String,
    /// The distribution the action was drawn from.
    pub evidence: Evidence,
}

impl GateOut {
    /// Re-check every schema bound. Called on construction; public so a decoded
    /// record from `decisions.jsonl` can be re-validated.
    pub fn validate(&self) -> Result<()> {
        in_range(P, "size_factor", self.size_factor as f64, 0.0, 1.0)?;
        at_most_chars(P, "reason", &self.reason, MAX_REASON)?;
        // The action and the size must agree. A hold that carries size, or a
        // reduce that carries none, is not a decision anyone can act on, and a
        // caller that silently rounded it either way would be inventing the
        // judgment rather than reporting it.
        if !self.action.acts() && self.size_factor != 0.0 {
            return Err(JevError::Contradiction {
                primitive: P,
                detail: format!(
                    "action `{}` does not act, but size_factor is {}",
                    self.action.as_str(),
                    self.size_factor
                ),
            });
        }
        if self.action.acts() && self.size_factor <= 0.0 {
            return Err(JevError::Contradiction {
                primitive: P,
                detail: format!(
                    "action `{}` acts, but size_factor is {}",
                    self.action.as_str(),
                    self.size_factor
                ),
            });
        }
        Ok(())
    }
}

/// Decide whether a solver-produced action proceeds, and at what fraction of its size.
#[async_trait::async_trait]
pub trait Gate<I: JevInput> {
    /// Gate `input`.
    async fn gate(&self, input: &I, spec: &GateSpec) -> Result<GateOut>;
}

#[async_trait::async_trait]
impl<I: JevInput> Gate<I> for Jev {
    async fn gate(&self, input: &I, spec: &GateSpec) -> Result<GateOut> {
        if spec.size_levels.len() < 2 {
            return Err(JevError::InvalidCall(
                "gate size rubric needs at least two levels".into(),
            ));
        }
        let asks = Asks::new()
            .with(
                "action",
                Ask::choice(
                    spec.question.clone(),
                    Action::ALL.iter().map(|a| (a.as_str(), a.describe())),
                ),
            )
            .with(
                "size_factor",
                Ask::score(
                    format!(
                        "Given the same state, how much of the solver's intended size do \
                         conditions justify for {}?",
                        spec.subject
                    ),
                    spec.size_levels.clone(),
                ),
            );

        let outcome = self.call(Primitive::Gate, input, asks).await?;
        let (label, probabilities, confidence) = need_choice(&outcome.reply, "action", P)?;
        let action = Action::parse(label).ok_or_else(|| JevError::UnknownLabel {
            primitive: P,
            label: label.to_owned(),
            allowed: Action::ALL.iter().map(Action::as_str).collect::<Vec<_>>().join(", "),
        })?;
        let (fraction, _) = need_score(&outcome.reply, "size_factor", P)?;
        in_range(P, "size_factor", fraction, 0.0, 1.0)?;

        // Hold and escalate do not put anything on; the rubric is not consulted.
        let size_factor = if action.acts() { fraction as f32 } else { 0.0 };
        let evidence = evidence_from(probabilities, confidence);

        let cue = priors_cue(&outcome.call.state);
        let top = evidence.distribution.first().map(|w| w.p).unwrap_or(0.0);
        let runner = evidence
            .runner_up()
            .map(|w| format!("; next {} {:.2}", w.label, w.p))
            .unwrap_or_default();
        let reason = fit(
            &format!(
                "{}: {} at {:.0}% of size{} (p={:.2}, conf {:.2}{})",
                spec.subject,
                action.as_str(),
                size_factor * 100.0,
                cue.map(|c| format!(" — {c}")).unwrap_or_default(),
                top,
                confidence,
                runner,
            ),
            MAX_REASON,
        );

        let out = GateOut { action, size_factor, reason, evidence };
        out.validate()?;
        self.record(&outcome, &out)?;
        Ok(out)
    }
}
