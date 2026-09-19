//! `Explain<I>` — turn a decision log into prose aimed at one audience.
//!
//! Jev chooses the framing, the severity and which facts belong in the summary.
//! The wording of each of those is the caller's, so the summary cannot assert
//! anything the log did not already contain.

use super::{at_most_chars, evidence_from, fit, in_range, need_choice, need_noul, need_score, Evidence, JevInput};
use crate::ask::{Ask, Asks};
use crate::client::Primitive;
use crate::error::{JevError, Result};
use crate::jev::Jev;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const P: &str = "explain";
const MAX_SUMMARY: usize = 300;

/// Who the summary is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    /// Wants the actionable shape.
    Trader,
    /// Wants the control story.
    Compliance,
    /// Wants what might need attention.
    Ops,
}

impl Audience {
    /// All three.
    pub const ALL: [Audience; 3] = [Audience::Trader, Audience::Compliance, Audience::Ops];

    /// Lower-case name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Audience::Trader => "trader",
            Audience::Compliance => "compliance",
            Audience::Ops => "ops",
        }
    }

    /// What this audience wants from a summary.
    pub fn wants(&self) -> &'static str {
        match self {
            Audience::Trader => {
                "what was put on, what was held back, and which condition drove it"
            }
            Audience::Compliance => {
                "that the decision was bounded, that the checks ran, and where the record is"
            }
            Audience::Ops => "what was escalated, what is stale, and what will repeat tomorrow",
        }
    }
}

impl std::str::FromStr for Audience {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Audience::ALL
            .into_iter()
            .find(|a| a.as_str() == s.to_ascii_lowercase())
            .ok_or_else(|| format!("unknown audience `{s}`; expected trader, compliance or ops"))
    }
}

/// One way the summary could lead.
#[derive(Debug, Clone)]
pub struct Framing {
    /// Wire label.
    pub label: String,
    /// What this framing means. Jev reads this when choosing.
    pub describe: String,
    /// The caller's prose for this framing, used when it is chosen.
    pub lead: String,
}

impl Framing {
    /// A framing: what Jev reads, and what gets written if it is chosen.
    pub fn new(
        label: impl Into<String>,
        describe: impl Into<String>,
        lead: impl Into<String>,
    ) -> Self {
        Self { label: label.into(), describe: describe.into(), lead: lead.into() }
    }
}

/// One fact from the log that may or may not belong in the summary.
#[derive(Debug, Clone)]
pub struct FactSpec {
    /// Short name.
    pub name: String,
    /// The claim Jev evaluates: does this belong in a summary for this audience?
    pub claim: String,
    /// The caller's prose for this fact, used when Jev includes it.
    pub phrase: String,
}

impl FactSpec {
    /// A candidate fact.
    pub fn new(
        name: impl Into<String>,
        claim: impl Into<String>,
        phrase: impl Into<String>,
    ) -> Self {
        Self { name: name.into(), claim: claim.into(), phrase: phrase.into() }
    }
}

/// What is being explained, to whom, out of what vocabulary.
#[derive(Debug, Clone)]
pub struct ExplainSpec {
    /// A short name for the subject, e.g. `EUR/USD session` or `battery day 34`.
    pub subject: String,
    /// The audience.
    pub audience: Audience,
    /// The candidate leads. At least two.
    pub framings: Vec<Framing>,
    /// The severity rubric, lowest first, paired with the word used in prose.
    pub severity: Vec<(String, String)>,
    /// The candidate facts. Those Jev includes are appended in spec order.
    pub facts: Vec<FactSpec>,
    /// Probability at or above which a fact is included. Defaults to 0.5.
    pub fact_threshold: f64,
}

impl ExplainSpec {
    /// An explain question for one audience.
    pub fn new(subject: impl Into<String>, audience: Audience) -> Self {
        Self {
            subject: subject.into(),
            audience,
            framings: Vec::new(),
            severity: vec![
                ("An ordinary session; nothing needed attention.".into(), "routine".into()),
                ("Some friction, all of it handled by the system.".into(), "unremarkable".into()),
                ("Notable: the judgment layer changed what happened.".into(), "notable".into()),
                ("Difficult: repeated intervention, or an escalation.".into(), "difficult".into()),
            ],
            facts: Vec::new(),
            fact_threshold: 0.5,
        }
    }

    /// Add a candidate lead.
    pub fn framing(
        mut self,
        label: impl Into<String>,
        describe: impl Into<String>,
        lead: impl Into<String>,
    ) -> Self {
        self.framings.push(Framing::new(label, describe, lead));
        self
    }

    /// Add a candidate fact.
    pub fn fact(
        mut self,
        name: impl Into<String>,
        claim: impl Into<String>,
        phrase: impl Into<String>,
    ) -> Self {
        self.facts.push(FactSpec::new(name, claim, phrase));
        self
    }

    /// Replace the severity rubric.
    pub fn severity<I, A, B>(mut self, levels: I) -> Self
    where
        I: IntoIterator<Item = (A, B)>,
        A: Into<String>,
        B: Into<String>,
    {
        self.severity = levels.into_iter().map(|(a, b)| (a.into(), b.into())).collect();
        self
    }
}

/// A summary written for one audience.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExplainOut {
    /// The summary; at most 300 characters.
    pub summary: String,
    /// Who it is for.
    pub for_audience: Audience,
    /// The framing Jev chose, and how certain it was.
    pub evidence: Evidence,
}

impl ExplainOut {
    /// Re-check every schema bound.
    pub fn validate(&self) -> Result<()> {
        at_most_chars(P, "summary", &self.summary, MAX_SUMMARY)
    }
}

/// Turn a decision log into prose for a named audience.
#[async_trait::async_trait]
pub trait Explain<I: JevInput> {
    /// Explain `input` to the audience in `spec`.
    async fn explain(&self, input: &I, spec: &ExplainSpec) -> Result<ExplainOut>;
}

#[async_trait::async_trait]
impl<I: JevInput> Explain<I> for Jev {
    async fn explain(&self, input: &I, spec: &ExplainSpec) -> Result<ExplainOut> {
        if spec.framings.len() < 2 {
            return Err(JevError::InvalidCall("explain needs at least two framings".into()));
        }
        if spec.severity.len() < 2 {
            return Err(JevError::InvalidCall(
                "explain needs at least two severity levels".into(),
            ));
        }
        let audience = spec.audience;
        let mut asks = Asks::new()
            .with(
                "framing",
                Ask::choice(
                    format!(
                        "A summary of {} is being written for {}, who wants {}. Which framing \
                         should lead?",
                        spec.subject,
                        audience.as_str(),
                        audience.wants()
                    ),
                    spec.framings.iter().map(|f| (f.label.clone(), f.describe.clone())),
                ),
            )
            .with(
                "severity",
                Ask::score(
                    format!("How serious was {} overall?", spec.subject),
                    spec.severity.iter().map(|(describe, _)| describe.clone()),
                ),
            );
        for fact in &spec.facts {
            asks = asks.with(
                format!("fact_{}", fact.name),
                Ask::noul(format!(
                    "{} Does this belong in a summary for {}?",
                    fact.claim,
                    audience.as_str()
                ))
                .criteria(
                    "This audience needs it to understand the day.",
                    "True, but not what this audience is reading for.",
                ),
            );
        }

        let outcome = self.call(Primitive::Explain, input, asks).await?;
        let (label, probabilities, confidence) = need_choice(&outcome.reply, "framing", P)?;
        let framing = spec.framings.iter().find(|f| f.label == label).ok_or_else(|| {
            JevError::UnknownLabel {
                primitive: P,
                label: label.to_owned(),
                allowed: spec
                    .framings
                    .iter()
                    .map(|f| f.label.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            }
        })?;

        let (fraction, _) = need_score(&outcome.reply, "severity", P)?;
        in_range(P, "severity", fraction, 0.0, 1.0)?;
        let level = (fraction * (spec.severity.len() - 1) as f64).round() as usize;
        let severity_word =
            spec.severity.get(level.min(spec.severity.len() - 1)).map(|(_, w)| w.as_str());

        let mut included = Vec::new();
        for fact in &spec.facts {
            let p = need_noul(&outcome.reply, &format!("fact_{}", fact.name), P)?;
            in_range(P, "fact", p, 0.0, 1.0)?;
            if p >= spec.fact_threshold {
                included.push(fact.phrase.clone());
            }
        }

        let mut summary = format!("{} — {}", spec.subject, end_sentence(&framing.lead));
        if let Some(word) = severity_word {
            summary.push_str(&format!(" The session was {word}."));
        }
        if !included.is_empty() {
            summary.push(' ');
            summary.push_str(&end_sentence(&join_sentence(&included)));
        }
        let summary = fit(&summary, MAX_SUMMARY);

        let out = ExplainOut {
            summary,
            for_audience: audience,
            evidence: evidence_from(probabilities, confidence),
        };
        out.validate()?;
        self.record(&outcome, &out)?;
        Ok(out)
    }
}

fn join_sentence(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => capitalise(one),
        [head @ .., last] => format!("{}, and {last}", capitalise(&head.join("; "))),
    }
}

/// Give a caller-supplied phrase a full stop if it does not already end a sentence.
fn end_sentence(s: &str) -> String {
    let trimmed = s.trim_end();
    if trimmed.ends_with(['.', '!', '?']) {
        trimmed.to_owned()
    } else {
        format!("{trimmed}.")
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
