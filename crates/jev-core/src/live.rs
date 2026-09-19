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

/// The live Jev backend.
pub struct LiveJev {
    client: Client,
    model: Option<String>,
    timeout: Option<Duration>,
}

impl std::fmt::Debug for LiveJev {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveJev")
            .field("model", &self.model.as_deref().unwrap_or(self.client.default_model()))
            .finish()
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
        Ok(Self { client, model: None, timeout: None })
    }

    /// Pin a model, e.g. `jev-latest`.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set a per-call timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// The state sent to Jev: the primitive's standing instructions, the domain
/// context block, and the decision state itself.
fn state_for(call: &JevCall) -> serde_json::Value {
    serde_json::json!({
        "instructions": call.instructions,
        "context": call.context,
        "state": call.state,
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
                questions = questions
                    .with(name, Score::new(instructions.clone(), levels.iter().cloned()));
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
        let mut request = self.client.system_one(state_for(call), questions_for(call));
        if let Some(model) = &self.model {
            request = request.model(model.clone());
        }
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
