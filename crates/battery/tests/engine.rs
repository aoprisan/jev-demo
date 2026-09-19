//! Executing a schedule against intraday prices.

use battery::engine::{execute, stood_down, summarise, Execution};
use battery::solver::{solve, ScheduleKind};
use synth::{BatteryWorld, DEFAULT_SEED};

const EPS: f64 = 1e-9;

fn world() -> BatteryWorld {
    BatteryWorld::default_world(DEFAULT_SEED)
}

#[test]
fn the_realised_margin_is_the_plan_priced_at_intraday() {
    let w = world();
    let day = 12;
    let s = solve(ScheduleKind::Balanced, w.day_ahead_for(day), &w.asset, w.afrr_on(day));
    let e = execute(&w, day, &s, 1.0);

    // Recompute by hand from the intraday curve: an import costs, an export earns.
    let expected: f64 = (0..24).map(|h| -s.power_mw[h] * w.intraday_hour(day, h as u32)).sum();
    assert!((e.energy_margin - expected).abs() < 1e-9, "{} vs {expected}", e.energy_margin);
    assert!(
        (e.realised_margin - (e.energy_margin + e.reserve_payment - e.degradation_cost)).abs()
            < EPS
    );
}

#[test]
fn realised_and_expected_differ_because_intraday_is_not_day_ahead() {
    let w = world();
    let surprises: Vec<f64> = (0..30)
        .map(|day| {
            let s = solve(ScheduleKind::Balanced, w.day_ahead_for(day), &w.asset, w.afrr_on(day));
            execute(&w, day, &s, 1.0).margin_surprise()
        })
        .collect();
    assert!(
        surprises.iter().any(|s| s.abs() > 1.0),
        "a day-ahead plan should not price out identically at intraday"
    );
    assert!(
        surprises.iter().any(|s| *s > 0.0) && surprises.iter().any(|s| *s < 0.0),
        "the surprise should cut both ways"
    );
}

#[test]
fn scaling_the_plan_scales_the_energy_and_the_wear() {
    let w = world();
    let day = 12;
    let s = solve(ScheduleKind::Balanced, w.day_ahead_for(day), &w.asset, w.afrr_on(day));
    let full = execute(&w, day, &s, 1.0);
    let half = execute(&w, day, &s, 0.5);

    assert!((half.energy_margin - full.energy_margin / 2.0).abs() < 1e-9);
    assert!((half.cycles - full.cycles / 2.0).abs() < 1e-9);
    assert!((half.degradation_cost - full.degradation_cost / 2.0).abs() < 1e-9);
    assert!((half.expected_margin - full.expected_margin / 2.0).abs() < 1e-9);
}

#[test]
fn running_smaller_never_makes_the_state_of_charge_less_feasible() {
    let w = world();
    for day in [4u32, 14, 40] {
        let s = solve(ScheduleKind::Balanced, w.day_ahead_for(day), &w.asset, w.afrr_on(day));
        let full = execute(&w, day, &s, 1.0);
        let quarter = execute(&w, day, &s, 0.25);
        let low = |e: &Execution| e.soc_mwh.iter().copied().fold(f64::MAX, f64::min);
        let high = |e: &Execution| e.soc_mwh.iter().copied().fold(f64::MIN, f64::max);
        assert!(low(&quarter) >= low(&full) - 1e-9, "day {day}");
        assert!(high(&quarter) <= high(&full) + 1e-9, "day {day}");
    }
}

#[test]
fn holding_the_reserve_earns_the_capacity_payment_and_breaching_it_does_not() {
    let w = world();
    let day = (0..w.days).find(|d| w.afrr_on(*d).is_some()).expect("some day has a window");
    let window = w.afrr_on(day).unwrap();

    let heavy = solve(ScheduleKind::ReserveHeavy, w.day_ahead_for(day), &w.asset, Some(window));
    let held = execute(&w, day, &heavy, 1.0);
    assert!(held.reserve_held(), "the reserve-heavy plan holds the reserve");
    assert!(
        (held.reserve_payment - window.reserve_mw * window.capacity_price * window.hours() as f64)
            .abs()
            < EPS
    );

    let balanced = solve(ScheduleKind::Balanced, w.day_ahead_for(day), &w.asset, Some(window));
    let floor = window.reserve_soc * w.asset.capacity_mwh;
    if balanced.dips_below(window, floor) {
        let ran = execute(&w, day, &balanced, 1.0);
        assert!(ran.reserve_breaches > 0);
        assert!((ran.reserve_payment - 0.0).abs() < EPS, "a breach forfeits the payment");
    }
}

#[test]
fn a_day_with_no_window_can_never_breach() {
    let w = world();
    let day = (0..w.days).find(|d| w.afrr_on(*d).is_none()).unwrap();
    let s = solve(ScheduleKind::Aggressive, w.day_ahead_for(day), &w.asset, None);
    let e = execute(&w, day, &s, 1.0);
    assert_eq!(e.reserve_breaches, 0);
    assert!((e.reserve_payment - 0.0).abs() < EPS);
}

#[test]
fn standing_down_earns_nothing_and_costs_nothing_but_forfeits_the_reserve() {
    let w = world();
    let day = (0..w.days).find(|d| w.afrr_on(*d).is_some()).unwrap();
    let s = solve(ScheduleKind::ReserveHeavy, w.day_ahead_for(day), &w.asset, w.afrr_on(day));
    let e = stood_down(day, &s);

    assert!((e.realised_margin - 0.0).abs() < EPS);
    assert!((e.cycles - 0.0).abs() < EPS);
    assert!((e.degradation_cost - 0.0).abs() < EPS);
    assert!(
        (e.reserve_payment - 0.0).abs() < EPS,
        "standing down forfeits the capacity payment; a hold is not free"
    );
    assert_eq!(e.soc_mwh.len(), 25);
    assert!(e.soc_mwh.windows(2).all(|p| (p[0] - p[1]).abs() < EPS), "the day is flat");
}

#[test]
fn summarise_totals_margin_breaches_and_cycles() {
    let w = world();
    let executions: Vec<Execution> = (0..20)
        .map(|day| {
            let s = solve(ScheduleKind::Balanced, w.day_ahead_for(day), &w.asset, w.afrr_on(day));
            execute(&w, day, &s, 1.0)
        })
        .collect();
    let book = summarise(executions.iter());

    assert_eq!(book.days_run + book.days_stood_down, 20);
    assert!((book.margin - executions.iter().map(|e| e.realised_margin).sum::<f64>()).abs() < 1e-9);
    assert_eq!(book.reserve_breaches, executions.iter().map(|e| e.reserve_breaches).sum::<u32>());
    assert!((book.cycles - executions.iter().map(|e| e.cycles).sum::<f64>()).abs() < 1e-9);
    assert!(book.margin_per_cycle() > 0.0);
}

#[test]
fn summarise_counts_stand_downs_separately() {
    let w = world();
    let s = solve(ScheduleKind::Balanced, w.day_ahead_for(0), &w.asset, None);
    let mixed = [execute(&w, 0, &s, 1.0), stood_down(1, &s), execute(&w, 2, &s, 0.5)];
    let book = summarise(mixed.iter());
    assert_eq!(book.days_run, 2);
    assert_eq!(book.days_stood_down, 1);
}

#[test]
fn an_empty_book_is_all_zeroes() {
    let book = summarise(std::iter::empty());
    assert_eq!(book.days_run, 0);
    assert!((book.margin_per_cycle() - 0.0).abs() < EPS);
}
