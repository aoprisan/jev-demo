//! Fills and P&L. Deterministic, and identical for the gated and ungated books
//! except for the size the gate allowed.

use crate::judgment::FxJudgment;
use crate::strategy::{Side, StrategyParams, TradeCandidate};
use jev_core::Action;
use serde::{Deserialize, Serialize};
use synth::{Bar, Stamp};

/// How a trade ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exit {
    /// The target was reached.
    Target,
    /// The stop was hit.
    Stop,
    /// The holding limit expired; closed at the market.
    Timeout,
    /// The series ended with the trade open.
    EndOfData,
}

/// One filled trade.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    /// When it was entered.
    pub entered: Stamp,
    /// When it was closed.
    pub exited: Stamp,
    /// How it ended.
    pub exit: Exit,
    /// Entry price.
    pub entry_price: f64,
    /// Exit price.
    pub exit_price: f64,
    /// The fraction of the solver's size that was actually put on.
    pub size_factor: f64,
    /// The notional actually put on, in quote currency.
    pub notional: f64,
    /// Profit or loss in quote currency.
    pub pnl: f64,
    /// The same, as basis points of the notional actually put on.
    pub pnl_bps: f64,
}

/// Simulate one candidate against the bars that follow it.
///
/// Both the stop and the target can be inside the same bar; the stop is taken
/// first. That is the pessimistic assumption, and applying it to both books
/// keeps the comparison fair.
pub fn simulate(
    candidate: &TradeCandidate,
    bars: &[Bar],
    size_factor: f64,
    p: &StrategyParams,
) -> Option<Fill> {
    if size_factor <= 0.0 {
        return None;
    }
    let entry_index = candidate.bar_index;
    let entry_price = candidate.price;
    let last = bars.len() - 1;
    let deadline = (entry_index + p.max_holding_bars).min(last);

    let mut exit = Exit::EndOfData;
    let mut exit_price = bars[last].close;
    let mut exited = bars[last].t;

    for i in entry_index + 1..=deadline {
        let bar = &bars[i];
        let (hit_stop, hit_target) = match candidate.side {
            Side::Long => (bar.low <= candidate.stop, bar.high >= candidate.target),
            Side::Short => (bar.high >= candidate.stop, bar.low <= candidate.target),
        };
        if hit_stop {
            exit = Exit::Stop;
            exit_price = candidate.stop;
            exited = bar.t;
            break;
        }
        if hit_target {
            exit = Exit::Target;
            exit_price = candidate.target;
            exited = bar.t;
            break;
        }
        if i == deadline {
            exit = if deadline == last { Exit::EndOfData } else { Exit::Timeout };
            exit_price = bar.close;
            exited = bar.t;
        }
    }

    let notional = candidate.notional() * size_factor;
    let move_fraction = (exit_price - entry_price) / entry_price * candidate.side.sign();
    Some(Fill {
        entered: candidate.t,
        exited,
        exit,
        entry_price,
        exit_price,
        size_factor,
        notional,
        pnl: move_fraction * notional,
        pnl_bps: move_fraction * 10_000.0,
    })
}

/// One candidate, its judgment, and what each book did with it.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionRecord {
    /// The solver's proposal.
    pub candidate: TradeCandidate,
    /// What Jev decided.
    pub judgment: FxJudgment,
    /// The generator's regime at the signal bar. **Evaluation only** — this is
    /// attached after the judgment is made and is never part of any Jev call.
    pub true_regime: synth::Regime,
    /// The fill the ungated book got: always the solver's full size.
    pub ungated: Option<Fill>,
    /// The fill the gated book got, if the gate let anything through.
    pub gated: Option<Fill>,
}

impl DecisionRecord {
    /// How much the gate changed this trade's P&L.
    pub fn pnl_delta(&self) -> f64 {
        self.gated.map(|f| f.pnl).unwrap_or(0.0)
            - self.ungated.map(|f| f.pnl).unwrap_or(0.0)
    }

    /// Whether the gate changed what happened at all.
    pub fn intervened(&self) -> bool {
        self.judgment.gate.action != Action::Execute
            || self.judgment.gate.size_factor < 1.0
    }

    /// Whether the classifier agreed with the generator.
    pub fn regime_correct(&self) -> bool {
        self.judgment.regime.label == crate::FxRegime::from_truth(self.true_regime)
    }
}

/// A book's outcome over a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BookResult {
    /// Trades actually put on.
    pub trades: usize,
    /// Total profit and loss, in quote currency.
    pub pnl: f64,
    /// Trades that made money.
    pub wins: usize,
    /// Trades that lost money.
    pub losses: usize,
    /// Worst peak-to-trough fall in cumulative P&L.
    pub max_drawdown: f64,
    /// Total notional put on.
    pub notional: f64,
}

impl BookResult {
    /// Fraction of trades that made money.
    pub fn hit_rate(&self) -> f64 {
        if self.trades == 0 {
            return 0.0;
        }
        self.wins as f64 / self.trades as f64
    }

    /// P&L per unit of notional, in basis points.
    pub fn return_bps(&self) -> f64 {
        if self.notional <= 0.0 {
            return 0.0;
        }
        self.pnl / self.notional * 10_000.0
    }
}

/// Aggregate a sequence of fills, in time order, into a book result.
pub fn summarise(fills: impl Iterator<Item = Fill>) -> BookResult {
    let mut result = BookResult::default();
    let mut cumulative: f64 = 0.0;
    let mut peak: f64 = 0.0;
    for fill in fills {
        result.trades += 1;
        result.pnl += fill.pnl;
        result.notional += fill.notional;
        if fill.pnl > 0.0 {
            result.wins += 1;
        } else if fill.pnl < 0.0 {
            result.losses += 1;
        }
        cumulative += fill.pnl;
        peak = peak.max(cumulative);
        result.max_drawdown = result.max_drawdown.max(peak - cumulative);
    }
    result
}
