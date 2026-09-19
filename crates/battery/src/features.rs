//! What the judgment layer is allowed to see about a battery day.
//!
//! As in the forex domain, this module is the honesty boundary: everything Jev
//! reads is assembled here, out of the day-ahead curve, the intraday prints
//! already observed, the reserve obligation, the grid notes, and the schedules
//! the solver produced.
//!
//! There is no hidden regime in the power world to leak — the generator has no
//! latent state the way the forex one does — but the same discipline applies:
//! nothing here reads a price the desk would not yet have seen.

use crate::solver::Schedule;
use serde::{Deserialize, Serialize};
use synth::{AfrrWindow, BatterySpec, BatteryWorld, GridNote};

/// Days of history used for the percentile and sigma comparisons.
const LOOKBACK_DAYS: u32 = 10;

/// The observable state of one battery day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatteryFeatures {
    /// Which day of the run.
    pub day: u32,
    /// The date.
    pub date: String,
    /// Where the day's day-ahead price dispersion sits in its recent history.
    pub volatility_percentile: f64,
    /// Where the day's intraday tick dispersion sits in its recent history.
    /// The observable stand-in for how thin the market is.
    pub spread_percentile: f64,
    /// A composite of the conditions that make a day hard to trade in.
    pub stress_indicator: f64,
    /// The widest day-ahead spread available, per MWh.
    pub day_ahead_spread: f64,
    /// Hours of the day-ahead curve that cleared below zero.
    pub negative_price_hours: f64,
    /// The day's highest day-ahead price.
    pub peak_price: f64,
    /// Whether a reserve obligation applies today.
    pub afrr_window_active: bool,
    /// The obligation's hours, when there is one.
    pub afrr_window: Option<String>,
    /// Whether a grid note overlaps the hours the schedule under consideration
    /// discharges in.
    pub grid_notice_overlaps_discharge: bool,
    /// How far yesterday's intraday settled from its day-ahead curve, measured
    /// in standard deviations of the recent hourly deviation. This is the
    /// observable reason to doubt a day-ahead margin estimate.
    pub id_deviation_sigmas: f64,
    /// Whether the schedule under consideration falls below the reserve state
    /// of charge at any point inside the window.
    pub schedule_dips_below_reserve_soc: bool,
    /// The cycles that schedule uses, as a fraction of the daily budget.
    pub cycle_budget_used_fraction: f64,
    /// Which schedule the schedule-level fields above describe.
    pub under_consideration: String,
}

impl BatteryFeatures {
    /// Assemble the observable state for `day`, with the schedule-level fields
    /// describing `schedule`.
    ///
    /// Before the ranking stage that is the solver's default (balanced); after
    /// it, the schedule the ranking chose. The field `under_consideration`
    /// always says which, so a reader of `decisions.jsonl` is never guessing.
    pub fn compute(world: &BatteryWorld, day: u32, schedule: &Schedule) -> Self {
        let prices = world.day_ahead_for(day);
        let asset = &world.asset;
        let afrr = world.afrr_on(day);

        let dispersion = stdev(prices);
        let volatility_percentile = percentile(
            dispersion,
            (day.saturating_sub(LOOKBACK_DAYS)..day).map(|d| stdev(world.day_ahead_for(d))),
        );
        let tick_dispersion = intraday_dispersion(world, day);
        let spread_percentile = percentile(
            tick_dispersion,
            (day.saturating_sub(LOOKBACK_DAYS)..day).map(|d| intraday_dispersion(world, d)),
        );

        let id_deviation_sigmas = if day == 0 {
            0.0
        } else {
            world.mean_deviation_on(day - 1) / world.recent_deviation_sigma(day, LOOKBACK_DAYS)
        };

        let discharge = schedule.discharge_block();
        let grid_notice_overlaps_discharge = match discharge {
            Some((from, to)) => world.grid_notes_on(day).iter().any(|n| n.overlaps(from, to)),
            None => false,
        };

        let dips = match afrr {
            Some(window) => schedule.dips_below(window, window.reserve_soc * asset.capacity_mwh),
            None => false,
        };

        let min = prices.iter().copied().fold(f64::MAX, f64::min);
        let peak = prices.iter().copied().fold(f64::MIN, f64::max);
        let negative_price_hours = prices.iter().filter(|p| **p < 0.0).count() as f64;

        // A day is hard when it is both wide and doubtful, or when the grid is
        // telling you something.
        let stress_indicator = (0.4 * volatility_percentile
            + 0.3 * (id_deviation_sigmas.abs() / 3.0).clamp(0.0, 1.0)
            + 0.3 * if grid_notice_overlaps_discharge { 1.0 } else { 0.0 })
        .clamp(0.0, 1.0);

        BatteryFeatures {
            day,
            date: world.start.plus_days(day as i64).to_string(),
            volatility_percentile,
            spread_percentile,
            stress_indicator,
            day_ahead_spread: peak - min,
            negative_price_hours,
            peak_price: peak,
            afrr_window_active: afrr.is_some(),
            afrr_window: afrr.map(|w| format!("{:02}:00-{:02}:00", w.from_hour, w.to_hour)),
            grid_notice_overlaps_discharge,
            id_deviation_sigmas,
            schedule_dips_below_reserve_soc: dips,
            cycle_budget_used_fraction: schedule.cycles / asset.daily_cycle_budget,
            under_consideration: schedule.kind.as_str().to_owned(),
        }
    }
}

/// Population standard deviation.
fn stdev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
}

/// How widely the day's intraday prints scattered around their own hours.
fn intraday_dispersion(world: &BatteryWorld, day: u32) -> f64 {
    let spreads: Vec<f64> = (0..24)
        .map(|h| {
            let ticks = world.intraday_ticks(day, h);
            let high = ticks.iter().copied().fold(f64::MIN, f64::max);
            let low = ticks.iter().copied().fold(f64::MAX, f64::min);
            high - low
        })
        .collect();
    spreads.iter().sum::<f64>() / spreads.len() as f64
}

/// Where `value` sits within `history`, as a fraction in `0..=1`.
fn percentile(value: f64, history: impl Iterator<Item = f64>) -> f64 {
    let history: Vec<f64> = history.collect();
    if history.len() < 2 {
        return 0.5;
    }
    let below = history.iter().filter(|h| **h < value).count();
    below as f64 / (history.len() - 1) as f64
}

/// A grid note rendered for Jev.
pub fn note_line(note: &GridNote) -> String {
    format!("[{}] {}", note.kind.as_str(), note.text)
}

/// The reserve obligation rendered for Jev.
pub fn afrr_line(window: &AfrrWindow, asset: &BatterySpec) -> String {
    format!(
        "{:.1} MW must be available from {:02}:00 to {:02}:00, which needs at least \
         {:.2} MWh held throughout; it pays {:.2} per MW per hour.",
        window.reserve_mw,
        window.from_hour,
        window.to_hour,
        window.reserve_soc * asset.capacity_mwh,
        window.capacity_price,
    )
}
