//! A synthetic day-ahead and intraday power market, plus the asset that trades it.

use crate::calendar::Date;
use crate::rng::Rng;
use serde::{Deserialize, Serialize};

/// Intraday prints per hour.
pub const TICKS_PER_HOUR: u32 = 4;

/// The asset. A 1 MW / 2 MWh battery.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BatterySpec {
    /// Maximum charge or discharge power, MW.
    pub power_mw: f64,
    /// Usable energy, MWh.
    pub capacity_mwh: f64,
    /// Round-trip efficiency, 0..=1.
    pub round_trip_efficiency: f64,
    /// Cost of one full equivalent cycle, in currency.
    pub degradation_cost_per_cycle: f64,
    /// Lower state-of-charge bound, as a fraction of capacity.
    pub soc_min: f64,
    /// Upper state-of-charge bound, as a fraction of capacity.
    pub soc_max: f64,
    /// State of charge at the start of each day, as a fraction of capacity.
    pub soc_start: f64,
    /// Equivalent full cycles allowed per day before the budget is spent.
    pub daily_cycle_budget: f64,
}

impl Default for BatterySpec {
    fn default() -> Self {
        Self {
            power_mw: 1.0,
            capacity_mwh: 2.0,
            round_trip_efficiency: 0.88,
            degradation_cost_per_cycle: 14.0,
            soc_min: 0.10,
            soc_max: 0.95,
            soc_start: 0.50,
            daily_cycle_budget: 1.6,
        }
    }
}

impl BatterySpec {
    /// One-way efficiency, applied on both charge and discharge.
    pub fn one_way_efficiency(&self) -> f64 {
        self.round_trip_efficiency.sqrt()
    }

    /// Energy at the lower bound, MWh.
    pub fn min_energy(&self) -> f64 {
        self.soc_min * self.capacity_mwh
    }

    /// Energy at the upper bound, MWh.
    pub fn max_energy(&self) -> f64 {
        self.soc_max * self.capacity_mwh
    }

    /// Energy at the start of a day, MWh.
    pub fn start_energy(&self) -> f64 {
        self.soc_start * self.capacity_mwh
    }
}

/// A window in which reserve must be held for aFRR.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AfrrWindow {
    /// The day it applies to.
    pub day: u32,
    /// First hour, inclusive.
    pub from_hour: u32,
    /// Last hour, inclusive.
    pub to_hour: u32,
    /// Power that must be available, MW.
    pub reserve_mw: f64,
    /// State of charge that must be maintained throughout, as a fraction.
    pub reserve_soc: f64,
    /// Payment for holding the reserve, per MW per hour.
    pub capacity_price: f64,
}

impl AfrrWindow {
    /// Whether `hour` falls inside the window.
    pub fn covers(&self, hour: u32) -> bool {
        (self.from_hour..=self.to_hour).contains(&hour)
    }

    /// Hours in the window.
    pub fn hours(&self) -> u32 {
        self.to_hour - self.from_hour + 1
    }
}

/// What a grid note is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridNoteKind {
    /// Import capacity reduced for a period.
    ImportCapacityReduction,
    /// A TSO notice affecting the connection.
    TsoNotice,
    /// Planned maintenance on the feeder.
    Maintenance,
}

impl GridNoteKind {
    /// Lower-case name.
    pub fn as_str(&self) -> &'static str {
        match self {
            GridNoteKind::ImportCapacityReduction => "import_capacity_reduction",
            GridNoteKind::TsoNotice => "tso_notice",
            GridNoteKind::Maintenance => "maintenance",
        }
    }
}

/// An operational note from the grid operator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridNote {
    /// The day it applies to.
    pub day: u32,
    /// First hour affected, inclusive.
    pub from_hour: u32,
    /// Last hour affected, inclusive.
    pub to_hour: u32,
    /// What kind of note.
    pub kind: GridNoteKind,
    /// The note itself.
    pub text: String,
}

impl GridNote {
    /// Whether `hour` falls inside the note's window.
    pub fn covers(&self, hour: u32) -> bool {
        (self.from_hour..=self.to_hour).contains(&hour)
    }

    /// Whether the note's window overlaps `from..=to`.
    pub fn overlaps(&self, from: u32, to: u32) -> bool {
        self.from_hour <= to && from <= self.to_hour
    }
}

/// A market headline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerHeadline {
    /// The day it printed.
    pub day: u32,
    /// The text.
    pub text: String,
    /// Whether it carries any information.
    pub informative: bool,
}

/// A whole synthetic power world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryWorld {
    /// The seed it was generated from.
    pub seed: u64,
    /// The first day.
    pub start: Date,
    /// How many days.
    pub days: u32,
    /// Day-ahead prices, `days * 24` of them, hour by hour.
    pub day_ahead: Vec<f64>,
    /// Intraday prints, `days * 24 * TICKS_PER_HOUR` of them.
    pub intraday: Vec<f64>,
    /// The asset.
    pub asset: BatterySpec,
    /// Reserve obligations, by day.
    pub afrr: Vec<AfrrWindow>,
    /// Grid notes, by day.
    pub grid_notes: Vec<GridNote>,
    /// The headline feed.
    pub headlines: Vec<PowerHeadline>,
}

impl BatteryWorld {
    /// Generate a world from `seed` over `days` days.
    pub fn generate(seed: u64, days: u32) -> Self {
        let start = Date::new(2025, 1, 6);
        let day_ahead = generate_day_ahead(seed, days, start);
        let intraday = generate_intraday(seed, &day_ahead);
        BatteryWorld {
            seed,
            start,
            days,
            asset: BatterySpec::default(),
            afrr: generate_afrr(seed, days),
            grid_notes: generate_grid_notes(seed, days),
            headlines: generate_headlines(seed, days, &day_ahead),
            day_ahead,
            intraday,
        }
    }

    /// The default 90-day world.
    pub fn default_world(seed: u64) -> Self {
        Self::generate(seed, 90)
    }

    /// The 24 day-ahead prices for `day`.
    pub fn day_ahead_for(&self, day: u32) -> &[f64] {
        let from = (day * 24) as usize;
        &self.day_ahead[from..from + 24]
    }

    /// The intraday prints within one hour.
    pub fn intraday_ticks(&self, day: u32, hour: u32) -> &[f64] {
        let from = ((day * 24 + hour) * TICKS_PER_HOUR) as usize;
        &self.intraday[from..from + TICKS_PER_HOUR as usize]
    }

    /// The hour's settled intraday price: the mean of its prints.
    pub fn intraday_hour(&self, day: u32, hour: u32) -> f64 {
        let ticks = self.intraday_ticks(day, hour);
        ticks.iter().sum::<f64>() / ticks.len() as f64
    }

    /// The 24 settled intraday prices for `day`.
    pub fn intraday_for(&self, day: u32) -> Vec<f64> {
        (0..24).map(|h| self.intraday_hour(day, h)).collect()
    }

    /// The reserve obligation on `day`, if any.
    pub fn afrr_on(&self, day: u32) -> Option<&AfrrWindow> {
        self.afrr.iter().find(|w| w.day == day)
    }

    /// The grid notes on `day`.
    pub fn grid_notes_on(&self, day: u32) -> Vec<&GridNote> {
        self.grid_notes.iter().filter(|n| n.day == day).collect()
    }

    /// The headlines on `day`.
    pub fn headlines_on(&self, day: u32) -> Vec<&PowerHeadline> {
        self.headlines.iter().filter(|h| h.day == day).collect()
    }

    /// The mean hourly intraday-to-day-ahead deviation over `day`.
    ///
    /// Signed: a positive value means intraday settled above the day-ahead
    /// curve. This is what is observable about yesterday when today's schedule
    /// is chosen.
    pub fn mean_deviation_on(&self, day: u32) -> f64 {
        let total: f64 = (0..24)
            .map(|h| self.intraday_hour(day, h) - self.day_ahead[(day * 24 + h) as usize])
            .sum();
        total / 24.0
    }

    /// Standard deviation of the last `lookback` days' hourly intraday-to-day-ahead
    /// deviations, up to but not including `day`. The denominator of the mock's
    /// "deviation beyond two sigma" rule.
    pub fn recent_deviation_sigma(&self, day: u32, lookback: u32) -> f64 {
        let from = day.saturating_sub(lookback);
        let mut deviations = Vec::new();
        for d in from..day {
            for h in 0..24 {
                deviations.push(self.intraday_hour(d, h) - self.day_ahead[(d * 24 + h) as usize]);
            }
        }
        if deviations.len() < 2 {
            return 1.0;
        }
        let mean = deviations.iter().sum::<f64>() / deviations.len() as f64;
        let variance = deviations.iter().map(|d| (d - mean).powi(2)).sum::<f64>()
            / (deviations.len() - 1) as f64;
        variance.sqrt().max(1e-6)
    }

    /// A day carrying no grid note, for the demo's counterfactual replay.
    pub fn first_quiet_day(&self, after: u32) -> Option<u32> {
        (after..self.days).find(|d| self.grid_notes_on(*d).is_empty())
    }

    /// The first day with both a reserve window and a grid note.
    pub fn first_contested_day(&self) -> Option<u32> {
        (0..self.days).find(|d| self.afrr_on(*d).is_some() && !self.grid_notes_on(*d).is_empty())
    }
}

/// Daily shape, weekly seasonality, an evening peak, and the occasional hour
/// that goes negative when it is windy and demand is low.
fn generate_day_ahead(seed: u64, days: u32, start: Date) -> Vec<f64> {
    let mut rng = Rng::stream(seed, "battery.da");
    let mut out = Vec::with_capacity((days * 24) as usize);
    let mut level = 82.0_f64;

    for day in 0..days {
        let date = start.plus_days(day as i64);
        // A slow random walk in the daily level, pulled back toward the long run.
        level += rng.normal() * 6.0 + (82.0 - level) * 0.08;
        let weekend = if date.is_weekend() { -14.0 } else { 0.0 };
        // Windy days flatten the shape and can push the midday trough negative.
        let wind = rng.unit();
        let shape_scale = 1.0 - 0.55 * wind;
        let windy = wind > 0.82;

        for hour in 0..24 {
            let h = hour as f64;
            // Morning ramp and a taller evening peak.
            let morning = 16.0 * (-(((h - 8.0) / 2.4).powi(2))).exp();
            let evening = 30.0 * (-(((h - 19.0) / 2.2).powi(2))).exp();
            let night = -18.0 * (-(((h - 3.5) / 3.0).powi(2))).exp();
            let solar = -22.0 * (-(((h - 13.0) / 3.0).powi(2))).exp() * wind.max(0.35);

            let mut price = level
                + weekend
                + (morning + evening + night) * shape_scale
                + solar
                + rng.normal() * 5.5;

            // Windy shoulder-season middays occasionally clear below zero.
            if windy && (10..=15).contains(&hour) && rng.chance(0.30) {
                price = rng.range(-42.0, -2.0);
            }
            // An evening spike when the peak is tight.
            if (17..=20).contains(&hour) && rng.chance(0.045) {
                price += rng.range(60.0, 240.0);
            }
            out.push(price);
        }
    }
    out
}

/// Intraday prints wander around the day-ahead curve and occasionally dislocate.
///
/// The deviation has two components. An hourly one mean-reverts quickly, and a
/// daily one persists from day to day — a structural imbalance, not noise.
/// Without that second component the previous day's deviation would say nothing
/// about today's, and a judgment about whether a day-ahead margin estimate is
/// still believable would have nothing observable to rest on.
fn generate_intraday(seed: u64, day_ahead: &[f64]) -> Vec<f64> {
    let mut rng = Rng::stream(seed, "battery.id");
    let mut out = Vec::with_capacity(day_ahead.len() * TICKS_PER_HOUR as usize);
    let mut hourly = 0.0_f64;
    let mut daily = 0.0_f64;

    for (i, da) in day_ahead.iter().enumerate() {
        if i % 24 == 0 {
            daily = daily * 0.80 + rng.normal() * 5.0;
        }
        hourly = hourly * 0.65 + rng.normal() * 4.0;
        // A rare, sharp dislocation: the state the "margin implausible" rule is for.
        let dislocation = if rng.chance(0.012) { rng.normal() * 38.0 } else { 0.0 };

        for _ in 0..TICKS_PER_HOUR {
            out.push(da + daily + hourly + dislocation + rng.normal() * 2.2);
        }
    }
    out
}

/// A reserve obligation on roughly a third of days, over the evening peak.
fn generate_afrr(seed: u64, days: u32) -> Vec<AfrrWindow> {
    let mut rng = Rng::stream(seed, "battery.afrr");
    let mut out = Vec::new();
    for day in 0..days {
        if !rng.chance(0.34) {
            continue;
        }
        let from_hour = rng.int(16, 19) as u32;
        let to_hour = from_hour + rng.int(2, 5) as u32;
        out.push(AfrrWindow {
            day,
            from_hour,
            to_hour: to_hour.min(23),
            reserve_mw: 0.5,
            // Holding 0.5 MW for the window needs headroom above the floor.
            reserve_soc: 0.45,
            capacity_price: rng.range(6.0, 22.0),
        });
    }
    out
}

fn generate_grid_notes(seed: u64, days: u32) -> Vec<GridNote> {
    let mut rng = Rng::stream(seed, "battery.grid");
    let mut out = Vec::new();
    for day in 0..days {
        if !rng.chance(0.18) {
            continue;
        }
        let kinds = [
            GridNoteKind::ImportCapacityReduction,
            GridNoteKind::TsoNotice,
            GridNoteKind::Maintenance,
        ];
        let kind = *rng.pick(&kinds);
        let from_hour = rng.int(6, 20) as u32;
        let to_hour = (from_hour + rng.int(2, 6) as u32).min(23);
        let text = match kind {
            GridNoteKind::ImportCapacityReduction => format!(
                "Import capacity reduced to {:.0}% between {:02}:00 and {:02}:00",
                rng.range(40.0, 75.0),
                from_hour,
                to_hour
            ),
            GridNoteKind::TsoNotice => format!(
                "TSO notice: constrained feeder, curtailment possible {:02}:00-{:02}:00",
                from_hour, to_hour
            ),
            GridNoteKind::Maintenance => format!(
                "Planned maintenance on the connection {:02}:00-{:02}:00",
                from_hour, to_hour
            ),
        };
        out.push(GridNote { day, from_hour, to_hour, kind, text });
    }
    out
}

const POWER_NOISE: [&str; 8] = [
    "Utility reiterates capex guidance",
    "Weekly storage build in line with expectations",
    "Analyst note: no change to power price deck",
    "Interconnector flows normal for the season",
    "Regulator publishes routine consultation",
    "Wind forecast revised marginally",
    "Gas storage levels seasonal",
    "Trade body publishes quarterly statistics",
];

fn generate_headlines(seed: u64, days: u32, day_ahead: &[f64]) -> Vec<PowerHeadline> {
    let mut rng = Rng::stream(seed, "battery.headlines");
    let mut out = Vec::new();
    for day in 0..days {
        for _ in 0..rng.int(1, 4) {
            out.push(PowerHeadline {
                day,
                text: (*rng.pick(&POWER_NOISE)).to_owned(),
                informative: false,
            });
        }
        let prices = &day_ahead[(day * 24) as usize..(day * 24 + 24) as usize];
        let negative = prices.iter().filter(|p| **p < 0.0).count();
        let peak = prices.iter().copied().fold(f64::MIN, f64::max);
        if negative > 0 {
            out.push(PowerHeadline {
                day,
                text: format!("{negative} hours cleared below zero in the day-ahead auction"),
                informative: true,
            });
        }
        if peak > 220.0 {
            out.push(PowerHeadline {
                day,
                text: format!("Evening peak cleared at {peak:.0}/MWh"),
                informative: true,
            });
        }
    }
    out
}
