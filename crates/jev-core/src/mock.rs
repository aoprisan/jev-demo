//! `MockJev` — the offline backend. Every rule lives in this file.
//!
//! # What the mock may look at
//!
//! The mock sees exactly what the live API sees: the serialised state, the
//! context block and the questions. It reads **observable features only** —
//! whatever the domain chose to put under `input.features` — and it has no
//! access to any ground truth a synthetic generator holds back. That is what
//! keeps the gated-vs-ungated P&L comparison honest: the mock is judging the
//! same information a live model would.
//!
//! In particular, the forex `signal_valid_in_regime` rule is specified against
//! the *true* regime, and the mock cannot read it. It uses `trend_strength`
//! instead — a normalised directional-persistence measure the fx crate computes
//! from the price window — so the mock's agreement with the true regime is
//! itself an empirical result rather than a guarantee. `tests/honesty.rs`
//! asserts that no state ever handed to a backend carries the true regime.
//!
//! # Determinism
//!
//! Answers are a pure function of the call. Where no rule applies, a
//! deterministic hash of the call and question name supplies a stable,
//! middling probability, so a run is reproducible without being informative.

use crate::ask::{Ask, Usage, Verdict};
use crate::client::{JevCall, JevClient, JevReply};
use crate::error::Result;
use indexmap::IndexMap;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

/// Thresholds the mock's rules are written against. Exposed so tests can state
/// the rule and the constant in one place.
pub mod thresholds {
    /// FX: an event this close counts as imminent (hours).
    pub const EVENT_IMMINENT_HOURS: f64 = 3.0;
    /// FX: a stop tighter than this multiple of ATR is not sane.
    pub const STOP_ATR_MULTIPLE: f64 = 1.2;
    /// FX: above this directional persistence the window reads as trending,
    /// where a mean-reversion signal is not valid.
    pub const TRENDING_STRENGTH: f64 = 0.62;
    /// FX: a trade multiplying an existing position by at least this much
    /// counts as doubling it.
    pub const EXPOSURE_DOUBLING: f64 = 2.0;
    /// FX: and it is only a problem if it also takes that currency past this
    /// share of the book.
    pub const EXPOSURE_SHARE_CAP: f64 = 0.35;
    /// Battery: intraday deviation beyond this multiple of recent sigma is implausible.
    pub const ID_DEVIATION_SIGMA: f64 = 2.0;
    /// Battery: within this fraction of the cycle budget counts as "near".
    pub const CYCLE_BUDGET_NEAR: f64 = 0.85;
}

/// The offline, rule-based Jev backend.
#[derive(Debug, Clone, Default)]
pub struct MockJev {
    latency: Duration,
}

impl MockJev {
    /// A mock that answers instantly.
    pub fn new() -> Self {
        Self::default()
    }

    /// A mock that reports `latency` on every call, for exercising timing paths.
    pub fn with_latency(latency: Duration) -> Self {
        Self { latency }
    }
}

#[async_trait::async_trait]
impl JevClient for MockJev {
    fn backend(&self) -> &'static str {
        "mock"
    }

    async fn ask(&self, call: &JevCall) -> Result<JevReply> {
        let f = Features::of(&call.state);
        let mut verdicts = IndexMap::with_capacity(call.asks.len());
        for (name, ask) in call.asks.iter() {
            verdicts.insert(name.to_owned(), answer(name, ask, &f, call));
        }
        // Token counts are proportional to what was sent, so the reported cost
        // moves with the size of a call the way a live one would.
        let input_tokens = (call.instructions.len()
            + call.context.len()
            + call.state.to_string().len())
            / 4;
        let output_tokens = 12 * call.asks.len();
        Ok(JevReply {
            verdicts,
            model: "mock-rules-1".to_owned(),
            usage: Usage {
                input_tokens: Some(input_tokens as u64),
                output_tokens: Some(output_tokens as u64),
            },
            latency: self.latency,
        })
    }
}

// ---- observable features -------------------------------------------------------------------

/// Typed access to `input.features`. Every rule reads through this, so the set
/// of things the mock can see is the set of fields listed here.
struct Features<'a> {
    map: Option<&'a serde_json::Map<String, Value>>,
}

impl<'a> Features<'a> {
    fn of(state: &'a Value) -> Self {
        Self { map: state.pointer("/input/features").and_then(Value::as_object) }
    }

    fn num(&self, key: &str) -> Option<f64> {
        self.map?.get(key)?.as_f64()
    }

    fn flag(&self, key: &str) -> Option<bool> {
        self.map?.get(key)?.as_bool()
    }

    fn text(&self, key: &str) -> Option<&'a str> {
        self.map?.get(key)?.as_str()
    }
}

/// A prior stage's typed output, by primitive name.
fn prior<'a>(call: &'a JevCall, primitive: &str) -> Option<&'a Value> {
    call.state
        .pointer("/prior_judgments")?
        .as_array()?
        .iter()
        .find(|e| e.get("primitive").and_then(Value::as_str) == Some(primitive))
        .and_then(|e| e.get("output"))
}

fn prior_check_failed(call: &JevCall, name: &str) -> bool {
    prior(call, "check")
        .and_then(|o| o.get("checks"))
        .and_then(Value::as_array)
        .map(|checks| {
            checks.iter().any(|c| {
                c.get("name").and_then(Value::as_str) == Some(name)
                    && c.get("ok").and_then(Value::as_bool) == Some(false)
            })
        })
        .unwrap_or(false)
}

fn prior_any_check_failed(call: &JevCall) -> bool {
    prior(call, "check")
        .and_then(|o| o.get("checks"))
        .and_then(Value::as_array)
        .map(|checks| {
            checks.iter().any(|c| c.get("ok").and_then(Value::as_bool) == Some(false))
        })
        .unwrap_or(false)
}

fn prior_score(call: &JevCall) -> Option<u64> {
    prior(call, "score")?.get("score")?.as_u64()
}

fn prior_label(call: &JevCall) -> Option<&str> {
    prior(call, "classify")?.get("label")?.as_str()
}

// ---- derived observations ------------------------------------------------------------------

/// FX: a scheduled event is imminent and the stop is tighter than the volatility
/// of the window justifies. Both halves are required, per the rule.
fn fx_event_pins_a_tight_stop(f: &Features) -> bool {
    let hours = f.num("hours_to_event");
    let stop_atr = f.num("stop_atr_multiple");
    match (hours, stop_atr) {
        (Some(h), Some(s)) => {
            h >= 0.0
                && h <= thresholds::EVENT_IMMINENT_HOURS
                && s < thresholds::STOP_ATR_MULTIPLE
        }
        _ => false,
    }
}

/// FX: the window reads as trending, where a mean-reversion signal is not valid.
/// Derived from observable directional persistence, never from the true regime.
fn fx_reads_trending(f: &Features) -> bool {
    f.num("trend_strength").map(|t| t >= thresholds::TRENDING_STRENGTH).unwrap_or(false)
}

/// FX: the trade doubles a currency the book already holds, *and* that leaves
/// the currency concentrated. Both halves are required: doubling a negligible
/// residual is not a risk, and inheriting a large position the trade barely
/// moves is not this trade's doing.
fn fx_exposure_doubles(f: &Features) -> bool {
    let multiple = f.num("exposure_multiple").unwrap_or(1.0);
    let post_share = f.num("post_trade_exposure_share").unwrap_or(0.0);
    multiple >= thresholds::EXPOSURE_DOUBLING && post_share > thresholds::EXPOSURE_SHARE_CAP
}

/// Battery: the reserve obligation is live, or a grid notice overlaps the
/// discharge block.
fn battery_reserve_pressure(f: &Features) -> bool {
    f.flag("afrr_window_active").unwrap_or(false)
        || f.flag("grid_notice_overlaps_discharge").unwrap_or(false)
}

/// Battery: the schedule dips below the reserve floor inside the window.
fn battery_dips_below_reserve(f: &Features) -> bool {
    f.flag("afrr_window_active").unwrap_or(false)
        && f.flag("schedule_dips_below_reserve_soc").unwrap_or(false)
}

/// Battery: intraday has moved further from day-ahead than recent variation explains.
fn battery_id_deviation_implausible(f: &Features) -> bool {
    f.num("id_deviation_sigmas")
        .map(|d| d.abs() > thresholds::ID_DEVIATION_SIGMA)
        .unwrap_or(false)
}

/// Battery: the cycle budget is nearly spent.
fn battery_cycles_near_budget(f: &Features) -> bool {
    f.num("cycle_budget_used_fraction")
        .map(|u| u >= thresholds::CYCLE_BUDGET_NEAR)
        .unwrap_or(false)
}

// ---- answering -----------------------------------------------------------------------------

fn answer(name: &str, ask: &Ask, f: &Features, call: &JevCall) -> Verdict {
    match ask {
        Ask::Noul { .. } => Verdict::Noul { p: noul(name, f, call) },
        Ask::Choice { options, .. } => choice(name, options, f, call),
        Ask::Score { levels, .. } => score(name, levels.len(), f, call),
    }
}

/// Yes/no rules, keyed on the check or driver name the domain chose.
fn noul(name: &str, f: &Features, call: &JevCall) -> f64 {
    let key = name.trim_start_matches("driver_").trim_start_matches("fact_");
    match key {
        // --- forex checks ---
        "stop_sane" => {
            let stop_atr = f.num("stop_atr_multiple").unwrap_or(1.5);
            if stop_atr >= thresholds::STOP_ATR_MULTIPLE {
                0.92
            } else if stop_atr >= 0.9 {
                0.38
            } else {
                0.08
            }
        }
        "signal_valid_in_regime" => {
            if fx_reads_trending(f) {
                0.11
            } else {
                0.88
            }
        }
        "correlated_exposure_ok" => {
            if fx_exposure_doubles(f) {
                0.14
            } else {
                0.9
            }
        }
        // --- forex score drivers ---
        "imminent_event" => {
            let h = f.num("hours_to_event").unwrap_or(99.0);
            if h < 0.0 {
                0.05
            } else if h <= thresholds::EVENT_IMMINENT_HOURS {
                0.94
            } else if h <= 12.0 {
                0.45
            } else {
                0.07
            }
        }
        "high_impact_event" => match f.text("next_event_kind") {
            Some("CPI") | Some("NFP") | Some("FOMC") => 0.9,
            Some("ECB") => 0.78,
            Some(_) => 0.4,
            None => 0.06,
        },
        "tight_stop" => {
            let stop_atr = f.num("stop_atr_multiple").unwrap_or(1.5);
            if stop_atr < thresholds::STOP_ATR_MULTIPLE { 0.87 } else { 0.12 }
        }
        "trend_pressure" => {
            let t = f.num("trend_strength").unwrap_or(0.3);
            (t * 1.2).clamp(0.03, 0.95)
        }
        "crowded_book" => {
            if fx_exposure_doubles(f) { 0.86 } else { 0.17 }
        }
        // --- battery checks ---
        "reserve_ok" => {
            if battery_dips_below_reserve(f) {
                0.07
            } else if battery_reserve_pressure(f) {
                0.72
            } else {
                0.95
            }
        }
        "margin_plausible" => {
            if battery_id_deviation_implausible(f) {
                0.12
            } else {
                let d = f.num("id_deviation_sigmas").map(f64::abs).unwrap_or(0.4);
                (0.95 - 0.2 * d).clamp(0.5, 0.95)
            }
        }
        "cycle_budget_ok" => {
            if battery_cycles_near_budget(f) {
                0.16
            } else {
                let u = f.num("cycle_budget_used_fraction").unwrap_or(0.3);
                (0.97 - 0.5 * u).clamp(0.4, 0.97)
            }
        }
        // --- battery score drivers ---
        "reserve_at_risk" => {
            if battery_dips_below_reserve(f) {
                0.93
            } else if battery_reserve_pressure(f) {
                0.42
            } else {
                0.06
            }
        }
        "price_dislocation" => {
            let d = f.num("id_deviation_sigmas").map(f64::abs).unwrap_or(0.3);
            (d / (thresholds::ID_DEVIATION_SIGMA * 1.5)).clamp(0.04, 0.96)
        }
        "cycles_nearly_spent" => {
            if battery_cycles_near_budget(f) { 0.91 } else { 0.13 }
        }
        "grid_constraint" => {
            if f.flag("grid_notice_overlaps_discharge").unwrap_or(false) { 0.89 } else { 0.08 }
        }
        "negative_price_hours" => {
            let n = f.num("negative_price_hours").unwrap_or(0.0);
            if n >= 1.0 { 0.85 } else { 0.1 }
        }
        // --- explain facts: chosen by audience, from the session's own totals ---
        _ => explain_fact(key, f, call).unwrap_or_else(|| stable(call, name, 0.35, 0.65)),
    }
}

/// Explain's candidate facts. Which ones matter is a function of the audience,
/// which the mock reads from the context block the primitive built.
fn explain_fact(key: &str, f: &Features, call: &JevCall) -> Option<f64> {
    let audience = f.text("audience")?;
    let interventions = f.num("intervention_count").unwrap_or(0.0);
    let escalations = f.num("escalation_count").unwrap_or(0.0);
    let happened = |n: f64, yes: f64| if n > 0.0 { yes } else { 0.06 };
    let _ = call;
    Some(match (key, audience) {
        ("pnl_effect", "trader") => 0.95,
        ("pnl_effect", "compliance") => 0.28,
        ("pnl_effect", "ops") => 0.2,
        ("gate_distribution", "trader") => 0.55,
        ("gate_distribution", "compliance") => 0.93,
        ("gate_distribution", "ops") => 0.5,
        ("checks_ran", "compliance") => 0.96,
        ("checks_ran", _) => 0.18,
        ("audit_location", "compliance") => 0.94,
        ("audit_location", "ops") => 0.6,
        ("audit_location", _) => 0.12,
        ("escalations", "ops") => happened(escalations, 0.95),
        ("escalations", "compliance") => happened(escalations, 0.88),
        ("escalations", _) => happened(escalations, 0.5),
        ("interventions", "trader") => happened(interventions, 0.9),
        ("interventions", "ops") => happened(interventions, 0.82),
        ("interventions", _) => happened(interventions, 0.6),
        _ => return None,
    })
}

/// Choice rules: the gate's action, a classification, or a ranking.
fn choice(
    name: &str,
    options: &[(String, String)],
    f: &Features,
    call: &JevCall,
) -> Verdict {
    let labels: Vec<&str> = options.iter().map(|(l, _)| l.as_str()).collect();
    let weights: Vec<f64> = match name {
        "action" => gate_weights(&labels, f, call),
        "best" => rank_weights(&labels, f),
        "label" => classify_weights(&labels, f),
        "framing" => framing_weights(&labels, f),
        _ => labels.iter().map(|l| stable(call, l, 0.2, 0.8)).collect(),
    };
    distribution(&labels, &weights)
}

/// One rule's verdict: what to do, and how much of the solver's size it leaves.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Decision {
    action: &'static str,
    size: f64,
}

impl Decision {
    /// How cautious this verdict is. Escalating outranks holding, which
    /// outranks reducing, which outranks executing.
    fn caution(&self) -> u8 {
        match self.action {
            "escalate" => 3,
            "hold" => 2,
            "reduce" => 1,
            _ => 0,
        }
    }
}

/// Every rule that fires, resolved to a single verdict.
///
/// The gate's action and its size come from *one* evaluation, not two. Deriving
/// them separately lets them contradict each other — a "reduce" carrying a size
/// of zero, which is not a decision anyone can act on — and `GateOut::validate`
/// rejects exactly that. The most cautious rule wins; among equally cautious
/// ones, the smallest size.
fn decide(f: &Features, call: &JevCall) -> Decision {
    let mut applicable = vec![Decision { action: "execute", size: 1.0 }];

    // An implausible input is for a human, not for sizing down.
    if prior_check_failed(call, "margin_plausible") {
        applicable.push(Decision { action: "escalate", size: 0.0 });
    }
    // FX: a release within three hours pinning a stop tighter than the window's
    // volatility justifies is the specified hold. Note the conjunction — a
    // tight stop on its own only reduces, below. If it held too, the release
    // would never be the deciding factor and the event-removed replay in the
    // demo would print two identical records.
    if fx_event_pins_a_tight_stop(f) {
        applicable.push(Decision { action: "hold", size: 0.0 });
    }
    // Battery: a reserve breach is not something to size down into.
    if prior_check_failed(call, "reserve_ok") {
        applicable.push(Decision { action: "hold", size: 0.0 });
    }
    if prior_label(call) == Some("stressed") {
        applicable.push(Decision { action: "hold", size: 0.0 });
    }
    // Battery: volatile with the cycle budget nearly spent is the specified reduce.
    if prior_label(call) == Some("volatile") && battery_cycles_near_budget(f) {
        applicable.push(Decision { action: "reduce", size: 0.5 });
    }
    if prior_check_failed(call, "signal_valid_in_regime") {
        applicable.push(Decision { action: "reduce", size: 0.25 });
    }
    for check in ["stop_sane", "correlated_exposure_ok", "cycle_budget_ok"] {
        if prior_check_failed(call, check) {
            applicable.push(Decision { action: "reduce", size: 0.5 });
        }
    }
    if let Some(score) = prior_score(call) {
        // Calibrated as a desk would: genuinely high risk stands the trade
        // down, middling risk trims it. Halving a position on a 55-of-100 is
        // harsher than anyone would actually trade, and since the most cautious
        // applicable rule wins, a punitive middle band would quietly dominate
        // every other rule in the table.
        applicable.push(match score {
            80..=100 => Decision { action: "hold", size: 0.0 },
            68..=79 => Decision { action: "reduce", size: 0.5 },
            55..=67 => Decision { action: "reduce", size: 0.75 },
            _ => Decision { action: "execute", size: 1.0 },
        });
    }

    applicable
        .into_iter()
        .reduce(|best, next| {
            let more_cautious = next.caution() > best.caution();
            let same_but_smaller = next.caution() == best.caution() && next.size < best.size;
            if more_cautious || same_but_smaller {
                next
            } else {
                best
            }
        })
        .expect("the list always holds the execute default")
}

/// The gate's action, from the resolved verdict.
fn gate_weights(labels: &[&str], f: &Features, call: &JevCall) -> Vec<f64> {
    let decision = decide(f, call);
    // The runner-up is the next most cautious option, so the distribution
    // reads the way a real one would rather than putting everything on one label.
    let runner = match decision.action {
        "execute" => "reduce",
        "reduce" => "hold",
        "hold" => "reduce",
        _ => "hold",
    };
    labels
        .iter()
        .map(|l| {
            if *l == decision.action {
                1.0
            } else if *l == runner {
                0.55
            } else {
                0.1
            }
        })
        .collect()
}

/// Ranking. The reserve-heavy schedule leads whenever the reserve is under
/// pressure; otherwise the balanced one does, with aggressive close behind.
fn rank_weights(labels: &[&str], f: &Features) -> Vec<f64> {
    let pressure = battery_reserve_pressure(f);
    let dislocated = battery_id_deviation_implausible(f);
    labels
        .iter()
        .map(|l| match (*l, pressure) {
            ("reserve_heavy", true) => 1.0,
            ("reserve_heavy", false) => 0.28,
            ("balanced", true) => 0.5,
            ("balanced", false) => 1.0,
            ("aggressive", true) => 0.12,
            ("aggressive", false) => {
                if dislocated {
                    0.3
                } else {
                    0.72
                }
            }
            _ => 0.4,
        })
        .collect()
}

/// Market-regime classification, from observable volatility, spread and depth.
fn classify_weights(labels: &[&str], f: &Features) -> Vec<f64> {
    let vol = f.num("volatility_percentile").unwrap_or(0.5);
    let spread = f.num("spread_percentile").unwrap_or(0.5);
    let trend = f.num("trend_strength").unwrap_or(0.3);
    let stress = f.num("stress_indicator").unwrap_or(0.0);
    labels
        .iter()
        .map(|l| match *l {
            // battery regimes
            "normal" => (1.2 - vol - stress).clamp(0.05, 1.0),
            "volatile" => (vol * 1.4 - stress * 0.5).clamp(0.05, 1.0),
            "illiquid" => (spread * 1.5).clamp(0.05, 1.0),
            "stressed" => (stress * 1.6).clamp(0.03, 1.0),
            // fx regimes
            "ranging" => (1.1 - trend - vol * 0.4).clamp(0.05, 1.0),
            "trending" => (trend * 1.5).clamp(0.05, 1.0),
            "event_driven" => {
                let h = f.num("hours_to_event").unwrap_or(99.0);
                if (0.0..=thresholds::EVENT_IMMINENT_HOURS * 2.0).contains(&h) {
                    1.0
                } else if h <= 12.0 {
                    0.35
                } else {
                    0.07
                }
            }
            _ => 0.3,
        })
        .collect()
}

/// Which framing an explain summary should lead with.
fn framing_weights(labels: &[&str], f: &Features) -> Vec<f64> {
    let interventions = f.num("intervention_count").unwrap_or(0.0);
    let escalations = f.num("escalation_count").unwrap_or(0.0);
    labels
        .iter()
        .map(|l| match *l {
            "quiet" if interventions == 0.0 => 1.0,
            "quiet" => 0.1,
            "intervened" if interventions > 0.0 && escalations == 0.0 => 1.0,
            "intervened" if interventions > 0.0 => 0.6,
            "intervened" => 0.12,
            "escalated" if escalations > 0.0 => 1.0,
            "escalated" => 0.05,
            _ => 0.35,
        })
        .collect()
}

/// Score rules: a position on the rubric, returned as a full distribution.
fn score(name: &str, levels: usize, f: &Features, call: &JevCall) -> Verdict {
    let fraction = match name {
        "size_factor" => size_fraction(f, call),
        "level" => risk_fraction(f, call),
        "severity" => severity_fraction(f),
        _ => stable(call, name, 0.25, 0.75),
    };
    score_distribution(levels, fraction.clamp(0.0, 1.0))
}

/// How much of the solver's size the conditions justify.
///
/// The same verdict the action came from, so the two can never disagree.
fn size_fraction(f: &Features, call: &JevCall) -> f64 {
    decide(f, call).size
}

/// Risk, 0..=1, as the maximum of whichever domain pressures are present.
fn risk_fraction(f: &Features, call: &JevCall) -> f64 {
    let mut risk: f64 = 0.08;
    let hours = f.num("hours_to_event").unwrap_or(99.0);
    if (0.0..=thresholds::EVENT_IMMINENT_HOURS).contains(&hours) {
        let high = matches!(f.text("next_event_kind"), Some("CPI" | "NFP" | "FOMC"));
        risk = risk.max(if high { 0.92 } else { 0.68 });
    } else if hours <= 12.0 {
        risk = risk.max(0.4);
    }
    if fx_reads_trending(f) {
        risk = risk.max(0.6);
    }
    if fx_exposure_doubles(f) {
        risk = risk.max(0.62);
    }
    if battery_dips_below_reserve(f) {
        risk = risk.max(0.88);
    } else if battery_reserve_pressure(f) {
        risk = risk.max(0.45);
    }
    if battery_id_deviation_implausible(f) {
        risk = risk.max(0.8);
    }
    if battery_cycles_near_budget(f) {
        risk = risk.max(0.58);
    }
    if prior_any_check_failed(call) {
        risk = risk.max(0.55);
    }
    risk
}

/// How serious the session was, for an explain summary.
fn severity_fraction(f: &Features) -> f64 {
    let interventions = f.num("intervention_count").unwrap_or(0.0);
    let escalations = f.num("escalation_count").unwrap_or(0.0);
    let total = f.num("decision_count").unwrap_or(1.0).max(1.0);
    if escalations > 0.0 {
        return 1.0;
    }
    (interventions / total * 2.2).clamp(0.0, 0.8)
}

// ---- shaping -------------------------------------------------------------------------------

/// Normalise weights into a distribution, pick the argmax, and derive a
/// confidence from how far clear the winner is.
fn distribution(labels: &[&str], weights: &[f64]) -> Verdict {
    let total: f64 = weights.iter().sum::<f64>().max(f64::EPSILON);
    let probabilities: IndexMap<String, f64> = labels
        .iter()
        .zip(weights)
        .map(|(l, w)| ((*l).to_owned(), w / total))
        .collect();
    let (label, top) = probabilities
        .iter()
        .fold((String::new(), -1.0), |(bl, bp), (l, p)| {
            if *p > bp {
                (l.clone(), *p)
            } else {
                (bl, bp)
            }
        });
    let mut sorted: Vec<f64> = probabilities.values().copied().collect();
    sorted.sort_by(|a, b| b.total_cmp(a));
    let runner = sorted.get(1).copied().unwrap_or(0.0);
    let confidence = ((top - runner) / top.max(f64::EPSILON)).clamp(0.0, 1.0);
    Verdict::Choice { label, probabilities, confidence }
}

/// Put mass on the two levels bracketing `fraction`, and nowhere else.
///
/// This is the minimal distribution consistent with the rule, which makes the
/// weighted score land *exactly* on the value the rule computed. Spreading a
/// little mass over the remaining levels would look more like a live answer,
/// but it also drags the weighted score away from the endpoints — so a rule
/// saying "all of the size" would come back as 95% of it, and the demo's gate
/// records would read as noise. A live model's distribution will be broader;
/// the primitive handles either.
fn score_distribution(levels: usize, fraction: f64) -> Verdict {
    let levels = levels.max(1);
    if levels == 1 {
        return Verdict::Score {
            score: 0.0,
            levels,
            probabilities: BTreeMap::from([(0, 1.0)]),
            confidence: 1.0,
        };
    }
    let exact = fraction * (levels - 1) as f64;
    let lower = exact.floor() as usize;
    let upper = (lower + 1).min(levels - 1);
    let frac = exact - lower as f64;

    let mut probabilities: BTreeMap<u32, f64> = BTreeMap::new();
    if lower == upper {
        probabilities.insert(lower as u32, 1.0);
    } else {
        probabilities.insert(lower as u32, 1.0 - frac);
        probabilities.insert(upper as u32, frac);
    }
    let score: f64 =
        probabilities.iter().map(|(level, p)| *level as f64 * p).sum();
    let top = probabilities.values().copied().fold(0.0_f64, f64::max);
    Verdict::Score { score, levels, probabilities, confidence: top.clamp(0.0, 1.0) }
}

/// A stable pseudo-probability in `[lo, hi]` for questions no rule covers.
/// Deterministic in the call, so runs reproduce.
fn stable(call: &JevCall, name: &str, lo: f64, hi: f64) -> f64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name
        .as_bytes()
        .iter()
        .chain(call.primitive.as_str().as_bytes())
        .chain(call.context.as_bytes())
    {
        h ^= *byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    lo + (h >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
}
