//! Composing primitives into a pipeline.
//!
//! A domain runs e.g. `[Classify(regime), Check(sanity), Score(risk)] -> Gate`.
//! Stages that are independent of one another go into one [`Batch`]: their
//! questions are fanned out in a single call, which System One answers in
//! parallel at no extra latency. A stage that needs an earlier stage's answer
//! goes in a later batch, and sees every earlier stage's typed output under
//! `prior_judgments` in its state.

use crate::ask::Asks;
use crate::client::{JevReply, Primitive};
use crate::error::{JevError, Result};
use crate::jev::Jev;
use crate::primitives::{
    check, classify, explain, gate, rank, score, Candidate, CandidateId, CheckOut, CheckSpec,
    ClassifyOut, ClassifySpec, ExplainOut, ExplainSpec, GateOut, GateSpec, JevInput, Label,
    RankOut, RankSpec, ScoreOut, ScoreSpec,
};
use crate::Audience;
use serde::Serialize;
use serde_json::Value;
use std::any::Any;
use std::marker::PhantomData;

/// One earlier stage's output, as later stages see it.
#[derive(Debug, Clone, Serialize)]
pub struct PriorEntry {
    /// The stage that produced it.
    pub stage: String,
    /// Which primitive.
    pub primitive: &'static str,
    /// The typed output, as JSON.
    pub output: Value,
    /// A one-line rendering, so a reader of the log has the gist next to the data.
    pub line: String,
}

/// The accumulated outputs of the stages run so far.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(transparent)]
pub struct Priors(Vec<PriorEntry>);

impl Priors {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every entry, in stage order.
    pub fn entries(&self) -> &[PriorEntry] {
        &self.0
    }

    /// Whether no stage has run.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn push(&mut self, stage: &str, primitive: &'static str, output: Value, line: String) {
        self.0.push(PriorEntry { stage: stage.to_owned(), primitive, output, line });
    }
}

/// A domain input carrying the prior stages' outputs.
///
/// This is what makes composition visible: the wrapper serialises as
/// `{"input": …, "prior_judgments": […]}`, so a later stage's state provably
/// contains every earlier stage's typed output.
pub struct Staged<'a, I> {
    inner: &'a I,
    priors: &'a Priors,
}

impl<I: Serialize> Serialize for Staged<'_, I> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Staged", 2)?;
        st.serialize_field("input", self.inner)?;
        st.serialize_field("prior_judgments", self.priors)?;
        st.end()
    }
}

impl<I: JevInput> JevInput for Staged<'_, I> {
    fn to_state(&self) -> Result<Value> {
        Ok(serde_json::json!({
            "input": serde_json::to_value(self.inner)
                .map_err(|e| JevError::InvalidCall(e.to_string()))?,
            "prior_judgments": self.priors,
        }))
    }

    fn context_block(&self) -> String {
        self.inner.context_block()
    }
}

/// A sequence of primitive calls that share one accumulating set of prior outputs.
///
/// Each single-stage method is a batch of one. Use [`Pipeline::batch`] to fan
/// several independent stages into one call.
pub struct Pipeline {
    jev: Jev,
    name: String,
    priors: Priors,
}

impl Pipeline {
    /// A pipeline over `jev`, named for the audit log.
    pub fn new(jev: &Jev, name: impl Into<String>) -> Self {
        Self { jev: jev.clone(), name: name.into(), priors: Priors::new() }
    }

    /// The same pipeline, tagging every call it makes with the decision it is
    /// judging.
    ///
    /// One decision takes several calls — forex judges a candidate in two,
    /// battery a day in three — so the id is what joins them back together:
    /// every [`crate::CallRecord`] the pipeline writes carries it, and the
    /// domain records the same id alongside the decision itself.
    pub fn judging(mut self, decision: impl AsRef<str>) -> Self {
        self.jev = self.jev.deciding(decision.as_ref());
        self
    }

    /// The pipeline's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The decision every call is tagged with, if the pipeline was told.
    pub fn decision(&self) -> Option<&str> {
        self.jev.decision()
    }

    /// Everything the stages have produced so far.
    pub fn priors(&self) -> &Priors {
        &self.priors
    }

    /// Discard the accumulated priors, keeping the audit log.
    pub fn reset(&mut self) {
        self.priors = Priors::new();
    }

    /// Start a batch of independent stages over `input`, tagged `stage` in the
    /// audit log. Every stage added sees the priors as they stand now, and
    /// none sees another stage in the same batch.
    pub fn batch<'p, 'i, I: JevInput>(&'p mut self, stage: &str, input: &'i I) -> Batch<'p, 'i, I> {
        Batch { pipeline: self, stage: stage.to_owned(), input, items: Vec::new() }
    }

    /// Run a `Classify` stage on its own.
    pub async fn classify<I: JevInput, E: Label + Serialize>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &ClassifySpec,
    ) -> Result<ClassifyOut<E>> {
        let mut batch = self.batch(stage, input);
        let slot = batch.classify::<E>(stage, spec)?;
        batch.send().await?.take(slot)
    }

    /// Run a `Check` stage on its own.
    pub async fn check<I: JevInput>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &CheckSpec,
    ) -> Result<CheckOut> {
        let mut batch = self.batch(stage, input);
        let slot = batch.check(stage, spec)?;
        batch.send().await?.take(slot)
    }

    /// Run a `Score` stage on its own.
    pub async fn score<I: JevInput>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &ScoreSpec,
    ) -> Result<ScoreOut> {
        let mut batch = self.batch(stage, input);
        let slot = batch.score(stage, spec)?;
        batch.send().await?.take(slot)
    }

    /// Run a `Rank` stage on its own.
    pub async fn rank<I: JevInput, T: CandidateId + Serialize>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &RankSpec<T>,
    ) -> Result<RankOut<T>> {
        let mut batch = self.batch(stage, input);
        let slot = batch.rank(stage, spec)?;
        batch.send().await?.take(slot)
    }

    /// Run a `Gate` stage on its own.
    pub async fn gate<I: JevInput>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &GateSpec,
    ) -> Result<GateOut> {
        let mut batch = self.batch(stage, input);
        let slot = batch.gate(stage, spec)?;
        batch.send().await?.take(slot)
    }

    /// Run an `Explain` stage on its own.
    pub async fn explain<I: JevInput>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &ExplainSpec,
    ) -> Result<ExplainOut> {
        let mut batch = self.batch(stage, input);
        let slot = batch.explain(stage, spec)?;
        batch.send().await?.take(slot)
    }
}

// ---- batching ------------------------------------------------------------------------------

/// What one stage in a batch produced, before it is handed back typed.
struct Composed {
    typed: Box<dyn Any + Send>,
    json: Value,
    line: String,
}

/// Compose one stage's output from the reply, the state that was judged and
/// the prefix its asks were sent under.
type Compose = Box<dyn FnOnce(&JevReply, &Value, &str) -> Result<Composed> + Send>;

struct Item {
    stage: String,
    primitive: Primitive,
    asks: Asks,
    compose: Compose,
}

/// A typed handle to one stage's output in a [`BatchOut`].
#[derive(Debug)]
pub struct Slot<T> {
    index: usize,
    stage: String,
    _out: PhantomData<fn() -> T>,
}

/// Several independent stages, asked in one call.
///
/// Add stages with the primitive-named methods, each returning a [`Slot`];
/// [`Batch::send`] makes the call, composes every stage's typed output,
/// appends each to the pipeline's priors in the order added, and records one
/// audit line for the call. In a batch of more than one, every ask is named
/// `<stage>.<ask>` so two stages' `level`s cannot collide; a batch of one
/// keeps bare names and is recorded under its own primitive.
pub struct Batch<'p, 'i, I> {
    pipeline: &'p mut Pipeline,
    stage: String,
    input: &'i I,
    items: Vec<Item>,
}

/// The outputs of a sent batch, claimed one slot at a time.
pub struct BatchOut {
    outputs: Vec<Option<Composed>>,
}

impl BatchOut {
    /// Take the typed output for `slot`.
    pub fn take<T: 'static>(&mut self, slot: Slot<T>) -> Result<T> {
        let composed =
            self.outputs.get_mut(slot.index).and_then(Option::take).ok_or_else(|| {
                JevError::InvalidCall(format!("stage `{}` was not in this batch", slot.stage))
            })?;
        composed.typed.downcast::<T>().map(|b| *b).map_err(|_| {
            JevError::InvalidCall(format!("stage `{}` is not the type asked for", slot.stage))
        })
    }
}

impl<I: JevInput> Batch<'_, '_, I> {
    fn add<T, F>(&mut self, stage: &str, primitive: Primitive, asks: Asks, compose: F) -> Slot<T>
    where
        T: Serialize + Send + 'static,
        F: FnOnce(&JevReply, &Value, &str) -> Result<(T, String)> + Send + 'static,
    {
        let index = self.items.len();
        self.items.push(Item {
            stage: stage.to_owned(),
            primitive,
            asks,
            compose: Box::new(move |reply, state, prefix| {
                let (out, line) = compose(reply, state, prefix)?;
                let json = serde_json::to_value(&out)
                    .map_err(|e| JevError::Audit(format!("output is not serialisable: {e}")))?;
                Ok(Composed { typed: Box::new(out), json, line })
            }),
        });
        Slot { index, stage: stage.to_owned(), _out: PhantomData }
    }

    /// Add a `Classify` stage.
    pub fn classify<E: Label + Serialize>(
        &mut self,
        stage: &str,
        spec: &ClassifySpec,
    ) -> Result<Slot<ClassifyOut<E>>> {
        let asks = classify::asks::<E>(spec, "")?;
        let spec = spec.clone();
        Ok(self.add(stage, Primitive::Classify, asks, move |reply, _, prefix| {
            let out = classify::compose::<E>(&spec, reply, prefix)?;
            let line = out.line();
            Ok((out, line))
        }))
    }

    /// Add a `Check` stage.
    pub fn check(&mut self, stage: &str, spec: &CheckSpec) -> Result<Slot<CheckOut>> {
        let asks = check::asks(spec, "")?;
        let spec = spec.clone();
        Ok(self.add(stage, Primitive::Check, asks, move |reply, _, prefix| {
            let out = check::compose(&spec, reply, prefix)?;
            let line = out.line();
            Ok((out, line))
        }))
    }

    /// Add a `Score` stage.
    pub fn score(&mut self, stage: &str, spec: &ScoreSpec) -> Result<Slot<ScoreOut>> {
        let asks = score::asks(spec, "")?;
        let spec = spec.clone();
        Ok(self.add(stage, Primitive::Score, asks, move |reply, _, prefix| {
            let out = score::compose(&spec, reply, prefix)?;
            let line = out.line();
            Ok((out, line))
        }))
    }

    /// Add a `Rank` stage.
    pub fn rank<T: CandidateId + Serialize>(
        &mut self,
        stage: &str,
        spec: &RankSpec<T>,
    ) -> Result<Slot<RankOut<T>>> {
        let asks = rank::asks(spec, "")?;
        let spec = spec.clone();
        Ok(self.add(stage, Primitive::Rank, asks, move |reply, _, prefix| {
            let out = rank::compose(&spec, reply, prefix)?;
            let line = out.line();
            Ok((out, line))
        }))
    }

    /// Add a `Gate` stage.
    pub fn gate(&mut self, stage: &str, spec: &GateSpec) -> Result<Slot<GateOut>> {
        let asks = gate::asks(spec, "")?;
        let spec = spec.clone();
        Ok(self.add(stage, Primitive::Gate, asks, move |reply, state, prefix| {
            let out = gate::compose(&spec, reply, state, prefix)?;
            let line = out.line();
            Ok((out, line))
        }))
    }

    /// Add an `Explain` stage.
    pub fn explain(&mut self, stage: &str, spec: &ExplainSpec) -> Result<Slot<ExplainOut>> {
        let asks = explain::asks(spec, "")?;
        let spec = spec.clone();
        Ok(self.add(stage, Primitive::Explain, asks, move |reply, _, prefix| {
            let out = explain::compose(&spec, reply, prefix)?;
            let line = out.line();
            Ok((out, line))
        }))
    }

    /// Make the call, compose every stage, extend the priors, record the call.
    pub async fn send(self) -> Result<BatchOut> {
        if self.items.is_empty() {
            return Err(JevError::InvalidCall(format!("batch `{}` has no stages", self.stage)));
        }
        let single = self.items.len() == 1;
        let primitive = if single { self.items[0].primitive } else { Primitive::Batch };
        let prefix = |stage: &str| if single { String::new() } else { format!("{stage}.") };

        let mut asks = Asks::new();
        for item in &self.items {
            let prefix = prefix(&item.stage);
            for (name, ask) in item.asks.iter() {
                let full = format!("{prefix}{name}");
                if asks.get(&full).is_some() {
                    return Err(JevError::InvalidCall(format!(
                        "batch `{}` asks `{full}` twice",
                        self.stage
                    )));
                }
                asks = asks.with(full, ask.clone());
            }
        }

        let jev = self.pipeline.jev.staged(&self.stage);
        let staged = Staged { inner: self.input, priors: &self.pipeline.priors };
        let outcome = jev.call(primitive, &staged, asks).await?;

        let mut outputs = Vec::with_capacity(self.items.len());
        let mut recorded = Vec::with_capacity(self.items.len());
        for item in self.items {
            let composed =
                (item.compose)(&outcome.reply, &outcome.call.state, &prefix(&item.stage))?;
            self.pipeline.priors.push(
                &item.stage,
                item.primitive.as_str(),
                composed.json.clone(),
                composed.line.clone(),
            );
            recorded.push(serde_json::json!({
                "stage": item.stage,
                "primitive": item.primitive.as_str(),
                "output": composed.json,
            }));
            outputs.push(Some(composed));
        }

        if single {
            jev.record(&outcome, &recorded[0]["output"])?;
        } else {
            jev.record(&outcome, &recorded)?;
        }
        Ok(BatchOut { outputs })
    }
}

/// Convenience: build a `RankSpec` from ids and summaries.
pub fn candidates<T: CandidateId, I, S>(items: I) -> Vec<Candidate<T>>
where
    I: IntoIterator<Item = (T, S)>,
    S: Into<String>,
{
    items.into_iter().map(|(id, s)| Candidate::new(id, s)).collect()
}

/// Every audience, for `demo-all`'s closing summary.
pub fn all_audiences() -> [Audience; 3] {
    Audience::ALL
}
