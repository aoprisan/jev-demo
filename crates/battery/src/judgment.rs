//! The battery judgment pipeline, in three calls:
//! `[Classify(regime), Rank(schedules)] -> [Check(sanity), Score(risk)] -> Gate`.
//!
//! The ranking is the stage that distinguishes this domain from the forex one.
//! The solver produces three complete, valid schedules and Jev rates them; the
//! later stages then judge the one that came first, which is why they cannot
//! share the first call: the state they read is rebuilt around the winner.

use crate::features::{afrr_line, note_line, BatteryFeatures};
use crate::solver::{Schedule, ScheduleKind};
use jev_core::{
    Candidate, CheckOut, CheckSpec, ClassifyOut, ClassifySpec, GateOut, GateSpec, JevInput, Label,
    Pipeline, RankOut, RankSpec, Result, ReviewPolicy, ScoreOut, ScoreSpec,
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
    /// Today's headlines, as printed. Most are noise; the model is the one to
    /// tell which.
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
    /// Only what the fields do not already say: what the asset is, which solver
    /// built the schedules, and that their energy is final. The conditions, the
    /// obligation and the grid notes are fields of the input.
    fn context_block(&self) -> String {
        format!(
            "A 1 MW / 2 MWh grid battery trading day-ahead arbitrage. `candidates` are three \
             complete, valid schedules for {} built by a deterministic solver; all respect the \
             state-of-charge bounds and differ in headroom and cycling, and the energy in each \
             is final. `features` describes the day and the `{}` schedule; `afrr` is today's \
             reserve obligation and `grid_notes` what the operator said.",
            self.features.date, self.features.under_consideration,
        )
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
    /// Why a person should look at this day, when the certainty behind it was
    /// thin. Set by [`REVIEW`] in code; it never changes the gate.
    pub review: Option<String>,
}

/// When a battery day is flagged for review.
pub const REVIEW: ReviewPolicy =
    ReviewPolicy { min_classify_confidence: 0.10, undecided_check_band: (0.4, 0.6) };

/// The classify stage.
pub fn regime_spec() -> ClassifySpec {
    ClassifySpec::new(
        "today's power market",
        "Which regime is this market in? Judge from the day-ahead shape, how far \
         intraday has been settling from it, and anything the grid operator has said.",
    )
}

/// The rank stage: one fit rating per schedule, ordered in code.
pub fn rank_spec(candidates: &[Schedule], views: &[ScheduleView]) -> RankSpec<ScheduleKind> {
    RankSpec::new(
        "today's schedules",
        "How well does this schedule suit today's conditions? It is one of the three in \
         `candidates`; all are valid and the energy in each is fixed. Rate the trade-offs \
         it makes against the day.",
        candidates
            .iter()
            .zip(views)
            .map(|(schedule, view)| Candidate::new(schedule.kind, view.summary.clone()))
            .collect(),
    )
}

/// The three sanity checks, exactly as the brief names them.
///
/// `reserve_ok` and `cycle_budget_ok` are comparisons the solver's own
/// numbers settle — the state of charge against the reserve floor inside the
/// window, the cycles against the budget — so the battery crate makes them
/// ([`BatteryFeatures::reserve_breached`], [`BatteryFeatures::cycles_within_budget`])
/// and reports them in the same shape. `margin_plausible` is a judgment about
/// whether an estimate still means anything, and goes to Jev.
pub fn sanity_spec(chosen: ScheduleKind, features: &BatteryFeatures) -> CheckSpec {
    CheckSpec::new(format!("the {chosen} schedule"))
        .rule(
            "reserve_ok",
            "The chosen schedule keeps the reserve available for the whole commitment window.",
            "the state of charge stays at or above the reserve level throughout",
            "the schedule draws the battery below the reserve level inside the window",
            !features.reserve_breached,
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
        .rule(
            "cycle_budget_ok",
            "The cycles this schedule uses sit comfortably inside the daily budget.",
            "there is room left in the budget after today",
            "the schedule spends most or all of the day's cycle budget",
            features.cycles_within_budget,
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
            "Severe: the reserve commitment is threatened while intraday prices are dislocated from the day-ahead curve or grid constraints obstruct the schedule.",
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

/// Run the full battery pipeline over one day, in three calls: the regime and
/// the ranking together, then the checks and the risk on the schedule that
/// won, then the gate.
pub async fn judge(
    pipeline: &mut Pipeline,
    world: &BatteryWorld,
    day: u32,
    schedules: &[Schedule],
    pre_rank: &BatteryDayInput,
) -> Result<(BatteryJudgment, BatteryDayInput)> {
    let mut read = pipeline.batch("read", pre_rank);
    let regime = read.classify::<MarketRegime>("regime", &regime_spec())?;
    let ranking = read.rank("rank", &rank_spec(schedules, &pre_rank.candidates))?;
    let mut read = read.send().await?;
    let regime: ClassifyOut<MarketRegime> = read.take(regime)?;
    let ranking: RankOut<ScheduleKind> = read.take(ranking)?;
    let chosen = ranking.top().expect("a ranking of three is never empty").id;

    // From here the pipeline judges the schedule that won, so the
    // schedule-level features are rebuilt to describe it.
    let chosen_schedule =
        schedules.iter().find(|s| s.kind == chosen).expect("the winner is one of the three");
    let post_rank = BatteryDayInput {
        features: BatteryFeatures::compute(world, day, chosen_schedule),
        ..pre_rank.clone()
    };

    let mut assess = pipeline.batch("assess", &post_rank);
    let checks = assess.check("sanity", &sanity_spec(chosen, &post_rank.features))?;
    let risk = assess.score("risk", &risk_spec())?;
    let mut assessed = assess.send().await?;
    let checks = assessed.take(checks)?;
    let risk = assessed.take(risk)?;

    let gate = pipeline.gate("gate", &post_rank, &gate_spec(chosen, day)).await?;
    let review = REVIEW.review(regime.confidence, &checks);

    Ok((BatteryJudgment { regime, ranking, chosen, checks, risk, gate, review }, post_rank))
}

/// Build the pre-ranking input for a day.
///
/// The schedule-level features describe the balanced schedule: the one a
/// solver-only desk would run without asking anybody.
pub fn build_input(world: &BatteryWorld, day: u32, schedules: &[Schedule]) -> BatteryDayInput {
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
