//! The dynamic program, on fixtures whose optimum is worked out in the comments.
//!
//! The test asset has a round-trip efficiency of exactly 1 so the arithmetic is
//! checkable on paper; efficiency losses are covered separately.

use battery::solver::{candidates, solve, ScheduleKind, SOC_STEP};
use synth::{AfrrWindow, BatterySpec};

/// 1 MW / 2 MWh, lossless, wear 40 per cycle, usable between 0.5 and 1.5 MWh,
/// starting at 1.0 MWh.
fn asset() -> BatterySpec {
    BatterySpec {
        power_mw: 1.0,
        capacity_mwh: 2.0,
        round_trip_efficiency: 1.0,
        degradation_cost_per_cycle: 40.0,
        soc_min: 0.25,
        soc_max: 0.75,
        soc_start: 0.50,
        daily_cycle_budget: 1.6,
    }
}

const EPS: f64 = 1e-9;

fn charged(power: &[f64]) -> f64 {
    power.iter().filter(|p| **p > 0.0).sum()
}

fn discharged(power: &[f64]) -> f64 {
    power.iter().filter(|p| **p < 0.0).map(|p| -p).sum()
}

#[test]
fn a_flat_curve_is_not_worth_trading() {
    // Every round trip pays the spread (zero) and the wear (positive), so the
    // best plan is to do nothing at all.
    let prices = vec![50.0; 24];
    let s = solve(ScheduleKind::Balanced, &prices, &asset(), None);

    assert!(s.power_mw.iter().all(|p| p.abs() < EPS), "{:?}", s.power_mw);
    assert!(s.soc_mwh.iter().all(|e| (e - 1.0).abs() < EPS));
    assert!((s.cycles - 0.0).abs() < EPS);
    assert!((s.expected_margin - 0.0).abs() < EPS);
}

#[test]
fn one_expensive_hour_produces_one_hand_checkable_round_trip() {
    // Flat 50 with a single 100 at hour 20.
    //
    // Per stored MWh a round trip earns 100 - 50 and costs wear of
    // 2 * 40 / (2 * 2) = 20, so it is worth 30 and the plan takes it.
    // Trading 50 against 50 costs 20 and is not taken.
    //
    // Starting at 1.0 with a ceiling of 1.5 and a floor of 0.5:
    //   charge   +0.5 MWh somewhere at 50  ->  -25
    //   discharge -1.0 MWh at hour 20      -> +100
    //   throughput 1.5 MWh = 0.375 cycles  ->   15 of wear
    //   margin = 75 - 15 = 60
    let mut prices = vec![50.0; 24];
    prices[20] = 100.0;
    let s = solve(ScheduleKind::Balanced, &prices, &asset(), None);

    assert!((s.power_mw[20] + 1.0).abs() < EPS, "hour 20 discharges: {:?}", s.power_mw);
    assert!((charged(&s.power_mw) - 0.5).abs() < EPS, "charged {}", charged(&s.power_mw));
    assert!((discharged(&s.power_mw) - 1.0).abs() < EPS);
    assert!((s.soc_mwh[24] - 0.5).abs() < EPS, "ends at the floor");
    assert!((s.soc_mwh.iter().copied().fold(f64::MIN, f64::max) - 1.5).abs() < EPS);
    assert!((s.cycles - 0.375).abs() < EPS, "cycles {}", s.cycles);
    assert!((s.degradation_cost - 15.0).abs() < EPS);
    assert!((s.expected_margin - 60.0).abs() < 1e-9, "margin {}", s.expected_margin);
}

#[test]
fn wear_that_exceeds_the_spread_blocks_the_trade() {
    // The same day, with wear raised to 400 per cycle: the round trip now costs
    // 2 * 400 / 4 = 200 per stored MWh against a spread of 50, so nothing runs.
    let mut prices = vec![50.0; 24];
    prices[20] = 100.0;
    let expensive = BatterySpec { degradation_cost_per_cycle: 400.0, ..asset() };
    let s = solve(ScheduleKind::Balanced, &prices, &expensive, None);
    assert!(s.power_mw.iter().all(|p| p.abs() < EPS), "{:?}", s.power_mw);
}

#[test]
fn the_state_of_charge_always_follows_from_the_power() {
    let world = synth::BatteryWorld::default_world(synth::DEFAULT_SEED);
    let efficiency = world.asset.one_way_efficiency();
    for day in [0u32, 7, 33, 61, 89] {
        for s in candidates(world.day_ahead_for(day), &world.asset, world.afrr_on(day)) {
            let mut energy = s.soc_mwh[0];
            for hour in 0..24 {
                let grid = s.power_mw[hour];
                energy += if grid > 0.0 { grid * efficiency } else { grid / efficiency };
                assert!(
                    (energy - s.soc_mwh[hour + 1]).abs() < 1e-6,
                    "day {day} {} hour {hour}: {energy} vs {}",
                    s.kind,
                    s.soc_mwh[hour + 1]
                );
            }
        }
    }
}

#[test]
fn every_variant_stays_inside_its_own_band_and_the_assets_envelope() {
    let world = synth::BatteryWorld::default_world(synth::DEFAULT_SEED);
    let asset = &world.asset;
    // Band floors, as fractions of capacity, matching ScheduleKind::band.
    let floors = [
        (ScheduleKind::Aggressive, 0.10),
        (ScheduleKind::Balanced, 0.15),
        (ScheduleKind::ReserveHeavy, 0.25),
    ];
    for day in 0..world.days {
        for s in candidates(world.day_ahead_for(day), asset, world.afrr_on(day)) {
            let floor = floors.iter().find(|(k, _)| *k == s.kind).unwrap().1;
            let low = (floor * asset.capacity_mwh).max(asset.min_energy());
            for (hour, energy) in s.soc_mwh.iter().enumerate() {
                assert!(
                    *energy >= low - 1e-6 && *energy <= asset.max_energy() + 1e-6,
                    "day {day} {} hour {hour}: {energy} outside {low}..{}",
                    s.kind,
                    asset.max_energy()
                );
            }
        }
    }
}

#[test]
fn power_never_exceeds_the_assets_rating() {
    let world = synth::BatteryWorld::default_world(synth::DEFAULT_SEED);
    for day in 0..world.days {
        for s in candidates(world.day_ahead_for(day), &world.asset, world.afrr_on(day)) {
            for (hour, p) in s.power_mw.iter().enumerate() {
                assert!(
                    p.abs() <= world.asset.power_mw + SOC_STEP,
                    "day {day} {} hour {hour}: {p} MW",
                    s.kind
                );
            }
        }
    }
}

#[test]
fn reserve_heavy_holds_the_reserve_and_the_others_need_not() {
    let world = synth::BatteryWorld::default_world(synth::DEFAULT_SEED);
    let asset = &world.asset;
    let mut reserve_days = 0;
    let mut balanced_dipped = 0;

    for day in 0..world.days {
        let Some(window) = world.afrr_on(day) else { continue };
        reserve_days += 1;
        let floor = window.reserve_soc * asset.capacity_mwh;
        let schedules = candidates(world.day_ahead_for(day), asset, Some(window));

        let heavy = schedules.iter().find(|s| s.kind == ScheduleKind::ReserveHeavy).unwrap();
        assert!(
            !heavy.dips_below(window, floor),
            "day {day}: the reserve-heavy schedule must hold the reserve"
        );

        let balanced = schedules.iter().find(|s| s.kind == ScheduleKind::Balanced).unwrap();
        if balanced.dips_below(window, floor) {
            balanced_dipped += 1;
        }
    }

    assert!(reserve_days > 10, "only {reserve_days} reserve days to check");
    assert!(
        balanced_dipped > reserve_days / 2,
        "the balanced schedule breached on only {balanced_dipped} of {reserve_days} days; \
         if it never breached there would be nothing for the judgment layer to catch"
    );
}

#[test]
fn the_reserve_floor_binds_when_the_peak_falls_inside_the_window() {
    // The tension in one day: the only expensive hour is inside the window, so
    // holding the reserve means giving up the trade.
    let mut prices = vec![50.0; 24];
    prices[19] = 200.0;
    let window = AfrrWindow {
        day: 0,
        from_hour: 18,
        to_hour: 20,
        reserve_mw: 0.5,
        reserve_soc: 0.70,
        capacity_price: 10.0,
    };
    let spec = BatterySpec { soc_min: 0.10, soc_max: 0.95, ..asset() };
    let floor = window.reserve_soc * spec.capacity_mwh;

    let balanced = solve(ScheduleKind::Balanced, &prices, &spec, Some(&window));
    let heavy = solve(ScheduleKind::ReserveHeavy, &prices, &spec, Some(&window));

    assert!(balanced.dips_below(&window, floor), "balanced empties into the peak");
    assert!(!heavy.dips_below(&window, floor), "reserve-heavy holds the floor");
    assert!(
        heavy.expected_margin < balanced.expected_margin,
        "holding the reserve costs margin: {} vs {}",
        heavy.expected_margin,
        balanced.expected_margin
    );
}

#[test]
fn solving_is_deterministic() {
    let world = synth::BatteryWorld::default_world(synth::DEFAULT_SEED);
    for day in [3u32, 44] {
        let a = candidates(world.day_ahead_for(day), &world.asset, world.afrr_on(day));
        let b = candidates(world.day_ahead_for(day), &world.asset, world.afrr_on(day));
        assert_eq!(a, b);
    }
}

#[test]
fn the_three_variants_differ_on_most_days() {
    // If they agreed everywhere there would be nothing to rank.
    let world = synth::BatteryWorld::default_world(synth::DEFAULT_SEED);
    let differing = (0..world.days)
        .filter(|day| {
            let s = candidates(world.day_ahead_for(*day), &world.asset, world.afrr_on(*day));
            s[0].power_mw != s[1].power_mw || s[1].power_mw != s[2].power_mw
        })
        .count();
    assert!(
        differing > world.days as usize / 2,
        "only {differing} of {} days had differing schedules",
        world.days
    );
}

#[test]
fn a_negative_price_hour_is_worth_charging_into() {
    // Paid to take energy: the plan should import in that hour.
    let mut prices = vec![50.0; 24];
    prices[12] = -30.0;
    prices[20] = 150.0;
    let s = solve(ScheduleKind::Balanced, &prices, &asset(), None);
    assert!(s.power_mw[12] > 0.0, "hour 12 should charge: {:?}", s.power_mw);
    assert!(s.power_mw[20] < 0.0, "hour 20 should discharge");
}

#[test]
fn efficiency_losses_reduce_the_margin() {
    let mut prices = vec![50.0; 24];
    prices[20] = 100.0;
    let lossless = solve(ScheduleKind::Balanced, &prices, &asset(), None);
    let lossy = solve(
        ScheduleKind::Balanced,
        &prices,
        &BatterySpec { round_trip_efficiency: 0.80, ..asset() },
        None,
    );
    assert!(
        lossy.expected_margin < lossless.expected_margin,
        "{} should be under {}",
        lossy.expected_margin,
        lossless.expected_margin
    );
}
