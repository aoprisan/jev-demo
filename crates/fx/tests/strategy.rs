//! The solver, on a fixture small enough to compute by hand.
//!
//! The arithmetic in the comments is the expectation; if an indicator's
//! definition drifts, these fail rather than quietly re-baselining.

use fx::strategy::{
    atr, efficiency_ratio, signal_at, sma, stdev, true_range, Side, StrategyParams,
};
use synth::{Bar, Pair, Stamp};

fn bar(i: u32, open: f64, high: f64, low: f64, close: f64) -> Bar {
    Bar { t: Stamp::new(i / 6, (i % 6) * 4), open, high, low, close }
}

/// closes: 100, 102, 101, 95
fn fixture() -> Vec<Bar> {
    vec![
        bar(0, 100.0, 100.5, 99.0, 100.0),
        bar(1, 100.0, 103.0, 101.0, 102.0),
        bar(2, 102.0, 102.5, 100.0, 101.0),
        bar(3, 101.0, 101.0, 94.0, 95.0),
    ]
}

/// period 3, k 1, atr 2, swing 2 — small enough to check on paper.
fn params() -> StrategyParams {
    params_with(1.5, 0.5)
}

/// The same, with the two stop terms varied so each can be made to bind.
fn params_with(stop_sd: f64, min_stop_atr: f64) -> StrategyParams {
    StrategyParams {
        period: 3,
        k: 1.0,
        atr_period: 2,
        swing_lookback: 2,
        stop_sd,
        min_stop_atr,
        size_units: 100.0,
        max_holding_bars: 12,
    }
}

const EPS: f64 = 1e-9;

#[test]
fn sma_is_the_mean_of_the_last_period_closes() {
    let bars = fixture();
    // (102 + 101 + 95) / 3 = 99.333...
    assert!((sma(&bars, 3, 3).unwrap() - 298.0 / 3.0).abs() < EPS);
    // (100 + 102 + 101) / 3 = 101
    assert!((sma(&bars, 2, 3).unwrap() - 101.0).abs() < EPS);
    assert_eq!(sma(&bars, 1, 3), None, "not enough history");
}

#[test]
fn stdev_is_the_population_standard_deviation() {
    let bars = fixture();
    // mean 99.3333; deviations 8/3, 5/3, -13/3.
    // squares (64 + 25 + 169) / 9 = 258/9; variance = that / 3 = 86/9.
    let expected = (86.0_f64 / 9.0).sqrt();
    assert!((stdev(&bars, 3, 3).unwrap() - expected).abs() < 1e-12, "{expected}");
    assert!((expected - 3.091_206_165_165_235_5).abs() < 1e-12, "{expected:.16}");
}

#[test]
fn true_range_takes_the_widest_of_the_three_measures() {
    let bars = fixture();
    // bar 2: high-low = 2.5, |high - prev close| = 0.5, |low - prev close| = 2.0
    assert!((true_range(&bars, 2).unwrap() - 2.5).abs() < EPS);
    // bar 3: high-low = 7, |101 - 101| = 0, |94 - 101| = 7
    assert!((true_range(&bars, 3).unwrap() - 7.0).abs() < EPS);
    assert_eq!(true_range(&bars, 0), None, "the first bar has no previous close");
}

#[test]
fn atr_is_the_simple_mean_of_true_ranges() {
    let bars = fixture();
    // (2.5 + 7) / 2 = 4.75
    assert!((atr(&bars, 3, 2).unwrap() - 4.75).abs() < EPS);
    assert_eq!(atr(&bars, 1, 2), None);
}

#[test]
fn efficiency_ratio_is_net_over_gross_movement() {
    let straight = vec![
        bar(0, 100.0, 100.0, 100.0, 100.0),
        bar(1, 101.0, 101.0, 101.0, 101.0),
        bar(2, 102.0, 102.0, 102.0, 102.0),
        bar(3, 103.0, 103.0, 103.0, 103.0),
    ];
    // net 3, gross 3 → a perfectly efficient trend.
    assert!((efficiency_ratio(&straight, 3, 3).unwrap() - 1.0).abs() < EPS);

    let choppy = vec![
        bar(0, 100.0, 100.0, 100.0, 100.0),
        bar(1, 101.0, 101.0, 101.0, 101.0),
        bar(2, 100.0, 100.0, 100.0, 100.0),
        bar(3, 101.0, 101.0, 101.0, 101.0),
    ];
    // net 1, gross 3.
    assert!((efficiency_ratio(&choppy, 3, 3).unwrap() - 1.0 / 3.0).abs() < EPS);

    let flat = vec![bar(0, 100.0, 100.0, 100.0, 100.0); 4];
    assert!((efficiency_ratio(&flat, 3, 3).unwrap() - 0.0).abs() < EPS);
}

#[test]
fn a_close_below_the_lower_band_is_a_long_with_a_hand_checkable_stop_and_target() {
    let bars = fixture();
    let p = params();
    let c = signal_at(&bars, 3, Pair::EurUsd, &p).expect("95 is below the lower band");

    assert_eq!(c.side, Side::Long);
    assert_eq!(c.pair, Pair::EurUsd);
    assert_eq!(c.bar_index, 3);
    assert!((c.price - 95.0).abs() < EPS, "entry is the signal bar's close");

    // mid 99.3333, sd 3.0912, k 1 → lower band 96.2421; close 95 is below it.
    assert!((c.target - 298.0 / 3.0).abs() < EPS, "target is the middle band");

    // The stop is the furthest of three claims:
    //   dispersion  1.5 * 3.091206 = 4.636809
    //   swing       95 - 94        = 1.0
    //   ATR floor   0.5 * 4.75     = 2.375
    // Dispersion wins, so the stop is 95 - 4.636809 = 90.363191.
    let dispersion = 1.5 * (86.0_f64 / 9.0).sqrt();
    assert!((c.stop_distance() - dispersion).abs() < 1e-12, "got {}", c.stop_distance());
    assert!((c.stop - (95.0 - dispersion)).abs() < 1e-12, "got {}", c.stop);
    assert!((c.target_distance() - (298.0 / 3.0 - 95.0)).abs() < EPS);
    assert!((c.reward_risk() - (298.0 / 3.0 - 95.0) / dispersion).abs() < 1e-12);
    assert!((c.notional() - 100_000.0).abs() < EPS);
}

#[test]
fn a_close_above_the_upper_band_is_a_short() {
    let mut bars = fixture();
    // Mirror the fixture upward: closes 100, 98, 99, 105.
    bars[1].close = 98.0;
    bars[2].close = 99.0;
    bars[3] = bar(3, 99.0, 106.0, 99.0, 105.0);
    let c = signal_at(&bars, 3, Pair::UsdJpy, &params()).expect("105 is above the upper band");

    assert_eq!(c.side, Side::Short);
    assert!(c.stop > c.price, "a short's stop sits above entry");
    assert!(c.target < c.price, "a short targets the middle band below");
}

#[test]
fn a_close_inside_the_bands_is_not_a_signal() {
    let mut bars = fixture();
    // closes 100, 102, 101, 101: mean 101.3333, sd 0.4714, so the band is
    // roughly 100.86..101.80 and the close sits inside it.
    bars[3] = bar(3, 101.0, 101.5, 100.5, 101.0);
    assert_eq!(signal_at(&bars, 3, Pair::EurUsd, &params()), None);

    // Dropping the same close to 100.5 puts it just below the band
    // (mean 101.1667, sd 0.6236, lower 100.543) and it becomes a long.
    // The two cases together pin where the edge is.
    bars[3] = bar(3, 101.0, 101.5, 100.0, 100.5);
    let edge = signal_at(&bars, 3, Pair::EurUsd, &params()).expect("100.5 is outside");
    assert_eq!(edge.side, Side::Long);
}

#[test]
fn no_signal_before_the_indicators_are_defined() {
    let bars = fixture();
    let p = params();
    assert_eq!(p.warmup(), 3);
    for i in 0..p.warmup() {
        assert_eq!(signal_at(&bars, i, Pair::EurUsd, &p), None, "bar {i}");
    }
}

#[test]
fn the_atr_floor_binds_when_dispersion_and_swing_are_both_small() {
    // stop_sd 0.1 → 0.309; swing → 1.0; ATR floor 0.5 * 4.75 → 2.375.
    let bars = fixture();
    let c = signal_at(&bars, 3, Pair::EurUsd, &params_with(0.1, 0.5)).unwrap();
    assert!((c.stop_distance() - 2.375).abs() < EPS, "got {}", c.stop_distance());
    assert!((c.stop - 92.625).abs() < EPS);
}

#[test]
fn the_swing_binds_when_it_is_the_furthest_of_the_three() {
    // stop_sd 0.2 → 0.618; ATR floor 0.1 * 4.75 → 0.475; swing → 1.0.
    let bars = fixture();
    let c = signal_at(&bars, 3, Pair::EurUsd, &params_with(0.2, 0.1)).unwrap();
    assert!((c.stop_distance() - 1.0).abs() < EPS, "got {}", c.stop_distance());
    assert!((c.stop - 94.0).abs() < EPS, "the stop sits at the swing low");
}

#[test]
fn the_stop_width_in_atrs_varies_across_real_signals() {
    // The point of sizing the stop off dispersion rather than off ATR: if every
    // stop were the same multiple of ATR, the `stop_sane` judgment would be a
    // constant and the demo would show nothing.
    let world = synth::FxWorld::generate(synth::DEFAULT_SEED, 90);
    let p = StrategyParams::default();
    let bars = &world.series_for(Pair::EurUsd).bars;
    let widths: Vec<f64> = fx::signals(bars, Pair::EurUsd, &p)
        .iter()
        .map(|c| c.stop_distance() / atr(bars, c.bar_index, p.atr_period).unwrap())
        .collect();

    assert!(widths.len() > 20);
    let min = widths.iter().copied().fold(f64::MAX, f64::min);
    let max = widths.iter().copied().fold(f64::MIN, f64::max);
    assert!(max - min > 1.0, "stop widths span only {min:.2}..{max:.2} ATRs");
    assert!(
        widths.iter().any(|w| *w < 1.2) && widths.iter().any(|w| *w >= 1.2),
        "stops should fall on both sides of the sanity threshold"
    );
}

#[test]
fn signals_scan_the_whole_series_in_order() {
    let world = synth::FxWorld::generate(synth::DEFAULT_SEED, 90);
    let series = world.series_for(Pair::EurUsd);
    let found = fx::signals(&series.bars, Pair::EurUsd, &StrategyParams::default());
    assert!(!found.is_empty(), "the default strategy should fire on 90 days");
    for pair in found.windows(2) {
        assert!(pair[0].bar_index < pair[1].bar_index, "signals come out in order");
    }
    for c in &found {
        assert!(c.stop_distance() > 0.0);
        match c.side {
            Side::Long => assert!(c.stop < c.price && c.target > c.price),
            Side::Short => assert!(c.stop > c.price && c.target < c.price),
        }
    }
}
