//! Each primitive's schema validation, and the composition of its output.

mod common;

use common::{choice, noul, score_at, Recorder, TestInput};
use jev_core::{
    Action, Audience, Check, CheckOut, CheckResult, CheckSpec, Classify, ClassifySpec, Evidence,
    Explain, ExplainSpec, Gate, GateOut, GateSpec, Jev, JevError, Label, Rank, RankSpec, Score,
    ScoreOut, ScoreSpec, Verdict,
};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum TestRegime {
    Calm,
    Rough,
}

impl Label for TestRegime {
    fn labels() -> &'static [Self] {
        &[TestRegime::Calm, TestRegime::Rough]
    }
    fn name(&self) -> &'static str {
        match self {
            TestRegime::Calm => "calm",
            TestRegime::Rough => "rough",
        }
    }
    fn describe(&self) -> &'static str {
        match self {
            TestRegime::Calm => "Quiet conditions.",
            TestRegime::Rough => "Disturbed conditions.",
        }
    }
}

fn jev_answering<F>(answer: F) -> Jev
where
    F: Fn(&str, &jev_core::Asks) -> Verdict + Send + Sync + 'static,
{
    Jev::new(Arc::new(Recorder::new(answer)))
}

fn input() -> TestInput {
    TestInput::new(json!({}))
}

// ---- gate ----------------------------------------------------------------------------------

#[tokio::test]
async fn gate_maps_the_size_rubric_onto_a_fraction() {
    let jev = jev_answering(|name, _| match name {
        "action" => choice("reduce", &["execute", "hold", "escalate"]),
        _ => score_at(0.5, 5),
    });
    let spec = GateSpec::new("EURUSD long", "Should this trade go on?");
    let out = Gate::gate(&jev, &input(), &spec).await.unwrap();

    assert_eq!(out.action, Action::Reduce);
    assert!((out.size_factor - 0.5).abs() < 1e-6, "got {}", out.size_factor);
    assert!(out.reason.contains("reduce at 50%"), "{}", out.reason);
    assert!(out.reason.chars().count() <= 160);
}

#[tokio::test]
async fn gate_forces_zero_size_when_it_does_not_act() {
    // The size rubric says "all of it"; hold must still put nothing on.
    let jev = jev_answering(|name, _| match name {
        "action" => choice("hold", &["execute", "reduce", "escalate"]),
        _ => score_at(1.0, 5),
    });
    let out = Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.unwrap();
    assert_eq!(out.action, Action::Hold);
    assert_eq!(out.size_factor, 0.0);
}

#[tokio::test]
async fn gate_rejects_a_label_outside_the_action_set() {
    let jev = jev_answering(|name, _| match name {
        "action" => choice("yolo", &["execute"]),
        _ => score_at(1.0, 5),
    });
    let err = Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.unwrap_err();
    assert!(matches!(err, JevError::UnknownLabel { .. }), "{err:?}");
    // The point of the hard error: nothing fell through to Execute.
    assert!(err.to_string().contains("yolo"));
}

#[tokio::test]
async fn gate_rejects_the_wrong_answer_kind() {
    let jev = jev_answering(|_, _| noul(0.9));
    let err = Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.unwrap_err();
    assert!(matches!(err, JevError::AnswerKind { want: "choice", .. }), "{err:?}");
}

#[tokio::test]
async fn gate_rejects_a_missing_answer() {
    // A backend that answers only the action, never the size.
    let jev = Jev::new(Arc::new(Recorder::new(|name, _| {
        if name == "action" {
            choice("execute", &["reduce"])
        } else {
            score_at(0.0, 5)
        }
    })));
    // Sanity: with both answered it succeeds, so the next assertion isolates the miss.
    assert!(Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.is_ok());

    struct Partial;
    #[async_trait::async_trait]
    impl jev_core::JevClient for Partial {
        fn backend(&self) -> &'static str {
            "partial"
        }
        async fn ask(&self, _call: &jev_core::JevCall) -> jev_core::Result<jev_core::JevReply> {
            let mut verdicts = indexmap::IndexMap::new();
            verdicts.insert("action".to_owned(), choice("execute", &["reduce"]));
            Ok(jev_core::JevReply {
                verdicts,
                model: "partial".into(),
                usage: Default::default(),
                latency: std::time::Duration::ZERO,
            })
        }
    }
    let jev = Jev::new(Arc::new(Partial));
    let err = Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.unwrap_err();
    assert!(matches!(err, JevError::MissingAnswer { .. }), "{err:?}");
}

#[test]
fn gate_schema_rejects_an_out_of_range_size() {
    let bad = GateOut {
        action: Action::Execute,
        size_factor: 1.4,
        reason: "x".into(),
        evidence: Evidence::default(),
    };
    assert!(matches!(bad.validate(), Err(JevError::OutOfRange { field: "size_factor", .. })));
}

#[test]
fn gate_schema_rejects_an_over_long_reason() {
    let bad = GateOut {
        action: Action::Execute,
        size_factor: 1.0,
        reason: "x".repeat(161),
        evidence: Evidence::default(),
    };
    assert!(matches!(bad.validate(), Err(JevError::TooLong { field: "reason", max: 160, .. })));
}

#[test]
fn gate_schema_rejects_a_non_acting_action_carrying_size() {
    let bad = GateOut {
        action: Action::Escalate,
        size_factor: 0.5,
        reason: "x".into(),
        evidence: Evidence::default(),
    };
    assert!(bad.validate().is_err());
}

// ---- classify ------------------------------------------------------------------------------

#[tokio::test]
async fn classify_parses_the_label_and_keeps_the_distribution() {
    let jev = jev_answering(|_, _| choice("rough", &["calm"]));
    let spec = ClassifySpec::new("session", "Which regime?");
    let out: jev_core::ClassifyOut<TestRegime> =
        Classify::<_, TestRegime>::classify(&jev, &input(), &spec).await.unwrap();

    assert_eq!(out.label, TestRegime::Rough);
    assert!((out.confidence - 0.85).abs() < 1e-6);
    assert_eq!(out.evidence.distribution[0].label, "rough");
    assert_eq!(out.evidence.runner_up().unwrap().label, "calm");
}

#[tokio::test]
async fn classify_rejects_a_label_outside_the_enum() {
    let jev = jev_answering(|_, _| choice("sideways", &["calm"]));
    let err = Classify::<_, TestRegime>::classify(&jev, &input(), &ClassifySpec::new("s", "q?"))
        .await
        .unwrap_err();
    assert!(matches!(err, JevError::UnknownLabel { .. }), "{err:?}");
}

// ---- score ---------------------------------------------------------------------------------

#[tokio::test]
async fn score_maps_the_rubric_to_0_100_and_reports_held_drivers() {
    let jev = jev_answering(|name, _| match name {
        "level" => score_at(0.75, 5),
        "driver_a" => noul(0.9),
        "driver_b" => noul(0.2),
        "driver_c" => noul(0.7),
        _ => noul(0.0),
    });
    let spec = ScoreSpec::new("event risk", "How risky?", ["none", "low", "mid", "high", "max"])
        .driver("a", "A holds")
        .driver("b", "B holds")
        .driver("c", "C holds");
    let out = Score::score(&jev, &input(), &spec).await.unwrap();

    assert_eq!(out.score, 75);
    // Strongest first, threshold 0.5, so b is dropped.
    assert_eq!(out.drivers, vec!["a".to_string(), "c".to_string()]);
    assert!(out.reason.contains("75/100"), "{}", out.reason);
}

#[tokio::test]
async fn score_caps_reported_drivers_at_three() {
    let jev = jev_answering(|name, _| {
        if name == "level" {
            score_at(1.0, 3)
        } else {
            noul(0.99)
        }
    });
    let mut spec = ScoreSpec::new("x", "q?", ["a", "b", "c"]);
    for n in ["d1", "d2", "d3", "d4", "d5"] {
        spec = spec.driver(n, "holds");
    }
    let out = Score::score(&jev, &input(), &spec).await.unwrap();
    assert_eq!(out.drivers.len(), 3);
    assert_eq!(out.evidence.distribution.len(), 5, "all candidates stay in the evidence");
}

#[test]
fn score_schema_rejects_an_out_of_range_score() {
    let bad = ScoreOut {
        score: 101,
        drivers: vec![],
        reason: "x".into(),
        evidence: Evidence::default(),
    };
    assert!(matches!(bad.validate(), Err(JevError::OutOfRange { field: "score", .. })));
}

#[test]
fn score_schema_rejects_more_than_three_drivers() {
    let bad = ScoreOut {
        score: 10,
        drivers: vec!["a".into(), "b".into(), "c".into(), "d".into()],
        reason: "x".into(),
        evidence: Evidence::default(),
    };
    assert!(matches!(bad.validate(), Err(JevError::TooLong { field: "drivers", max: 3, .. })));
}

// ---- check ---------------------------------------------------------------------------------

#[tokio::test]
async fn check_thresholds_each_claim_and_names_the_failures() {
    let jev = jev_answering(|name, _| match name {
        "stop_sane" => noul(0.9),
        "signal_valid" => noul(0.3),
        _ => noul(0.5),
    });
    let spec = CheckSpec::new("trade")
        .item("stop_sane", "The stop is sane.", "wide enough", "too tight")
        .item("signal_valid", "The signal is valid.", "valid here", "not valid here");
    let out = Check::check(&jev, &input(), &spec).await.unwrap();

    assert_eq!(out.checks.len(), 2);
    assert!(out.get("stop_sane").unwrap().ok);
    assert!(!out.get("signal_valid").unwrap().ok);
    assert_eq!(out.failed(), vec!["signal_valid"]);
    assert!(out.get("signal_valid").unwrap().note.contains("not valid here"));
    assert!(!out.all_ok());
}

#[tokio::test]
async fn check_is_exactly_at_the_threshold_inclusive() {
    let jev = jev_answering(|_, _| noul(0.5));
    let spec = CheckSpec::new("t").item("edge", "c", "ok", "not ok");
    let out = Check::check(&jev, &input(), &spec).await.unwrap();
    assert!(out.get("edge").unwrap().ok, "p == threshold must pass");
}

#[test]
fn check_schema_rejects_an_out_of_range_probability() {
    let bad = CheckOut {
        checks: vec![CheckResult { name: "x".into(), ok: true, note: "n".into(), p: 1.5 }],
    };
    assert!(matches!(bad.validate(), Err(JevError::OutOfRange { field: "p", .. })));
}

// ---- rank ----------------------------------------------------------------------------------

#[tokio::test]
async fn rank_reads_the_ordering_off_the_distribution() {
    let jev = jev_answering(|_, _| {
        let mut p = indexmap::IndexMap::new();
        // Offer order is aggressive, balanced, reserve_heavy; the ordering is not.
        p.insert("aggressive".to_owned(), 0.15);
        p.insert("balanced".to_owned(), 0.25);
        p.insert("reserve_heavy".to_owned(), 0.60);
        Verdict::Choice { label: "reserve_heavy".into(), probabilities: p, confidence: 0.6 }
    });
    let spec = RankSpec::new(
        "schedules",
        "Which schedule suits today?",
        vec![
            jev_core::Candidate::new("aggressive".to_string(), "max arbitrage"),
            jev_core::Candidate::new("balanced".to_string(), "middle"),
            jev_core::Candidate::new("reserve_heavy".to_string(), "holds reserve"),
        ],
    );
    let out = Rank::<_, String>::rank(&jev, &input(), &spec).await.unwrap();

    let order: Vec<&str> = out.ordered.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(order, vec!["reserve_heavy", "balanced", "aggressive"]);
    assert_eq!(out.top().unwrap().id, "reserve_heavy");
    assert!((out.margin() - 0.35).abs() < 1e-6, "margin {}", out.margin());
    assert!(out.ordered[0].rationale.starts_with("1st of 3"), "{}", out.ordered[0].rationale);
}

#[tokio::test]
async fn rank_rejects_a_reply_that_drops_a_candidate() {
    let jev = jev_answering(|_, _| choice("a", &["b"]));
    let spec = RankSpec::new(
        "s",
        "q?",
        vec![
            jev_core::Candidate::new("a".to_string(), "x"),
            jev_core::Candidate::new("b".to_string(), "y"),
            jev_core::Candidate::new("c".to_string(), "z"),
        ],
    );
    let err = Rank::<_, String>::rank(&jev, &input(), &spec).await.unwrap_err();
    assert!(matches!(err, JevError::BadRanking { got: 2, want: 3, .. }), "{err:?}");
}

#[tokio::test]
async fn rank_refuses_fewer_than_two_candidates() {
    let jev = jev_answering(|_, _| choice("a", &[]));
    let spec = RankSpec::new("s", "q?", vec![jev_core::Candidate::new("a".to_string(), "x")]);
    let err = Rank::<_, String>::rank(&jev, &input(), &spec).await.unwrap_err();
    assert!(matches!(err, JevError::InvalidCall(_)), "{err:?}");
}

// ---- explain -------------------------------------------------------------------------------

#[tokio::test]
async fn explain_composes_from_the_chosen_framing_severity_and_facts() {
    let jev = jev_answering(|name, _| match name {
        "framing" => choice("intervened", &["quiet"]),
        "severity" => score_at(2.0 / 3.0, 4),
        "fact_pnl" => noul(0.9),
        "fact_audit" => noul(0.1),
        _ => noul(0.0),
    });
    let spec = ExplainSpec::new("the EUR/USD session", Audience::Trader)
        .framing("quiet", "Nothing intervened.", "the judgment layer stayed out of the way")
        .framing("intervened", "The gate changed outcomes.", "the gate changed what went on")
        .fact("pnl", "The gated book outperformed.", "gating added 18 bps")
        .fact("audit", "Every call was logged.", "every call is in decisions.jsonl");
    let out = Explain::explain(&jev, &input(), &spec).await.unwrap();

    assert_eq!(out.for_audience, Audience::Trader);
    assert!(out.summary.contains("the gate changed what went on"), "{}", out.summary);
    assert!(out.summary.contains("notable"), "{}", out.summary);
    // Facts are capitalised as they open their own sentence.
    assert!(out.summary.contains("Gating added 18 bps."), "{}", out.summary);
    assert!(
        out.summary.contains("went on. The session"),
        "the lead should close its own sentence: {}",
        out.summary
    );
    assert!(!out.summary.contains("decisions.jsonl"), "excluded fact leaked: {}", out.summary);
    assert!(out.summary.chars().count() <= 300);
}

#[tokio::test]
async fn explain_refuses_a_single_framing() {
    let jev = jev_answering(|_, _| choice("only", &[]));
    let spec = ExplainSpec::new("x", Audience::Ops).framing("only", "d", "l");
    let err = Explain::explain(&jev, &input(), &spec).await.unwrap_err();
    assert!(matches!(err, JevError::InvalidCall(_)), "{err:?}");
}
