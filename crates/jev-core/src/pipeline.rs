//! Composing primitives into a pipeline.
//!
//! A domain runs e.g. `Classify(regime) -> Check(sanity) -> Score(risk) -> Gate`.
//! Each stage's typed output is appended to the state of every later stage under
//! `prior_judgments`, and rendered into the later stage's context block, so a
//! gate can see that a check failed without the domain having to re-state it.

use crate::error::Result;
use crate::jev::Jev;
use crate::primitives::{
    Audience, Candidate, CandidateId, Check, CheckOut, CheckSpec, Classify, ClassifyOut,
    ClassifySpec, Explain, ExplainOut, ExplainSpec, Gate, GateOut, GateSpec, JevInput, Label, Rank,
    RankOut, RankSpec, Score, ScoreOut, ScoreSpec,
};
use serde::Serialize;

/// One earlier stage's output, as later stages see it.
#[derive(Debug, Clone, Serialize)]
pub struct PriorEntry {
    /// The stage that produced it.
    pub stage: String,
    /// Which primitive.
    pub primitive: &'static str,
    /// The typed output, as JSON.
    pub output: serde_json::Value,
    /// A one-line rendering, used in the context block.
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

    fn push<T: Serialize>(
        &mut self,
        stage: &str,
        primitive: &'static str,
        output: &T,
        line: String,
    ) {
        self.0.push(PriorEntry {
            stage: stage.to_owned(),
            primitive,
            output: serde_json::to_value(output).unwrap_or(serde_json::Value::Null),
            line,
        });
    }

    /// The prior stages rendered for a context block.
    pub fn render(&self) -> String {
        if self.0.is_empty() {
            return String::new();
        }
        let mut s = String::from("Earlier judgments in this decision:\n");
        for e in &self.0 {
            s.push_str(&format!("- [{}/{}] {}\n", e.stage, e.primitive, e.line));
        }
        s
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
    fn to_state(&self) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "input": serde_json::to_value(self.inner)
                .map_err(|e| crate::error::JevError::InvalidCall(e.to_string()))?,
            "prior_judgments": self.priors,
        }))
    }

    fn context_block(&self) -> String {
        let base = self.inner.context_block();
        let priors = self.priors.render();
        if priors.is_empty() {
            base
        } else {
            format!("{base}\n\n{priors}")
        }
    }
}

/// A sequence of primitive calls that share one accumulating set of prior outputs.
///
/// Each method tags its call with the stage name, so `decisions.jsonl` shows the
/// pipeline shape as well as the individual calls.
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

    /// The pipeline's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Everything the stages have produced so far.
    pub fn priors(&self) -> &Priors {
        &self.priors
    }

    /// Discard the accumulated priors, keeping the audit log.
    pub fn reset(&mut self) {
        self.priors = Priors::new();
    }

    /// Run a `Classify` stage.
    pub async fn classify<I: JevInput, E: Label + Serialize>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &ClassifySpec,
    ) -> Result<ClassifyOut<E>> {
        let staged = Staged { inner: input, priors: &self.priors };
        let out: ClassifyOut<E> =
            Classify::<_, E>::classify(&self.jev.staged(stage), &staged, spec).await?;
        self.priors.push(stage, "classify", &out, out.reason.clone());
        Ok(out)
    }

    /// Run a `Check` stage.
    pub async fn check<I: JevInput>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &CheckSpec,
    ) -> Result<CheckOut> {
        let staged = Staged { inner: input, priors: &self.priors };
        let out = Check::check(&self.jev.staged(stage), &staged, spec).await?;
        let line = if out.all_ok() {
            format!("all {} checks hold", out.checks.len())
        } else {
            format!("failed: {}", out.failed().join(", "))
        };
        self.priors.push(stage, "check", &out, line);
        Ok(out)
    }

    /// Run a `Score` stage.
    pub async fn score<I: JevInput>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &ScoreSpec,
    ) -> Result<ScoreOut> {
        let staged = Staged { inner: input, priors: &self.priors };
        let out = Score::score(&self.jev.staged(stage), &staged, spec).await?;
        self.priors.push(stage, "score", &out, out.reason.clone());
        Ok(out)
    }

    /// Run a `Rank` stage.
    pub async fn rank<I: JevInput, T: CandidateId + Serialize>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &RankSpec<T>,
    ) -> Result<RankOut<T>> {
        let staged = Staged { inner: input, priors: &self.priors };
        let out: RankOut<T> = Rank::<_, T>::rank(&self.jev.staged(stage), &staged, spec).await?;
        let line = match out.top() {
            Some(top) => format!("{} first (margin {:.2})", top.id.label(), out.margin()),
            None => "no candidates".to_owned(),
        };
        self.priors.push(stage, "rank", &out, line);
        Ok(out)
    }

    /// Run a `Gate` stage.
    pub async fn gate<I: JevInput>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &GateSpec,
    ) -> Result<GateOut> {
        let staged = Staged { inner: input, priors: &self.priors };
        let out = Gate::gate(&self.jev.staged(stage), &staged, spec).await?;
        self.priors.push(stage, "gate", &out, out.reason.clone());
        Ok(out)
    }

    /// Run an `Explain` stage.
    pub async fn explain<I: JevInput>(
        &mut self,
        stage: &str,
        input: &I,
        spec: &ExplainSpec,
    ) -> Result<ExplainOut> {
        let staged = Staged { inner: input, priors: &self.priors };
        let out = Explain::explain(&self.jev.staged(stage), &staged, spec).await?;
        self.priors.push(stage, "explain", &out, out.summary.clone());
        Ok(out)
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
