//! A seed pins a world exactly, and the worlds have the shape the demo needs.

use synth::{BatteryWorld, Date, EventKind, FxWorld, Pair, Regime, Stamp, DEFAULT_SEED};

// ---- determinism ---------------------------------------------------------------------------

#[test]
fn the_same_seed_gives_an_identical_fx_world() {
    let a = FxWorld::generate(DEFAULT_SEED, 90);
    let b = FxWorld::generate(DEFAULT_SEED, 90);
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap(),
        "the same seed must give byte-identical output"
    );
}

#[test]
fn the_same_seed_gives_an_identical_battery_world() {
    let a = BatteryWorld::generate(DEFAULT_SEED, 90);
    let b = BatteryWorld::generate(DEFAULT_SEED, 90);
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

#[test]
fn a_different_seed_gives_a_different_world() {
    let a = FxWorld::generate(1, 30);
    let b = FxWorld::generate(2, 30);
    assert_ne!(a.series_for(Pair::EurUsd).bars, b.series_for(Pair::EurUsd).bars);

    let a = BatteryWorld::generate(1, 30);
    let b = BatteryWorld::generate(2, 30);
    assert_ne!(a.day_ahead, b.day_ahead);
}

#[test]
fn streams_are_independent_so_one_concern_does_not_shift_another() {
    // Two worlds of different lengths share a prefix of prices, because each
    // concern draws from its own stream and prices are generated bar by bar.
    let short = FxWorld::generate(7, 20);
    let long = FxWorld::generate(7, 90);
    let s = &short.series_for(Pair::EurUsd).bars;
    let l = &long.series_for(Pair::EurUsd).bars;
    assert_eq!(&s[..s.len().min(60)], &l[..s.len().min(60)]);
}

// ---- forex shape ---------------------------------------------------------------------------

#[test]
fn fx_generates_six_four_hour_bars_a_day_for_every_pair() {
    let w = FxWorld::generate(DEFAULT_SEED, 90);
    assert_eq!(w.series.len(), 3);
    for s in &w.series {
        assert_eq!(s.bars.len(), 90 * 6, "{}", s.pair);
        assert_eq!(s.truth.len(), s.bars.len(), "truth is parallel to the bars");
        for (i, bar) in s.bars.iter().enumerate() {
            assert_eq!(bar.t, Stamp::new(i as u32 / 6, (i as u32 % 6) * 4));
        }
    }
}

#[test]
fn fx_candles_are_internally_consistent() {
    let w = FxWorld::generate(DEFAULT_SEED, 90);
    for s in &w.series {
        for bar in &s.bars {
            assert!(bar.high >= bar.open.max(bar.close), "{:?}", bar);
            assert!(bar.low <= bar.open.min(bar.close), "{:?}", bar);
            assert!(bar.low > 0.0, "prices stay positive: {:?}", bar);
        }
    }
}

#[test]
fn fx_visits_every_regime() {
    let w = FxWorld::generate(DEFAULT_SEED, 90);
    let truth = &w.series_for(Pair::EurUsd).truth;
    for regime in Regime::ALL {
        assert!(truth.contains(&regime), "{regime:?} never occurred");
    }
}

#[test]
fn fx_regimes_persist_rather_than_flickering_bar_to_bar() {
    let w = FxWorld::generate(DEFAULT_SEED, 90);
    let truth = &w.series_for(Pair::EurUsd).truth;
    let switches = truth.windows(2).filter(|p| p[0] != p[1]).count();
    // 540 bars: a few dozen switches is a regime, hundreds is noise.
    assert!(switches < truth.len() / 6, "{switches} switches in {} bars", truth.len());
}

#[test]
fn fx_calendar_lands_on_weekdays_and_covers_every_release() {
    let w = FxWorld::generate(DEFAULT_SEED, 90);
    assert!(!w.calendar.is_empty());
    for e in &w.calendar {
        let date = w.start.plus_days(e.t.day as i64);
        assert!(!date.is_weekend(), "{:?} landed on a weekend", e);
    }
    for kind in [EventKind::Cpi, EventKind::Nfp, EventKind::Ecb, EventKind::Fomc] {
        assert!(
            w.calendar.iter().any(|e| e.kind == kind),
            "{kind:?} never appears in 90 days"
        );
    }
}

#[test]
fn fx_event_bars_are_wider_than_quiet_bars() {
    let w = FxWorld::generate(DEFAULT_SEED, 90);
    let s = w.series_for(Pair::EurUsd);
    let range = |i: usize| (s.bars[i].high - s.bars[i].low) / s.bars[i].open;

    let mut event_ranges = Vec::new();
    let mut quiet_ranges = Vec::new();
    for i in 0..s.bars.len() {
        let t = s.bars[i].t;
        let has_event = w.calendar.iter().any(|e| {
            e.t.day == t.day
                && e.t.hour >= t.hour
                && e.t.hour < t.hour + 4
                && (e.currency == Pair::EurUsd.base() || e.currency == Pair::EurUsd.quote())
        });
        if has_event {
            event_ranges.push(range(i));
        } else {
            quiet_ranges.push(range(i));
        }
    }
    assert!(!event_ranges.is_empty());
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    assert!(
        mean(&event_ranges) > mean(&quiet_ranges) * 1.5,
        "event bars {:.5} vs quiet {:.5}",
        mean(&event_ranges),
        mean(&quiet_ranges)
    );
}

#[test]
fn fx_headlines_are_mostly_noise() {
    let w = FxWorld::generate(DEFAULT_SEED, 90);
    let informative = w.headlines.iter().filter(|h| h.informative).count();
    let ratio = informative as f64 / w.headlines.len() as f64;
    assert!(ratio < 0.2, "{ratio:.2} of headlines were informative");
    assert!(informative > 0, "some headline should carry information");
}

#[test]
fn fx_book_carries_every_currency() {
    let w = FxWorld::generate(DEFAULT_SEED, 90);
    assert_eq!(w.book.len(), 4);
}

// ---- battery shape -------------------------------------------------------------------------

#[test]
fn battery_generates_hourly_day_ahead_and_a_tick_level_intraday() {
    let w = BatteryWorld::generate(DEFAULT_SEED, 90);
    assert_eq!(w.day_ahead.len(), 90 * 24);
    assert_eq!(w.intraday.len(), 90 * 24 * synth::TICKS_PER_HOUR as usize);
    assert_eq!(w.day_ahead_for(3).len(), 24);
    assert_eq!(w.intraday_ticks(3, 18).len(), synth::TICKS_PER_HOUR as usize);
}

#[test]
fn battery_prices_peak_in_the_evening() {
    let w = BatteryWorld::generate(DEFAULT_SEED, 90);
    let hour_mean = |hour: usize| -> f64 {
        (0..90).map(|d| w.day_ahead[d * 24 + hour]).sum::<f64>() / 90.0
    };
    let evening = hour_mean(19);
    let night = hour_mean(3);
    let midday = hour_mean(13);
    assert!(evening > night, "evening {evening:.1} vs night {night:.1}");
    assert!(evening > midday, "evening {evening:.1} vs midday {midday:.1}");
}

#[test]
fn battery_has_a_weekly_seasonality() {
    let w = BatteryWorld::generate(DEFAULT_SEED, 90);
    let mean_for = |weekend: bool| -> f64 {
        let days: Vec<u32> = (0..90)
            .filter(|d| w.start.plus_days(*d as i64).is_weekend() == weekend)
            .collect();
        let total: f64 =
            days.iter().map(|d| w.day_ahead_for(*d).iter().sum::<f64>() / 24.0).sum();
        total / days.len() as f64
    };
    assert!(mean_for(true) < mean_for(false), "weekends should clear lower");
}

#[test]
fn battery_has_negative_hours_and_evening_spikes() {
    let w = BatteryWorld::generate(DEFAULT_SEED, 90);
    let negative = w.day_ahead.iter().filter(|p| **p < 0.0).count();
    assert!(negative > 0, "no negative-price hours in 90 days");
    assert!(negative < w.day_ahead.len() / 10, "{negative} negative hours is too many");

    let spikes = (0..90)
        .flat_map(|d| (17..=20).map(move |h| (d, h)))
        .filter(|(d, h)| w.day_ahead[(d * 24 + h) as usize] > 250.0)
        .count();
    assert!(spikes > 0, "no evening spikes in 90 days");
}

#[test]
fn battery_intraday_deviates_from_day_ahead_without_drifting_away() {
    let w = BatteryWorld::generate(DEFAULT_SEED, 90);
    let deviations: Vec<f64> = (0..90)
        .flat_map(|d| (0..24).map(move |h| (d, h)))
        .map(|(d, h)| w.intraday_hour(d, h) - w.day_ahead[(d * 24 + h) as usize])
        .collect();
    let mean = deviations.iter().sum::<f64>() / deviations.len() as f64;
    let max = deviations.iter().copied().fold(0.0_f64, |a, b| a.max(b.abs()));
    assert!(mean.abs() < 3.0, "intraday should not drift off day-ahead: mean {mean:.2}");
    assert!(max > 15.0, "intraday should sometimes dislocate: max {max:.2}");
}

#[test]
fn battery_has_reserve_windows_on_some_days_but_not_all() {
    let w = BatteryWorld::generate(DEFAULT_SEED, 90);
    assert!(!w.afrr.is_empty());
    assert!(w.afrr.len() < 90, "a reserve obligation every day is not 'some days'");
    for window in &w.afrr {
        assert!(window.from_hour <= window.to_hour);
        assert!(window.to_hour <= 23);
        assert!(window.hours() >= 2);
    }
}

#[test]
fn battery_has_grid_notes_and_a_contested_day_for_the_demo() {
    let w = BatteryWorld::generate(DEFAULT_SEED, 90);
    assert!(!w.grid_notes.is_empty());
    assert!(
        w.first_contested_day().is_some(),
        "the demo needs a day with both a reserve window and a grid note"
    );
    assert!(w.first_quiet_day(0).is_some());
}

#[test]
fn battery_spec_is_the_asset_the_brief_describes() {
    let w = BatteryWorld::generate(DEFAULT_SEED, 90);
    assert_eq!(w.asset.power_mw, 1.0);
    assert_eq!(w.asset.capacity_mwh, 2.0);
    assert!(w.asset.round_trip_efficiency < 1.0);
    assert!(w.asset.degradation_cost_per_cycle > 0.0);
    assert!(w.asset.soc_min < w.asset.soc_start && w.asset.soc_start < w.asset.soc_max);
    let one_way = w.asset.one_way_efficiency();
    assert!((one_way * one_way - w.asset.round_trip_efficiency).abs() < 1e-12);
}

// ---- dates ---------------------------------------------------------------------------------

#[test]
fn civil_dates_round_trip() {
    for days in [-20000_i64, -1, 0, 1, 19000, 20000, 25000] {
        assert_eq!(Date::from_days(days).to_days(), days);
    }
    assert_eq!(Date::new(1970, 1, 1).to_days(), 0);
    assert_eq!(Date::new(2000, 3, 1).to_days(), 11017);
    assert_eq!(Date::new(2025, 1, 6).weekday(), 0, "the world starts on a Monday");
    assert!(Date::new(2025, 1, 11).is_weekend());
    assert_eq!(Date::new(2024, 2, 29).plus_days(1), Date::new(2024, 3, 1));
}

#[test]
fn stamps_index_and_compare() {
    assert_eq!(Stamp::new(2, 6).index(), 54);
    assert_eq!(Stamp::from_index(54), Stamp::new(2, 6));
    assert_eq!(Stamp::new(2, 6).hours_to(Stamp::new(2, 9)), 3.0);
    assert_eq!(Stamp::new(2, 9).hours_to(Stamp::new(2, 6)), -3.0);
}

#[test]
fn fx_calendar_holds_at_most_one_release_of_each_kind_per_month() {
    let w = FxWorld::generate(DEFAULT_SEED, 90);
    let mut seen: Vec<(i32, u32, EventKind)> = Vec::new();
    for e in &w.calendar {
        let date = w.start.plus_days(e.t.day as i64);
        let key = (date.year, date.month, e.kind);
        assert!(!seen.contains(&key), "two {:?} in {}-{:02}", e.kind, date.year, date.month);
        seen.push(key);
    }
    // Three months of data should carry roughly one of each release per month.
    assert!(w.calendar.len() >= 8, "only {} events in 90 days", w.calendar.len());
}

#[test]
fn battery_deviation_persists_from_one_day_to_the_next() {
    // The "is this margin still believable" judgment needs yesterday's
    // deviation to say something about today's. Pure hourly noise would not.
    let w = BatteryWorld::generate(DEFAULT_SEED, 90);
    let means: Vec<f64> = (0..90).map(|d| w.mean_deviation_on(d)).collect();
    let mean = means.iter().sum::<f64>() / means.len() as f64;

    let mut covariance = 0.0;
    let mut variance = 0.0;
    for pair in means.windows(2) {
        covariance += (pair[0] - mean) * (pair[1] - mean);
    }
    for m in &means {
        variance += (m - mean).powi(2);
    }
    let autocorrelation = covariance / variance;
    assert!(
        autocorrelation > 0.3,
        "day-to-day deviation autocorrelation is only {autocorrelation:.2}"
    );
}

#[test]
fn battery_deviation_sigma_is_positive_and_finite() {
    let w = BatteryWorld::generate(DEFAULT_SEED, 90);
    for day in [1u32, 5, 30, 89] {
        let sigma = w.recent_deviation_sigma(day, 10);
        assert!(sigma > 0.0 && sigma.is_finite(), "day {day}: {sigma}");
    }
    // With no history at all it falls back rather than dividing by zero.
    assert!(w.recent_deviation_sigma(0, 10) > 0.0);
}
