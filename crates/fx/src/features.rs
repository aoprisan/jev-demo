//! What the judgment layer is allowed to see.
//!
//! This module is the honesty boundary. Everything Jev — live or mock — reads
//! about a forex decision is assembled here, from the price window, the
//! calendar, the book and the candidate. The generator's true regime is not
//! available to this module by construction: [`FxFeatures::compute`] takes the
//! bars, never the [`synth::PairSeries`] that holds the truth alongside them.

use crate::strategy::{atr, efficiency_ratio, StrategyParams, TradeCandidate};
use serde::{Deserialize, Serialize};
use synth::{Bar, CalendarEvent, Currency, Exposure, Pair, Stamp};

/// Bars used for the volatility and range percentiles.
const PERCENTILE_LOOKBACK: usize = 60;
/// Bars used for the efficiency ratio.
const TREND_LOOKBACK: usize = 12;

/// The observable state of one forex decision.
///
/// Field names are the contract with `MockJev`'s rules; they are also what a
/// live model reads, so they are named for what they mean rather than for the
/// rule that consumes them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FxFeatures {
    /// The pair, e.g. `EUR/USD`.
    pub pair: String,
    /// `long` or `short`.
    pub side: String,
    /// Hours until the next scheduled release touching either leg. Large when
    /// there is none in sight.
    pub hours_to_event: f64,
    /// The kind of that release, e.g. `CPI`. Absent when none is scheduled.
    pub next_event_kind: Option<String>,
    /// The stop's distance from entry, in ATRs. The solver's stop, measured.
    pub stop_atr_multiple: f64,
    /// Reward over risk, as the solver sized it.
    pub reward_risk: f64,
    /// Directional persistence of the recent window, `0..=1`. High means the
    /// market has been going one way, where mean reversion is a poor bet.
    pub trend_strength: f64,
    /// Where current ATR sits in its own recent history, `0..=1`.
    pub volatility_percentile: f64,
    /// Where the current bar's range sits in its recent history, `0..=1`. The
    /// closest observable stand-in for a spread in this synthetic market.
    pub spread_percentile: f64,
    /// How far the close is outside its band, in standard deviations.
    pub band_excursion: f64,
    /// Units already held in the currency this trade adds to.
    pub pre_trade_exposure_units: f64,
    /// Units held after it.
    pub post_trade_exposure_units: f64,
    /// How many times over the trade multiplies the existing position. A value
    /// of 2 means the trade doubles it; a large residual gives a value near 1.
    pub exposure_multiple: f64,
    /// The share of the book already held in the currency this trade adds to.
    pub pre_trade_exposure_share: f64,
    /// That share after the trade.
    pub post_trade_exposure_share: f64,
    /// Informative headlines printed today.
    pub informative_headlines: u32,
    /// A composite of the conditions that make a market hard to act in at all.
    pub stress_indicator: f64,
}

impl FxFeatures {
    /// Assemble the observable state for `candidate`.
    ///
    /// Takes `bars` rather than a `PairSeries` so the generator's truth is not
    /// in scope. See the module docs.
    pub fn compute(
        candidate: &TradeCandidate,
        bars: &[Bar],
        calendar: &[CalendarEvent],
        book: &[Exposure],
        headlines_today: u32,
        p: &StrategyParams,
    ) -> Self {
        let i = candidate.bar_index;
        let pair = candidate.pair;
        let current_atr = atr(bars, i, p.atr_period).unwrap_or(f64::EPSILON).max(f64::EPSILON);

        let next_event = next_event_for(pair, candidate.t, calendar);
        let hours_to_event = next_event.map(|e| candidate.t.hours_to(e.t)).unwrap_or(999.0);

        let trend_strength = efficiency_ratio(bars, i, TREND_LOOKBACK).unwrap_or(0.0);
        let volatility_percentile = percentile_of(
            current_atr,
            (0..PERCENTILE_LOOKBACK)
                .filter_map(|back| i.checked_sub(back).and_then(|j| atr(bars, j, p.atr_period))),
        );
        let range = bars[i].high - bars[i].low;
        let spread_percentile = percentile_of(
            range,
            (0..PERCENTILE_LOOKBACK)
                .filter_map(|back| i.checked_sub(back))
                .map(|j| bars[j].high - bars[j].low),
        );

        let sd =
            crate::strategy::stdev(bars, i, p.period).unwrap_or(f64::EPSILON).max(f64::EPSILON);
        let mid = crate::strategy::sma(bars, i, p.period).unwrap_or(candidate.price);
        let band_excursion = ((candidate.price - mid).abs() / sd - p.k).max(0.0);

        let exposure = Exposures::of(candidate, book);

        // Stress is not one observation but the coincidence of several: a wide
        // market, an event on the way, and a book already leaning one way.
        let event_pressure =
            if hours_to_event <= 6.0 { 1.0 - (hours_to_event / 6.0).clamp(0.0, 1.0) } else { 0.0 };
        let stress_indicator = (0.45 * volatility_percentile
            + 0.35 * event_pressure
            + 0.20 * exposure.post_share.min(1.0))
        .clamp(0.0, 1.0);

        FxFeatures {
            pair: pair.code().to_owned(),
            side: candidate.side.as_str().to_owned(),
            hours_to_event,
            next_event_kind: next_event.map(|e| e.kind.code().to_owned()),
            stop_atr_multiple: candidate.stop_distance() / current_atr,
            reward_risk: candidate.reward_risk(),
            trend_strength,
            volatility_percentile,
            spread_percentile,
            band_excursion,
            pre_trade_exposure_units: exposure.pre_units,
            post_trade_exposure_units: exposure.post_units,
            exposure_multiple: exposure.multiple,
            pre_trade_exposure_share: exposure.pre_share,
            post_trade_exposure_share: exposure.post_share,
            informative_headlines: headlines_today,
            stress_indicator,
        }
    }
}

/// The next release at or after `t` touching either leg of `pair`.
fn next_event_for(pair: Pair, t: Stamp, calendar: &[CalendarEvent]) -> Option<&CalendarEvent> {
    calendar.iter().find(|e| {
        e.t.index() >= t.index() && (e.currency == pair.base() || e.currency == pair.quote())
    })
}

/// Where `value` sits within `history`, as a fraction in `0..=1`.
fn percentile_of(value: f64, history: impl Iterator<Item = f64>) -> f64 {
    let history: Vec<f64> = history.collect();
    if history.len() < 2 {
        return 0.5;
    }
    let below = history.iter().filter(|h| **h < value).count();
    below as f64 / (history.len() - 1) as f64
}

/// What this trade does to the book's exposure in the currency it adds to.
///
/// A long EUR/USD adds EUR; a short EUR/USD adds USD. Two things are measured,
/// because the brief's rule needs both: how far the trade *multiplies* an
/// existing position (the "doubles it" part) and how concentrated the book
/// becomes as a result (the "past a threshold" part). A share ratio alone will
/// not do — adding to the numerator and the denominator together damps it, so
/// a trade that genuinely doubles a position barely moves its share.
struct Exposures {
    pre_units: f64,
    post_units: f64,
    multiple: f64,
    pre_share: f64,
    post_share: f64,
}

impl Exposures {
    fn of(candidate: &TradeCandidate, book: &[Exposure]) -> Self {
        let added = added_currency(candidate.pair, candidate.side);
        let gross: f64 = book.iter().map(|e| e.units.abs()).sum::<f64>().max(f64::EPSILON);
        let held: f64 = book.iter().filter(|e| e.currency == added).map(|e| e.units.abs()).sum();
        let post_units = held + candidate.size_units;
        Self {
            pre_units: held,
            post_units,
            // A flat position is multiplied without bound; report it as large
            // rather than infinite so it serialises and compares cleanly.
            multiple: if held > f64::EPSILON { post_units / held } else { 99.0 },
            pre_share: held / gross,
            post_share: post_units / (gross + candidate.size_units),
        }
    }
}

/// The currency a side adds to, for reporting.
pub fn added_currency(pair: Pair, side: crate::strategy::Side) -> Currency {
    match side {
        crate::strategy::Side::Long => pair.base(),
        crate::strategy::Side::Short => pair.quote(),
    }
}

/// How the features' next release reads on a report: `"CPI in 0.4h"`, or
/// `"none scheduled"` when nothing is close enough to matter.
///
/// Shared so the terminal replay and the HTTP API describe the same distance
/// the same way.
pub fn event_label(features: &FxFeatures) -> String {
    match features.next_event_kind.as_deref() {
        Some(kind) if features.hours_to_event < 72.0 => {
            format!("{kind} in {:.1}h", features.hours_to_event)
        }
        _ => "none scheduled".to_owned(),
    }
}
