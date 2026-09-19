//! The deterministic side: a dynamic program over the day's state of charge.
//!
//! Maximises day-ahead arbitrage less degradation, subject to the state-of-charge
//! bounds and — for the reserve-heavy variant — the aFRR reserve floor. Every
//! number in a [`Schedule`] is the solver's. Jev picks between schedules; it
//! never edits one.
//!
//! # Conventions
//!
//! Power is signed in MW: positive charges, negative discharges. Over a
//! one-hour step, with one-way efficiency `e`:
//!
//! - charging draws `g` MWh from the grid and stores `g * e`;
//! - discharging removes `d` MWh from storage and delivers `d * e` to the grid.
//!
//! The dynamic program is stated over the *stored* energy change, so every
//! transition lands exactly on the discretisation grid.

use serde::{Deserialize, Serialize};
use synth::{AfrrWindow, BatterySpec};

/// Energy resolution of the state-of-charge grid, MWh.
pub const SOC_STEP: f64 = 0.05;

/// Which of the three schedules this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    /// Chases the spread. Under-weights wear and ignores the reserve floor.
    Aggressive,
    /// Prices wear honestly and respects the standard bounds.
    Balanced,
    /// Holds the reserve through the commitment window, and cycles least.
    ReserveHeavy,
}

impl ScheduleKind {
    /// All three, in the order they are offered to Jev.
    pub const ALL: [ScheduleKind; 3] =
        [ScheduleKind::Aggressive, ScheduleKind::Balanced, ScheduleKind::ReserveHeavy];

    /// Wire label.
    pub fn as_str(&self) -> &'static str {
        match self {
            ScheduleKind::Aggressive => "aggressive",
            ScheduleKind::Balanced => "balanced",
            ScheduleKind::ReserveHeavy => "reserve_heavy",
        }
    }

    /// What this schedule is for, as Jev reads it.
    pub fn describe(&self) -> &'static str {
        match self {
            ScheduleKind::Aggressive => {
                "Takes the widest spread available, cycling hard and treating wear as cheap. \
                 It does not hold anything back for a reserve obligation."
            }
            ScheduleKind::Balanced => {
                "Takes the spread that is worth taking once wear is priced properly, and \
                 keeps the state of charge inside its ordinary bounds."
            }
            ScheduleKind::ReserveHeavy => {
                "Keeps enough charge to meet the reserve commitment throughout its window, \
                 giving up arbitrage to do so, and cycles the least of the three."
            }
        }
    }

    /// How heavily this variant prices wear, as a multiple of the asset's own
    /// degradation cost.
    fn degradation_weight(&self) -> f64 {
        match self {
            ScheduleKind::Aggressive => 0.4,
            ScheduleKind::Balanced => 1.0,
            ScheduleKind::ReserveHeavy => 1.6,
        }
    }

    /// The state-of-charge band this variant will operate in, as fractions of
    /// capacity, before any reserve obligation is applied.
    ///
    /// This, rather than the wear multiplier, is what makes the three
    /// schedules genuinely different. A multiplier only bites when the spread
    /// is marginal, and on a day with a wide evening peak all three would
    /// otherwise return the same plan — leaving nothing to rank.
    fn band(&self) -> (f64, f64) {
        match self {
            // Will use the whole of the physical envelope.
            ScheduleKind::Aggressive => (0.10, 0.95),
            // Keeps a margin away from the physical limits, as an operator would.
            ScheduleKind::Balanced => (0.15, 0.90),
            // Keeps real headroom, so there is always something to deliver.
            ScheduleKind::ReserveHeavy => (0.25, 0.90),
        }
    }

    /// Whether this variant enforces the reserve floor inside the window.
    fn honours_reserve(&self) -> bool {
        matches!(self, ScheduleKind::ReserveHeavy)
    }
}

impl jev_core::CandidateId for ScheduleKind {
    fn label(&self) -> String {
        self.as_str().to_owned()
    }
}

impl std::fmt::Display for ScheduleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One day's plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    /// Which variant produced it.
    pub kind: ScheduleKind,
    /// Signed power for each of the 24 hours, MW. Positive charges.
    pub power_mw: Vec<f64>,
    /// State of charge at the start of each hour plus the end of the day: 25
    /// values, MWh.
    pub soc_mwh: Vec<f64>,
    /// Margin the day-ahead curve implies, net of degradation.
    pub expected_margin: f64,
    /// Equivalent full cycles the plan uses.
    pub cycles: f64,
    /// Degradation cost the plan incurs, at the asset's own rate.
    pub degradation_cost: f64,
}

impl Schedule {
    /// The lowest state of charge the plan reaches, MWh.
    pub fn min_soc(&self) -> f64 {
        self.soc_mwh.iter().copied().fold(f64::MAX, f64::min)
    }

    /// Whether the plan ever falls below `floor` MWh during `window`.
    ///
    /// The reserve must be available *throughout* the window, so the state of
    /// charge is checked at the start of every covered hour and at the end of
    /// the last one.
    pub fn dips_below(&self, window: &AfrrWindow, floor: f64) -> bool {
        (window.from_hour..=window.to_hour).any(|h| self.soc_mwh[h as usize] < floor - 1e-9)
            || self.soc_mwh[(window.to_hour + 1) as usize] < floor - 1e-9
    }

    /// Total energy delivered to the grid, MWh.
    pub fn discharged_mwh(&self) -> f64 {
        self.power_mw.iter().filter(|p| **p < 0.0).map(|p| -p).sum()
    }

    /// Total energy drawn from the grid, MWh.
    pub fn charged_mwh(&self) -> f64 {
        self.power_mw.iter().filter(|p| **p > 0.0).sum()
    }

    /// The hours the plan discharges in.
    pub fn discharge_hours(&self) -> Vec<u32> {
        self.power_mw
            .iter()
            .enumerate()
            .filter(|(_, p)| **p < -1e-9)
            .map(|(h, _)| h as u32)
            .collect()
    }

    /// The span the plan discharges over, as `(first, last)`.
    pub fn discharge_block(&self) -> Option<(u32, u32)> {
        let hours = self.discharge_hours();
        Some((*hours.first()?, *hours.last()?))
    }

    /// A one-line summary for Jev.
    pub fn summary(&self) -> String {
        let block = match self.discharge_block() {
            Some((a, b)) if a == b => format!("discharges at {a:02}:00"),
            Some((a, b)) => format!("discharges between {a:02}:00 and {b:02}:00"),
            None => "does not discharge".to_owned(),
        };
        format!(
            "{}. Charges {:.2} MWh, {block}, lowest state of charge {:.2} MWh, \
             {:.2} equivalent cycles, day-ahead margin {:.2} net of {:.2} degradation.",
            self.kind.describe(),
            self.charged_mwh(),
            self.min_soc(),
            self.cycles,
            self.expected_margin,
            self.degradation_cost,
        )
    }
}

/// Solve one day for one variant.
///
/// `prices` is the day's 24 day-ahead prices. `afrr` is the reserve obligation,
/// if there is one.
pub fn solve(
    kind: ScheduleKind,
    prices: &[f64],
    asset: &BatterySpec,
    afrr: Option<&AfrrWindow>,
) -> Schedule {
    assert_eq!(prices.len(), 24, "a day is 24 hourly prices");
    let efficiency = asset.one_way_efficiency();
    let nodes = (asset.capacity_mwh / SOC_STEP).round() as usize + 1;
    let energy = |index: usize| index as f64 * SOC_STEP;

    // Per-hour floors and ceilings on stored energy. The variant's own band is
    // clamped into the asset's physical envelope, then the reserve obligation
    // raises the floor inside its window for the variant that honours it.
    let (band_low, band_high) = kind.band();
    let standing_floor = (band_low * asset.capacity_mwh).max(asset.min_energy());
    let ceiling = (band_high * asset.capacity_mwh).min(asset.max_energy());
    let reserve_floor = afrr
        .filter(|_| kind.honours_reserve())
        .map(|w| (w.reserve_soc * asset.capacity_mwh).min(ceiling));
    let floor_at = |hour: usize| -> f64 {
        match (reserve_floor, afrr) {
            // The reserve must hold throughout the window, which means it must
            // already be in place when the window opens and still be there when
            // the last covered hour ends.
            (Some(floor), Some(window))
                if window.covers(hour as u32) || hour as u32 == window.to_hour + 1 =>
            {
                standing_floor.max(floor)
            }
            _ => standing_floor,
        }
    };

    // Wear costs the asset's rate, weighted by how honestly this variant
    // prices it. One equivalent cycle is twice the capacity in throughput.
    let wear_per_mwh =
        asset.degradation_cost_per_cycle * kind.degradation_weight() / (2.0 * asset.capacity_mwh);

    // Leftover energy is worth the day's mean price, so the plan does not
    // simply dump the battery into the last hour.
    let mean_price = prices.iter().sum::<f64>() / prices.len() as f64;

    const NEG: f64 = f64::NEG_INFINITY;
    // value[hour][node] = best achievable from `hour` onward, holding `node`.
    let mut value = vec![vec![NEG; nodes]; 25];
    let mut action = vec![vec![0i64; nodes]; 24];

    // The node index addresses several parallel grids at once (`value`,
    // `action`, and the energy it stands for), so it cannot become an iterator.
    #[allow(clippy::needless_range_loop)]
    for node in 0..nodes {
        let e = energy(node);
        value[24][node] = if e >= floor_at(24) - 1e-9 && e <= ceiling + 1e-9 {
            e * efficiency * mean_price
        } else {
            NEG
        };
    }

    // Stored-energy steps the asset can manage in an hour, in grid nodes.
    let max_charge_steps = (asset.power_mw * efficiency / SOC_STEP).floor() as i64;
    let max_discharge_steps = (asset.power_mw / efficiency / SOC_STEP).floor() as i64;

    for hour in (0..24).rev() {
        let price = prices[hour];
        let floor = floor_at(hour);
        let next_floor = floor_at(hour + 1);
        for node in 0..nodes {
            let e = energy(node);
            if e < floor - 1e-9 || e > ceiling + 1e-9 {
                continue;
            }
            let mut best = NEG;
            let mut best_delta = 0i64;
            for delta in -max_discharge_steps..=max_charge_steps {
                let next = node as i64 + delta;
                if next < 0 || next >= nodes as i64 {
                    continue;
                }
                let next_energy = energy(next as usize);
                if next_energy < next_floor - 1e-9 || next_energy > ceiling + 1e-9 {
                    continue;
                }
                let ahead = value[hour + 1][next as usize];
                if ahead == NEG {
                    continue;
                }
                let stored_change = delta as f64 * SOC_STEP;
                // Cash now, then wear, then whatever the rest of the day is worth.
                let cash = if stored_change > 0.0 {
                    -(stored_change / efficiency) * price
                } else {
                    -stored_change * efficiency * price
                };
                let wear = stored_change.abs() * wear_per_mwh;
                let total = cash - wear + ahead;
                if total > best {
                    best = total;
                    best_delta = delta;
                }
            }
            value[hour][node] = best;
            action[hour][node] = best_delta;
        }
    }

    // Walk the policy forward from the starting state of charge.
    // The day opens at the asset's starting charge, pulled into this variant's
    // band if its band is tighter.
    let start_energy = asset.start_energy().clamp(floor_at(0), ceiling);
    let start_node = ((start_energy / SOC_STEP).round() as usize).min(nodes - 1);
    let mut node = start_node;
    let mut power_mw = Vec::with_capacity(24);
    let mut soc_mwh = Vec::with_capacity(25);
    let mut margin = 0.0;
    let mut throughput = 0.0;

    soc_mwh.push(energy(node));
    for hour in 0..24 {
        let delta = if value[hour][node] == NEG { 0 } else { action[hour][node] };
        let stored_change = delta as f64 * SOC_STEP;
        let (grid_mwh, cash) = if stored_change > 0.0 {
            let g = stored_change / efficiency;
            (g, -g * prices[hour])
        } else {
            let g = -stored_change * efficiency;
            (-g, g * prices[hour])
        };
        margin += cash;
        throughput += stored_change.abs();
        power_mw.push(grid_mwh);
        node = (node as i64 + delta).clamp(0, nodes as i64 - 1) as usize;
        soc_mwh.push(energy(node));
    }

    let cycles = throughput / (2.0 * asset.capacity_mwh);
    let degradation_cost = cycles * asset.degradation_cost_per_cycle;

    Schedule {
        kind,
        power_mw,
        soc_mwh,
        expected_margin: margin - degradation_cost,
        cycles,
        degradation_cost,
    }
}

/// Solve all three variants for one day.
pub fn candidates(prices: &[f64], asset: &BatterySpec, afrr: Option<&AfrrWindow>) -> Vec<Schedule> {
    ScheduleKind::ALL.iter().map(|k| solve(*k, prices, asset, afrr)).collect()
}
