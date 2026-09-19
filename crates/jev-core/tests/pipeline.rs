//! The pipeline threads each stage's typed output into every later stage.

mod common;

use common::{choice, noul, score_at, Recorder, TestInput};
use jev_core::{
    CheckSpec, ClassifySpec, GateSpec, Jev, Label, Pipeline, ScoreSpec, Verdict,
};
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

fn scripted(name: &str, _asks: &jev_core::Asks) -> Verdict {
    match name {
        "label" => choice("rough", &["calm"]),
        "action" => choice("reduce", &["execute", "hold", "escalate"]),
        "stop_sane" => noul(0.2),
        "other_check" => noul(0.95),
        "level" => score_at(0.75, 5),
        "size_factor" => score_at(0.5, 5),
        n if n.starts_with("driver_") => noul(0.8),
        _ => noul(0.5),
    }
}

async fn run_four_stages() -> (Arc<Recorder>, Jev) {
    let recorder = Arc::new(Recorder::new(scripted));
    let jev = Jev::new(recorder.clone());
    let input = TestInput::new(json!({ "pair": "EURUSD" }));
    let mut p = Pipeline::new(&jev, "fx");

    let _: jev_core::ClassifyOut<Regime> =
        p.classify("regime", &input, &ClassifySpec::new("session", "Which regime?")).await.unwrap();
    p.check(
        "sanity",
        &input,
        &CheckSpec::new("trade")
            .item("stop_sane", "The stop is sane.", "wide enough", "too tight")
            .item("other_check", "Fine.", "fine", "not fine"),
    )
    .await
    .unwrap();
    p.score(
        "risk",
        &input,
        &ScoreSpec::new("event risk", "How risky?", ["none", "low", "mid", "high", "max"])
            .driver("imminent_event", "An event is imminent."),
    )
    .await
    .unwrap();
    p.gate("gate", &input, &GateSpec::new("EURUSD long", "Go?")).await.unwrap();

    (recorder, jev)
}

#[tokio::test]
async fn every_stage_sees_all_earlier_outputs_and_no_later_ones() {
    let (recorder, _) = run_four_stages().await;
    let states = recorder.states();
    assert_eq!(states.len(), 4);

    let priors = |i: usize| -> Vec<String> {
        states[i]["prior_judgments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["stage"].as_str().unwrap().to_owned())
            .collect()
    };

    assert!(priors(0).is_empty(), "the first stage has no priors");
    assert_eq!(priors(1), vec!["regime"]);
    assert_eq!(priors(2), vec!["regime", "sanity"]);
    assert_eq!(priors(3), vec!["regime", "sanity", "risk"]);
}

#[tokio::test]
async fn a_later_stage_sees_the_earlier_stages_typed_output_not_a_summary() {
    let (recorder, _) = run_four_stages().await;
    let gate_state = &recorder.states()[3];
    let priors = gate_state["prior_judgments"].as_array().unwrap();

    // The classify output arrives whole: label, confidence, reason, evidence.
    let regime = &priors[0]["output"];
    assert_eq!(regime["label"], json!("rough"));
    assert!(regime["confidence"].as_f64().unwrap() > 0.0);
    assert!(regime["evidence"]["distribution"].is_array());

    // The failing check arrives as a boolean the gate can act on.
    let checks = priors[1]["output"]["checks"].as_array().unwrap();
    let stop = checks.iter().find(|c| c["name"] == json!("stop_sane")).unwrap();
    assert_eq!(stop["ok"], json!(false));

    // The score arrives as the 0..=100 integer, with its drivers.
    assert_eq!(priors[2]["output"]["score"], json!(75));
    assert_eq!(priors[2]["output"]["drivers"], json!(["imminent_event"]));
}

#[tokio::test]
async fn the_domain_input_is_unchanged_by_staging() {
    let (recorder, _) = run_four_stages().await;
    for state in recorder.states() {
        assert_eq!(state["input"]["features"]["pair"], json!("EURUSD"));
    }
}

#[tokio::test]
async fn prior_outputs_reach_the_context_block_as_well_as_the_state() {
    let (recorder, _) = run_four_stages().await;
    let calls = recorder.calls.lock().unwrap();
    let gate_context = &calls[3].context;
    assert!(gate_context.contains("Earlier judgments"), "{gate_context}");
    assert!(gate_context.contains("[sanity/check] failed: stop_sane"), "{gate_context}");
    assert!(gate_context.contains("[regime/classify]"), "{gate_context}");
}

#[tokio::test]
async fn the_audit_log_records_every_call_with_its_stage_and_cost() {
    let (_, jev) = run_four_stages().await;
    let records = jev.audit().records();
    assert_eq!(records.len(), 4);

    let stages: Vec<_> = records.iter().map(|r| r.stage.clone().unwrap()).collect();
    assert_eq!(stages, vec!["regime", "sanity", "risk", "gate"]);

    let primitives: Vec<_> = records.iter().map(|r| r.primitive.as_str()).collect();
    assert_eq!(primitives, vec!["classify", "check", "score", "gate"]);

    // Every record carries what was asked, what came back, and what it produced.
    for r in &records {
        assert!(!r.asks.is_empty());
        assert_eq!(r.verdicts.len(), r.asks.len());
        assert!(!r.output.is_null());
        assert!(r.tokens() > 0);
    }
    let (tokens, _latency) = jev.audit().totals();
    assert_eq!(tokens, 4 * 14);
}

#[tokio::test]
async fn reset_clears_the_priors_but_keeps_the_audit() {
    let recorder = Arc::new(Recorder::new(scripted));
    let jev = Jev::new(recorder.clone());
    let input = TestInput::new(json!({}));
    let mut p = Pipeline::new(&jev, "fx");

    let _: jev_core::ClassifyOut<Regime> =
        p.classify("regime", &input, &ClassifySpec::new("s", "q?")).await.unwrap();
    assert_eq!(p.priors().entries().len(), 1);

    p.reset();
    assert!(p.priors().is_empty());

    let _: jev_core::ClassifyOut<Regime> =
        p.classify("regime2", &input, &ClassifySpec::new("s", "q?")).await.unwrap();
    assert!(
        recorder.states()[1]["prior_judgments"].as_array().unwrap().is_empty(),
        "the second run starts clean"
    );
    assert_eq!(jev.audit().len(), 2, "the audit spans both runs");
}

#[tokio::test]
async fn a_direct_call_and_a_first_stage_call_have_the_same_state_shape() {
    use jev_core::{Classify, ClassifyOut};
    let recorder = Arc::new(Recorder::new(scripted));
    let jev = Jev::new(recorder.clone());
    let input = TestInput::new(json!({ "pair": "EURUSD" }));

    let _: ClassifyOut<Regime> =
        Classify::<_, Regime>::classify(&jev, &input, &ClassifySpec::new("s", "q?"))
            .await
            .unwrap();
    let mut p = Pipeline::new(&jev, "fx");
    let _: ClassifyOut<Regime> =
        p.classify("regime", &input, &ClassifySpec::new("s", "q?")).await.unwrap();

    let states = recorder.states();
    let keys = |v: &serde_json::Value| -> Vec<String> {
        v.as_object().unwrap().keys().cloned().collect()
    };
    assert_eq!(keys(&states[0]), keys(&states[1]));
    assert_eq!(states[0], states[1]);
}
