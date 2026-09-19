//! The figures quoted in `README.md`.
//!
//! The worlds are deterministic in their seed, so these can be exact. If one of
//! them fails, the run changed and the README is now wrong: update both
//! together rather than relaxing the assertion.

use jev_core::{Jev, MockJev};
use std::sync::Arc;
use synth::{BatteryWorld, FxWorld, DEFAULT_SEED};

fn jev() -> Jev {
    Jev::new(Arc::new(MockJev::new()))
}

fn close(actual: f64, claimed: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - claimed).abs() <= tolerance,
        "README claims {what} = {claimed}, run produced {actual}"
    );
}

#[tokio::test]
async fn the_readmes_battery_table_is_what_the_run_produces() {
    let world = BatteryWorld::default_world(DEFAULT_SEED);
    let session = battery::run_session(&jev(), &world, None).await.unwrap();

    assert_eq!(session.solver_only.days_run, 90, "solver-only days run");
    assert_eq!(session.gated.days_run, 71, "gated days run");
    close(session.solver_only.margin, 11_311.0, 1.0, "solver-only margin");
    close(session.gated.margin, 7_155.0, 1.0, "gated margin");
    close(session.solver_only.reserve_payment, 87.0, 1.0, "solver-only reserve payment");
    close(session.gated.reserve_payment, 812.0, 1.0, "gated reserve payment");

    assert_eq!(session.reviews(), 33, "days flagged for review");

    assert_eq!(session.solver_only.reserve_breaches, 77, "solver-only breach hours");
    assert_eq!(session.solver_only.days_with_breach, 30, "solver-only breach days");
    assert_eq!(session.gated.reserve_breaches, 0, "the gated desk breaches nothing");
    assert_eq!(session.gated.days_with_breach, 0);

    close(session.solver_only.cycles, 127.6, 0.05, "solver-only cycles");
    close(session.gated.cycles, 71.1, 0.05, "gated cycles");
    close(session.solver_only.margin_per_cycle(), 88.7, 0.05, "solver-only margin per cycle");
    close(session.gated.margin_per_cycle(), 100.7, 0.05, "gated margin per cycle");

    // The prose claims around the table.
    assert!(
        session.gated.reserve_payment > 9.0 * session.solver_only.reserve_payment,
        "the README says nine times the capacity payment"
    );
    assert!(
        session.gated.cycles / session.solver_only.cycles > 0.5,
        "the README says a little over half the wear, not less than half"
    );
    assert!(
        session.gated.margin_per_cycle() > session.solver_only.margin_per_cycle(),
        "the README says it earns more per cycle of wear"
    );
}

#[tokio::test]
async fn the_readmes_forex_scorecard_is_what_the_run_produces() {
    let world = FxWorld::default_world(DEFAULT_SEED);
    let session =
        fx::run_session(&jev(), &world, &fx::StrategyParams::default(), None).await.unwrap();

    assert_eq!(session.decisions.len(), 214, "candidates");
    assert_eq!(session.reviews(), 16, "decisions flagged for review");
    close(session.ungated.return_bps(), 16.65, 0.01, "ungated return on notional");
    close(session.gated.return_bps(), 9.70, 0.01, "gated return on notional");

    let stop = session.score_check("stop_sane");
    assert_eq!(stop.failed_n, 73);
    assert_eq!(stop.passed_n, 141);
    close(stop.failed_bps, -3.7, 0.05, "stop_sane flagged mean");
    close(stop.passed_bps, 27.2, 0.05, "stop_sane passed mean");
    close(stop.edge_bps(), 30.9, 0.05, "stop_sane edge");
    assert!(!stop.inverted(), "stop_sane earns its keep");

    let signal = session.score_check("signal_valid_in_regime");
    assert_eq!(signal.failed_n, 42);
    close(signal.failed_bps, 64.1, 0.05, "signal_valid_in_regime flagged mean");
    close(signal.passed_bps, 5.1, 0.05, "signal_valid_in_regime passed mean");
    close(signal.edge_bps(), -59.1, 0.05, "signal_valid_in_regime edge");
    assert!(signal.inverted(), "the README's central finding");

    let exposure = session.score_check("correlated_exposure_ok");
    assert_eq!(exposure.failed_n, 21);
    close(exposure.edge_bps(), -12.0, 0.05, "correlated_exposure_ok edge");
}

#[tokio::test]
async fn the_readmes_claim_about_the_regime_detector_holds() {
    // "er >= 0.62 identifies the trending regime with 76% precision against a
    // 42% base rate", and trending really is the worst regime to mean-revert in.
    use fx::strategy::efficiency_ratio;
    let world = FxWorld::default_world(DEFAULT_SEED);
    let params = fx::StrategyParams::default();

    let mut flagged = 0usize;
    let mut flagged_trending = 0usize;
    let mut total = 0usize;
    let mut trending = 0usize;
    let mut by_regime: std::collections::BTreeMap<&str, (usize, f64)> = Default::default();

    for pair in synth::Pair::ALL {
        let series = world.series_for(pair);
        let bars = &series.bars;
        for c in fx::signals(bars, pair, &params) {
            let Some(fill) = fx::simulate(&c, bars, 1.0, &params) else { continue };
            let is_trending = series.true_regime(c.bar_index) == synth::Regime::Trending;
            total += 1;
            trending += usize::from(is_trending);
            if efficiency_ratio(bars, c.bar_index, 12).unwrap_or(0.0) >= 0.62 {
                flagged += 1;
                flagged_trending += usize::from(is_trending);
            }
            let entry = by_regime.entry(series.true_regime(c.bar_index).as_str()).or_default();
            entry.0 += 1;
            entry.1 += fill.pnl_bps;
        }
    }

    let precision = flagged_trending as f64 / flagged as f64;
    let base = trending as f64 / total as f64;
    close(precision * 100.0, 76.0, 1.0, "detector precision");
    close(base * 100.0, 42.0, 1.0, "trending base rate");
    assert!(precision > base, "the detector must beat the base rate");

    let mean = |name: &str| -> f64 {
        let (n, total) = by_regime[name];
        total / n as f64
    };
    close(mean("trending"), 8.5, 0.2, "mean outcome in the true trending regime");
    close(mean("ranging"), 19.1, 0.2, "mean outcome in the true ranging regime");
    assert!(
        mean("trending") < mean("ranging"),
        "the belief behind the rule checks out; it is the inference that does not"
    );
}

#[tokio::test]
async fn the_readmes_cost_line_is_what_the_run_produces() {
    // The mock's token counts are a function of the state it is sent, so a
    // seed pins the cost line the same way it pins the P&L. If this moves,
    // the README's `## Cost` block is now wrong.
    let jev = jev();
    let world = FxWorld::default_world(DEFAULT_SEED);
    let params = fx::StrategyParams::default();
    let session = fx::run_session(&jev, &world, &params, None).await.unwrap();
    report::fx_report::render(&jev, &world, &session, &params).await.unwrap();

    let ledger = jev.audit().ledger(jev_core::Rates::ASSUMED);
    let total = ledger.total();
    assert_eq!(total.calls, 430, "428 for the decisions, two Explain for the report");
    assert_eq!(total.tokens(), 648_242, "tokens across the run");

    assert_eq!(ledger.decisions(), 214, "every decision tagged, none merged");
    assert_eq!(ledger.untagged().calls, 2, "the Explain calls judge no one decision");

    close(total.usd, 2.29, 0.005, "estimated cost of a 90-day forex run");
    close(ledger.usd_per_decision(), 0.0106, 0.0001, "estimated cost per decision");
}
