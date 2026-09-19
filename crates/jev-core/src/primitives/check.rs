//! `Check<I>` — named plausibility booleans over the state.

use super::{at_most_chars, fit, in_range, need_noul, JevInput};
use crate::ask::{Ask, Asks};
use crate::client::Primitive;
use crate::error::{JevError, Result};
use crate::jev::Jev;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const P: &str = "check";
const MAX_NOTE: usize = 160;

/// One named plausibility claim.
#[derive(Debug, Clone)]
pub struct CheckItem {
    /// The check's name, e.g. `stop_sane` or `reserve_ok`.
    pub name: String,
    /// The claim Jev evaluates. Phrased so that yes means the check passes.
    pub claim: String,
    /// What a pass looks like.
    pub when_ok: String,
    /// What a failure looks like.
    pub when_not: String,
}

impl CheckItem {
    /// A named check whose claim is phrased so yes means pass.
    pub fn new(
        name: impl Into<String>,
        claim: impl Into<String>,
        when_ok: impl Into<String>,
        when_not: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            claim: claim.into(),
            when_ok: when_ok.into(),
            when_not: when_not.into(),
        }
    }
}

/// The set of checks to run.
#[derive(Debug, Clone)]
pub struct CheckSpec {
    /// A short name for the subject.
    pub subject: String,
    /// The checks, in report order.
    pub items: Vec<CheckItem>,
    /// Probability at or above which a check passes. Defaults to 0.5.
    pub threshold: f64,
}

impl CheckSpec {
    /// An empty check set.
    pub fn new(subject: impl Into<String>) -> Self {
        Self { subject: subject.into(), items: Vec::new(), threshold: 0.5 }
    }

    /// Add a check.
    pub fn item(
        mut self,
        name: impl Into<String>,
        claim: impl Into<String>,
        when_ok: impl Into<String>,
        when_not: impl Into<String>,
    ) -> Self {
        self.items.push(CheckItem::new(name, claim, when_ok, when_not));
        self
    }
}

/// One check's outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckResult {
    /// The check's name.
    pub name: String,
    /// Whether it passed.
    pub ok: bool,
    /// Composed from the verdict; at most 160 characters.
    pub note: String,
    /// The probability Jev gave the claim, 0..=1.
    pub p: f32,
}

/// Every check's outcome, in spec order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckOut {
    /// The results.
    pub checks: Vec<CheckResult>,
}

impl CheckOut {
    /// Re-check every schema bound.
    pub fn validate(&self) -> Result<()> {
        for c in &self.checks {
            in_range(P, "p", c.p as f64, 0.0, 1.0)?;
            at_most_chars(P, "note", &c.note, MAX_NOTE)?;
        }
        Ok(())
    }

    /// The names of the checks that failed.
    pub fn failed(&self) -> Vec<&str> {
        self.checks.iter().filter(|c| !c.ok).map(|c| c.name.as_str()).collect()
    }

    /// Whether every check passed.
    pub fn all_ok(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }

    /// One check by name.
    pub fn get(&self, name: &str) -> Option<&CheckResult> {
        self.checks.iter().find(|c| c.name == name)
    }
}

/// Run named plausibility checks over the state.
#[async_trait::async_trait]
pub trait Check<I: JevInput> {
    /// Check `input`.
    async fn check(&self, input: &I, spec: &CheckSpec) -> Result<CheckOut>;
}

#[async_trait::async_trait]
impl<I: JevInput> Check<I> for Jev {
    async fn check(&self, input: &I, spec: &CheckSpec) -> Result<CheckOut> {
        if spec.items.is_empty() {
            return Err(JevError::InvalidCall("check needs at least one item".into()));
        }
        let mut asks = Asks::new();
        for item in &spec.items {
            asks = asks.with(
                item.name.clone(),
                Ask::noul(item.claim.clone()).criteria(item.when_ok.clone(), item.when_not.clone()),
            );
        }

        let outcome = self.call(Primitive::Check, input, asks).await?;
        let mut checks = Vec::with_capacity(spec.items.len());
        for item in &spec.items {
            let p = need_noul(&outcome.reply, &item.name, P)?;
            in_range(P, "p", p, 0.0, 1.0)?;
            let ok = p >= spec.threshold;
            let note = fit(
                &format!(
                    "{} (p={:.2}): {}",
                    if ok { "holds" } else { "fails" },
                    p,
                    if ok { &item.when_ok } else { &item.when_not }
                ),
                MAX_NOTE,
            );
            checks.push(CheckResult { name: item.name.clone(), ok, note, p: p as f32 });
        }

        let out = CheckOut { checks };
        out.validate()?;
        self.record(&outcome, &out)?;
        Ok(out)
    }
}
