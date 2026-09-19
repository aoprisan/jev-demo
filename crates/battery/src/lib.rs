//! The battery domain: a dynamic-programming arbitrage solver under a Jev
//! judgment pipeline, and an executor that runs the day against intraday prices.
//!
//! The solver produces three complete, valid schedules. Jev ranks them, checks
//! the winner, scores the day's risk, and gates it. The comparison the demo
//! draws is between a desk that always runs the balanced schedule and one that
//! runs whatever the judgment layer chose.

#![warn(missing_docs)]

pub mod engine;
pub mod features;
pub mod judgment;
pub mod solver;

pub use engine::{execute, stood_down, summarise, BookResult, Execution};
pub use features::BatteryFeatures;
pub use judgment::{
    build_input, gate_spec, judge, rank_spec, regime_spec, risk_spec, sanity_spec, BatteryDayInput,
    BatteryJudgment, MarketRegime, ScheduleView,
};
pub use solver::{candidates, solve, Schedule, ScheduleKind};

use jev_core::{Action, Jev, Pipeline, Result};
use synth::BatteryWorld;

/// One day: what the solver offered, what Jev decided, and what each desk ran.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DayRecord {
    /// Which day.
    pub day: u32,
    /// The date.
    pub date: String,
    /// All three schedules.
    pub schedules: Vec<Schedule>,
    /// What Jev decided.
    pub judgment: BatteryJudgment,
    /// What a solver-only desk ran: the balanced schedule, always, whole.
    pub solver_only: Execution,
    /// What the gated desk ran.
    pub gated: Execution,
}

impl DayRecord {
    /// The schedule Jev's ranking chose.
    pub fn chosen(&self) -> &Schedule {
        self.schedules
            .iter()
            .find(|s| s.kind == self.judgment.chosen)
            .expect("the winner is one of the three")
    }

    /// How much the judgment layer changed the day's margin.
    pub fn margin_delta(&self) -> f64 {
        self.gated.realised_margin - self.solver_only.realised_margin
    }

    /// Whether the judgment layer changed what happened.
    pub fn intervened(&self) -> bool {
        self.judgment.chosen != ScheduleKind::Balanced
            || self.judgment.gate.action != Action::Execute
            || self.judgment.gate.size_factor < 1.0
    }
}

/// Everything one battery run produced.
#[derive(Debug)]
pub struct BatterySession {
    /// The world it ran on.
    pub seed: u64,
    /// Every day, in order.
    pub days: Vec<DayRecord>,
    /// The desk that always ran the balanced schedule.
    pub solver_only: BookResult,
    /// The desk that ran what the judgment layer chose.
    pub gated: BookResult,
}

impl BatterySession {
    /// How many days ended in each gate action.
    pub fn gate_distribution(&self) -> Vec<(Action, usize)> {
        Action::ALL
            .iter()
            .map(|a| (*a, self.days.iter().filter(|d| d.judgment.gate.action == *a).count()))
            .collect()
    }

    /// How often each schedule was ranked first.
    pub fn rank_distribution(&self) -> Vec<(ScheduleKind, usize)> {
        ScheduleKind::ALL
            .iter()
            .map(|k| (*k, self.days.iter().filter(|d| d.judgment.chosen == *k).count()))
            .collect()
    }

    /// How many days the judgment layer changed.
    pub fn interventions(&self) -> usize {
        self.days.iter().filter(|d| d.intervened()).count()
    }

    /// How many days went to a human.
    pub fn escalations(&self) -> usize {
        self.days.iter().filter(|d| d.judgment.gate.action == Action::Escalate).count()
    }

    /// The days where the judgment layer mattered most, largest first.
    pub fn most_consequential(&self, n: usize) -> Vec<&DayRecord> {
        let mut ranked: Vec<&DayRecord> = self.days.iter().filter(|d| d.intervened()).collect();
        ranked.sort_by(|a, b| b.margin_delta().abs().total_cmp(&a.margin_delta().abs()));
        ranked.into_iter().take(n).collect()
    }
}

/// Run a world's days through the solver and the judgment pipeline.
pub async fn run_session(
    jev: &Jev,
    world: &BatteryWorld,
    day_limit: Option<u32>,
) -> Result<BatterySession> {
    let horizon = day_limit.unwrap_or(world.days).min(world.days);
    let mut days = Vec::with_capacity(horizon as usize);

    for day in 0..horizon {
        days.push(judge_day(jev, world, day).await?);
    }

    let solver_only = summarise(days.iter().map(|d| &d.solver_only));
    let gated = summarise(days.iter().map(|d| &d.gated));
    Ok(BatterySession { seed: world.seed, days, solver_only, gated })
}

/// Solve, judge and execute one day.
pub async fn judge_day(jev: &Jev, world: &BatteryWorld, day: u32) -> Result<DayRecord> {
    let schedules = candidates(world.day_ahead_for(day), &world.asset, world.afrr_on(day));
    let input = build_input(world, day, &schedules);

    let mut pipeline = Pipeline::new(jev, "battery");
    let (judgment, _post_rank) = judge(&mut pipeline, world, day, &schedules, &input).await?;

    let balanced = schedules
        .iter()
        .find(|s| s.kind == ScheduleKind::Balanced)
        .expect("balanced is always solved");
    let chosen = schedules
        .iter()
        .find(|s| s.kind == judgment.chosen)
        .expect("the winner is one of the three");

    let solver_only = execute(world, day, balanced, 1.0);
    let gated = if judgment.gate.action.acts() {
        execute(world, day, chosen, judgment.gate.size_factor as f64)
    } else {
        stood_down(day, chosen)
    };

    Ok(DayRecord {
        day,
        date: world.start.plus_days(day as i64).to_string(),
        schedules,
        judgment,
        solver_only,
        gated,
    })
}

/// The same world with a grid notice added over `hours` on `day`.
///
/// The demo replays one day with and without a notice and prints the ranking
/// and the gate side by side. Only the note changes; the prices are identical,
/// so any flip in the ranking is the judgment layer reacting to the note.
pub fn with_grid_notice(
    world: &BatteryWorld,
    day: u32,
    from_hour: u32,
    to_hour: u32,
) -> BatteryWorld {
    let mut clone = world.clone();
    clone.grid_notes.retain(|n| n.day != day);
    clone.grid_notes.push(synth::GridNote {
        day,
        from_hour,
        to_hour,
        kind: synth::GridNoteKind::ImportCapacityReduction,
        text: format!(
            "Import capacity reduced to 55% between {from_hour:02}:00 and {to_hour:02}:00"
        ),
    });
    clone.grid_notes.sort_by_key(|n| (n.day, n.from_hour));
    clone
}

/// The same world with every grid note on `day` removed.
pub fn without_grid_notices(world: &BatteryWorld, day: u32) -> BatteryWorld {
    let mut clone = world.clone();
    clone.grid_notes.retain(|n| n.day != day);
    clone
}

/// The first day inside `horizon` that carries no grid note.
///
/// The replay adds a notice to a day that had none, so the difference between
/// the two runs is the notice and nothing else. Returns `None` when every day
/// already carries one.
pub fn first_quiet_day(world: &BatteryWorld, horizon: u32) -> Option<u32> {
    (0..horizon.min(world.days)).find(|d| world.grid_notes_on(*d).is_empty())
}
