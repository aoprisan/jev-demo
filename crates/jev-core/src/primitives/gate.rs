//! `Gate<I>` — the last judgment before a deterministic system acts.

use super::{
    at_most_chars, evidence_from, fit, in_range, need_choice, need_score, priors_cue, Evidence,
    JevInput,
};
use crate::ask::{Ask, Asks};
use crate::client::{JevReply, Primitive};
use crate::error::{JevError, Result};
use crate::jev::Jev;
use crate::prompts;
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
    /// The one-line rendering later stages see in `prior_judgments`.
    pub fn line(&self) -> String {
        self.reason.clone()
    }

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
        if (self.action == Action::Execute && self.size_factor != 1.0)
            || (self.action == Action::Reduce && self.size_factor >= 1.0)
        {
            return Err(JevError::Contradiction {
                primitive: P,
                detail:
                    "execute requires full size; reduce requires a strictly smaller positive size"
                        .into(),
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
        let outcome = self.call(Primitive::Gate, input, asks(spec, "")?).await?;
        let out = compose(spec, &outcome.reply, &outcome.call.state, "")?;
        self.record(&outcome, &out)?;
        Ok(out)
    }
}

/// The gate's two questions: the action, and how much of the size.
pub(crate) fn asks(spec: &GateSpec, prefix: &str) -> Result<Asks> {
    if spec.size_levels.len() < 2 {
        return Err(JevError::InvalidCall("gate size rubric needs at least two levels".into()));
    }
    Ok(Asks::new()
        .with(
            format!("{prefix}action"),
            Ask::choice(
                prompts::instructions(Primitive::Gate, "action", spec.question.clone(), []),
                Action::ALL.iter().map(|a| (a.as_str(), a.describe())),
            ),
        )
        .with(
            format!("{prefix}size_factor"),
            Ask::score(
                prompts::instructions(
                    Primitive::Gate,
                    "size_factor",
                    format!(
                        "Assuming a reduced action is appropriate, what smaller positive fraction of the solver's intended size do conditions justify for {}?",
                        spec.subject
                    ),
                    [],
                ),
                spec.size_levels.clone(),
            ),
        ))
}

/// The verdict, composed from the reply and validated.
pub(crate) fn compose(
    spec: &GateSpec,
    reply: &JevReply,
    state: &serde_json::Value,
    prefix: &str,
) -> Result<GateOut> {
    let (label, probabilities, confidence) = need_choice(reply, &format!("{prefix}action"), P)?;
    let action = Action::parse(label).ok_or_else(|| JevError::UnknownLabel {
        primitive: P,
        label: label.to_owned(),
        allowed: Action::ALL.iter().map(Action::as_str).collect::<Vec<_>>().join(", "),
    })?;
    let (fraction, _) = need_score(reply, &format!("{prefix}size_factor"), P)?;
    in_range(P, "size_factor", fraction, 0.0, 1.0)?;

    // Only a reduced action consumes the speculative sizing judgment.
    let size_factor = match action {
        Action::Execute => 1.0,
        Action::Reduce => fraction as f32,
        Action::Hold | Action::Escalate => 0.0,
    };
    let evidence = evidence_from(probabilities, confidence);

    let cue = priors_cue(state);
    let top = evidence.distribution.first().map(|w| w.p).unwrap_or(0.0);
    let runner =
        evidence.runner_up().map(|w| format!("; next {} {:.2}", w.label, w.p)).unwrap_or_default();
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
    Ok(out)
}
