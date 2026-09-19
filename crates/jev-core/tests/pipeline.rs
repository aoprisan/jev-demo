//! The pipeline threads each stage's typed output into every later stage, and
//! a batch fans independent stages into one call.

mod common;

use common::{choice, noul, score_at, Recorder, TestInput};
use jev_core::{
    CheckSpec, ClassifyOut, ClassifySpec, GateSpec, Jev, Label, Pipeline, ScoreSpec, Verdict,
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

fn scripted(name: &str, asks: &jev_core::Asks) -> Verdict {
    let levels = match asks.get(name) {
        Some(jev_core::Ask::Score { levels, .. }) => levels.len(),
        _ => 5,
    };
    // A batched call prefixes each ask with its stage; the script keys on the
    // bare name either way.
    let name = name.rsplit('.').next().unwrap_or(name);
    match name {
        "label" => choice("rough", &["calm"]),
        "action" => choice("reduce", &["execute", "hold", "escalate"]),
        "stop_sane" => noul(0.2),
        "other_check" => noul(0.95),
        "level" => score_at(0.75, levels),
        "size_factor" => score_at(0.5, levels),
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
async fn prior_outputs_are_in_the_state_once_with_a_one_line_gist() {
    let (recorder, _) = run_four_stages().await;
    let calls = recorder.calls.lock().unwrap();
    let gate_state = &calls[3].state;
    // The framing is a named field of the state, and it is not repeated.
    assert_eq!(gate_state["context"], json!("test domain"));
    let keys: Vec<&str> = gate_state.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["context", "input", "prior_judgments"]);
    let priors = gate_state["prior_judgments"].as_array().unwrap();
    assert_eq!(priors[1]["line"], json!("failed: stop_sane"));
    assert_eq!(priors[0]["stage"], json!("regime"));
}

#[tokio::test]
async fn nothing_instruction_like_is_in_the_state() {
    let (recorder, _) = run_four_stages().await;
    for call in recorder.calls.lock().unwrap().iter() {
        let state = call.state.to_string();
        assert!(!state.contains("not_for"), "guidance leaked into the state: {state}");
        assert!(call.state.get("instructions").is_none());
        // And every ask carries its guidance as named fields.
        for (_, ask) in call.asks.iter() {
            let instructions = ask.instructions().as_object().expect("structured instructions");
            assert!(instructions.contains_key("question"));
            assert!(instructions.contains_key("what"), "{instructions:?}");
        }
    }
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

// ---- batching ------------------------------------------------------------------------------

async fn run_batched() -> (Arc<Recorder>, Jev, ClassifyOut<Regime>, jev_core::CheckOut) {
    let recorder = Arc::new(Recorder::new(scripted));
    let jev = Jev::new(recorder.clone());
    let input = TestInput::new(json!({ "pair": "EURUSD" }));
    let mut p = Pipeline::new(&jev, "fx");

    let mut batch = p.batch("assess", &input);
    let regime =
        batch.classify::<Regime>("regime", &ClassifySpec::new("session", "Which regime?")).unwrap();
    let sanity = batch
        .check(
            "sanity",
            &CheckSpec::new("trade")
                .item("stop_sane", "The stop is sane.", "wide enough", "too tight")
                .item("other_check", "Fine.", "fine", "not fine"),
        )
        .unwrap();
    let risk = batch
        .score(
            "risk",
            &ScoreSpec::new("event risk", "How risky?", ["none", "low", "mid", "high", "max"])
                .driver("imminent_event", "An event is imminent."),
        )
        .unwrap();
    let mut out = batch.send().await.unwrap();
    let regime = out.take(regime).unwrap();
    let sanity = out.take(sanity).unwrap();
    let _risk = out.take(risk).unwrap();
    p.gate("gate", &input, &GateSpec::new("EURUSD long", "Go?")).await.unwrap();
    (recorder, jev, regime, sanity)
}

#[tokio::test]
async fn a_batch_asks_every_stage_in_one_call_under_stage_prefixed_names() {
    let (recorder, jev, regime, sanity) = run_batched().await;
    let calls = recorder.calls.lock().unwrap();
    assert_eq!(calls.len(), 2, "three independent stages and a gate are two calls");

    let names: Vec<&str> = calls[0].asks.iter().map(|(n, _)| n).collect();
    assert_eq!(
        names,
        vec![
            "regime.label",
            "sanity.stop_sane",
            "sanity.other_check",
            "risk.level",
            "risk.driver_imminent_event"
        ]
    );
    assert!(calls[0].state["prior_judgments"].as_array().unwrap().is_empty());

    // Each stage's typed output is the same as it would be on its own.
    assert_eq!(regime.label, Regime::Rough);
    assert_eq!(sanity.failed(), vec!["stop_sane"]);

    // The audit has one record for the batch, listing every stage.
    let records = jev.audit().records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].primitive.as_str(), "batch");
    assert_eq!(records[0].stage.as_deref(), Some("assess"));
    let listed: Vec<&str> = records[0]
        .output
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["stage"].as_str().unwrap())
        .collect();
    assert_eq!(listed, vec!["regime", "sanity", "risk"]);
    assert_eq!(records[0].output[1]["primitive"], json!("check"));
}

#[tokio::test]
async fn the_gate_after_a_batch_sees_every_batched_stage_as_a_prior() {
    let (recorder, _, _, _) = run_batched().await;
    let calls = recorder.calls.lock().unwrap();
    let priors = calls[1].state["prior_judgments"].as_array().unwrap();
    let stages: Vec<&str> = priors.iter().map(|e| e["stage"].as_str().unwrap()).collect();
    assert_eq!(stages, vec!["regime", "sanity", "risk"]);
    assert_eq!(priors[1]["output"]["checks"][0]["ok"], json!(false));
    // Ask names in a batch of one are bare.
    let names: Vec<&str> = calls[1].asks.iter().map(|(n, _)| n).collect();
    assert_eq!(names, vec!["action", "size_factor"]);
}

#[tokio::test]
async fn a_slot_can_only_be_claimed_once() {
    let recorder = Arc::new(Recorder::new(scripted));
    let jev = Jev::new(recorder);
    let input = TestInput::new(json!({}));
    let mut p = Pipeline::new(&jev, "fx");
    let mut batch = p.batch("one", &input);
    let a = batch.classify::<Regime>("regime", &ClassifySpec::new("s", "q?")).unwrap();
    let b = batch.score("risk", &ScoreSpec::new("r", "q?", ["a", "b"])).unwrap();
    let mut out = batch.send().await.unwrap();
    out.take(b).unwrap();
    out.take(a).unwrap();
    let mut batch = p.batch("two", &input);
    let c = batch.score("risk", &ScoreSpec::new("r", "q?", ["a", "b"])).unwrap();
    let mut second = batch.send().await.unwrap();
    // A slot from another batch is refused rather than handed the wrong output.
    assert!(second.take(c).is_ok());
    assert!(out.take(batch_slot()).is_err());
}

fn batch_slot() -> jev_core::Slot<jev_core::ScoreOut> {
    // The only way to get a slot is from a batch; this one is taken and dropped.
    let recorder = Arc::new(Recorder::new(scripted));
    let jev = Jev::new(recorder);
    let mut p = Pipeline::new(&jev, "x");
    let input = TestInput::new(json!({}));
    let mut batch = p.batch("z", &input);
    batch.classify::<Regime>("a", &ClassifySpec::new("s", "q?")).unwrap();
    batch.classify::<Regime>("b", &ClassifySpec::new("s", "q?")).unwrap();
    batch.score("c", &ScoreSpec::new("r", "q?", ["a", "b"])).unwrap()
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
        Classify::<_, Regime>::classify(&jev, &input, &ClassifySpec::new("s", "q?")).await.unwrap();
    let mut p = Pipeline::new(&jev, "fx");
    let _: ClassifyOut<Regime> =
        p.classify("regime", &input, &ClassifySpec::new("s", "q?")).await.unwrap();

    let states = recorder.states();
    let keys =
        |v: &serde_json::Value| -> Vec<String> { v.as_object().unwrap().keys().cloned().collect() };
    assert_eq!(keys(&states[0]), keys(&states[1]));
    assert_eq!(states[0], states[1]);
}
