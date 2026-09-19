//! Executing a schedule against the intraday market.
//!
//! The schedule is a day-ahead plan. What it actually earns depends on where
//! intraday settled, which is exactly the gap the `margin_plausible` judgment
//! is about.

use crate::solver::Schedule;
use serde::{Deserialize, Serialize};
use synth::{AfrrWindow, BatterySpec, BatteryWorld};

/// What running a schedule actually produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Execution {
    /// The day it ran on.
    pub day: u32,
    /// Which schedule ran.
    pub schedule: String,
    /// The fraction of the plan's power that was actually run.
    pub size_factor: f64,
    /// Cash from energy, at intraday prices.
    pub energy_margin: f64,
    /// Payment for holding the reserve, when it was held.
    pub reserve_payment: f64,
    /// Degradation cost incurred.
    pub degradation_cost: f64,
    /// Energy margin plus reserve payment, less degradation.
    pub realised_margin: f64,
    /// What the day-ahead curve said the plan was worth, scaled to what ran.
    pub expected_margin: f64,
    /// State of charge through the day, 25 values in MWh.
    pub soc_mwh: Vec<f64>,
    /// Hours inside the reserve window where the state of charge fell short.
    pub reserve_breaches: u32,
    /// Equivalent full cycles used.
    pub cycles: f64,
}

impl Execution {
    /// Realised less expected: how wrong the day-ahead estimate turned out to be.
    pub fn margin_surprise(&self) -> f64 {
        self.realised_margin - self.expected_margin
    }

    /// Whether the reserve was honoured.
    pub fn reserve_held(&self) -> bool {
        self.reserve_breaches == 0
    }
}

/// Run `schedule` at `size_factor` of its power against the day's intraday prints.
///
/// Scaling the power scales every energy movement, so the state-of-charge
/// trajectory stays inside the bounds the solver respected — a schedule run
/// smaller is always at least as feasible as the schedule run whole.
pub fn execute(world: &BatteryWorld, day: u32, schedule: &Schedule, size_factor: f64) -> Execution {
    let asset = &world.asset;
    let efficiency = asset.one_way_efficiency();
    let factor = size_factor.clamp(0.0, 1.0);
    let afrr = world.afrr_on(day);

    let mut soc = vec![schedule.soc_mwh[0]];
    let mut energy_margin = 0.0;
    let mut throughput = 0.0;

    for hour in 0..24 {
        let grid_mwh = schedule.power_mw[hour] * factor;
        let price = world.intraday_hour(day, hour as u32);
        // Positive grid power is an import, which costs; negative is an export.
        energy_margin -= grid_mwh * price;

        // Stored energy moves by the grid energy adjusted for the one-way loss.
        let stored_change =
            if grid_mwh > 0.0 { grid_mwh * efficiency } else { grid_mwh / efficiency };
        throughput += stored_change.abs();
        soc.push(soc[hour] + stored_change);
    }

    let cycles = throughput / (2.0 * asset.capacity_mwh);
    let degradation_cost = cycles * asset.degradation_cost_per_cycle;
    let reserve_breaches = count_breaches(&soc, afrr, asset);
    let reserve_payment = match afrr {
        Some(window) if reserve_breaches == 0 => {
            window.reserve_mw * window.capacity_price * window.hours() as f64
        }
        _ => 0.0,
    };

    Execution {
        day,
        schedule: schedule.kind.as_str().to_owned(),
        size_factor: factor,
        energy_margin,
        reserve_payment,
        degradation_cost,
        realised_margin: energy_margin + reserve_payment - degradation_cost,
        // The plan's own estimate covers energy and wear; scaling the plan
        // scales both, so the comparison is like for like.
        expected_margin: schedule.expected_margin * factor,
        soc_mwh: soc,
        reserve_breaches,
        cycles,
    }
}

/// A day on which nothing ran: the gate held or escalated.
///
/// Standing down is not free. The reserve goes unheld and unpaid, which is part
/// of what a hold costs.
pub fn stood_down(day: u32, schedule: &Schedule) -> Execution {
    let flat = schedule.soc_mwh[0];
    Execution {
        day,
        schedule: schedule.kind.as_str().to_owned(),
        size_factor: 0.0,
        energy_margin: 0.0,
        reserve_payment: 0.0,
        degradation_cost: 0.0,
        realised_margin: 0.0,
        expected_margin: 0.0,
        soc_mwh: vec![flat; 25],
        reserve_breaches: 0,
        cycles: 0.0,
    }
}

/// Hours inside the window where the state of charge fell below the reserve.
fn count_breaches(soc: &[f64], afrr: Option<&AfrrWindow>, asset: &BatterySpec) -> u32 {
    let Some(window) = afrr else {
        return 0;
    };
    let floor = window.reserve_soc * asset.capacity_mwh;
    (window.from_hour..=window.to_hour + 1)
        .filter(|h| soc.get(*h as usize).is_some_and(|e| *e < floor - 1e-9))
        .count() as u32
}

/// A run's totals for one book.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BookResult {
    /// Days on which something ran.
    pub days_run: usize,
    /// Days on which nothing ran.
    pub days_stood_down: usize,
    /// Total realised margin.
    pub margin: f64,
    /// Total reserve payments collected.
    pub reserve_payment: f64,
    /// Total degradation cost.
    pub degradation_cost: f64,
    /// Hours the reserve went unmet.
    pub reserve_breaches: u32,
    /// Days on which the reserve went unmet at all.
    pub days_with_breach: usize,
    /// Total equivalent full cycles.
    pub cycles: f64,
    /// Worst peak-to-trough fall in cumulative margin.
    pub max_drawdown: f64,
}

impl BookResult {
    /// Margin per equivalent cycle: what each cycle of wear bought.
    pub fn margin_per_cycle(&self) -> f64 {
        if self.cycles <= 0.0 {
            return 0.0;
        }
        self.margin / self.cycles
    }
}

/// Aggregate executions, in day order.
pub fn summarise<'a>(executions: impl Iterator<Item = &'a Execution>) -> BookResult {
    let mut result = BookResult::default();
    let mut cumulative: f64 = 0.0;
    let mut peak: f64 = 0.0;
    for e in executions {
        if e.size_factor > 0.0 {
            result.days_run += 1;
        } else {
            result.days_stood_down += 1;
        }
        result.margin += e.realised_margin;
        result.reserve_payment += e.reserve_payment;
        result.degradation_cost += e.degradation_cost;
        result.reserve_breaches += e.reserve_breaches;
        if e.reserve_breaches > 0 {
            result.days_with_breach += 1;
        }
        result.cycles += e.cycles;
        cumulative += e.realised_margin;
        peak = peak.max(cumulative);
        result.max_drawdown = result.max_drawdown.max(peak - cumulative);
    }
    result
}
