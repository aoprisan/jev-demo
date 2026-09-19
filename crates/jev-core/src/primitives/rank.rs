//! `Rank<I, T>` — order solver-produced candidates by fit to the conditions.
//!
//! One Score question per candidate, all in one call, on a shared fit rubric.
//! Each candidate is rated on its own terms against the same state, and the
//! ordering is read off the ratings in code. That is what makes the ordering
//! honest: a single Choice over the candidates would say which one is *best*,
//! and the mass it left on the others would be the probability that *they* are
//! best — not a second place, a third place, or a margin. Comparable
//! per-candidate ratings give all three.

use super::{at_most_chars, fit, in_range, need_score, Evidence, JevInput, Weight};
use crate::ask::{Ask, Asks};
use crate::client::{JevReply, Primitive};
use crate::error::{JevError, Result};
use crate::jev::Jev;
use crate::prompts;
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
    /// The question put to Jev about each candidate.
    pub question: String,
    /// The candidates, in offer order. All of them are valid; Jev rates them.
    pub candidates: Vec<Candidate<T>>,
    /// The fit rubric every candidate is rated on, lowest first.
    pub fit_levels: Vec<String>,
}

impl<T: CandidateId> RankSpec<T> {
    /// A ranking question over the given candidates, on the default four-level
    /// fit rubric (unsuited / workable / good fit / best fit).
    pub fn new(
        subject: impl Into<String>,
        question: impl Into<String>,
        candidates: Vec<Candidate<T>>,
    ) -> Self {
        Self {
            subject: subject.into(),
            question: question.into(),
            candidates,
            fit_levels: vec![
                "Unsuited: today's conditions work against what this candidate does.".into(),
                "Workable: nothing today argues against it, and nothing argues for it.".into(),
                "Good fit: today's conditions favour this candidate's trade-offs.".into(),
                "Best fit: today's conditions call for exactly this candidate's trade-offs.".into(),
            ],
        }
    }

    /// Replace the fit rubric.
    pub fn fit_levels<I: IntoIterator<Item = S>, S: Into<String>>(mut self, levels: I) -> Self {
        self.fit_levels = levels.into_iter().map(Into::into).collect();
        self
    }
}

/// One candidate's place in the ordering.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Ranked<T> {
    /// The caller's identifier.
    pub id: T,
    /// Composed from the verdict; at most 160 characters.
    pub rationale: String,
    /// Jev's rating of this candidate's fit, 0..=1 of the rubric.
    pub fit: f32,
}

/// Every candidate, best first.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct RankOut<T> {
    /// The ordering, best first. Contains exactly the candidates offered.
    pub ordered: Vec<Ranked<T>>,
    /// Mean certainty across the ratings, and every candidate's fit.
    pub evidence: Evidence,
}

impl<T: CandidateId> RankOut<T> {
    /// The winner.
    pub fn top(&self) -> Option<&Ranked<T>> {
        self.ordered.first()
    }

    /// How far the winner's fit is clear of the runner-up's, 0..=1. A small
    /// margin means the candidates were close, which is itself worth reporting.
    pub fn margin(&self) -> f32 {
        match (self.ordered.first(), self.ordered.get(1)) {
            (Some(a), Some(b)) => a.fit - b.fit,
            _ => 1.0,
        }
    }

    /// The one-line rendering later stages see in `prior_judgments`.
    pub fn line(&self) -> String {
        match self.top() {
            Some(top) => format!("{} first (margin {:.2})", top.id.label(), self.margin()),
            None => "no candidates".to_owned(),
        }
    }

    /// Re-check every schema bound.
    pub fn validate(&self) -> Result<()> {
        for r in &self.ordered {
            in_range(P, "fit", r.fit as f64, 0.0, 1.0)?;
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
        let outcome = self.call(Primitive::Rank, input, asks(spec, "")?).await?;
        let out = compose(spec, &outcome.reply, "")?;
        self.record(&outcome, &out)?;
        Ok(out)
    }
}

/// The ask name for one candidate.
fn fit_name(prefix: &str, label: &str) -> String {
    format!("{prefix}fit_{label}")
}

/// One fit Score per candidate, on the shared rubric.
pub(crate) fn asks<T: CandidateId>(spec: &RankSpec<T>, prefix: &str) -> Result<Asks> {
    if spec.candidates.len() < 2 {
        return Err(JevError::InvalidCall("rank needs at least two candidates".into()));
    }
    if spec.fit_levels.len() < 2 {
        return Err(JevError::InvalidCall("rank fit rubric needs at least two levels".into()));
    }
    let labels: Vec<String> = spec.candidates.iter().map(|c| c.id.label()).collect();
    if labels.iter().collect::<HashSet<_>>().len() != labels.len() {
        return Err(JevError::InvalidCall(
            "rank candidate labels must be unique within a call".into(),
        ));
    }
    let mut asks = Asks::new();
    for c in &spec.candidates {
        asks = asks.with(
            fit_name(prefix, &c.id.label()),
            Ask::score(
                prompts::instructions(
                    Primitive::Rank,
                    "fit",
                    spec.question.clone(),
                    [(
                        "candidate",
                        serde_json::json!({ "id": c.id.label(), "summary": c.summary }),
                    )],
                ),
                spec.fit_levels.clone(),
            ),
        );
    }
    Ok(asks)
}

/// The ordering, read off the per-candidate ratings.
pub(crate) fn compose<T: CandidateId>(
    spec: &RankSpec<T>,
    reply: &JevReply,
    prefix: &str,
) -> Result<RankOut<T>> {
    let mut rated: Vec<(&Candidate<T>, f64, f64)> = Vec::with_capacity(spec.candidates.len());
    for c in &spec.candidates {
        let (fraction, confidence) = need_score(reply, &fit_name(prefix, &c.id.label()), P)?;
        in_range(P, "fit", fraction, 0.0, 1.0)?;
        rated.push((c, fraction, confidence));
    }
    // Stable, so equal ratings keep their offer order and the result is
    // deterministic for a given reply.
    rated.sort_by(|a, b| b.1.total_cmp(&a.1));

    let n = spec.candidates.len();
    let ordered = rated
        .iter()
        .enumerate()
        .map(|(position, (c, fraction, _))| Ranked {
            id: c.id.clone(),
            rationale: fit(
                &format!(
                    "{} of {} for {} (fit {:.2}) — {}",
                    ordinal(position + 1),
                    n,
                    spec.subject,
                    fraction,
                    c.summary
                ),
                MAX_RATIONALE,
            ),
            fit: *fraction as f32,
        })
        .collect();
    let confidence = rated.iter().map(|(_, _, c)| c).sum::<f64>() / n as f64;
    let distribution =
        rated.iter().map(|(c, f, _)| Weight { label: c.id.label(), p: *f as f32 }).collect();

    let out =
        RankOut { ordered, evidence: Evidence { confidence: confidence as f32, distribution } };
    out.validate()?;
    Ok(out)
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
