//! The battery judgment pipeline:
//! `Classify(regime) -> Rank(schedules) -> Check(sanity) -> Score(risk) -> Gate`.
//!
//! The ranking is the stage that distinguishes this domain from the forex one.
//! The solver produces three complete, valid schedules and Jev orders them; the
//! later stages then judge the one that came first.

use crate::features::{afrr_line, note_line, BatteryFeatures};
use crate::solver::{Schedule, ScheduleKind};
use jev_core::{
    Candidate, CheckOut, CheckSpec, ClassifyOut, ClassifySpec, GateOut, GateSpec, JevInput, Label,
    Pipeline, RankOut, RankSpec, Result, ScoreOut, ScoreSpec,
};
use serde::{Deserialize, Serialize};
use synth::BatteryWorld;

/// The regime a battery day is being traded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketRegime {
    /// An ordinary day: the shape is familiar and the market absorbs size.
    Normal,
    /// Wide and moving: the spread is there, but so is the risk of being wrong.
    Volatile,
    /// Thin: prints scatter and a position cannot be unwound quickly.
    Illiquid,
    /// Disorderly: prices have come away from anything the curve implies.
    Stressed,
}

impl Label for MarketRegime {
    fn labels() -> &'static [Self] {
        &[
            MarketRegime::Normal,
            MarketRegime::Volatile,
            MarketRegime::Illiquid,
            MarketRegime::Stressed,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            MarketRegime::Normal => "normal",
            MarketRegime::Volatile => "volatile",
            MarketRegime::Illiquid => "illiquid",
            MarketRegime::Stressed => "stressed",
        }
    }

    fn describe(&self) -> &'static str {
        match self {
            MarketRegime::Normal => {
                "The day-ahead shape is the usual one, dispersion is unremarkable, and \
                 intraday has been tracking the curve."
            }
            MarketRegime::Volatile => {
                "Dispersion is high: the spread on offer is wide, and so is the range of \
                 outcomes around any plan built on it."
            }
            MarketRegime::Illiquid => {
                "Prints are scattered and the market is thin. A position taken now may not \
                 be unwindable at anything near the price that justified it."
            }
            MarketRegime::Stressed => {
                "Intraday has come away from the day-ahead curve by more than recent \
                 variation explains, or the grid is signalling a constraint. The curve is \
                 no longer a good description of what will actually clear."
            }
        }
    }
}

/// One battery day, as Jev sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatteryDayInput {
    /// The observable state.
    pub features: BatteryFeatures,
    /// All three schedules the solver produced. Every one is valid.
    pub candidates: Vec<ScheduleView>,
    /// The reserve obligation, if there is one.
    pub afrr: Option<String>,
    /// Notes from the grid operator.
    pub grid_notes: Vec<String>,
    /// Today's headlines, mostly noise.
    pub headlines: Vec<String>,
}

/// A schedule as Jev sees it: what it does, not how it was derived.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleView {
    /// Which variant.
    pub kind: String,
    /// Margin the day-ahead curve implies, net of degradation.
    pub expected_margin: f64,
    /// Equivalent full cycles.
    pub cycles: f64,
    /// Lowest state of charge reached, MWh.
    pub min_soc_mwh: f64,
    /// The hours it discharges over.
    pub discharge_block: Option<String>,
    /// Whether it falls below the reserve state of charge inside the window.
    pub dips_below_reserve: bool,
    /// A sentence describing it.
    pub summary: String,
}

impl ScheduleView {
    /// Render a schedule for Jev.
    pub fn of(schedule: &Schedule, world: &BatteryWorld, day: u32) -> Self {
        let dips = match world.afrr_on(day) {
            Some(w) => schedule.dips_below(w, w.reserve_soc * world.asset.capacity_mwh),
            None => false,
        };
        Self {
            kind: schedule.kind.as_str().to_owned(),
            expected_margin: round(schedule.expected_margin, 2),
            cycles: round(schedule.cycles, 3),
            min_soc_mwh: round(schedule.min_soc(), 3),
            discharge_block: schedule
                .discharge_block()
                .map(|(a, b)| format!("{a:02}:00-{b:02}:00")),
            dips_below_reserve: dips,
            summary: schedule.summary(),
        }
    }
}

fn round(v: f64, places: u32) -> f64 {
    let f = 10f64.powi(places as i32);
    (v * f).round() / f
}

impl JevInput for BatteryDayInput {
    fn context_block(&self) -> String {
        let f = &self.features;
        let mut s = format!(
            "Domain: a 1 MW / 2 MWh grid battery trading day-ahead arbitrage. A \
             deterministic solver has produced three complete, valid schedules for {} \
             ({}). All three respect the state-of-charge bounds; they differ in how much \
             headroom they keep and how hard they cycle.\n\
             Conditions: day-ahead spread {:.0} per MWh, peaking at {:.0}. Dispersion at \
             the {:.0}th percentile of the last ten days. Intraday settled {:+.1} sigma \
             from its curve yesterday. {} negative-price hour(s).\n",
            f.date,
            f.day,
            f.day_ahead_spread,
            f.peak_price,
            f.volatility_percentile * 100.0,
            f.id_deviation_sigmas,
            f.negative_price_hours,
        );
        match &self.afrr {
            Some(line) => s.push_str(&format!("Reserve obligation: {line}\n")),
            None => s.push_str("No reserve obligation today.\n"),
        }
        if self.grid_notes.is_empty() {
            s.push_str("No notes from the grid operator.\n");
        } else {
            s.push_str("Grid operator:\n");
            for note in &self.grid_notes {
                s.push_str(&format!("- {note}\n"));
            }
        }
        s.push_str(
            "The solver owns the energy in every schedule. You are judging which one \
             suits today, and whether it should run.",
        );
        s
    }
}

/// Everything the pipeline decided about one day.
#[derive(Debug, Clone, Serialize)]
pub struct BatteryJudgment {
    /// Which regime Jev read.
    pub regime: ClassifyOut<MarketRegime>,
    /// The ranking of the three schedules.
    pub ranking: RankOut<ScheduleKind>,
    /// The schedule that came first.
    pub chosen: ScheduleKind,
    /// The sanity checks, on the chosen schedule.
    pub checks: CheckOut,
    /// Risk, 0..=100.
    pub risk: ScoreOut,
    /// The gate, on the chosen schedule.
    pub gate: GateOut,
}

/// The classify stage.
pub fn regime_spec() -> ClassifySpec {
    ClassifySpec::new(
        "today's power market",
        "Which regime is this market in? Judge from the day-ahead shape, how far \
         intraday has been settling from it, and anything the grid operator has said.",
    )
}

/// The rank stage.
pub fn rank_spec(candidates: &[Schedule], views: &[ScheduleView]) -> RankSpec<ScheduleKind> {
    RankSpec::new(
        "today's schedules",
        "Order these schedules by how well they suit today's conditions, best first. \
         All three are valid and the energy in each is fixed; you are choosing which \
         set of trade-offs today deserves.",
        candidates
            .iter()
            .zip(views)
            .map(|(schedule, view)| Candidate::new(schedule.kind, view.summary.clone()))
            .collect(),
    )
}

/// The three sanity checks, exactly as the brief names them.
pub fn sanity_spec(chosen: ScheduleKind) -> CheckSpec {
    CheckSpec::new(format!("the {chosen} schedule"))
        .item(
            "reserve_ok",
            "The chosen schedule keeps the reserve available for the whole commitment window.",
            "the state of charge stays at or above the reserve level throughout",
            "the schedule draws the battery below the reserve level inside the window",
        )
        .item(
            "margin_plausible",
            "The margin this schedule expects is still believable given where intraday \
             has actually been settling.",
            "intraday has been tracking the day-ahead curve closely enough for the \
             estimate to mean something",
            "intraday has come away from the curve by more than recent variation \
             explains, so a day-ahead margin estimate is not reliable",
        )
        .item(
            "cycle_budget_ok",
            "The cycles this schedule uses sit comfortably inside the daily budget.",
            "there is room left in the budget after today",
            "the schedule spends most or all of the day's cycle budget",
        )
}

/// The risk score and its driver vocabulary.
pub fn risk_spec() -> ScoreSpec {
    ScoreSpec::new(
        "operational risk",
        "How much risk does running this schedule today carry, given the conditions, \
         the reserve obligation and anything the grid operator has said?",
        [
            "Nothing unusual: an ordinary day for an ordinary plan.",
            "Minor: one condition worth noting, none of it binding.",
            "Real: a constraint that will bind, or an estimate that may not hold.",
            "High: the reserve is at risk, or the curve has stopped describing the market.",
            "Severe: several of those at once.",
        ],
    )
    .driver("reserve_at_risk", "The reserve commitment could be missed if this schedule runs.")
    .driver(
        "price_dislocation",
        "Intraday has come away from the day-ahead curve by more than usual.",
    )
    .driver("cycles_nearly_spent", "This schedule uses most of the day's cycle budget.")
    .driver("grid_constraint", "A grid note overlaps the hours this schedule discharges in.")
    .driver("negative_price_hours", "The day-ahead curve has hours clearing below zero.")
}

/// The gate.
pub fn gate_spec(chosen: ScheduleKind, day: u32) -> GateSpec {
    GateSpec::new(
        format!("the {chosen} schedule for day {day}"),
        "Should this schedule run as the solver built it, and if not, what instead? \
         Reducing means running the same shape at a smaller fraction of its power.",
    )
}

/// Run the full battery pipeline over one day.
pub async fn judge(
    pipeline: &mut Pipeline,
    world: &BatteryWorld,
    day: u32,
    schedules: &[Schedule],
    pre_rank: &BatteryDayInput,
) -> Result<(BatteryJudgment, BatteryDayInput)> {
    let regime: ClassifyOut<MarketRegime> =
        pipeline.classify("regime", pre_rank, &regime_spec()).await?;

    let ranking: RankOut<ScheduleKind> = pipeline
        .rank("rank", pre_rank, &rank_spec(schedules, &pre_rank.candidates))
        .await?;
    let chosen = ranking.top().expect("a ranking of three is never empty").id;

    // From here the pipeline judges the schedule that won, so the
    // schedule-level features are rebuilt to describe it.
    let chosen_schedule =
        schedules.iter().find(|s| s.kind == chosen).expect("the winner is one of the three");
    let post_rank = BatteryDayInput {
        features: BatteryFeatures::compute(world, day, chosen_schedule),
        ..pre_rank.clone()
    };

    let checks = pipeline.check("sanity", &post_rank, &sanity_spec(chosen)).await?;
    let risk = pipeline.score("risk", &post_rank, &risk_spec()).await?;
    let gate = pipeline.gate("gate", &post_rank, &gate_spec(chosen, day)).await?;

    Ok((BatteryJudgment { regime, ranking, chosen, checks, risk, gate }, post_rank))
}

/// Build the pre-ranking input for a day.
///
/// The schedule-level features describe the balanced schedule: the one a
/// solver-only desk would run without asking anybody.
pub fn build_input(
    world: &BatteryWorld,
    day: u32,
    schedules: &[Schedule],
) -> BatteryDayInput {
    let default = schedules
        .iter()
        .find(|s| s.kind == ScheduleKind::Balanced)
        .expect("balanced is always solved");
    BatteryDayInput {
        features: BatteryFeatures::compute(world, day, default),
        candidates: schedules.iter().map(|s| ScheduleView::of(s, world, day)).collect(),
        afrr: world.afrr_on(day).map(|w| afrr_line(w, &world.asset)),
        grid_notes: world.grid_notes_on(day).iter().map(|n| note_line(n)).collect(),
        headlines: world.headlines_on(day).iter().map(|h| h.text.clone()).collect(),
    }
}
