//! Fills and P&L, on fixtures whose arithmetic is written out.

use fx::engine::{simulate, summarise, Exit, Fill};
use fx::strategy::{Side, StrategyParams, TradeCandidate};
use synth::{Bar, Pair, Stamp};

fn bar(i: u32, open: f64, high: f64, low: f64, close: f64) -> Bar {
    Bar { t: Stamp::new(i / 6, (i % 6) * 4), open, high, low, close }
}

/// A fixed candidate, stated outright rather than taken from the strategy:
/// long at 95, stop 92.625, target 99.3333, 100k units. The engine's job is to
/// fill whatever it is handed, so it is tested against numbers that do not move
/// when the solver's stop rule changes.
fn candidate() -> TradeCandidate {
    TradeCandidate {
        pair: Pair::EurUsd,
        side: Side::Long,
        t: Stamp::new(0, 12),
        bar_index: 3,
        price: 95.0,
        stop: 92.625,
        target: 298.0 / 3.0,
        size_units: 100.0,
    }
}

fn params() -> StrategyParams {
    StrategyParams { max_holding_bars: 3, ..StrategyParams::default() }
}

fn bars_with(next: Vec<Bar>) -> Vec<Bar> {
    let mut bars = vec![
        bar(0, 100.0, 100.5, 99.0, 100.0),
        bar(1, 100.0, 103.0, 101.0, 102.0),
        bar(2, 102.0, 102.5, 100.0, 101.0),
        bar(3, 101.0, 101.0, 94.0, 95.0),
    ];
    bars.extend(next);
    bars
}

const EPS: f64 = 1e-6;

#[test]
fn a_target_fill_pays_the_move_to_the_target() {
    let bars = bars_with(vec![bar(4, 95.0, 100.0, 96.0, 99.5)]);
    let fill = simulate(&candidate(), &bars, 1.0, &params()).unwrap();

    assert_eq!(fill.exit, Exit::Target);
    assert!((fill.exit_price - 298.0 / 3.0).abs() < EPS);
    // (99.3333 - 95) / 95 = 0.0456140...; on 100,000 that is 4561.40.
    assert!((fill.pnl - (298.0 / 3.0 - 95.0) / 95.0 * 100_000.0).abs() < EPS);
    assert!((fill.pnl - 4_561.403_508_771_93).abs() < 1e-6, "{}", fill.pnl);
    assert!((fill.pnl_bps - 456.140_350_877_193).abs() < 1e-6);
    assert!((fill.notional - 100_000.0).abs() < EPS);
}

#[test]
fn a_stop_fill_loses_the_move_to_the_stop() {
    let bars = bars_with(vec![bar(4, 95.0, 96.0, 92.0, 93.0)]);
    let fill = simulate(&candidate(), &bars, 1.0, &params()).unwrap();

    assert_eq!(fill.exit, Exit::Stop);
    assert!((fill.exit_price - 92.625).abs() < EPS);
    // (92.625 - 95) / 95 = -0.025 exactly; on 100,000 that is -2500.
    assert!((fill.pnl + 2_500.0).abs() < EPS, "{}", fill.pnl);
    assert!((fill.pnl_bps + 250.0).abs() < EPS);
}

#[test]
fn a_bar_touching_both_is_taken_as_a_stop() {
    // High 100 clears the target and low 92 clears the stop. The pessimistic
    // reading applies to both books, so the comparison stays fair.
    let bars = bars_with(vec![bar(4, 95.0, 100.0, 92.0, 97.0)]);
    let fill = simulate(&candidate(), &bars, 1.0, &params()).unwrap();
    assert_eq!(fill.exit, Exit::Stop);
    assert!((fill.pnl + 2_500.0).abs() < EPS);
}

#[test]
fn an_untouched_trade_times_out_at_the_close() {
    let bars = bars_with(vec![
        bar(4, 95.0, 96.0, 94.0, 95.5),
        bar(5, 95.5, 96.5, 94.5, 96.0),
        bar(6, 96.0, 97.0, 95.0, 96.5),
        bar(7, 96.5, 97.0, 95.5, 96.8),
    ]);
    // max_holding_bars is 3, so the deadline is bar 6, closing at 96.5.
    let fill = simulate(&candidate(), &bars, 1.0, &params()).unwrap();
    assert_eq!(fill.exit, Exit::Timeout);
    assert!((fill.exit_price - 96.5).abs() < EPS);
    assert_eq!(fill.exited, bars[6].t);
    assert!((fill.pnl - (96.5 - 95.0) / 95.0 * 100_000.0).abs() < EPS);
}

#[test]
fn a_trade_still_open_when_the_data_runs_out_is_marked_as_such() {
    let bars = bars_with(vec![bar(4, 95.0, 96.0, 94.0, 95.5)]);
    let fill = simulate(&candidate(), &bars, 1.0, &params()).unwrap();
    assert_eq!(fill.exit, Exit::EndOfData);
    assert!((fill.exit_price - 95.5).abs() < EPS);
}

#[test]
fn a_short_is_the_mirror_image() {
    let short = TradeCandidate {
        side: Side::Short,
        price: 105.0,
        stop: 107.375,
        target: 100.0,
        ..candidate()
    };
    let bars = bars_with(vec![bar(4, 105.0, 106.0, 99.0, 100.5)]);
    let fill = simulate(&short, &bars, 1.0, &params()).unwrap();

    assert_eq!(fill.exit, Exit::Target);
    // A short makes money when price falls: (105 - 100) / 105.
    assert!((fill.pnl - (105.0 - 100.0) / 105.0 * 100_000.0).abs() < EPS, "{}", fill.pnl);
    assert!(fill.pnl > 0.0);
}

#[test]
fn size_factor_scales_the_notional_and_the_pnl_but_not_the_bps() {
    let bars = bars_with(vec![bar(4, 95.0, 100.0, 96.0, 99.5)]);
    let full = simulate(&candidate(), &bars, 1.0, &params()).unwrap();
    let half = simulate(&candidate(), &bars, 0.5, &params()).unwrap();

    assert!((half.pnl - full.pnl / 2.0).abs() < EPS);
    assert!((half.notional - full.notional / 2.0).abs() < EPS);
    assert!(
        (half.pnl_bps - full.pnl_bps).abs() < EPS,
        "basis points are size-independent, which is what makes the books comparable"
    );
}

#[test]
fn a_gated_out_trade_produces_no_fill() {
    let bars = bars_with(vec![bar(4, 95.0, 100.0, 96.0, 99.5)]);
    assert_eq!(simulate(&candidate(), &bars, 0.0, &params()), None);
    assert_eq!(simulate(&candidate(), &bars, -0.5, &params()), None);
}

// ---- aggregation ---------------------------------------------------------------------------

fn fill_of(pnl: f64) -> Fill {
    Fill {
        entered: Stamp::new(0, 0),
        exited: Stamp::new(0, 4),
        exit: Exit::Target,
        entry_price: 100.0,
        exit_price: 100.0,
        size_factor: 1.0,
        notional: 100_000.0,
        pnl,
        pnl_bps: pnl / 10.0,
    }
}

#[test]
fn summarise_counts_wins_losses_and_total() {
    let result = summarise([100.0, -40.0, 60.0, -10.0].into_iter().map(fill_of));
    assert_eq!(result.trades, 4);
    assert_eq!(result.wins, 2);
    assert_eq!(result.losses, 2);
    assert!((result.pnl - 110.0).abs() < EPS);
    assert!((result.hit_rate() - 0.5).abs() < EPS);
    assert!((result.notional - 400_000.0).abs() < EPS);
    // 110 on 400,000 of notional is 2.75 bps.
    assert!((result.return_bps() - 2.75).abs() < EPS);
}

#[test]
fn summarise_measures_drawdown_peak_to_trough() {
    // Cumulative: 100, 60, 130, 30, 80. Peaks 100 then 130; the deepest fall is
    // 130 down to 30.
    let result = summarise([100.0, -40.0, 70.0, -100.0, 50.0].into_iter().map(fill_of));
    assert!((result.max_drawdown - 100.0).abs() < EPS, "{}", result.max_drawdown);
}

#[test]
fn a_book_that_only_makes_money_has_no_drawdown() {
    let result = summarise([10.0, 20.0, 30.0].into_iter().map(fill_of));
    assert!((result.max_drawdown - 0.0).abs() < EPS);
}

#[test]
fn an_empty_book_is_all_zeroes() {
    let result = summarise(std::iter::empty());
    assert_eq!(result.trades, 0);
    assert!((result.hit_rate() - 0.0).abs() < EPS);
    assert!((result.return_bps() - 0.0).abs() < EPS);
}
