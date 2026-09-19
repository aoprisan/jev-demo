//! The deterministic side: Bollinger mean reversion on 4h bars.
//!
//! Every quantity here belongs to the solver. Jev never adjusts a price, a stop
//! or a size — it only decides what happens to the candidate the solver built.
//!
//! The indicator definitions are spelled out because the tests compute them by
//! hand: simple (not Wilder) averages, and a population standard deviation.

use serde::{Deserialize, Serialize};
use synth::{Bar, Pair, Stamp};

/// Which way the trade goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// Buy, expecting a move back up to the mean.
    Long,
    /// Sell, expecting a move back down to the mean.
    Short,
}

impl Side {
    /// `+1` for long, `-1` for short.
    pub fn sign(&self) -> f64 {
        match self {
            Side::Long => 1.0,
            Side::Short => -1.0,
        }
    }

    /// Lower-case name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Side::Long => "long",
            Side::Short => "short",
        }
    }
}

/// The strategy's parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrategyParams {
    /// Bars in the moving average and the standard deviation.
    pub period: usize,
    /// Band width in standard deviations.
    pub k: f64,
    /// Bars in the average true range.
    pub atr_period: usize,
    /// Bars searched for the swing the stop sits behind.
    pub swing_lookback: usize,
    /// The stop is sized at this many standard deviations of the band window.
    ///
    /// Sizing off the same dispersion the bands use — rather than off ATR —
    /// is what makes the stop's width in ATRs vary from trade to trade, since
    /// a standard deviation of closes and an average true range diverge as
    /// conditions change. A fixed ATR multiple would make every stop identical
    /// and the `stop_sane` check meaningless.
    pub stop_sd: f64,
    /// The stop is never closer than this many ATRs from entry.
    pub min_stop_atr: f64,
    /// Notional units per candidate, in thousands.
    pub size_units: f64,
    /// Bars after which an open trade is closed at the market.
    pub max_holding_bars: usize,
}

impl Default for StrategyParams {
    fn default() -> Self {
        Self {
            period: 20,
            k: 2.0,
            atr_period: 14,
            swing_lookback: 6,
            stop_sd: 1.5,
            min_stop_atr: 0.5,
            size_units: 100.0,
            max_holding_bars: 12,
        }
    }
}

impl StrategyParams {
    /// The first bar index at which every indicator is defined.
    pub fn warmup(&self) -> usize {
        self.period.max(self.atr_period + 1).max(self.swing_lookback)
    }
}

/// A trade the solver proposes. Every number in it is the solver's.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TradeCandidate {
    /// Which pair.
    pub pair: Pair,
    /// Which way.
    pub side: Side,
    /// When the signal printed.
    pub t: Stamp,
    /// The index of the signal bar in the pair's series.
    pub bar_index: usize,
    /// Entry price: the close of the signal bar.
    pub price: f64,
    /// Protective stop.
    pub stop: f64,
    /// Profit target: the middle band.
    pub target: f64,
    /// Notional units, in thousands.
    pub size_units: f64,
}

impl TradeCandidate {
    /// Distance from entry to stop, in price units.
    pub fn stop_distance(&self) -> f64 {
        (self.price - self.stop).abs()
    }

    /// Distance from entry to target, in price units.
    pub fn target_distance(&self) -> f64 {
        (self.target - self.price).abs()
    }

    /// Reward divided by risk, as the solver sized it.
    pub fn reward_risk(&self) -> f64 {
        if self.stop_distance() <= 0.0 {
            return 0.0;
        }
        self.target_distance() / self.stop_distance()
    }

    /// Notional, in quote currency.
    pub fn notional(&self) -> f64 {
        self.size_units * 1_000.0
    }
}

/// Simple moving average of the last `period` closes ending at `i`.
pub fn sma(bars: &[Bar], i: usize, period: usize) -> Option<f64> {
    if period == 0 || i + 1 < period {
        return None;
    }
    let sum: f64 = bars[i + 1 - period..=i].iter().map(|b| b.close).sum();
    Some(sum / period as f64)
}

/// Population standard deviation of the last `period` closes ending at `i`.
///
/// Population, not sample: the divisor is `period`, which is what the band
/// arithmetic in the tests assumes.
pub fn stdev(bars: &[Bar], i: usize, period: usize) -> Option<f64> {
    let mean = sma(bars, i, period)?;
    let variance: f64 = bars[i + 1 - period..=i]
        .iter()
        .map(|b| (b.close - mean).powi(2))
        .sum::<f64>()
        / period as f64;
    Some(variance.sqrt())
}

/// True range of bar `i`, which needs bar `i - 1`.
pub fn true_range(bars: &[Bar], i: usize) -> Option<f64> {
    if i == 0 {
        return None;
    }
    let prev_close = bars[i - 1].close;
    let b = bars[i];
    Some((b.high - b.low).max((b.high - prev_close).abs()).max((b.low - prev_close).abs()))
}

/// Average true range: the simple mean of the last `period` true ranges ending
/// at `i`. Not Wilder's smoothing.
pub fn atr(bars: &[Bar], i: usize, period: usize) -> Option<f64> {
    if period == 0 || i < period {
        return None;
    }
    let mut sum = 0.0;
    for j in i + 1 - period..=i {
        sum += true_range(bars, j)?;
    }
    Some(sum / period as f64)
}

/// Kaufman's efficiency ratio over the last `period` closes ending at `i`:
/// net movement divided by total movement, in `0..=1`.
///
/// This is the observable stand-in for "the market is trending". It is computed
/// from prices alone, which is exactly why the judgment layer is allowed to see
/// it and is not allowed to see the generator's regime.
pub fn efficiency_ratio(bars: &[Bar], i: usize, period: usize) -> Option<f64> {
    if period == 0 || i < period {
        return None;
    }
    let net = (bars[i].close - bars[i - period].close).abs();
    let mut gross = 0.0;
    for j in i + 1 - period..=i {
        gross += (bars[j].close - bars[j - 1].close).abs();
    }
    if gross <= 0.0 {
        return Some(0.0);
    }
    Some((net / gross).clamp(0.0, 1.0))
}

/// The signal on bar `i`, if there is one.
///
/// A close outside the band is a mean-reversion signal back toward the middle:
/// below the lower band is a long, above the upper band is a short.
pub fn signal_at(
    bars: &[Bar],
    i: usize,
    pair: Pair,
    p: &StrategyParams,
) -> Option<TradeCandidate> {
    if i < p.warmup() {
        return None;
    }
    let mid = sma(bars, i, p.period)?;
    let sd = stdev(bars, i, p.period)?;
    let atr = atr(bars, i, p.atr_period)?;
    if sd <= 0.0 || atr <= 0.0 {
        return None;
    }
    let upper = mid + p.k * sd;
    let lower = mid - p.k * sd;
    let close = bars[i].close;

    let side = if close < lower {
        Side::Long
    } else if close > upper {
        Side::Short
    } else {
        return None;
    };

    // The stop is the furthest of three claims on where "wrong" starts: the
    // window's dispersion, the recent swing, and an absolute ATR floor.
    let window = &bars[i + 1 - p.swing_lookback..=i];
    let swing_distance = match side {
        Side::Long => close - window.iter().map(|b| b.low).fold(f64::MAX, f64::min),
        Side::Short => window.iter().map(|b| b.high).fold(f64::MIN, f64::max) - close,
    };
    let distance = (p.stop_sd * sd)
        .max(swing_distance.max(0.0))
        .max(p.min_stop_atr * atr);
    let stop = close - side.sign() * distance;

    Some(TradeCandidate {
        pair,
        side,
        t: bars[i].t,
        bar_index: i,
        price: close,
        stop,
        target: mid,
        size_units: p.size_units,
    })
}

/// Every signal in a series, in time order.
pub fn signals(bars: &[Bar], pair: Pair, p: &StrategyParams) -> Vec<TradeCandidate> {
    (0..bars.len()).filter_map(|i| signal_at(bars, i, pair, p)).collect()
}
