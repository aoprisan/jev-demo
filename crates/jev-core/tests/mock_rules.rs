//! The mock's rules, stated against the specification they implement.
//!
//! The stop, reserve and cycle-budget checks are rules the domain evaluates
//! in code; here they are stated as rule items with the outcome the features
//! would give, and the mock reads the same outcome as a flag where a driver
//! or the gate needs it.

mod common;

use common::TestInput;
use jev_core::mock::thresholds;
use jev_core::{
    Action, Audience, Check, CheckSpec, ClassifySpec, Explain, ExplainSpec, Gate, GateSpec, Jev,
    Label, MockJev, Pipeline, Rank, RankSpec, Score, ScoreSpec,
};
use serde_json::json;
use std::sync::Arc;

fn mock() -> Jev {
    Jev::new(Arc::new(MockJev::new()))
}

fn input(features: serde_json::Value) -> TestInput {
    TestInput::new(features)
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum MarketRegime {
    Normal,
    Volatile,
    Illiquid,
    Stressed,
}

impl Label for MarketRegime {
    fn labels() -> &'static [Self] {
        &[
            MarketRegime::Normal,
            MarketRegime::Volatile,
            MarketRegime::Illiquid,
            MarketRegime::Stressed,
        ]
    }
    fn name(&self) -> &'static str {
        match self {
            MarketRegime::Normal => "normal",
            MarketRegime::Volatile => "volatile",
            MarketRegime::Illiquid => "illiquid",
            MarketRegime::Stressed => "stressed",
        }
    }
    fn describe(&self) -> &'static str {
        "a market regime"
    }
}

fn flag(features: &serde_json::Value, key: &str, default: bool) -> bool {
    features.get(key).and_then(serde_json::Value::as_bool).unwrap_or(default)
}

fn fx_checks(features: &serde_json::Value) -> CheckSpec {
    CheckSpec::new("trade")
        .rule(
            "stop_sane",
            "The stop is wide enough.",
            "wide enough",
            "too tight",
            flag(features, "stop_clears_noise", true),
        )
        .item("signal_valid_in_regime", "The signal suits the regime.", "suits", "does not suit")
        .item("correlated_exposure_ok", "Exposure stays balanced.", "balanced", "concentrated")
}

fn battery_checks(features: &serde_json::Value) -> CheckSpec {
    CheckSpec::new("schedule")
        .rule(
            "reserve_ok",
            "The reserve is respected.",
            "respected",
            "breached",
            !flag(features, "reserve_breached", false),
        )
        .item("margin_plausible", "The margin is plausible.", "plausible", "implausible")
        .rule(
            "cycle_budget_ok",
            "The cycle budget holds.",
            "holds",
            "nearly spent",
            flag(features, "cycles_within_budget", true),
        )
}

fn gate_spec() -> GateSpec {
    GateSpec::new("the action", "Should this go on?")
}

// ---- FX rules ------------------------------------------------------------------------------

#[tokio::test]
async fn fx_holds_when_an_event_is_within_three_hours_and_the_stop_is_tight() {
    let jev = mock();
    let out = Gate::gate(
        &jev,
        &input(json!({
            "hours_to_event": 2.0,
            "next_event_kind": "CPI",
            "stop_clears_noise": false,
        })),
        &gate_spec(),
    )
    .await
    .unwrap();

    assert_eq!(out.action, Action::Hold);
    assert_eq!(out.size_factor, 0.0);
}

#[tokio::test]
async fn fx_does_not_hold_when_only_one_half_of_the_rule_is_met() {
    let jev = mock();

    // Event imminent, but the stop is wide enough for the volatility.
    let wide = Gate::gate(
        &jev,
        &input(json!({
            "hours_to_event": 2.0,
            "next_event_kind": "ECB",
            "stop_clears_noise": true,
        })),
        &gate_spec(),
    )
    .await
    .unwrap();
    assert_ne!(wide.action, Action::Hold, "a wide stop is not held for an event alone");

    // Stop tight, but no event anywhere near.
    let quiet = Gate::gate(
        &jev,
        &input(json!({ "hours_to_event": 40.0, "stop_clears_noise": false })),
        &gate_spec(),
    )
    .await
    .unwrap();
    assert_ne!(quiet.action, Action::Hold, "a tight stop alone is not an event hold");
}

#[tokio::test]
async fn a_tight_stop_alone_reduces_rather_than_holding() {
    // The conjunction matters. If a failed stop_sane held on its own, removing
    // the event from a replay could never change the outcome, and the demo's
    // side-by-side would show two identical records.
    let jev = mock();
    let features = json!({ "hours_to_event": 40.0, "stop_clears_noise": false });
    let mut p = Pipeline::new(&jev, "fx");
    let checks = p.check("sanity", &input(features.clone()), &fx_checks(&features)).await.unwrap();
    assert!(!checks.get("stop_sane").unwrap().ok);

    let out = p.gate("gate", &input(features), &gate_spec()).await.unwrap();
    assert_eq!(out.action, Action::Reduce);
    assert!(out.size_factor > 0.0);
}

#[tokio::test]
async fn the_event_is_what_turns_a_reduce_into_a_hold() {
    // The same tight stop, judged with and without an imminent release.
    let tight_stop = |hours: f64| json!({ "hours_to_event": hours, "next_event_kind": "CPI", "stop_clears_noise": false });
    let judge = |features: serde_json::Value| async move {
        let jev = mock();
        let mut p = Pipeline::new(&jev, "fx");
        p.check("sanity", &input(features.clone()), &fx_checks(&features)).await.unwrap();
        p.gate("gate", &input(features), &gate_spec()).await.unwrap()
    };

    let imminent = judge(tight_stop(2.0)).await;
    let distant = judge(tight_stop(40.0)).await;

    assert_eq!(imminent.action, Action::Hold);
    assert_eq!(imminent.size_factor, 0.0);
    assert_eq!(distant.action, Action::Reduce);
    assert!(distant.size_factor > 0.0);
}

#[tokio::test]
async fn fx_event_hold_boundary_is_the_documented_threshold() {
    let jev = mock();
    let at = |hours: f64| async move {
        Gate::gate(
            &mock(),
            &input(json!({
                "hours_to_event": hours,
                "next_event_kind": "CPI",
                "stop_clears_noise": false,
            })),
            &gate_spec(),
        )
        .await
        .unwrap()
        .action
    };
    let _ = &jev;
    assert_eq!(at(thresholds::EVENT_IMMINENT_HOURS).await, Action::Hold, "inclusive at 3h");
    assert_ne!(at(thresholds::EVENT_IMMINENT_HOURS + 0.1).await, Action::Hold);
}

#[tokio::test]
async fn fx_signal_is_invalid_when_the_window_reads_as_trending() {
    let jev = mock();
    let features = json!({ "trend_strength": 0.8, "stop_clears_noise": true });
    let trending =
        Check::check(&jev, &input(features.clone()), &fx_checks(&features)).await.unwrap();
    assert!(!trending.get("signal_valid_in_regime").unwrap().ok);

    let features = json!({ "trend_strength": 0.2, "stop_clears_noise": true });
    let ranging =
        Check::check(&jev, &input(features.clone()), &fx_checks(&features)).await.unwrap();
    assert!(ranging.get("signal_valid_in_regime").unwrap().ok);
}

#[tokio::test]
async fn fx_exposure_fails_when_a_trade_doubles_a_currency_past_the_cap() {
    let jev = mock();
    let features = json!({
        "exposure_multiple": 2.1,
        "post_trade_exposure_share": 0.44,
        "stop_clears_noise": true,
    });
    let doubling =
        Check::check(&jev, &input(features.clone()), &fx_checks(&features)).await.unwrap();
    assert!(!doubling.get("correlated_exposure_ok").unwrap().ok);

    // Doubling a negligible residual: the book stays spread, so this is fine.
    let features = json!({
        "exposure_multiple": 2.4,
        "post_trade_exposure_share": 0.12,
        "stop_clears_noise": true,
    });
    let small = Check::check(&jev, &input(features.clone()), &fx_checks(&features)).await.unwrap();
    assert!(small.get("correlated_exposure_ok").unwrap().ok);

    // Concentrated already, but this trade barely moved it: not this trade's doing.
    let features = json!({
        "exposure_multiple": 1.1,
        "post_trade_exposure_share": 0.42,
        "stop_clears_noise": true,
    });
    let inherited =
        Check::check(&jev, &input(features.clone()), &fx_checks(&features)).await.unwrap();
    assert!(inherited.get("correlated_exposure_ok").unwrap().ok);
}

// ---- Battery rules -------------------------------------------------------------------------

#[tokio::test]
async fn battery_ranks_reserve_heavy_first_when_the_afrr_window_is_active() {
    let out = Rank::<_, String>::rank(
        &mock(),
        &input(json!({ "afrr_window_active": true })),
        &schedules(),
    )
    .await
    .unwrap();
    assert_eq!(out.top().unwrap().id, "reserve_heavy");
}

#[tokio::test]
async fn battery_ranks_reserve_heavy_first_when_a_grid_notice_overlaps_the_discharge_block() {
    let out = Rank::<_, String>::rank(
        &mock(),
        &input(json!({ "afrr_window_active": false, "grid_notice_overlaps_discharge": true })),
        &schedules(),
    )
    .await
    .unwrap();
    assert_eq!(out.top().unwrap().id, "reserve_heavy");
}

#[tokio::test]
async fn battery_ranks_balanced_first_on_an_ordinary_day() {
    let out = Rank::<_, String>::rank(
        &mock(),
        &input(json!({ "afrr_window_active": false, "grid_notice_overlaps_discharge": false })),
        &schedules(),
    )
    .await
    .unwrap();
    assert_eq!(out.top().unwrap().id, "balanced");
    assert_eq!(out.ordered.last().unwrap().id, "reserve_heavy");
    // Each candidate's fit is its own rating, not a share of one distribution.
    assert!((out.top().unwrap().fit - 1.0).abs() < 1e-6);
    assert!(out.margin() > 0.2 && out.margin() < 0.3, "aggressive is close behind");
}

fn schedules() -> RankSpec<String> {
    RankSpec::new(
        "the day's schedules",
        "Which schedule suits today's conditions?",
        vec![
            jev_core::Candidate::new("aggressive".to_string(), "maximise arbitrage"),
            jev_core::Candidate::new("balanced".to_string(), "arbitrage with headroom"),
            jev_core::Candidate::new("reserve_heavy".to_string(), "hold the reserve"),
        ],
    )
}

#[tokio::test]
async fn battery_holds_on_a_reserve_breach_and_does_not_size_down_into_it() {
    let features = json!({ "afrr_window_active": true, "reserve_breached": true });
    let jev = mock();
    let mut p = Pipeline::new(&jev, "battery");
    let checks =
        p.check("sanity", &input(features.clone()), &battery_checks(&features)).await.unwrap();
    assert!(!checks.get("reserve_ok").unwrap().ok);
    let out = p.gate("gate", &input(features), &gate_spec()).await.unwrap();
    assert_eq!(out.action, Action::Hold);
    assert_eq!(out.size_factor, 0.0);
}

#[tokio::test]
async fn battery_margin_is_implausible_beyond_two_sigma_of_intraday_deviation() {
    let check = |sigmas: f64| async move {
        let features = json!({ "id_deviation_sigmas": sigmas });
        Check::check(&mock(), &input(features.clone()), &battery_checks(&features)).await.unwrap()
    };
    let wild = check(thresholds::ID_DEVIATION_SIGMA + 0.5).await;
    assert!(!wild.get("margin_plausible").unwrap().ok);

    let calm = check(1.0).await;
    assert!(calm.get("margin_plausible").unwrap().ok);

    // The rule is on the magnitude, so a large negative deviation fails too.
    let negative = check(-3.0).await;
    assert!(!negative.get("margin_plausible").unwrap().ok);
}

#[tokio::test]
async fn battery_reduces_when_the_regime_is_volatile_and_cycles_are_near_budget() {
    let jev = mock();
    let features = json!({
        "volatility_percentile": 0.85,
        "stress_indicator": 0.0,
        "spread_percentile": 0.2,
        "cycles_within_budget": false,
        "afrr_window_active": false,
        "id_deviation_sigmas": 0.3,
        "reserve_breached": false,
    });
    let mut p = Pipeline::new(&jev, "battery");
    let regime: jev_core::ClassifyOut<MarketRegime> = p
        .classify("regime", &input(features.clone()), &ClassifySpec::new("market", "Which regime?"))
        .await
        .unwrap();
    assert_eq!(regime.label, MarketRegime::Volatile);

    let out = p.gate("gate", &input(features), &gate_spec()).await.unwrap();
    assert_eq!(out.action, Action::Reduce);
    assert!(out.size_factor > 0.0 && out.size_factor <= 0.5, "got {}", out.size_factor);
}

#[tokio::test]
async fn an_implausible_margin_escalates_rather_than_resizing() {
    let jev = mock();
    let features = json!({ "id_deviation_sigmas": 4.0 });
    let mut p = Pipeline::new(&jev, "battery");
    p.check("sanity", &input(features.clone()), &battery_checks(&features)).await.unwrap();
    let out = p.gate("gate", &input(features), &gate_spec()).await.unwrap();

    assert_eq!(out.action, Action::Escalate, "an implausible input is for a human");
    assert_eq!(out.size_factor, 0.0);
}

// ---- shape ---------------------------------------------------------------------------------

#[tokio::test]
async fn the_mock_is_deterministic() {
    let features =
        json!({ "hours_to_event": 2.0, "next_event_kind": "NFP", "stop_clears_noise": false });
    let a = Gate::gate(&mock(), &input(features.clone()), &gate_spec()).await.unwrap();
    let b = Gate::gate(&mock(), &input(features), &gate_spec()).await.unwrap();
    assert_eq!(a, b);
}

#[tokio::test]
async fn the_mock_answers_questions_it_has_no_rule_for() {
    let out = Score::score(
        &mock(),
        &input(json!({})),
        &ScoreSpec::new("x", "q?", ["a", "b", "c"])
            .driver("something_unmodelled", "This is not in the rule set."),
    )
    .await
    .unwrap();
    assert!(out.score <= 100);
    assert_eq!(out.evidence.distribution.len(), 1);
}

#[tokio::test]
async fn the_mock_tailors_an_explain_summary_to_its_audience() {
    let features = json!({
        "intervention_count": 4.0,
        "escalation_count": 0.0,
        "decision_count": 20.0,
    });
    let spec = |audience: Audience| {
        ExplainSpec::new("the session", audience)
            .framing("quiet", "Nothing intervened.", "the layer stayed out of the way")
            .framing("intervened", "The gate changed outcomes.", "the gate changed what went on")
            .framing("escalated", "Something went to a human.", "a decision went to a human")
            .fact("pnl_effect", "Gating changed the P&L.", "gating moved the P&L")
            .fact("checks_ran", "Every check ran.", "every check ran on every candidate")
            .fact("audit_location", "The log exists.", "the record is in decisions.jsonl")
    };

    let mut with_audience = features.clone();
    with_audience["audience"] = json!("trader");
    let trader =
        Explain::explain(&mock(), &input(with_audience), &spec(Audience::Trader)).await.unwrap();

    let mut with_audience = features;
    with_audience["audience"] = json!("compliance");
    let compliance = Explain::explain(&mock(), &input(with_audience), &spec(Audience::Compliance))
        .await
        .unwrap();

    assert_ne!(trader.summary, compliance.summary);
    assert!(trader.summary.contains("P&L"), "{}", trader.summary);
    assert!(compliance.summary.contains("decisions.jsonl"), "{}", compliance.summary);
    assert!(!trader.summary.contains("decisions.jsonl"), "{}", trader.summary);
}
