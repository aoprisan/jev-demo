//! Every call a pipeline makes carries the decision it judges, and the ledger
//! totals a run's cost by that tag.

mod common;

use common::{choice, noul, score_at, Recorder, TestInput};
use jev_core::{CheckSpec, ClassifySpec, GateSpec, Jev, Label, Pipeline, Rates, Verdict};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Regime {
    Calm,
    Rough,
}

impl Label for Regime {
    fn labels() -> &'static [Self] {
        &[Regime::Calm, Regime::Rough]
    }
    fn name(&self) -> &'static str {
        match self {
            Regime::Calm => "calm",
            Regime::Rough => "rough",
        }
    }
    fn describe(&self) -> &'static str {
        "a regime"
    }
}

fn scripted(name: &str, asks: &jev_core::Asks) -> Verdict {
    let levels = match asks.get(name) {
        Some(jev_core::Ask::Score { levels, .. }) => levels.len(),
        _ => 5,
    };
    match name.rsplit('.').next().unwrap_or(name) {
        "label" => choice("calm", &["rough"]),
        "action" => choice("execute", &["reduce", "hold", "escalate"]),
        "size_factor" => score_at(1.0, levels),
        "level" => score_at(0.5, levels),
        _ => noul(0.9),
    }
}

/// One decision: the batch of two independent stages, then the gate.
async fn judge_one(jev: &Jev, decision: &str) {
    let input = TestInput::new(json!({ "pair": "EURUSD" }));
    let mut p = Pipeline::new(jev, "fx").judging(decision);
    assert_eq!(p.decision(), Some(decision));

    let mut assess = p.batch("assess", &input);
    let regime = assess.classify::<Regime>("regime", &ClassifySpec::new("s", "Which?")).unwrap();
    let checks = assess
        .check("sanity", &CheckSpec::new("trade").item("stop_sane", "Sane.", "wide", "tight"))
        .unwrap();
    let mut out = assess.send().await.unwrap();
    out.take(regime).unwrap();
    out.take(checks).unwrap();

    p.gate("gate", &input, &GateSpec::new("EURUSD long", "Go?")).await.unwrap();
}

#[tokio::test]
async fn every_call_of_a_decision_carries_its_id() {
    let jev = Jev::new(Arc::new(Recorder::new(scripted)));
    judge_one(&jev, "fx:0000").await;
    judge_one(&jev, "fx:0001").await;

    let records = jev.audit().records();
    assert_eq!(records.len(), 4, "two calls a decision");
    assert!(records.iter().all(|r| r.decision.is_some()), "no call is untagged");

    let first = jev.audit().records_for("fx:0000");
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].stage.as_deref(), Some("assess"));
    assert_eq!(first[1].stage.as_deref(), Some("gate"));
    assert!(jev.audit().records_for("fx:0002").is_empty());
}

#[tokio::test]
async fn a_call_with_no_decision_is_left_untagged() {
    let jev = Jev::new(Arc::new(Recorder::new(scripted)));
    let input = TestInput::new(json!({ "pair": "EURUSD" }));
    let mut p = Pipeline::new(&jev, "fx");
    assert_eq!(p.decision(), None);
    p.gate("gate", &input, &GateSpec::new("EURUSD long", "Go?")).await.unwrap();

    let records = jev.audit().records();
    assert_eq!(records[0].decision, None);
    let ledger = jev.audit().ledger(Rates::ASSUMED);
    assert_eq!(ledger.decisions(), 0, "an untagged call belongs to no decision");
    assert_eq!(ledger.untagged().calls, 1);
    assert_eq!(ledger.total().calls, 1);
}

#[tokio::test]
async fn the_ledger_splits_a_runs_cost_by_decision() {
    let jev = Jev::new(Arc::new(Recorder::new(scripted)));
    judge_one(&jev, "fx:0000").await;
    judge_one(&jev, "fx:0001").await;
    // The report's summary call belongs to the run, not to any one decision.
    let input = TestInput::new(json!({ "pair": "EURUSD" }));
    Pipeline::new(&jev, "fx")
        .gate("summary", &input, &GateSpec::new("the session", "Go?"))
        .await
        .unwrap();

    // The recorder reports 10 input and 4 output tokens per call.
    let rates = Rates { input_usd_per_mtok: 100.0, output_usd_per_mtok: 1000.0 };
    let ledger = jev.audit().ledger(rates);

    let one_call = rates.usd(10, 4);
    assert_eq!(ledger.total().calls, 5);
    assert_eq!(ledger.total().tokens(), 5 * 14);
    assert!((ledger.total().usd - 5.0 * one_call).abs() < 1e-12);

    assert_eq!(ledger.decisions(), 2);
    let first = ledger.of_decision("fx:0000");
    assert_eq!(first.calls, 2);
    assert_eq!(first.input_tokens, 20);
    assert_eq!(first.output_tokens, 8);
    assert!((first.usd - 2.0 * one_call).abs() < 1e-12);

    // The mean is over decisions, so the untagged call is in the total but not
    // in the per-decision figure.
    assert!((ledger.usd_per_decision() - 2.0 * one_call).abs() < 1e-12);
    assert_eq!(ledger.untagged().calls, 1);
    assert_eq!(ledger.of_decision("fx:0002"), jev_core::CostEstimate::default());

    let ids: Vec<&str> = ledger.by_decision().map(|(id, _)| id).collect();
    assert_eq!(ids, ["fx:0000", "fx:0001"], "in the order they were judged");
}

#[test]
fn rates_come_from_the_environment_and_fall_back_to_the_assumed_ones() {
    // 1M input tokens at $3 plus 1M output at $15, the stand-in prices.
    assert!((Rates::ASSUMED.usd(1_000_000, 1_000_000) - 18.0).abs() < 1e-12);
    assert!(Rates::ASSUMED.assumed());
    assert!(!Rates { input_usd_per_mtok: 1.0, output_usd_per_mtok: 2.0 }.assumed());
    assert_eq!(Rates::FREE.usd(1_000_000, 1_000_000), 0.0);

    // `from_env` is process-wide state, so this is the one test that touches it.
    unsafe {
        std::env::set_var(jev_core::cost::INPUT_RATE_ENV, "2.5");
        std::env::set_var(jev_core::cost::OUTPUT_RATE_ENV, "not a number");
    }
    let rates = Rates::from_env();
    unsafe {
        std::env::remove_var(jev_core::cost::INPUT_RATE_ENV);
        std::env::remove_var(jev_core::cost::OUTPUT_RATE_ENV);
    }
    assert_eq!(rates.input_usd_per_mtok, 2.5, "the environment wins");
    assert_eq!(
        rates.output_usd_per_mtok,
        Rates::ASSUMED.output_usd_per_mtok,
        "an unreadable price falls back"
    );
}
