//! Each primitive's schema validation, and the composition of its output.

mod common;

use common::{choice, noul, score_at, Recorder, TestInput};
use jev_core::{
    Action, Audience, Check, CheckOut, CheckResult, CheckSource, CheckSpec, Classify, ClassifySpec,
    Evidence, Explain, ExplainSpec, Gate, GateOut, GateSpec, Jev, JevError, Label, Rank, RankSpec,
    Score, ScoreOut, ScoreSpec, Verdict,
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

/// A backend that may leave a question unanswered.
struct Partial<F>(F);

#[async_trait::async_trait]
impl<F: Fn(&str) -> Option<Verdict> + Send + Sync> jev_core::JevClient for Partial<F> {
    fn backend(&self) -> &'static str {
        "partial"
    }
    async fn ask(&self, call: &jev_core::JevCall) -> jev_core::Result<jev_core::JevReply> {
        let mut verdicts = indexmap::IndexMap::new();
        for (name, _) in call.asks.iter() {
            if let Some(v) = (self.0)(name) {
                verdicts.insert(name.to_owned(), v);
            }
        }
        Ok(jev_core::JevReply {
            verdicts,
            model: "partial".into(),
            usage: jev_core::Usage::default(),
            latency: std::time::Duration::ZERO,
        })
    }
}

fn jev_answering_partially<F>(answer: F) -> Jev
where
    F: Fn(&str) -> Option<Verdict> + Send + Sync + 'static,
{
    Jev::new(Arc::new(Partial(answer)))
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
            choice("execute", &["reduce", "hold", "escalate"])
        } else {
            score_at(1.0, 5)
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
            verdicts
                .insert("action".to_owned(), choice("execute", &["reduce", "hold", "escalate"]));
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
    assert!(matches!(bad.validate(), Err(JevError::Contradiction { .. })), "{:?}", bad.validate());
}

#[test]
fn gate_schema_rejects_an_acting_action_carrying_no_size() {
    // "Reduce to nothing" is not a decision anyone can act on. Rounding it to a
    // hold would be inventing the judgment rather than reporting it.
    for action in [Action::Execute, Action::Reduce] {
        let bad =
            GateOut { action, size_factor: 0.0, reason: "x".into(), evidence: Evidence::default() };
        assert!(
            matches!(bad.validate(), Err(JevError::Contradiction { .. })),
            "{action:?} at zero size should not validate"
        );
    }
}

#[tokio::test]
async fn gate_errors_rather_than_returning_a_contradiction() {
    // Jev says reduce; the size rubric says none. The primitive must not
    // silently pick one of them.
    let jev = jev_answering(|name, _| match name {
        "action" => choice("reduce", &["execute", "hold", "escalate"]),
        _ => score_at(0.0, 5),
    });
    let err = Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.unwrap_err();
    assert!(matches!(err, JevError::Contradiction { .. }), "{err:?}");
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
    let jev = jev_answering(|name, _| if name == "level" { score_at(1.0, 3) } else { noul(0.99) });
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
    let bad =
        ScoreOut { score: 101, drivers: vec![], reason: "x".into(), evidence: Evidence::default() };
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
        checks: vec![CheckResult {
            name: "x".into(),
            ok: true,
            note: "n".into(),
            p: 1.5,
            source: CheckSource::Jev,
        }],
    };
    assert!(matches!(bad.validate(), Err(JevError::OutOfRange { field: "p", .. })));
}

#[tokio::test]
async fn a_rule_check_is_decided_in_code_and_never_asked() {
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = asked.clone();
    let jev = jev_answering(move |name, _| {
        seen.lock().unwrap().push(name.to_owned());
        noul(0.9)
    });
    let spec = CheckSpec::new("trade")
        .rule("stop_sane", "The stop clears noise.", "clears it", "inside one bar", false)
        .item("signal_valid", "The signal is valid here.", "valid", "not valid");
    let out = Check::check(&jev, &input(), &spec).await.unwrap();

    assert_eq!(asked.lock().unwrap().as_slice(), ["signal_valid"]);
    let stop = out.get("stop_sane").unwrap();
    assert_eq!((stop.ok, stop.p, stop.source), (false, 0.0, CheckSource::Rule));
    assert!(stop.note.starts_with("rule fails: inside one bar"), "{}", stop.note);
    let signal = out.get("signal_valid").unwrap();
    assert_eq!((signal.ok, signal.source), (true, CheckSource::Jev));
    // Report order is spec order, whoever decided.
    assert_eq!(out.failed(), vec!["stop_sane"]);
    assert_eq!(
        out.checks.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["stop_sane", "signal_valid"]
    );
}

#[tokio::test]
async fn a_check_of_only_rules_is_refused_as_not_a_judgment() {
    let jev = jev_answering(|_, _| noul(0.9));
    let spec = CheckSpec::new("t").rule("a", "c", "ok", "not ok", true);
    let err = Check::check(&jev, &input(), &spec).await.unwrap_err();
    assert!(matches!(err, JevError::InvalidCall(_)), "{err:?}");
}

#[test]
fn check_schema_rejects_a_rule_with_a_fractional_probability() {
    let bad = CheckOut {
        checks: vec![CheckResult {
            name: "x".into(),
            ok: true,
            note: "n".into(),
            p: 0.7,
            source: CheckSource::Rule,
        }],
    };
    assert!(matches!(bad.validate(), Err(JevError::Contradiction { .. })));
}

#[tokio::test]
async fn undecided_checks_are_the_judged_ones_near_the_threshold() {
    let jev = jev_answering(|name, _| match name {
        "a" => noul(0.55),
        "b" => noul(0.95),
        _ => noul(0.1),
    });
    let spec = CheckSpec::new("t")
        .item("a", "c", "ok", "not ok")
        .item("b", "c", "ok", "not ok")
        .item("c", "c", "ok", "not ok")
        .rule("d", "c", "ok", "not ok", true);
    let out = Check::check(&jev, &input(), &spec).await.unwrap();
    let names: Vec<&str> = out.undecided(0.4, 0.6).iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["a"]);
}

// ---- rank ----------------------------------------------------------------------------------

#[tokio::test]
async fn rank_reads_the_ordering_off_per_candidate_fit_scores() {
    // Offer order is aggressive, balanced, reserve_heavy; the ordering is not.
    // Each candidate gets its own Score on the same four-level rubric.
    let jev = jev_answering(|name, _| match name {
        "fit_aggressive" => score_at(0.25, 4),
        "fit_balanced" => score_at(0.5, 4),
        "fit_reserve_heavy" => score_at(0.85, 4),
        other => panic!("unexpected ask {other}"),
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
    assert!((out.ordered[1].fit - 0.5).abs() < 1e-6);
}

#[tokio::test]
async fn rank_asks_one_fit_question_per_candidate_naming_it() {
    let recorder = std::sync::Arc::new(common::Recorder::new(|_, _| score_at(0.5, 4)));
    let jev = Jev::new(recorder.clone());
    let spec = RankSpec::new(
        "s",
        "q?",
        vec![
            jev_core::Candidate::new("a".to_string(), "does x"),
            jev_core::Candidate::new("b".to_string(), "does y"),
        ],
    );
    Rank::<_, String>::rank(&jev, &input(), &spec).await.unwrap();
    let calls = recorder.calls.lock().unwrap();
    let names: Vec<&str> = calls[0].asks.iter().map(|(n, _)| n).collect();
    assert_eq!(names, vec!["fit_a", "fit_b"]);
    let ask = calls[0].asks.get("fit_b").unwrap();
    assert_eq!(ask.instructions()["candidate"]["summary"], serde_json::json!("does y"));
    assert!(matches!(ask, jev_core::Ask::Score { levels, .. } if levels.len() == 4));
}

#[tokio::test]
async fn rank_keeps_offer_order_between_equal_fits() {
    let jev = jev_answering(|_, _| score_at(0.5, 4));
    let spec = RankSpec::new(
        "s",
        "q?",
        vec![
            jev_core::Candidate::new("b".to_string(), "y"),
            jev_core::Candidate::new("a".to_string(), "x"),
        ],
    );
    let out = Rank::<_, String>::rank(&jev, &input(), &spec).await.unwrap();
    let order: Vec<&str> = out.ordered.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(order, vec!["b", "a"]);
    assert_eq!(out.margin(), 0.0);
}

#[tokio::test]
async fn rank_rejects_a_reply_that_drops_a_candidate() {
    let jev = jev_answering_partially(|name| (name != "fit_c").then(|| score_at(0.5, 4)));
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
    assert!(matches!(err, JevError::MissingAnswer { asked: 3, .. }), "{err:?}");
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

#[tokio::test]
async fn explain_drops_a_fact_that_will_not_fit_rather_than_cutting_it() {
    // Every fact wants in, and together they cannot fit in 300 characters.
    let jev = jev_answering(|name, _| match name {
        "framing" => choice("a", &["b"]),
        "severity" => score_at(0.5, 4),
        _ => noul(0.99),
    });
    let mut spec = ExplainSpec::new("the session", Audience::Ops)
        .framing("a", "d", "the layer did something")
        .framing("b", "d", "the layer did something else");
    for i in 0..8 {
        spec = spec.fact(
            format!("f{i}"),
            "relevant?",
            format!("fact number {i} carries a reasonably long clause of its own"),
        );
    }
    let out = Explain::explain(&jev, &input(), &spec).await.unwrap();

    assert!(out.summary.chars().count() <= 300);
    assert!(
        !out.summary.contains('\u{2026}'),
        "a fact was cut off instead of dropped: {}",
        out.summary
    );
    // Whatever it kept, it kept whole.
    for i in 0..8 {
        let phrase = format!("fact number {i} carries a reasonably long clause of its own");
        let mentioned = out.summary.contains(&phrase);
        let partial = out.summary.contains(&format!("fact number {i}")) && !mentioned;
        assert!(!partial, "fact {i} appears only partially: {}", out.summary);
    }
}

#[tokio::test]
async fn execute_uses_full_size_and_reduce_rejects_full_size() {
    let jev = jev_answering(|name, _| match name {
        "action" => choice("execute", &["reduce", "hold", "escalate"]),
        _ => score_at(0.5, 5),
    });
    let out = Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.unwrap();
    assert_eq!(out.size_factor, 1.0);
    let jev = jev_answering(|name, _| match name {
        "action" => choice("reduce", &["execute", "hold", "escalate"]),
        _ => score_at(1.0, 5),
    });
    assert!(matches!(
        Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await,
        Err(JevError::Contradiction { .. })
    ));
    assert!(jev.audit().is_empty());
}

#[tokio::test]
async fn rounded_distributions_and_omitted_empty_levels_are_accepted() {
    // The API reports rounded probabilities; a three-way 0.333 split sums to
    // 0.999, which is rounding, not a malformed answer.
    let jev = jev_answering(|name, _| match name {
        "action" => {
            let mut p = indexmap::IndexMap::new();
            p.insert("execute".to_owned(), 0.333);
            p.insert("reduce".to_owned(), 0.333);
            p.insert("hold".to_owned(), 0.333);
            p.insert("escalate".to_owned(), 0.0);
            Verdict::Choice { label: "execute".into(), probabilities: p, confidence: 0.1 }
        }
        _ => {
            // Only the levels that carry mass; the empty ones are filled in.
            let probabilities = std::collections::BTreeMap::from([(3, 0.4), (4, 0.6)]);
            Verdict::Score { score: 3.6, levels: 5, probabilities, confidence: 0.6 }
        }
    });
    let out = Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.unwrap();
    assert_eq!(out.action, Action::Execute);
    assert_eq!(jev.audit().len(), 1);
}

#[tokio::test]
async fn malformed_distributions_never_become_audited_decisions() {
    for case in 0..7 {
        let jev = jev_answering(move |name, _| {
            if name == "action" {
                let mut answer = choice("execute", &["reduce", "hold", "escalate"]);
                if let Verdict::Choice { probabilities, confidence, label } = &mut answer {
                    match case {
                        0 => {
                            probabilities.shift_remove("hold");
                        }
                        1 => {
                            probabilities.insert("hold".into(), -0.1);
                        }
                        2 => {
                            probabilities.insert("hold".into(), 0.5);
                        }
                        3 => {
                            *label = "reduce".into();
                        }
                        4 => {
                            *confidence = 1.1;
                        }
                        _ => {}
                    }
                }
                answer
            } else {
                let mut answer = score_at(0.5, 5);
                if let Verdict::Score { score, probabilities, .. } = &mut answer {
                    match case {
                        5 => {
                            *score = 3.0;
                        }
                        6 => {
                            // Dropping the level that carries the mass, not an
                            // empty one: an omitted empty level is filled in.
                            probabilities.remove(&2);
                        }
                        _ => {}
                    }
                }
                answer
            }
        });
        assert!(
            Gate::gate(&jev, &input(), &GateSpec::new("x", "go?")).await.is_err(),
            "case {case}"
        );
        assert!(jev.audit().is_empty(), "case {case}");
    }
}
