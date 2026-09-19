//! `Classify<I, E>` — assign the state to one of a domain enum's labels.

use super::{at_most_chars, evidence_from, fit, in_range, need_choice, Evidence, JevInput};
use crate::ask::{Ask, Asks};
use crate::client::Primitive;
use crate::error::{JevError, Result};
use crate::jev::Jev;
use schemars::JsonSchema;
use serde::Serialize;

const P: &str = "classify";
const MAX_REASON: usize = 160;

/// A domain enum Jev can classify into.
///
/// The labels are a partition: every state belongs to exactly one. Descriptions
/// are the whole definition Jev sees, so they carry the domain's meaning rather
/// than the variant's name.
pub trait Label: Sized + Copy + PartialEq + std::fmt::Debug + Send + Sync + 'static {
    /// Every label, in the order they should be offered.
    fn labels() -> &'static [Self];
    /// The wire name of this label.
    fn name(&self) -> &'static str;
    /// What this label means. Jev reads this and nothing else about the variant.
    fn describe(&self) -> &'static str;

    /// Parse a wire name back to a label.
    fn from_name(name: &str) -> Option<Self> {
        Self::labels().iter().copied().find(|l| l.name() == name)
    }
}

/// What is being classified.
#[derive(Debug, Clone)]
pub struct ClassifySpec {
    /// A short name for the subject, used in the composed reason.
    pub subject: String,
    /// The question put to Jev alongside the standing classify instructions.
    pub question: String,
}

impl ClassifySpec {
    /// A classification question.
    pub fn new(subject: impl Into<String>, question: impl Into<String>) -> Self {
        Self { subject: subject.into(), question: question.into() }
    }
}

/// The chosen label and how certain Jev was.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ClassifyOut<E> {
    /// The label Jev chose.
    pub label: E,
    /// Certainty, 0..=1.
    pub confidence: f32,
    /// Composed from the verdict; at most 160 characters.
    pub reason: String,
    /// The distribution the label was drawn from.
    pub evidence: Evidence,
}

impl<E> ClassifyOut<E> {
    /// Re-check every schema bound.
    pub fn validate(&self) -> Result<()> {
        in_range(P, "confidence", self.confidence as f64, 0.0, 1.0)?;
        at_most_chars(P, "reason", &self.reason, MAX_REASON)
    }
}

/// Assign the state to exactly one label of a domain enum.
#[async_trait::async_trait]
pub trait Classify<I: JevInput, E: Label> {
    /// Classify `input`.
    async fn classify(&self, input: &I, spec: &ClassifySpec) -> Result<ClassifyOut<E>>;
}

#[async_trait::async_trait]
impl<I: JevInput, E: Label + Serialize> Classify<I, E> for Jev {
    async fn classify(&self, input: &I, spec: &ClassifySpec) -> Result<ClassifyOut<E>> {
        if E::labels().len() < 2 {
            return Err(JevError::InvalidCall(
                "classify needs at least two labels to choose between".into(),
            ));
        }
        let asks = Asks::new().with(
            "label",
            Ask::choice(
                spec.question.clone(),
                E::labels().iter().map(|l| (l.name(), l.describe())),
            ),
        );

        let outcome = self.call(Primitive::Classify, input, asks).await?;
        let (name, probabilities, confidence) = need_choice(&outcome.reply, "label", P)?;
        let label = E::from_name(name).ok_or_else(|| JevError::UnknownLabel {
            primitive: P,
            label: name.to_owned(),
            allowed: E::labels().iter().map(|l| l.name()).collect::<Vec<_>>().join(", "),
        })?;
        in_range(P, "confidence", confidence, 0.0, 1.0)?;
        let evidence = evidence_from(probabilities, confidence);

        let runner = evidence
            .runner_up()
            .map(|w| format!(", next {} {:.2}", w.label, w.p))
            .unwrap_or_default();
        let reason = fit(
            &format!("{} reads as {} (conf {:.2}{})", spec.subject, name, confidence, runner),
            MAX_REASON,
        );

        let out = ClassifyOut { label, confidence: confidence as f32, reason, evidence };
        out.validate()?;
        self.record(&outcome, &out)?;
        Ok(out)
    }
}
