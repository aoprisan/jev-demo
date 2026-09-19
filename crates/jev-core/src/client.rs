//! The transport seam: one trait, two implementations (live API, rule-based mock).

use crate::ask::{Asks, Usage, Verdict};
use crate::error::Result;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Which primitive originated a call. Used for prompt selection and for the audit log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Primitive {
    /// [`crate::Gate`].
    Gate,
    /// [`crate::Classify`].
    Classify,
    /// [`crate::Score`].
    Score,
    /// [`crate::Check`].
    Check,
    /// [`crate::Rank`].
    Rank,
    /// [`crate::Explain`].
    Explain,
}

impl Primitive {
    /// Lower-case name, matching the file stem in `prompts/`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Primitive::Gate => "gate",
            Primitive::Classify => "classify",
            Primitive::Score => "score",
            Primitive::Check => "check",
            Primitive::Rank => "rank",
            Primitive::Explain => "explain",
        }
    }
}

impl std::fmt::Display for Primitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One outbound call: the primitive's standing instructions, the domain framing
/// block, the state and the questions.
#[derive(Debug, Clone, Serialize)]
pub struct JevCall {
    /// Which primitive is asking.
    pub primitive: Primitive,
    /// The primitive's standing instructions, from `prompts/<primitive>.md`.
    pub instructions: String,
    /// Domain-specific framing, injected as a context block rather than a separate prompt.
    pub context: String,
    /// The state Jev judges. Always an object; a pipeline stage nests the domain
    /// input under `input` and prior judgments under `prior_judgments`.
    pub state: serde_json::Value,
    /// The questions, in offer order.
    pub asks: Asks,
}

/// One inbound reply.
#[derive(Debug, Clone)]
pub struct JevReply {
    /// Answers keyed by question name.
    pub verdicts: IndexMap<String, Verdict>,
    /// The model that answered.
    pub model: String,
    /// Token usage.
    pub usage: Usage,
    /// Wall-clock time of the call.
    pub latency: Duration,
}

/// A Jev backend. Implemented by the live System One client and by the mock.
#[async_trait::async_trait]
pub trait JevClient: Send + Sync {
    /// Short backend name for the audit log (`"jev"` or `"mock"`).
    fn backend(&self) -> &'static str;

    /// Put the questions to Jev and return an answer for each.
    async fn ask(&self, call: &JevCall) -> Result<JevReply>;
}

#[async_trait::async_trait]
impl<T: JevClient + ?Sized> JevClient for std::sync::Arc<T> {
    fn backend(&self) -> &'static str {
        (**self).backend()
    }
    async fn ask(&self, call: &JevCall) -> Result<JevReply> {
        (**self).ask(call).await
    }
}

/// One line of the audit trail: what was asked, what came back, what it cost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRecord {
    /// Milliseconds since the Unix epoch when the call returned.
    pub at_ms: u128,
    /// Which backend answered.
    pub backend: String,
    /// Which primitive asked.
    pub primitive: Primitive,
    /// Pipeline stage name, when the call came through a [`crate::Pipeline`].
    pub stage: Option<String>,
    /// The model that answered.
    pub model: String,
    /// The state that was judged.
    pub state: serde_json::Value,
    /// The questions that were asked.
    pub asks: Asks,
    /// The answers that came back.
    pub verdicts: IndexMap<String, Verdict>,
    /// The typed output the primitive composed, as JSON.
    pub output: serde_json::Value,
    /// Input tokens, when reported.
    pub input_tokens: Option<u64>,
    /// Output tokens, when reported.
    pub output_tokens: Option<u64>,
    /// Wall-clock milliseconds.
    pub latency_ms: u64,
}

impl CallRecord {
    /// Total tokens, treating unreported counts as zero.
    pub fn tokens(&self) -> u64 {
        self.input_tokens.unwrap_or(0) + self.output_tokens.unwrap_or(0)
    }
}

/// Milliseconds since the Unix epoch, saturating to 0 before 1970.
pub(crate) fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
