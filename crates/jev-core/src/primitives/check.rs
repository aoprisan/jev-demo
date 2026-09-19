//! `Check<I>` — named plausibility booleans over the state.
//!
//! A check is one of two things, and the output says which. A **judged** check
//! is a claim Jev evaluates from the state: whether a mean-reversion signal is
//! valid in the regime the window is in, whether an estimate is still
//! believable. A **rule** check is a comparison the domain already knows how to
//! make — a stop against an ATR multiple, a cycle count against a budget — and
//! it is evaluated in code, because a threshold is not a judgment and Jev is
//! not a calculator. Both kinds share the same result shape so a report can
//! score them the same way; `source` keeps them apart.

use super::{at_most_chars, fit, in_range, need_noul, JevInput};
use crate::ask::{Ask, Asks};
use crate::client::{JevReply, Primitive};
use crate::error::{JevError, Result};
use crate::jev::Jev;
use crate::prompts;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const P: &str = "check";
const MAX_NOTE: usize = 160;

/// One named plausibility claim.
#[derive(Debug, Clone)]
pub struct CheckItem {
    /// The check's name, e.g. `stop_sane` or `reserve_ok`.
    pub name: String,
    /// The claim. Phrased so that yes means the check passes.
    pub claim: String,
    /// What a pass looks like.
    pub when_ok: String,
    /// What a failure looks like.
    pub when_not: String,
    /// `Some(holds)` when the domain evaluated the claim itself; `None` when
    /// Jev is asked.
    pub rule: Option<bool>,
}

impl CheckItem {
    /// A named check whose claim is phrased so yes means pass, put to Jev.
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
            rule: None,
        }
    }

    /// The same check, already decided by the domain's own rule.
    pub fn rule(mut self, holds: bool) -> Self {
        self.rule = Some(holds);
        self
    }
}

/// The set of checks to run.
#[derive(Debug, Clone)]
pub struct CheckSpec {
    /// A short name for the subject.
    pub subject: String,
    /// The checks, in report order.
    pub items: Vec<CheckItem>,
    /// Probability at or above which a judged check passes. Defaults to 0.5.
    pub threshold: f64,
}

impl CheckSpec {
    /// An empty check set.
    pub fn new(subject: impl Into<String>) -> Self {
        Self { subject: subject.into(), items: Vec::new(), threshold: 0.5 }
    }

    /// Add a check for Jev to judge.
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

    /// Add a check the domain has already decided by rule. It is reported in
    /// the same shape as a judged check, marked as a rule, and costs no call.
    pub fn rule(
        mut self,
        name: impl Into<String>,
        claim: impl Into<String>,
        when_ok: impl Into<String>,
        when_not: impl Into<String>,
        holds: bool,
    ) -> Self {
        self.items.push(CheckItem::new(name, claim, when_ok, when_not).rule(holds));
        self
    }

    /// The items Jev is asked about.
    pub fn judged(&self) -> impl Iterator<Item = &CheckItem> {
        self.items.iter().filter(|i| i.rule.is_none())
    }
}

/// Who decided a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckSource {
    /// Jev judged the claim from the state.
    Jev,
    /// The domain evaluated a known rule in code.
    Rule,
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
    /// The probability the claim holds, 0..=1. Jev's answer for a judged
    /// check; exactly 0 or 1 for a rule.
    pub p: f32,
    /// Who decided it.
    pub source: CheckSource,
}

/// Every check's outcome, in spec order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckOut {
    /// The results.
    pub checks: Vec<CheckResult>,
}

impl CheckOut {
    /// The one-line rendering later stages see in `prior_judgments`.
    pub fn line(&self) -> String {
        if self.all_ok() {
            format!("all {} checks hold", self.checks.len())
        } else {
            format!("failed: {}", self.failed().join(", "))
        }
    }

    /// Re-check every schema bound.
    pub fn validate(&self) -> Result<()> {
        for c in &self.checks {
            in_range(P, "p", c.p as f64, 0.0, 1.0)?;
            at_most_chars(P, "note", &c.note, MAX_NOTE)?;
            if c.source == CheckSource::Rule && c.p != 0.0 && c.p != 1.0 {
                return Err(JevError::Contradiction {
                    primitive: P,
                    detail: format!("rule check `{}` carries p={}", c.name, c.p),
                });
            }
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

    /// The judged checks whose probability sits inside `(lo, hi)`: neither a
    /// pass nor a fail anyone should lean on. A caller routes these to review.
    pub fn undecided(&self, lo: f32, hi: f32) -> Vec<&CheckResult> {
        self.checks
            .iter()
            .filter(|c| c.source == CheckSource::Jev && c.p > lo && c.p < hi)
            .collect()
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
        let outcome = self.call(Primitive::Check, input, asks(spec, "")?).await?;
        let out = compose(spec, &outcome.reply, "")?;
        self.record(&outcome, &out)?;
        Ok(out)
    }
}

/// One noul per judged item. Rules ask nothing.
pub(crate) fn asks(spec: &CheckSpec, prefix: &str) -> Result<Asks> {
    if spec.items.is_empty() {
        return Err(JevError::InvalidCall("check needs at least one item".into()));
    }
    if spec.judged().next().is_none() {
        return Err(JevError::InvalidCall(
            "check has only rules; evaluate them in code without a call".into(),
        ));
    }
    let mut asks = Asks::new();
    for item in spec.judged() {
        asks = asks.with(
            format!("{prefix}{}", item.name),
            Ask::noul(prompts::instructions(Primitive::Check, "claim", item.claim.clone(), []))
                .criteria(item.when_ok.clone(), item.when_not.clone()),
        );
    }
    Ok(asks)
}

/// Every check's outcome, judged ones from the reply and rules from the spec.
pub(crate) fn compose(spec: &CheckSpec, reply: &JevReply, prefix: &str) -> Result<CheckOut> {
    let mut checks = Vec::with_capacity(spec.items.len());
    for item in &spec.items {
        let (ok, p, source) = match item.rule {
            Some(holds) => (holds, if holds { 1.0 } else { 0.0 }, CheckSource::Rule),
            None => {
                let p = need_noul(reply, &format!("{prefix}{}", item.name), P)?;
                in_range(P, "p", p, 0.0, 1.0)?;
                (p >= spec.threshold, p, CheckSource::Jev)
            }
        };
        let verdict = match (source, ok) {
            (CheckSource::Rule, true) => "rule holds".to_owned(),
            (CheckSource::Rule, false) => "rule fails".to_owned(),
            (CheckSource::Jev, true) => format!("holds (p={p:.2})"),
            (CheckSource::Jev, false) => format!("fails (p={p:.2})"),
        };
        let note = fit(
            &format!("{verdict}: {}", if ok { &item.when_ok } else { &item.when_not }),
            MAX_NOTE,
        );
        checks.push(CheckResult { name: item.name.clone(), ok, note, p: p as f32, source });
    }

    let out = CheckOut { checks };
    out.validate()?;
    Ok(out)
}
