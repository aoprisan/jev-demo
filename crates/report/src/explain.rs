//! The Explain stage: the same day, written for different readers.
//!
//! The digest handed to Jev carries only counts and totals the run actually
//! produced. Jev chooses the framing, the severity and which of the caller's
//! facts belong in a summary for that audience; the wording of each is the
//! caller's own, so a summary cannot assert anything the run did not.

use jev_core::{Audience, Explain, ExplainOut, ExplainSpec, Jev, JevInput, Result};
use serde::Serialize;

/// What a finished run looked like, as Jev reads it.
#[derive(Debug, Clone, Serialize)]
pub struct SessionDigest {
    /// The observable totals.
    pub features: DigestFeatures,
    /// The headline numbers, already formatted.
    pub headlines: Vec<String>,
}

/// The counts the explain rules read.
#[derive(Debug, Clone, Serialize)]
pub struct DigestFeatures {
    /// Which domain.
    pub domain: String,
    /// Who the summary is for. The framing depends on it.
    pub audience: String,
    /// Decisions taken.
    pub decision_count: f64,
    /// Decisions the judgment layer changed.
    pub intervention_count: f64,
    /// Decisions sent to a human.
    pub escalation_count: f64,
    /// Primitive calls made.
    pub jev_calls: f64,
    /// Checks that failed across the run.
    pub failed_checks: f64,
}

impl JevInput for SessionDigest {
    /// Only what the counts do not already say: that the run is over and
    /// logged. The counts and the audience are fields of `features`.
    fn context_block(&self) -> String {
        format!(
            "A completed {} run; `features` holds its totals and who the summary is for. \
             The decision log holds every call and its output.",
            self.features.domain
        )
    }
}

/// The counts a domain hands over for its summary.
pub struct Counts {
    /// Which domain.
    pub domain: &'static str,
    /// Decisions taken.
    pub decisions: usize,
    /// Decisions the judgment layer changed.
    pub interventions: usize,
    /// Decisions sent to a human.
    pub escalations: usize,
    /// Primitive calls made.
    pub jev_calls: usize,
    /// Checks that failed.
    pub failed_checks: usize,
    /// Headline numbers, already formatted, for the record.
    pub headlines: Vec<String>,
}

/// Build a digest for one audience.
pub fn digest(counts: &Counts, audience: Audience) -> SessionDigest {
    SessionDigest {
        features: DigestFeatures {
            domain: counts.domain.to_owned(),
            audience: audience.as_str().to_owned(),
            decision_count: counts.decisions as f64,
            intervention_count: counts.interventions as f64,
            escalation_count: counts.escalations as f64,
            jev_calls: counts.jev_calls as f64,
            failed_checks: counts.failed_checks as f64,
        },
        headlines: counts.headlines.clone(),
    }
}

/// The explain specification for a domain and audience.
///
/// The framings and facts are the caller's vocabulary. Jev picks among them.
pub fn spec(counts: &Counts, audience: Audience, subject: &str) -> ExplainSpec {
    let interventions = counts.interventions;
    let escalations = counts.escalations;
    let decisions = counts.decisions;

    ExplainSpec::new(subject.to_owned(), audience)
        .framing(
            "quiet",
            "The judgment layer changed nothing of consequence; the solver ran as built.",
            "the judgment layer let the solver run and changed nothing of consequence",
        )
        .framing(
            "intervened",
            "The judgment layer changed what happened, repeatedly, without needing a human.",
            format!(
                "the judgment layer changed {interventions} of {decisions} decisions \
                 without needing a human"
            ),
        )
        .framing(
            "escalated",
            "At least one decision was sent to a human because the state was implausible.",
            format!(
                "{escalations} decision(s) went to a human because the state did not \
                 hold together"
            ),
        )
        .fact(
            "pnl_effect",
            "The run's headline result is worth stating.",
            counts.headlines.first().cloned().unwrap_or_else(|| "the result is logged".into()),
        )
        .fact(
            "gate_distribution",
            "How the gate's decisions were distributed is worth stating.",
            format!("{interventions} of {decisions} were held or sized down"),
        )
        .fact(
            "checks_ran",
            "Every candidate passed through the same named checks.",
            format!("the same named checks ran on every one, {} failing", counts.failed_checks),
        )
        .fact(
            "audit_location",
            "The decision log exists and is complete.",
            format!("all {} typed calls are in decisions.jsonl", counts.jev_calls),
        )
        .fact(
            "escalations",
            "Something was sent to a human.",
            format!("{escalations} went to a human rather than being sized down"),
        )
        .fact(
            "interventions",
            "The judgment layer changed outcomes.",
            counts
                .headlines
                .get(1)
                .cloned()
                .unwrap_or_else(|| format!("{interventions} outcomes changed")),
        )
}

/// Run Explain for one audience.
pub async fn for_audience(
    jev: &Jev,
    counts: &Counts,
    audience: Audience,
    subject: &str,
) -> Result<ExplainOut> {
    let input = digest(counts, audience);
    Explain::explain(&jev.staged("explain"), &input, &spec(counts, audience, subject)).await
}

/// Run Explain for several audiences.
pub async fn for_audiences(
    jev: &Jev,
    counts: &Counts,
    audiences: &[Audience],
    subject: &str,
) -> Result<Vec<ExplainOut>> {
    let mut out = Vec::with_capacity(audiences.len());
    for audience in audiences {
        out.push(for_audience(jev, counts, *audience, subject).await?);
    }
    Ok(out)
}
