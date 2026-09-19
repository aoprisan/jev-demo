//! Shared test fixtures. Not a test module itself.

use jev_core::{Asks, JevCall, JevClient, JevInput, JevReply, Usage, Verdict};
use indexmap::IndexMap;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;

/// A minimal domain input: a bag of observable features plus a context line.
#[derive(Debug, Clone, Serialize)]
pub struct TestInput {
    pub features: serde_json::Value,
    #[serde(skip)]
    pub context: String,
}

impl TestInput {
    pub fn new(features: serde_json::Value) -> Self {
        Self { features, context: "test domain".to_owned() }
    }
}

impl JevInput for TestInput {
    fn context_block(&self) -> String {
        self.context.clone()
    }
}

/// A backend that records every call it receives and replays scripted verdicts.
pub struct Recorder {
    pub calls: Mutex<Vec<JevCall>>,
    answer: Box<dyn Fn(&str, &Asks) -> Verdict + Send + Sync>,
}

impl Recorder {
    pub fn new<F>(answer: F) -> Self
    where
        F: Fn(&str, &Asks) -> Verdict + Send + Sync + 'static,
    {
        Self { calls: Mutex::new(Vec::new()), answer: Box::new(answer) }
    }

    pub fn states(&self) -> Vec<serde_json::Value> {
        self.calls.lock().unwrap().iter().map(|c| c.state.clone()).collect()
    }
}

#[async_trait::async_trait]
impl JevClient for Recorder {
    fn backend(&self) -> &'static str {
        "recorder"
    }

    async fn ask(&self, call: &JevCall) -> jev_core::Result<JevReply> {
        self.calls.lock().unwrap().push(call.clone());
        let mut verdicts = IndexMap::new();
        for (name, _) in call.asks.iter() {
            verdicts.insert(name.to_owned(), (self.answer)(name, &call.asks));
        }
        Ok(JevReply {
            verdicts,
            model: "recorder".into(),
            usage: Usage { input_tokens: Some(10), output_tokens: Some(4) },
            latency: Duration::from_millis(1),
        })
    }
}

/// A choice verdict with the given label carrying all but a sliver of the mass.
pub fn choice(label: &str, others: &[&str]) -> Verdict {
    let mut probabilities = IndexMap::new();
    let share = 0.1 / (others.len().max(1)) as f64;
    probabilities.insert(label.to_owned(), 0.9);
    for o in others {
        probabilities.insert((*o).to_owned(), share);
    }
    Verdict::Choice { label: label.to_owned(), probabilities, confidence: 0.85 }
}

/// A score verdict landing exactly on `fraction` of the rubric.
pub fn score_at(fraction: f64, levels: usize) -> Verdict {
    let mut probabilities = std::collections::BTreeMap::new();
    let exact = fraction * (levels - 1) as f64;
    let lower = exact.floor() as u32;
    let upper = (lower + 1).min(levels as u32 - 1);
    let frac = exact - lower as f64;
    if lower == upper {
        probabilities.insert(lower, 1.0);
    } else {
        probabilities.insert(lower, 1.0 - frac);
        probabilities.insert(upper, frac);
    }
    Verdict::Score { score: exact, levels, probabilities, confidence: 0.8 }
}

pub fn noul(p: f64) -> Verdict {
    Verdict::Noul { p }
}
