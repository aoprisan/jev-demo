//! The question/answer vocabulary of a Jev call.
//!
//! The System One API answers three kinds of question and nothing else: a
//! [`Ask::Noul`] (probability of yes), an [`Ask::Choice`] (one label out of a set
//! you define, plus the whole distribution) and an [`Ask::Score`] (a
//! probability-weighted position on an ordered rubric). Every primitive in this
//! crate is built out of those three.
//!
//! Instructions are JSON, not prose. System One reads a question's
//! `instructions` as the judgment to make, and accepts an object there, so each
//! primitive composes one from the caller's question plus its own standing
//! guidance (`prompts/<primitive>.json`) — named fields such as `what`,
//! `not_for` and `evidence` rather than a paragraph the model would have to
//! parse. Nothing instruction-like goes into the state.
//!
//! These types mirror the SDK's rather than re-export it, for two reasons: the
//! SDK's answer structs are `#[non_exhaustive]` and so cannot be constructed by
//! a mock outside their crate, and keeping our own vocabulary lets `jev-core`'s
//! tests run with no transport at all.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// One question put to Jev.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Ask {
    /// Yes/no; answered with a probability.
    Noul {
        /// What is being asserted: a string, or an object of named parts.
        instructions: Value,
        /// What a yes means.
        #[serde(skip_serializing_if = "Option::is_none")]
        yes: Option<String>,
        /// What a no means.
        #[serde(skip_serializing_if = "Option::is_none")]
        no: Option<String>,
    },
    /// One of N labels; answered with a label and the full distribution.
    Choice {
        /// What is being decided: a string, or an object of named parts.
        instructions: Value,
        /// Label -> description, in offer order.
        options: Vec<(String, String)>,
    },
    /// A position on an ordered rubric; answered with a weighted level.
    Score {
        /// What is being rated: a string, or an object of named parts.
        instructions: Value,
        /// Ordered level descriptions, lowest first.
        levels: Vec<String>,
    },
}

impl Ask {
    /// A yes/no question.
    pub fn noul(instructions: impl Into<Value>) -> Self {
        Ask::Noul { instructions: instructions.into(), yes: None, no: None }
    }

    /// Describe the yes and no outcomes of a noul.
    pub fn criteria(mut self, yes_desc: impl Into<String>, no_desc: impl Into<String>) -> Self {
        if let Ask::Noul { yes, no, .. } = &mut self {
            *yes = Some(yes_desc.into());
            *no = Some(no_desc.into());
        }
        self
    }

    /// A choice between described labels.
    pub fn choice<I, A, B>(instructions: impl Into<Value>, options: I) -> Self
    where
        I: IntoIterator<Item = (A, B)>,
        A: Into<String>,
        B: Into<String>,
    {
        Ask::Choice {
            instructions: instructions.into(),
            options: options.into_iter().map(|(a, b)| (a.into(), b.into())).collect(),
        }
    }

    /// A score over ordered levels, lowest first.
    pub fn score<I, S>(instructions: impl Into<Value>, levels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Ask::Score {
            instructions: instructions.into(),
            levels: levels.into_iter().map(Into::into).collect(),
        }
    }

    /// The wire tag of this question kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Ask::Noul { .. } => "noul",
            Ask::Choice { .. } => "choice",
            Ask::Score { .. } => "score",
        }
    }

    /// The instructions, whatever their shape.
    pub fn instructions(&self) -> &Value {
        match self {
            Ask::Noul { instructions, .. }
            | Ask::Choice { instructions, .. }
            | Ask::Score { instructions, .. } => instructions,
        }
    }

    /// The `question` field of structured instructions, or the whole string.
    pub fn question(&self) -> Option<&str> {
        match self.instructions() {
            Value::String(s) => Some(s),
            Value::Object(o) => o.get("question").and_then(Value::as_str),
            _ => None,
        }
    }
}

/// An ordered set of named questions. Answers come back under the same names.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Asks(IndexMap<String, Ask>);

impl Asks {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a question, builder-style.
    pub fn with(mut self, name: impl Into<String>, ask: Ask) -> Self {
        self.0.insert(name.into(), ask);
        self
    }

    /// Look up a question by name.
    pub fn get(&self, name: &str) -> Option<&Ask> {
        self.0.get(name)
    }

    /// Iterate in offer order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Ask)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// How many questions.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One answer from Jev.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Verdict {
    /// Probability of yes.
    Noul {
        /// 0..=1.
        p: f64,
    },
    /// The selected label and the distribution it came from.
    Choice {
        /// Highest-probability label.
        label: String,
        /// Every offered label mapped to its probability.
        probabilities: IndexMap<String, f64>,
        /// Certainty derived from the distribution, 0..=1.
        confidence: f64,
    },
    /// A probability-weighted position on the rubric.
    Score {
        /// Weighted level; may fall between levels.
        score: f64,
        /// Number of levels in the rubric.
        levels: usize,
        /// Level index -> probability.
        probabilities: BTreeMap<u32, f64>,
        /// Certainty derived from the distribution, 0..=1.
        confidence: f64,
    },
}

impl Verdict {
    /// The wire tag of this answer kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Verdict::Noul { .. } => "noul",
            Verdict::Choice { .. } => "choice",
            Verdict::Score { .. } => "score",
        }
    }

    /// The probability, if this is a noul.
    pub fn as_noul(&self) -> Option<f64> {
        match self {
            Verdict::Noul { p } => Some(*p),
            _ => None,
        }
    }

    /// The label, distribution and confidence, if this is a choice.
    pub fn as_choice(&self) -> Option<(&str, &IndexMap<String, f64>, f64)> {
        match self {
            Verdict::Choice { label, probabilities, confidence } => {
                Some((label.as_str(), probabilities, *confidence))
            }
            _ => None,
        }
    }

    /// The weighted score, level count and confidence, if this is a score.
    pub fn as_score(&self) -> Option<(f64, usize, f64)> {
        match self {
            Verdict::Score { score, levels, confidence, .. } => {
                Some((*score, *levels, *confidence))
            }
            _ => None,
        }
    }

    /// Labels ordered by descending probability. Ties break on offer order, so the
    /// ordering is deterministic for a given reply.
    pub fn ranked(&self) -> Vec<(&str, f64)> {
        match self {
            Verdict::Choice { probabilities, .. } => {
                let mut v: Vec<(&str, f64)> =
                    probabilities.iter().map(|(k, p)| (k.as_str(), *p)).collect();
                v.sort_by(|a, b| b.1.total_cmp(&a.1));
                v
            }
            _ => Vec::new(),
        }
    }

    /// A score mapped onto `0..=1` by its position on the rubric.
    pub fn score_fraction(&self) -> Option<f64> {
        match self {
            Verdict::Score { score, levels, .. } if *levels > 1 => {
                Some(score / (*levels as f64 - 1.0))
            }
            Verdict::Score { .. } => Some(0.0),
            _ => None,
        }
    }
}

/// Token usage reported for a call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens, when reported.
    pub input_tokens: Option<u64>,
    /// Output tokens, when reported.
    pub output_tokens: Option<u64>,
}

impl Usage {
    /// Input + output, treating unreported counts as zero.
    pub fn total(&self) -> u64 {
        self.input_tokens.unwrap_or(0) + self.output_tokens.unwrap_or(0)
    }
}

/// How far a distribution may stray from summing to one. The API reports
/// probabilities rounded to a few decimals, so a three-way split can arrive
/// as 0.333 + 0.333 + 0.333; that is rounding, not a malformed answer.
const SUM_TOLERANCE: f64 = 0.01;
/// How far a reported `score` may sit from the weighted sum of the rounded
/// levels it came with, in levels.
const SCORE_TOLERANCE: f64 = 0.05;

impl Verdict {
    /// Fill in levels the answer left out of a Score distribution as zero mass,
    /// so a service that omits empty levels is not read as a malformed reply.
    /// Only levels the rubric offered are added; anything else is left for
    /// [`Verdict::validate_for`] to reject.
    pub(crate) fn fill_missing_levels(&mut self, ask: &Ask) {
        if let (Verdict::Score { probabilities, .. }, Ask::Score { levels, .. }) = (self, ask) {
            for level in 0..levels.len() as u32 {
                probabilities.entry(level).or_insert(0.0);
            }
        }
    }

    /// Validate the complete answer against the question before composition.
    pub(crate) fn validate_for(
        &self,
        ask: &Ask,
        name: &str,
        primitive: &'static str,
    ) -> crate::error::Result<()> {
        use crate::error::JevError;
        use crate::primitives::in_range;
        let invalid = |detail: &str| JevError::Contradiction {
            primitive,
            detail: format!("{name}: {detail}"),
        };
        let distribution = |values: Vec<f64>| -> crate::error::Result<()> {
            for p in &values {
                in_range(primitive, "probability", *p, 0.0, 1.0)?;
            }
            if (values.iter().sum::<f64>() - 1.0).abs() > SUM_TOLERANCE {
                return Err(invalid("probabilities must sum to one"));
            }
            Ok(())
        };
        match (self, ask) {
            (Verdict::Noul { p }, Ask::Noul { .. }) => {
                in_range(primitive, "probability", *p, 0.0, 1.0)
            }
            (Verdict::Choice { label, probabilities, confidence }, Ask::Choice { options, .. }) => {
                if !options.iter().any(|(key, _)| key == label) {
                    return Err(JevError::UnknownLabel {
                        primitive,
                        label: label.clone(),
                        allowed: options
                            .iter()
                            .map(|(key, _)| key.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    });
                }
                in_range(primitive, "confidence", *confidence, 0.0, 1.0)?;
                if probabilities.len() != options.len()
                    || options.iter().any(|(key, _)| !probabilities.contains_key(key))
                {
                    return Err(invalid("distribution must contain exactly the offered options"));
                }
                distribution(probabilities.values().copied().collect())?;
                if probabilities.values().any(|p| *p > probabilities[label] + SUM_TOLERANCE) {
                    return Err(invalid("selected label is not a highest-probability option"));
                }
                Ok(())
            }
            (
                Verdict::Score { score, levels, probabilities, confidence },
                Ask::Score { levels: offered, .. },
            ) => {
                in_range(primitive, "confidence", *confidence, 0.0, 1.0)?;
                if *levels != offered.len()
                    || *levels < 2
                    || probabilities.len() != *levels
                    || (0..*levels).any(|i| !probabilities.contains_key(&(i as u32)))
                {
                    return Err(invalid("distribution must contain exactly the offered levels"));
                }
                in_range(primitive, "score", *score, 0.0, (*levels - 1) as f64)?;
                distribution(probabilities.values().copied().collect())?;
                let expected: f64 = probabilities.iter().map(|(i, p)| *i as f64 * p).sum();
                if (*score - expected).abs() > SCORE_TOLERANCE {
                    return Err(invalid("score disagrees with its probability-weighted levels"));
                }
                Ok(())
            }
            _ => Err(JevError::AnswerKind {
                name: name.to_owned(),
                primitive,
                got: self.kind(),
                want: ask.kind(),
            }),
        }
    }
}
