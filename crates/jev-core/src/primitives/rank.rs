//! `Rank<I, T>` — order solver-produced candidates by fit to the conditions.
//!
//! A single Choice question does the whole job: the label Jev picks is the
//! winner, and the distribution behind it *is* the ordering. One call, N
//! candidates, and the margins come out with it.

use super::{at_most_chars, fit, in_range, need_choice, Evidence, JevInput, Weight};
use crate::ask::{Ask, Asks};
use crate::client::Primitive;
use crate::error::{JevError, Result};
use crate::jev::Jev;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::HashSet;

const P: &str = "rank";
const MAX_RATIONALE: usize = 160;

/// An identifier for a solver-produced candidate.
pub trait CandidateId:
    Clone + PartialEq + Eq + std::hash::Hash + std::fmt::Debug + Send + Sync + 'static
{
    /// The wire label offered to Jev. Must be unique within one call.
    fn label(&self) -> String;
}

impl CandidateId for String {
    fn label(&self) -> String {
        self.clone()
    }
}

/// One candidate, as Jev sees it.
#[derive(Debug, Clone)]
pub struct Candidate<T> {
    /// The caller's identifier.
    pub id: T,
    /// What this candidate does, in the caller's words. This is all Jev reads
    /// about it beyond the shared state.
    pub summary: String,
}

impl<T> Candidate<T> {
    /// A candidate with a summary.
    pub fn new(id: T, summary: impl Into<String>) -> Self {
        Self { id, summary: summary.into() }
    }
}

/// What is being ranked.
#[derive(Debug, Clone)]
pub struct RankSpec<T> {
    /// A short name for the subject.
    pub subject: String,
    /// The question put to Jev alongside the standing rank instructions.
    pub question: String,
    /// The candidates, in offer order. All of them are valid; Jev orders them.
    pub candidates: Vec<Candidate<T>>,
}

impl<T: CandidateId> RankSpec<T> {
    /// A ranking question over the given candidates.
    pub fn new(
        subject: impl Into<String>,
        question: impl Into<String>,
        candidates: Vec<Candidate<T>>,
    ) -> Self {
        Self { subject: subject.into(), question: question.into(), candidates }
    }
}

/// One candidate's place in the ordering.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Ranked<T> {
    /// The caller's identifier.
    pub id: T,
    /// Composed from the verdict; at most 160 characters.
    pub rationale: String,
    /// The probability mass Jev put on this candidate, 0..=1.
    pub p: f32,
}

/// Every candidate, best first.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct RankOut<T> {
    /// The ordering, best first. Contains exactly the candidates offered.
    pub ordered: Vec<Ranked<T>>,
    /// Certainty, and the full distribution.
    pub evidence: Evidence,
}

impl<T> RankOut<T> {
    /// The winner.
    pub fn top(&self) -> Option<&Ranked<T>> {
        self.ordered.first()
    }

    /// How far clear the winner is of the runner-up, 0..=1. A small margin means
    /// the candidates were close, which is itself worth reporting.
    pub fn margin(&self) -> f32 {
        match (self.ordered.first(), self.ordered.get(1)) {
            (Some(a), Some(b)) => a.p - b.p,
            _ => 1.0,
        }
    }

    /// Re-check every schema bound.
    pub fn validate(&self) -> Result<()> {
        for r in &self.ordered {
            in_range(P, "p", r.p as f64, 0.0, 1.0)?;
            at_most_chars(P, "rationale", &r.rationale, MAX_RATIONALE)?;
        }
        Ok(())
    }
}

/// Order candidates the solver has already produced.
#[async_trait::async_trait]
pub trait Rank<I: JevInput, T: CandidateId> {
    /// Rank the candidates in `spec` against `input`.
    async fn rank(&self, input: &I, spec: &RankSpec<T>) -> Result<RankOut<T>>;
}

#[async_trait::async_trait]
impl<I: JevInput, T: CandidateId + Serialize> Rank<I, T> for Jev {
    async fn rank(&self, input: &I, spec: &RankSpec<T>) -> Result<RankOut<T>> {
        if spec.candidates.len() < 2 {
            return Err(JevError::InvalidCall("rank needs at least two candidates".into()));
        }
        let labels: Vec<String> = spec.candidates.iter().map(|c| c.id.label()).collect();
        if labels.iter().collect::<HashSet<_>>().len() != labels.len() {
            return Err(JevError::InvalidCall(
                "rank candidate labels must be unique within a call".into(),
            ));
        }

        let asks = Asks::new().with(
            "best",
            Ask::choice(
                spec.question.clone(),
                spec.candidates.iter().map(|c| (c.id.label(), c.summary.clone())),
            ),
        );

        let outcome = self.call(Primitive::Rank, input, asks).await?;
        let (_, probabilities, confidence) = need_choice(&outcome.reply, "best", P)?;

        // The distribution is the ranking.
        let mut weights: Vec<Weight> = probabilities
            .iter()
            .map(|(label, p)| Weight { label: label.clone(), p: *p as f32 })
            .collect();
        weights.sort_by(|a, b| b.p.total_cmp(&a.p));

        let mut ordered = Vec::with_capacity(spec.candidates.len());
        let mut seen: HashSet<String> = HashSet::new();
        for (position, w) in weights.iter().enumerate() {
            let Some(candidate) = spec.candidates.iter().find(|c| c.id.label() == w.label) else {
                return Err(JevError::UnknownLabel {
                    primitive: P,
                    label: w.label.clone(),
                    allowed: labels.join(", "),
                });
            };
            if !seen.insert(candidate.id.label()) {
                continue;
            }
            in_range(P, "p", w.p as f64, 0.0, 1.0)?;
            let rationale = fit(
                &format!(
                    "{} of {} for {} (p={:.2}) — {}",
                    ordinal(position + 1),
                    spec.candidates.len(),
                    spec.subject,
                    w.p,
                    candidate.summary
                ),
                MAX_RATIONALE,
            );
            ordered.push(Ranked { id: candidate.id.clone(), rationale, p: w.p });
        }

        if ordered.len() != spec.candidates.len() {
            return Err(JevError::BadRanking {
                primitive: P,
                got: ordered.len(),
                want: spec.candidates.len(),
            });
        }

        let out = RankOut {
            ordered,
            evidence: Evidence { confidence: confidence as f32, distribution: weights },
        };
        out.validate()?;
        self.record(&outcome, &out)?;
        Ok(out)
    }
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}
