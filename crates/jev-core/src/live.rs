//! `LiveJev` — the System One backend, over the `typesafe-ai-sdk` crate.
//!
//! The translation is mechanical: our [`Ask`] becomes the SDK's question type,
//! the SDK's answer becomes our [`Verdict`]. The primitives above are unchanged
//! by which backend answers, which is the whole point of the seam.

use crate::ask::{Ask, Usage, Verdict};
use crate::client::{JevCall, JevClient, JevReply};
use crate::error::{JevError, Result};
use indexmap::IndexMap;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use typesafe::{Answer, Choice, Client, Noul, Questions, Score};

/// The model pinned unless a caller chooses another: the docs' recommended
/// alias for the current Jev.
pub const DEFAULT_MODEL: &str = "jev-latest";

/// The live Jev backend.
pub struct LiveJev {
    client: Client,
    model: String,
    timeout: Option<Duration>,
}

impl std::fmt::Debug for LiveJev {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveJev").field("model", &self.model).finish()
    }
}

impl LiveJev {
    /// A client configured from `TYPESAFE_API_KEY` and the usual environment.
    pub fn from_env() -> Result<Self> {
        let client = Client::from_env().map_err(|e| {
            JevError::Transport(format!(
                "could not build a System One client ({e}). Set TYPESAFE_API_KEY, \
                 or run with --mock to use the offline rule-based backend."
            ))
        })?;
        Ok(Self { client, model: DEFAULT_MODEL.to_owned(), timeout: None })
    }

    /// A client over an already-built SDK client.
    ///
    /// Useful for pointing the backend at a stub server in tests, and for
    /// callers that configure retries, proxies or headers themselves.
    pub fn new(client: Client) -> Self {
        Self { client, model: DEFAULT_MODEL.to_owned(), timeout: None }
    }

    /// Pin a different model.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Set a per-call timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// The endpoint the body goes to.
pub const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

/// The `POST /v1/systemone` body for `call`, exactly as [`LiveJev`] sends it:
/// the state verbatim, the model, and the questions in the SDK's wire shape,
/// in offer order. Built from the same translation the live client uses, so
/// what an audit viewer shows is what a live run put on the wire — for a mock
/// call, what it would have.
pub fn wire_request(call: &JevCall, model: &str) -> serde_json::Value {
    let questions = serde_json::to_value(questions_for(call)).unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "state": call.state,
        "model": model,
        "questions": questions,
    })
}

fn questions_for(call: &JevCall) -> Questions {
    let mut questions = Questions::new();
    for (name, ask) in call.asks.iter() {
        match ask {
            Ask::Noul { instructions, yes, no } => {
                let mut q = Noul::new(instructions.clone());
                if let Some(yes) = yes {
                    q = q.when_true(yes.clone());
                }
                if let Some(no) = no {
                    q = q.when_false(no.clone());
                }
                questions = questions.with(name, q);
            }
            Ask::Choice { instructions, options } => {
                let mut q = Choice::new(instructions.clone());
                for (label, describe) in options {
                    q = q.option(label.clone(), describe.clone());
                }
                questions = questions.with(name, q);
            }
            Ask::Score { instructions, levels } => {
                questions =
                    questions.with(name, Score::new(instructions.clone(), levels.iter().cloned()));
            }
        }
    }
    questions
}

#[async_trait::async_trait]
impl JevClient for LiveJev {
    fn backend(&self) -> &'static str {
        "jev"
    }

    async fn ask(&self, call: &JevCall) -> Result<JevReply> {
        let started = Instant::now();
        // The state goes as built: content only. Every instruction travels in
        // its question.
        let mut request = self
            .client
            .system_one(call.state.clone(), questions_for(call))
            .model(self.model.clone());
        if let Some(timeout) = self.timeout {
            request = request.timeout(timeout);
        }
        let response = request.send().await.map_err(|e| JevError::Transport(e.to_string()))?;
        let latency = started.elapsed();

        let mut verdicts = IndexMap::with_capacity(response.answers.len());
        for (name, answer) in &response.answers {
            verdicts.insert(name.clone(), verdict_from(answer, call, name)?);
        }

        Ok(JevReply {
            verdicts,
            model: response.model.clone(),
            usage: Usage {
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
            },
            latency,
        })
    }
}

fn verdict_from(answer: &Answer, call: &JevCall, name: &str) -> Result<Verdict> {
    Ok(match answer {
        Answer::Noul(a) => Verdict::Noul { p: a.noul },
        Answer::Choice(a) => Verdict::Choice {
            label: a.choice.clone(),
            probabilities: a.probabilities.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            confidence: a.confidence,
        },
        Answer::Score(a) => {
            // The SDK reports the legend it was given; the level count is the
            // rubric we sent, which is authoritative for mapping score to a
            // fraction.
            let levels = match call.asks.get(name) {
                Some(Ask::Score { levels, .. }) => levels.len(),
                _ => a.legend.len(),
            };
            Verdict::Score {
                score: a.score,
                levels,
                probabilities: a
                    .probabilities
                    .iter()
                    .map(|(k, v)| (*k, *v))
                    .collect::<BTreeMap<u32, f64>>(),
                confidence: a.confidence,
            }
        }
        other => {
            return Err(JevError::Transport(format!(
                "System One answered `{name}` with an unsupported answer type `{}`",
                other.kind()
            )))
        }
    })
}
