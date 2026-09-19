//! The `JevClient`-backed implementation shared by all six primitives.
//!
//! One handle holds the transport, the audit log and the current pipeline stage.
//! Every primitive goes through [`Jev::call`], so tokens, latency and the full
//! question/answer pair are recorded in exactly one place.

use crate::ask::Asks;
use crate::audit::Audit;
use crate::client::{now_ms, CallRecord, JevCall, JevClient, JevReply, Primitive};
use crate::error::{JevError, Result};
use crate::primitives::JevInput;
use serde::Serialize;
use std::sync::Arc;

/// A call that has returned, kept until the primitive has composed its output so
/// the audit record can carry both.
pub struct CallOutcome {
    /// What was sent.
    pub call: JevCall,
    /// What came back.
    pub reply: JevReply,
}

/// The primitives' shared handle: transport plus audit plus stage.
#[derive(Clone)]
pub struct Jev {
    client: Arc<dyn JevClient>,
    audit: Arc<Audit>,
    stage: Option<Arc<str>>,
}

impl std::fmt::Debug for Jev {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jev")
            .field("backend", &self.client.backend())
            .field("stage", &self.stage)
            .field("calls", &self.audit.len())
            .finish()
    }
}

impl Jev {
    /// A handle over `client`, logging to a fresh in-memory audit.
    pub fn new(client: Arc<dyn JevClient>) -> Self {
        Self { client, audit: Arc::new(Audit::new()), stage: None }
    }

    /// A handle over `client`, sharing an existing audit log.
    pub fn with_audit(client: Arc<dyn JevClient>, audit: Arc<Audit>) -> Self {
        Self { client, audit, stage: None }
    }

    /// The same handle, tagging its calls with a pipeline stage name.
    pub fn staged(&self, stage: &str) -> Self {
        Self { client: self.client.clone(), audit: self.audit.clone(), stage: Some(stage.into()) }
    }

    /// The backend answering these calls (`"jev"` or `"mock"`).
    pub fn backend(&self) -> &'static str {
        self.client.backend()
    }

    /// The shared audit log.
    pub fn audit(&self) -> &Arc<Audit> {
        &self.audit
    }

    /// Put `asks` to the backend with `input` as the state.
    pub(crate) async fn call<I: JevInput>(
        &self,
        primitive: Primitive,
        input: &I,
        asks: Asks,
    ) -> Result<CallOutcome> {
        if asks.is_empty() {
            return Err(JevError::InvalidCall(format!("{primitive} asked nothing")));
        }
        let call = JevCall { primitive, state: state_of(input)?, asks };
        let mut reply = self.client.ask(&call).await?;
        for (name, ask) in call.asks.iter() {
            let Some(verdict) = reply.verdicts.get_mut(name) else {
                return Err(JevError::MissingAnswer {
                    name: name.to_owned(),
                    primitive: primitive.as_str(),
                    asked: call.asks.len(),
                });
            };
            verdict.fill_missing_levels(ask);
            verdict.validate_for(ask, name, primitive.as_str())?;
        }
        Ok(CallOutcome { call, reply })
    }

    /// Record a completed call together with the typed output it produced.
    ///
    /// For a [`Primitive::Batch`] call the output is the list of every stage's
    /// `{stage, primitive, output}`, in the order they were asked.
    pub(crate) fn record<T: Serialize>(&self, outcome: &CallOutcome, output: &T) -> Result<()> {
        let output = serde_json::to_value(output)
            .map_err(|e| JevError::Audit(format!("output is not serialisable: {e}")))?;
        self.audit.push(CallRecord {
            at_ms: now_ms(),
            backend: self.client.backend().to_owned(),
            primitive: outcome.call.primitive,
            stage: self.stage.as_ref().map(|s| s.to_string()),
            model: outcome.reply.model.clone(),
            state: outcome.call.state.clone(),
            asks: outcome.call.asks.clone(),
            verdicts: outcome.reply.verdicts.clone(),
            output,
            input_tokens: outcome.reply.usage.input_tokens,
            output_tokens: outcome.reply.usage.output_tokens,
            latency_ms: outcome.reply.latency.as_millis() as u64,
        })
    }
}

/// The state of a call: the domain framing, the input and the priors, as one
/// object. The framing is content about the situation (which solver proposed
/// this, what it may not change), so it lives in the state under its own name;
/// it is not a prompt.
pub(crate) fn state_of<I: JevInput>(input: &I) -> Result<serde_json::Value> {
    let mut state = input.to_state()?;
    let context = input.context_block();
    if let (Some(obj), false) = (state.as_object_mut(), context.is_empty()) {
        let mut with_context = serde_json::Map::with_capacity(obj.len() + 1);
        with_context.insert("context".to_owned(), serde_json::Value::String(context));
        with_context.extend(std::mem::take(obj));
        state = serde_json::Value::Object(with_context);
    }
    Ok(state)
}
