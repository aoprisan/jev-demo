//! `Score<I>` — rate the state 0..=100 on a caller-supplied rubric, and say which
//! of a caller-supplied vocabulary of drivers actually hold.

use super::{
    at_most_chars, at_most_items, fit, in_range, need_noul, need_score, Evidence, JevInput, Weight,
};
use crate::ask::{Ask, Asks};
use crate::client::Primitive;
use crate::error::{JevError, Result};
use crate::jev::Jev;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const P: &str = "score";
const MAX_REASON: usize = 160;
const MAX_DRIVERS: usize = 3;

/// One candidate driver of the score: an independent claim Jev answers yes/no.
///
/// Drivers are a fixed vocabulary rather than free text, so that every driver
/// reported carries its own probability and can be tested.
#[derive(Debug, Clone)]
pub struct DriverSpec {
    /// Short name, as it appears in `drivers`.
    pub name: String,
    /// The claim Jev evaluates.
    pub claim: String,
}

impl DriverSpec {
    /// A named driver claim.
    pub fn new(name: impl Into<String>, claim: impl Into<String>) -> Self {
        Self { name: name.into(), claim: claim.into() }
    }
}

/// What is being scored, on what rubric, with which candidate drivers.
#[derive(Debug, Clone)]
pub struct ScoreSpec {
    /// A short name for the subject, used in the composed reason.
    pub subject: String,
    /// The question put to Jev alongside the standing score instructions.
    pub question: String,
    /// The rubric, lowest first. Level `i` maps to `100 * i/(n-1)`.
    pub bands: Vec<String>,
    /// Candidate drivers. Those Jev affirms are reported, at most three.
    pub drivers: Vec<DriverSpec>,
    /// Probability at or above which a driver counts as holding. Defaults to 0.5.
    pub driver_threshold: f64,
}

impl ScoreSpec {
    /// A score question over the given rubric.
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(
        subject: impl Into<String>,
        question: impl Into<String>,
        bands: I,
    ) -> Self {
        Self {
            subject: subject.into(),
            question: question.into(),
            bands: bands.into_iter().map(Into::into).collect(),
            drivers: Vec::new(),
            driver_threshold: 0.5,
        }
    }

    /// Add a candidate driver.
    pub fn driver(mut self, name: impl Into<String>, claim: impl Into<String>) -> Self {
        self.drivers.push(DriverSpec::new(name, claim));
        self
    }
}

/// A 0..=100 score with the drivers that carried it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScoreOut {
    /// The score, 0..=100.
    pub score: u8,
    /// The affirmed drivers, strongest first, at most three.
    pub drivers: Vec<String>,
    /// Composed from the verdict; at most 160 characters.
    pub reason: String,
    /// Certainty, and every candidate driver with its probability.
    pub evidence: Evidence,
}

impl ScoreOut {
    /// Re-check every schema bound.
    pub fn validate(&self) -> Result<()> {
        in_range(P, "score", self.score as f64, 0.0, 100.0)?;
        at_most_items(P, "drivers", &self.drivers, MAX_DRIVERS)?;
        at_most_chars(P, "reason", &self.reason, MAX_REASON)
    }
}

/// Rate the state on an ordered rubric.
#[async_trait::async_trait]
pub trait Score<I: JevInput> {
    /// Score `input`.
    async fn score(&self, input: &I, spec: &ScoreSpec) -> Result<ScoreOut>;
}

#[async_trait::async_trait]
impl<I: JevInput> Score<I> for Jev {
    async fn score(&self, input: &I, spec: &ScoreSpec) -> Result<ScoreOut> {
        if spec.bands.len() < 2 {
            return Err(JevError::InvalidCall("score rubric needs at least two bands".into()));
        }
        let mut asks =
            Asks::new().with("level", Ask::score(spec.question.clone(), spec.bands.clone()));
        for d in &spec.drivers {
            asks = asks.with(
                format!("driver_{}", d.name),
                Ask::noul(d.claim.clone()).criteria(
                    "This is true of the state as given.",
                    "This is not true of the state as given.",
                ),
            );
        }

        let outcome = self.call(Primitive::Score, input, asks).await?;
        let (fraction, confidence) = need_score(&outcome.reply, "level", P)?;
        in_range(P, "level", fraction, 0.0, 1.0)?;
        let score = (fraction * 100.0).round() as u8;

        let mut weighted: Vec<Weight> = Vec::with_capacity(spec.drivers.len());
        for d in &spec.drivers {
            let p = need_noul(&outcome.reply, &format!("driver_{}", d.name), P)?;
            in_range(P, "driver", p, 0.0, 1.0)?;
            weighted.push(Weight { label: d.name.clone(), p: p as f32 });
        }
        weighted.sort_by(|a, b| b.p.total_cmp(&a.p));

        let drivers: Vec<String> = weighted
            .iter()
            .filter(|w| w.p as f64 >= spec.driver_threshold)
            .take(MAX_DRIVERS)
            .map(|w| w.label.clone())
            .collect();

        let tail = if drivers.is_empty() {
            "no driver held".to_owned()
        } else {
            format!("driven by {}", drivers.join(", "))
        };
        let reason = fit(&format!("{} scores {}/100 — {}", spec.subject, score, tail), MAX_REASON);

        let out = ScoreOut {
            score,
            drivers,
            reason,
            evidence: Evidence { confidence: confidence as f32, distribution: weighted },
        };
        out.validate()?;
        self.record(&outcome, &out)?;
        Ok(out)
    }
}
