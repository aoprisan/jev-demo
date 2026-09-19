/**
 * The wire types, mirroring `crates/server/src/dto.rs` field for field.
 *
 * This file is the other half of the contract: a field renamed there without
 * the same rename here is a silent break at runtime, which is why the server's
 * tests assert the key lists these interfaces declare.
 */

export type Domain = "fx" | "battery" | "all";
export type RunStatus = "running" | "done" | "failed";
export type Action = "execute" | "reduce" | "hold" | "escalate";

/** What a client asks for when it starts a run. */
export interface RunRequest {
  domain: Domain;
  seed?: number;
  days?: number;
  limit?: number | null;
  mock?: boolean;
}

/** A request with every default resolved, as the run actually ran. */
export interface RunSpec {
  domain: Domain;
  seed: number;
  days: number;
  limit: number | null;
  mock: boolean;
}

/** A run without its result: enough for a list, and for polling. */
export interface RunSummary {
  id: string;
  spec: RunSpec;
  status: RunStatus;
  calls: number;
  started_at_ms: number;
  finished_at_ms: number | null;
  error: string | null;
}

/** A run and, once it is done, everything it produced. */
export interface RunView extends RunSummary {
  result: RunResult | null;
}

/** What a finished run produced. */
export interface RunResult {
  fx: FxResult | null;
  battery: BatteryResult | null;
  compliance: ExplainView | null;
  cost: Cost;
}

/** What the judgment layer cost. */
export interface Cost {
  calls: number;
  tokens: number;
  latency_ms: number;
}

/** An Explain output as the UI shows it. */
export interface ExplainView {
  summary: string;
  for_audience: string;
  confidence: number;
}

// ---------------------------------------------------------------------------
// The primitive outputs
// ---------------------------------------------------------------------------

/** One label and its probability. */
export interface WeightView {
  label: string;
  p: number;
}

/** A `Classify` output. */
export interface ClassifyView {
  label: string;
  confidence: number;
  reason: string;
  distribution: WeightView[];
}

/** A `Check` result. */
export interface CheckView {
  name: string;
  ok: boolean;
  note: string;
  p: number;
}

/** A `Score` output. */
export interface ScoreView {
  score: number;
  drivers: string[];
  reason: string;
  confidence: number;
  distribution: WeightView[];
}

/** A `Gate` output. */
export interface GateView {
  action: Action;
  size_factor: number;
  reason: string;
  confidence: number;
  distribution: WeightView[];
}

/** One entry of a `Rank` output. */
export interface RankedView {
  id: string;
  rationale: string;
  p: number;
}

// ---------------------------------------------------------------------------
// Forex
// ---------------------------------------------------------------------------

/** How many decisions ended in one gate action. */
export interface ActionCount {
  action: Action;
  count: number;
}

/** A forex book's outcome. */
export interface FxBook {
  trades: number;
  pnl: number;
  wins: number;
  losses: number;
  hit_rate: number;
  max_drawdown: number;
  notional: number;
  return_bps: number;
}

/** How one named check fared against the outcomes it flagged. */
export interface CheckScorecardView {
  name: string;
  failed_n: number;
  failed_bps: number;
  failed_hit: number;
  passed_n: number;
  passed_bps: number;
  passed_hit: number;
  edge_bps: number;
  inverted: boolean;
}

/** One point on the cumulative P&L curve. */
export interface EquityPoint {
  index: number;
  day: number;
  ungated: number;
  gated: number;
}

/** One candidate, its judgment and what each book did with it. */
export interface FxDecisionRow {
  index: number;
  day: number;
  hour: number;
  date: string;
  pair: string;
  side: string;
  price: number;
  stop: number;
  target: number;
  size_units: number;
  regime: string;
  regime_confidence: number;
  true_regime: string;
  regime_correct: boolean;
  risk_score: number;
  checks_failed: string[];
  action: Action;
  size_factor: number;
  reason: string;
  ungated_pnl: number | null;
  gated_pnl: number | null;
  pnl_delta: number;
  intervened: boolean;
}

/**
 * The only thing the judgment layer reads.
 *
 * The generator's latent regime is deliberately absent: it lives in an array
 * parallel to the bars and never reaches a state a Jev call is made on.
 */
export interface FxFeatures {
  pair: string;
  side: string;
  hours_to_event: number;
  next_event_kind: string | null;
  stop_atr_multiple: number;
  reward_risk: number;
  trend_strength: number;
  volatility_percentile: number;
  spread_percentile: number;
  band_excursion: number;
  pre_trade_exposure_units: number;
  post_trade_exposure_units: number;
  exposure_multiple: number;
  pre_trade_exposure_share: number;
  post_trade_exposure_share: number;
  informative_headlines: number;
  stress_indicator: number;
}

/** One filled trade. */
export interface FillView {
  entered_day: number;
  entered_hour: number;
  exited_day: number;
  exited_hour: number;
  exit: string;
  entry_price: number;
  exit_price: number;
  size_factor: number;
  notional: number;
  pnl: number;
  pnl_bps: number;
}

/** One candidate in full: the features Jev read and every stage's output. */
export interface FxDecisionDetail {
  row: FxDecisionRow;
  features: FxFeatures;
  headlines: string[];
  regime: ClassifyView;
  checks: CheckView[];
  risk: ScoreView;
  gate: GateView;
  ungated: FillView | null;
  gated: FillView | null;
}

/** One side of the forex replay. */
export interface FxReplayPane {
  label: string;
  event: string;
  risk_score: number;
  gate: GateView;
}

/** The CPI day judged twice: once with the release, once without it. */
export interface FxReplay {
  pair: string;
  date: string;
  price: number;
  stop: number;
  target: number;
  side: string;
  with_event: FxReplayPane;
  without_event: FxReplayPane;
}

/** Everything one forex run produced. */
export interface FxResult {
  decisions_judged: number;
  ungated: FxBook;
  gated: FxBook;
  gate_distribution: ActionCount[];
  regime_accuracy: number;
  interventions: number;
  escalations: number;
  failed_checks: number;
  scorecard: CheckScorecardView[];
  equity: EquityPoint[];
  decisions: FxDecisionRow[];
  replay: FxReplay | null;
}

// ---------------------------------------------------------------------------
// Battery
// ---------------------------------------------------------------------------

/** A battery desk's outcome. */
export interface BatteryBook {
  days_run: number;
  days_stood_down: number;
  margin: number;
  reserve_payment: number;
  degradation_cost: number;
  reserve_breaches: number;
  days_with_breach: number;
  cycles: number;
  margin_per_cycle: number;
  max_drawdown: number;
}

/** How often one schedule was ranked first. */
export interface ScheduleCount {
  schedule: string;
  count: number;
}

/** One point on the cumulative margin curve. */
export interface MarginPoint {
  day: number;
  solver_only: number;
  gated: number;
}

/** One day: what the solver offered, what Jev decided, what each desk ran. */
export interface BatteryDayRow {
  day: number;
  date: string;
  regime: string;
  regime_confidence: number;
  chosen: string;
  rank_margin: number;
  risk_score: number;
  checks_failed: string[];
  action: Action;
  size_factor: number;
  reason: string;
  solver_margin: number;
  gated_margin: number;
  margin_delta: number;
  solver_breaches: number;
  gated_breaches: number;
  gated_cycles: number;
  intervened: boolean;
}

/** One schedule the solver produced. */
export interface ScheduleView {
  kind: string;
  power_mw: number[];
  soc_mwh: number[];
  expected_margin: number;
  cycles: number;
  degradation_cost: number;
  min_soc_mwh: number;
}

/** What running a schedule actually produced. */
export interface ExecutionView {
  schedule: string;
  size_factor: number;
  energy_margin: number;
  reserve_payment: number;
  degradation_cost: number;
  realised_margin: number;
  expected_margin: number;
  soc_mwh: number[];
  reserve_breaches: number;
  cycles: number;
}

/** One day in full: all three schedules, every stage, both executions. */
export interface BatteryDayDetail {
  row: BatteryDayRow;
  schedules: ScheduleView[];
  regime: ClassifyView;
  ranking: RankedView[];
  checks: CheckView[];
  risk: ScoreView;
  gate: GateView;
  solver_only: ExecutionView;
  gated: ExecutionView;
}

/** One side of the battery replay. */
export interface BatteryReplayPane {
  label: string;
  chosen: string;
  ranking: RankedView[];
  risk_score: number;
  checks: CheckView[];
  reserve_held: boolean;
  gate: GateView;
}

/** One day judged quiet, then with a grid notice over its discharge block. */
export interface BatteryReplay {
  day: number;
  date: string;
  from_hour: number;
  to_hour: number;
  notice: string;
  quiet: BatteryReplayPane;
  noticed: BatteryReplayPane;
  flipped: boolean;
}

/** Everything one battery run produced. */
export interface BatteryResult {
  days_judged: number;
  solver_only: BatteryBook;
  gated: BatteryBook;
  gate_distribution: ActionCount[];
  rank_distribution: ScheduleCount[];
  interventions: number;
  escalations: number;
  failed_checks: number;
  margin_curve: MarginPoint[];
  days: BatteryDayRow[];
  replay: BatteryReplay | null;
}

// ---------------------------------------------------------------------------
// The audit log, and the catalogue
// ---------------------------------------------------------------------------

/** One typed call, without its payloads. */
export interface CallRow {
  index: number;
  at_ms: number;
  backend: string;
  primitive: string;
  stage: string | null;
  model: string;
  asks: number;
  input_tokens: number | null;
  output_tokens: number | null;
  latency_ms: number;
}

/** A page of the audit log. */
export interface CallPage {
  total: number;
  offset: number;
  calls: CallRow[];
}

/**
 * One call in full, as it is written to `decisions.jsonl`.
 *
 * The payloads stay `unknown`: this is the audit artefact, and the UI renders
 * it as the JSON it is rather than pretending to a shape it does not check.
 */
export interface CallRecord {
  at_ms: number;
  backend: string;
  primitive: string;
  stage: string | null;
  model: string;
  state: unknown;
  asks: unknown;
  verdicts: unknown;
  output: unknown;
  input_tokens: number | null;
  output_tokens: number | null;
  latency_ms: number;
}

/** One primitive's standing instructions. */
export interface PromptView {
  primitive: string;
  text: string;
}

/** What the server can do, and how it is configured. */
export interface ServerInfo {
  version: string;
  default_mock: boolean;
  live_available: boolean;
  max_days: number;
  run_capacity: number;
}

/** The body every failure answers with. */
export interface ErrorBody {
  error: "not_found" | "bad_request" | "internal";
  message: string;
}
