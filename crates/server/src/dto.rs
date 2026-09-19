//! The wire types.
//!
//! Every response the API can produce is one of these structs. They are the
//! contract the TypeScript client mirrors in `ui/src/api/types.ts`, so the
//! field names here are the field names there — nothing reaches the browser as
//! an untyped blob except the raw audit record, which is deliberately opaque.

use serde::{Deserialize, Serialize};

/// Which desk a run exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    /// Spot forex: mean reversion under a four-stage pipeline.
    Fx,
    /// Battery energy trading: a DP arbitrage solver under a five-stage pipeline.
    Battery,
    /// Both, then one Explain call over the pair for Compliance.
    All,
}

impl Domain {
    /// The wire label, matching the serde representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Domain::Fx => "fx",
            Domain::Battery => "battery",
            Domain::All => "all",
        }
    }
}

/// What a client asks for when it starts a run.
#[derive(Debug, Clone, Deserialize)]
pub struct RunRequest {
    /// Which desk.
    pub domain: Domain,
    /// Seed for the synthetic worlds.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Days to generate.
    #[serde(default)]
    pub days: Option<u32>,
    /// Judge only the first N days. Keeps a live run's call count bounded.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Use the offline rule-based backend rather than the System One API.
    #[serde(default)]
    pub mock: Option<bool>,
}

/// A request with every default resolved, as the run actually ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RunSpec {
    /// Which desk.
    pub domain: Domain,
    /// The seed the worlds were generated from.
    pub seed: u64,
    /// Days generated.
    pub days: u32,
    /// Days judged, when fewer than generated.
    pub limit: Option<u32>,
    /// Whether the offline backend answered.
    pub mock: bool,
}

/// Where a run has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Still judging.
    Running,
    /// Finished; `result` is present.
    Done,
    /// Failed; `error` says why.
    Failed,
}

/// A run without its result: enough for a list, and for polling.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    /// Server-assigned identifier.
    pub id: String,
    /// What was asked for, with defaults resolved.
    pub spec: RunSpec,
    /// Where it has got to.
    pub status: RunStatus,
    /// Typed calls made so far. Rises while a run is `running`.
    pub calls: usize,
    /// Milliseconds since the Unix epoch when it started.
    pub started_at_ms: u128,
    /// The same, when it finished.
    pub finished_at_ms: Option<u128>,
    /// Why it failed, when it did.
    pub error: Option<String>,
}

/// A run and, once it is done, everything it produced.
#[derive(Debug, Clone, Serialize)]
pub struct RunView {
    /// The summary half.
    #[serde(flatten)]
    pub summary: RunSummary,
    /// The result, once the status is `done`.
    pub result: Option<RunResult>,
}

/// What a finished run produced.
#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    /// The forex desk, for the `fx` and `all` domains.
    pub fx: Option<FxResult>,
    /// The battery desk, for the `battery` and `all` domains.
    pub battery: Option<BatteryResult>,
    /// One Explain call over both desks, for the `all` domain.
    pub compliance: Option<ExplainView>,
    /// Tokens and latency across every typed call the run made.
    pub cost: Cost,
}

/// What the judgment layer cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Cost {
    /// Typed calls made.
    pub calls: usize,
    /// Tokens reported, input plus output.
    pub tokens: u64,
    /// Wall-clock milliseconds across every call.
    pub latency_ms: u64,
}

/// An Explain output as the UI shows it.
#[derive(Debug, Clone, Serialize)]
pub struct ExplainView {
    /// The composed prose.
    pub summary: String,
    /// Who it was written for.
    pub for_audience: String,
    /// How certain the framing was.
    pub confidence: f32,
}

// ---------------------------------------------------------------------------
// Forex
// ---------------------------------------------------------------------------

/// Everything one forex run produced.
#[derive(Debug, Clone, Serialize)]
pub struct FxResult {
    /// Candidates judged.
    pub decisions_judged: usize,
    /// The book that ignored the judgment layer.
    pub ungated: FxBook,
    /// The book that obeyed it.
    pub gated: FxBook,
    /// How many candidates ended in each gate action.
    pub gate_distribution: Vec<ActionCount>,
    /// How often the classifier agreed with the generator's hidden regime.
    pub regime_accuracy: f64,
    /// Decisions the gate changed at all.
    pub interventions: usize,
    /// Decisions sent to a human.
    pub escalations: usize,
    /// Decisions the review policy flagged on thin certainty. The gate stands.
    pub reviews: usize,
    /// Named checks that failed, across every decision.
    pub failed_checks: usize,
    /// Each named check held up against the outcomes it flagged.
    pub scorecard: Vec<CheckScorecardView>,
    /// Cumulative P&L for both books, in trade order.
    pub equity: Vec<EquityPoint>,
    /// One row per candidate, in time order.
    pub decisions: Vec<FxDecisionRow>,
    /// The CPI day judged twice, with the release and without it.
    pub replay: Option<FxReplay>,
}

/// A forex book's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FxBook {
    /// Trades actually put on.
    pub trades: usize,
    /// Total profit and loss, in quote currency.
    pub pnl: f64,
    /// Trades that made money.
    pub wins: usize,
    /// Trades that lost money.
    pub losses: usize,
    /// Fraction of trades that made money.
    pub hit_rate: f64,
    /// Worst peak-to-trough fall in cumulative P&L.
    pub max_drawdown: f64,
    /// Total notional put on.
    pub notional: f64,
    /// P&L per unit of notional, in basis points.
    pub return_bps: f64,
}

/// How many decisions ended in one gate action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionCount {
    /// `execute`, `reduce`, `hold` or `escalate`.
    pub action: String,
    /// How many.
    pub count: usize,
}

/// How one named check fared against the outcomes it flagged.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckScorecardView {
    /// The check's name.
    pub name: String,
    /// Decisions where it failed.
    pub failed_n: usize,
    /// Their mean ungated outcome, in basis points.
    pub failed_bps: f64,
    /// Their ungated hit rate.
    pub failed_hit: f64,
    /// Decisions where it held.
    pub passed_n: usize,
    /// Their mean ungated outcome, in basis points.
    pub passed_bps: f64,
    /// Their ungated hit rate.
    pub passed_hit: f64,
    /// How much worse the flagged decisions did. Negative means inverted.
    pub edge_bps: f64,
    /// Whether the check is flagging the better decisions.
    pub inverted: bool,
}

/// One point on the cumulative P&L curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EquityPoint {
    /// Which decision, by index into `decisions`.
    pub index: usize,
    /// Day of the simulation.
    pub day: u32,
    /// Cumulative ungated P&L after this decision.
    pub ungated: f64,
    /// Cumulative gated P&L after this decision.
    pub gated: f64,
}

/// One candidate, its judgment and what each book did with it.
#[derive(Debug, Clone, Serialize)]
pub struct FxDecisionRow {
    /// Index into the run's decisions, and the id of its detail endpoint.
    pub index: usize,
    /// Day of the simulation.
    pub day: u32,
    /// Hour of that day.
    pub hour: u32,
    /// The calendar date.
    pub date: String,
    /// Which pair.
    pub pair: String,
    /// Which way.
    pub side: String,
    /// Entry price.
    pub price: f64,
    /// Protective stop.
    pub stop: f64,
    /// Profit target.
    pub target: f64,
    /// Notional units the solver asked for, in thousands.
    pub size_units: f64,
    /// The regime Jev classified.
    pub regime: String,
    /// How certain that call was.
    pub regime_confidence: f32,
    /// The generator's regime. Evaluation only; no Jev call ever saw it.
    pub true_regime: String,
    /// Whether the classifier agreed with the generator.
    pub regime_correct: bool,
    /// Event risk, 0..=100.
    pub risk_score: u8,
    /// The named checks that failed.
    pub checks_failed: Vec<String>,
    /// What the gate did.
    pub action: String,
    /// How much size it allowed, 0..=1.
    pub size_factor: f32,
    /// The gate's composed reason.
    pub reason: String,
    /// What the ungated book made on this candidate.
    pub ungated_pnl: Option<f64>,
    /// What the gated book made.
    pub gated_pnl: Option<f64>,
    /// The difference the judgment layer made.
    pub pnl_delta: f64,
    /// Whether the gate changed anything at all.
    pub intervened: bool,
}

/// One candidate in full: the features Jev read and every stage's output.
#[derive(Debug, Clone, Serialize)]
pub struct FxDecisionDetail {
    /// The row this expands.
    pub row: FxDecisionRow,
    /// The only thing the judgment layer reads. The generator's truth is not in here.
    pub features: fx::FxFeatures,
    /// The day's headlines, as they were passed in.
    pub headlines: Vec<String>,
    /// Stage one: the regime.
    pub regime: ClassifyView,
    /// Stage two: the named plausibility checks.
    pub checks: Vec<CheckView>,
    /// Stage three: event risk.
    pub risk: ScoreView,
    /// Stage four: the gate.
    pub gate: GateView,
    /// Why the review policy flagged this decision, if it did.
    pub review: Option<String>,
    /// The fill the ungated book got.
    pub ungated: Option<FillView>,
    /// The fill the gated book got.
    pub gated: Option<FillView>,
}

/// One filled trade.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FillView {
    /// When it was entered, as a day and hour.
    pub entered_day: u32,
    /// Hour of entry.
    pub entered_hour: u32,
    /// When it closed.
    pub exited_day: u32,
    /// Hour of the close.
    pub exited_hour: u32,
    /// `target`, `stop`, `timeout` or `end_of_data`.
    pub exit: String,
    /// Entry price.
    pub entry_price: f64,
    /// Exit price.
    pub exit_price: f64,
    /// The fraction of the solver's size actually put on.
    pub size_factor: f64,
    /// The notional actually put on.
    pub notional: f64,
    /// Profit or loss in quote currency.
    pub pnl: f64,
    /// The same, in basis points of that notional.
    pub pnl_bps: f64,
}

/// The CPI day judged twice: once with the release, once without it.
#[derive(Debug, Clone, Serialize)]
pub struct FxReplay {
    /// Which pair the replayed candidate traded.
    pub pair: String,
    /// The calendar date.
    pub date: String,
    /// Entry price of the candidate, identical across both panes.
    pub price: f64,
    /// Its stop.
    pub stop: f64,
    /// Its target.
    pub target: f64,
    /// Which way it traded.
    pub side: String,
    /// With the release on the calendar.
    pub with_event: FxReplayPane,
    /// With every event on that day removed. Same bars, same candidate.
    pub without_event: FxReplayPane,
}

/// One side of the forex replay.
#[derive(Debug, Clone, Serialize)]
pub struct FxReplayPane {
    /// What to call this pane.
    pub label: String,
    /// The next release as the features rendered it, or `none scheduled`.
    pub event: String,
    /// Event risk, 0..=100.
    pub risk_score: u8,
    /// The gate.
    pub gate: GateView,
}

// ---------------------------------------------------------------------------
// Battery
// ---------------------------------------------------------------------------

/// Everything one battery run produced.
#[derive(Debug, Clone, Serialize)]
pub struct BatteryResult {
    /// Days judged.
    pub days_judged: usize,
    /// The desk that ran the balanced schedule every day.
    pub solver_only: BatteryBook,
    /// The desk that ran what the judgment layer chose.
    pub gated: BatteryBook,
    /// How many days ended in each gate action.
    pub gate_distribution: Vec<ActionCount>,
    /// How often each schedule was ranked first.
    pub rank_distribution: Vec<ScheduleCount>,
    /// Days the judgment layer changed.
    pub interventions: usize,
    /// Days sent to a human.
    pub escalations: usize,
    /// Days the review policy flagged on thin certainty. The gate stands.
    pub reviews: usize,
    /// Named checks that failed, across every day.
    pub failed_checks: usize,
    /// Cumulative margin for both desks, in day order.
    pub margin_curve: Vec<MarginPoint>,
    /// One row per day.
    pub days: Vec<BatteryDayRow>,
    /// One day judged twice, with a grid notice and without it.
    pub replay: Option<BatteryReplay>,
}

/// A battery desk's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BatteryBook {
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
    /// Margin per equivalent cycle.
    pub margin_per_cycle: f64,
    /// Worst peak-to-trough fall in cumulative margin.
    pub max_drawdown: f64,
}

/// How often one schedule was ranked first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleCount {
    /// `aggressive`, `balanced` or `reserve_heavy`.
    pub schedule: String,
    /// How many days.
    pub count: usize,
}

/// One point on the cumulative margin curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct MarginPoint {
    /// Day of the simulation.
    pub day: u32,
    /// Cumulative solver-only margin.
    pub solver_only: f64,
    /// Cumulative gated margin.
    pub gated: f64,
}

/// One day: what the solver offered, what Jev decided, what each desk ran.
#[derive(Debug, Clone, Serialize)]
pub struct BatteryDayRow {
    /// Day of the simulation, and the id of its detail endpoint.
    pub day: u32,
    /// The calendar date.
    pub date: String,
    /// The regime Jev classified.
    pub regime: String,
    /// How certain that call was.
    pub regime_confidence: f32,
    /// The schedule the ranking put first.
    pub chosen: String,
    /// The margin between the first and second schedule.
    pub rank_margin: f32,
    /// Operational risk, 0..=100.
    pub risk_score: u8,
    /// The named checks that failed.
    pub checks_failed: Vec<String>,
    /// What the gate did.
    pub action: String,
    /// How much size it allowed, 0..=1.
    pub size_factor: f32,
    /// The gate's composed reason.
    pub reason: String,
    /// What the solver-only desk realised.
    pub solver_margin: f64,
    /// What the gated desk realised.
    pub gated_margin: f64,
    /// The difference the judgment layer made.
    pub margin_delta: f64,
    /// Hours the solver-only desk missed the reserve.
    pub solver_breaches: u32,
    /// Hours the gated desk missed it.
    pub gated_breaches: u32,
    /// Equivalent cycles the gated desk used.
    pub gated_cycles: f64,
    /// Whether the judgment layer changed anything at all.
    pub intervened: bool,
}

/// One day in full: all three schedules, every stage's output, both executions.
#[derive(Debug, Clone, Serialize)]
pub struct BatteryDayDetail {
    /// The row this expands.
    pub row: BatteryDayRow,
    /// All three schedules the solver produced.
    pub schedules: Vec<ScheduleView>,
    /// Stage one: the regime.
    pub regime: ClassifyView,
    /// Stage two: the ranking, in order.
    pub ranking: Vec<RankedView>,
    /// Stage three: the named plausibility checks.
    pub checks: Vec<CheckView>,
    /// Stage four: operational risk.
    pub risk: ScoreView,
    /// Stage five: the gate.
    pub gate: GateView,
    /// Why the review policy flagged this day, if it did.
    pub review: Option<String>,
    /// What the solver-only desk ran.
    pub solver_only: ExecutionView,
    /// What the gated desk ran.
    pub gated: ExecutionView,
}

/// One schedule the solver produced.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleView {
    /// `aggressive`, `balanced` or `reserve_heavy`.
    pub kind: String,
    /// Signed power for each of the 24 hours, MW. Positive charges.
    pub power_mw: Vec<f64>,
    /// State of charge at the start of each hour plus the end of the day, MWh.
    pub soc_mwh: Vec<f64>,
    /// Margin the day-ahead curve implies, net of degradation.
    pub expected_margin: f64,
    /// Equivalent full cycles the plan uses.
    pub cycles: f64,
    /// Degradation cost the plan incurs.
    pub degradation_cost: f64,
    /// The lowest state of charge the plan reaches.
    pub min_soc_mwh: f64,
}

/// What running a schedule actually produced.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionView {
    /// Which schedule ran.
    pub schedule: String,
    /// The fraction of the plan's power actually run.
    pub size_factor: f64,
    /// Cash from energy, at intraday prices.
    pub energy_margin: f64,
    /// Payment for holding the reserve.
    pub reserve_payment: f64,
    /// Degradation cost incurred.
    pub degradation_cost: f64,
    /// Energy margin plus reserve payment, less degradation.
    pub realised_margin: f64,
    /// What the day-ahead curve said the plan was worth.
    pub expected_margin: f64,
    /// State of charge through the day, 25 values in MWh.
    pub soc_mwh: Vec<f64>,
    /// Hours inside the reserve window where the charge fell short.
    pub reserve_breaches: u32,
    /// Equivalent full cycles used.
    pub cycles: f64,
}

/// One day judged twice: once quiet, once with a grid notice over the
/// discharge block.
#[derive(Debug, Clone, Serialize)]
pub struct BatteryReplay {
    /// Which day.
    pub day: u32,
    /// The calendar date.
    pub date: String,
    /// First hour the notice covers.
    pub from_hour: u32,
    /// Last hour it covers.
    pub to_hour: u32,
    /// The notice, as the judgment layer read it.
    pub notice: String,
    /// With every grid note on that day removed.
    pub quiet: BatteryReplayPane,
    /// With the notice added. Same prices, same three schedules.
    pub noticed: BatteryReplayPane,
    /// Whether the ranking changed its mind.
    pub flipped: bool,
}

/// One side of the battery replay.
#[derive(Debug, Clone, Serialize)]
pub struct BatteryReplayPane {
    /// What to call this pane.
    pub label: String,
    /// The schedule the ranking put first.
    pub chosen: String,
    /// The ranking, in order.
    pub ranking: Vec<RankedView>,
    /// Operational risk, 0..=100.
    pub risk_score: u8,
    /// The named plausibility checks.
    pub checks: Vec<CheckView>,
    /// Whether the reserve was honoured by what ran.
    pub reserve_held: bool,
    /// The gate.
    pub gate: GateView,
}

// ---------------------------------------------------------------------------
// The primitive outputs, as the UI shows them
// ---------------------------------------------------------------------------

/// A `Classify` output.
#[derive(Debug, Clone, Serialize)]
pub struct ClassifyView {
    /// The label Jev chose.
    pub label: String,
    /// How certain it was.
    pub confidence: f32,
    /// Composed from the verdict, never free text.
    pub reason: String,
    /// Every offered label with its probability.
    pub distribution: Vec<WeightView>,
}

/// A `Check` result.
#[derive(Debug, Clone, Serialize)]
pub struct CheckView {
    /// The claim's name.
    pub name: String,
    /// Whether it held.
    pub ok: bool,
    /// Composed from the verdict.
    pub note: String,
    /// The probability behind it; exactly 0 or 1 for a rule.
    pub p: f32,
    /// `jev` when Jev judged it, `rule` when the domain decided it in code.
    pub source: String,
}

/// A `Score` output.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreView {
    /// The score, 0..=100.
    pub score: u8,
    /// The affirmed drivers, strongest first.
    pub drivers: Vec<String>,
    /// Composed from the verdict.
    pub reason: String,
    /// How certain it was.
    pub confidence: f32,
    /// Every candidate driver with its probability.
    pub distribution: Vec<WeightView>,
}

/// A `Gate` output.
#[derive(Debug, Clone, Serialize)]
pub struct GateView {
    /// `execute`, `reduce`, `hold` or `escalate`.
    pub action: String,
    /// How much size it allowed, 0..=1.
    pub size_factor: f32,
    /// Composed from the verdict.
    pub reason: String,
    /// How certain it was.
    pub confidence: f32,
    /// Every offered action with its probability.
    pub distribution: Vec<WeightView>,
}

/// One entry of a `Rank` output.
#[derive(Debug, Clone, Serialize)]
pub struct RankedView {
    /// Which candidate.
    pub id: String,
    /// Composed from the verdict.
    pub rationale: String,
    /// Jev's rating of this candidate's fit, 0..=1 of the shared rubric.
    pub fit: f32,
}

/// One label and its probability.
#[derive(Debug, Clone, Serialize)]
pub struct WeightView {
    /// The label.
    pub label: String,
    /// Its probability, 0..=1.
    pub p: f32,
}

// ---------------------------------------------------------------------------
// The audit log
// ---------------------------------------------------------------------------

/// A page of the audit log.
#[derive(Debug, Clone, Serialize)]
pub struct CallPage {
    /// How many calls the run has recorded.
    pub total: usize,
    /// Where this page starts.
    pub offset: usize,
    /// The calls themselves.
    pub calls: Vec<CallRow>,
}

/// One typed call, without its payloads.
#[derive(Debug, Clone, Serialize)]
pub struct CallRow {
    /// Index into the run's audit log, and the id of its detail endpoint.
    pub index: usize,
    /// Milliseconds since the Unix epoch when the call returned.
    pub at_ms: u128,
    /// Which backend answered.
    pub backend: String,
    /// Which primitive asked.
    pub primitive: String,
    /// The pipeline stage it came through.
    pub stage: Option<String>,
    /// The model that answered.
    pub model: String,
    /// How many questions were asked.
    pub asks: usize,
    /// Input tokens, when reported.
    pub input_tokens: Option<u64>,
    /// Output tokens, when reported.
    pub output_tokens: Option<u64>,
    /// Wall-clock milliseconds.
    pub latency_ms: u64,
}

/// One call's HTTP request to System One, as the live client builds it.
#[derive(Debug, Clone, Serialize)]
pub struct CallRequestView {
    /// Always `POST`.
    pub method: String,
    /// The endpoint.
    pub url: String,
    /// Whether this body actually went over the wire (a live call) or is what
    /// a mock call would have sent.
    pub sent: bool,
    /// The body: `{ state, model, questions }`.
    pub body: serde_json::Value,
}

/// One primitive's standing guidance: the JSON merged into each of its questions.
#[derive(Debug, Clone, Serialize)]
pub struct PromptView {
    /// Which primitive.
    pub primitive: String,
    /// The prompt, verbatim.
    pub text: String,
}

/// What the server can do, and how it is configured.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    /// The workspace version.
    pub version: String,
    /// Whether `mock` defaults to true.
    pub default_mock: bool,
    /// Whether a live run is possible: `TYPESAFE_API_KEY` is set.
    pub live_available: bool,
    /// The largest `days` a request may ask for.
    pub max_days: u32,
    /// How many finished runs are kept.
    pub run_capacity: usize,
}
