//! The claim the whole comparison rests on: the judgment layer never sees the
//! generator's regime.
//!
//! If these fail, the gated-versus-ungated P&L is meaningless, because the
//! judgment layer would be reading an answer the market does not contain.

use fx::{run_session, FxRegime, StrategyParams};
use jev_core::{Audit, Jev, MockJev};
use std::sync::Arc;
use synth::{FxWorld, DEFAULT_SEED};

/// Strings that would indicate ground truth had leaked into a call.
const FORBIDDEN_KEYS: [&str; 3] = ["true_regime", "truth", "regime_correct"];

#[tokio::test]
async fn no_state_ever_sent_to_a_backend_carries_the_generators_regime() {
    let audit = Arc::new(Audit::new());
    let jev = Jev::with_audit(Arc::new(MockJev::new()), audit.clone());
    let world = FxWorld::generate(DEFAULT_SEED, 12);

    let session = run_session(&jev, &world, &StrategyParams::default(), None).await.unwrap();
    assert!(!session.decisions.is_empty(), "the run must actually decide something");

    let records = audit.records();
    assert!(!records.is_empty());
    for record in &records {
        let state = serde_json::to_string(&record.state).unwrap();
        for key in FORBIDDEN_KEYS {
            assert!(!state.contains(key), "`{key}` reached a {} call:\n{state}", record.primitive);
        }
    }
}

#[tokio::test]
async fn the_framing_does_not_name_the_true_regime_either() {
    let audit = Arc::new(Audit::new());
    let jev = Jev::with_audit(Arc::new(MockJev::new()), audit.clone());
    let world = FxWorld::generate(DEFAULT_SEED, 12);
    run_session(&jev, &world, &StrategyParams::default(), None).await.unwrap();

    // The framing under `context` is prose, so this checks the phrasing rather
    // than a key, over the whole record.
    for record in audit.records() {
        let rendered = serde_json::to_string(&record).unwrap();
        for key in FORBIDDEN_KEYS {
            assert!(!rendered.contains(key), "`{key}` appears in a call record");
        }
    }
}

#[test]
fn the_decision_input_type_has_no_field_that_could_carry_truth() {
    // A structural check: whatever the values, the shape cannot express it.
    let world = FxWorld::generate(DEFAULT_SEED, 30);
    let series = world.series_for(synth::Pair::EurUsd);
    let params = StrategyParams::default();
    let candidate = fx::signals(&series.bars, synth::Pair::EurUsd, &params)
        .into_iter()
        .next()
        .expect("the strategy fires within 30 days");

    let input = fx::build_input(&world, &series.bars, &candidate, &params, &world.book);
    let json = serde_json::to_value(&input).unwrap();
    let mut keys = Vec::new();
    collect_keys(&json, &mut keys);
    for key in FORBIDDEN_KEYS {
        assert!(!keys.iter().any(|k| k == key), "FxDecisionInput exposes `{key}`");
    }
    // And positively: the observable features the mock's rules are written
    // against are all present.
    for expected in [
        "hours_to_event",
        "stop_atr_multiple",
        "stop_clears_noise",
        "trend_strength",
        "pre_trade_exposure_share",
        "post_trade_exposure_share",
    ] {
        assert!(keys.iter().any(|k| k == expected), "missing observable `{expected}`");
    }
    // The generator labels each headline `informative`; the text goes, the
    // label and any count of it do not.
    assert!(!keys.iter().any(|k| k.contains("informative")), "a headline label leaked");
}

fn collect_keys(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                collect_keys(v, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_keys(v, out)),
        _ => {}
    }
}

#[tokio::test]
async fn the_classifier_is_evaluated_against_truth_but_does_not_match_it_perfectly() {
    // The point of keeping truth out: agreement is an empirical result, not a
    // guarantee. A perfect score would mean the truth had leaked.
    let jev = Jev::new(Arc::new(MockJev::new()));
    let world = FxWorld::generate(DEFAULT_SEED, 45);
    let session = run_session(&jev, &world, &StrategyParams::default(), None).await.unwrap();

    let accuracy = session.regime_accuracy();
    assert!(
        accuracy < 1.0,
        "perfect regime agreement ({accuracy:.2}) would mean ground truth had leaked"
    );
    assert!(
        session.decisions.iter().any(|d| !d.regime_correct()),
        "the classifier should disagree with the generator at least once"
    );
}

#[test]
fn the_domain_regime_and_the_generator_regime_are_the_same_partition() {
    // Evaluation only makes sense if the two enums line up label for label.
    for truth in synth::Regime::ALL {
        let mapped = FxRegime::from_truth(truth);
        assert_eq!(
            jev_core::Label::name(&mapped),
            truth.as_str(),
            "the domain enum must mirror the generator's"
        );
    }
}
