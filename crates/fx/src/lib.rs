//! The forex domain: a deterministic mean-reversion solver under a Jev
//! judgment pipeline, and a simulator that runs the same signals twice —
//! once as the solver sized them, once as the gate allowed.
//!
//! The split is strict. [`strategy`] produces every number: entry, stop,
//! target, size. [`judgment`] decides what happens to that proposal.
//! [`features`] is the boundary between them, and is the only thing Jev reads.

#![warn(missing_docs)]

pub mod engine;
pub mod features;
pub mod judgment;
pub mod strategy;

pub use engine::{simulate, summarise, BookResult, DecisionRecord, Exit, Fill};
pub use features::FxFeatures;
pub use judgment::{
    gate_spec, judge, regime_spec, risk_spec, sanity_spec, CandidateView, FxDecisionInput,
    FxJudgment, FxRegime,
};
pub use strategy::{signals, Side, StrategyParams, TradeCandidate};

use jev_core::{Action, Jev, Pipeline, Result};
use synth::{FxWorld, Pair};

/// Everything one forex run produced.
#[derive(Debug)]
pub struct FxSession {
    /// The world it ran on.
    pub seed: u64,
    /// Every candidate, its judgment and both books' outcomes, in time order.
    pub decisions: Vec<DecisionRecord>,
    /// The book that ignored the judgment layer.
    pub ungated: BookResult,
    /// The book that obeyed it.
    pub gated: BookResult,
}

impl FxSession {
    /// How many candidates the gate let through untouched.
    pub fn executed(&self) -> usize {
        self.count_action(Action::Execute)
    }

    /// How many of each gate action were taken.
    pub fn count_action(&self, action: Action) -> usize {
        self.decisions.iter().filter(|d| d.judgment.gate.action == action).count()
    }

    /// The gate's distribution over actions.
    pub fn gate_distribution(&self) -> Vec<(Action, usize)> {
        Action::ALL.iter().map(|a| (*a, self.count_action(*a))).collect()
    }

    /// How often the classifier matched the generator's regime.
    pub fn regime_accuracy(&self) -> f64 {
        if self.decisions.is_empty() {
            return 0.0;
        }
        let correct = self.decisions.iter().filter(|d| d.regime_correct()).count();
        correct as f64 / self.decisions.len() as f64
    }

    /// The decisions where the gate changed the outcome most, largest first.
    pub fn most_consequential(&self, n: usize) -> Vec<&DecisionRecord> {
        let mut ranked: Vec<&DecisionRecord> =
            self.decisions.iter().filter(|d| d.intervened()).collect();
        ranked.sort_by(|a, b| b.pnl_delta().abs().total_cmp(&a.pnl_delta().abs()));
        ranked.into_iter().take(n).collect()
    }

    /// How many decisions the gate changed at all.
    pub fn interventions(&self) -> usize {
        self.decisions.iter().filter(|d| d.intervened()).count()
    }

    /// How many went to a human.
    pub fn escalations(&self) -> usize {
        self.count_action(Action::Escalate)
    }
}

/// Run every pair of a world through the strategy and the judgment pipeline.
///
/// Candidates are judged in time order across all three pairs, against a book
/// that carries whatever the gated session currently has open. That is what
/// makes concentration a live condition rather than a constant: the same
/// EUR/USD long is a different proposition on an empty book than on one
/// already long three EUR positions.
///
/// The running book follows the *gated* session, because that is the book
/// actually being traded. The ungated column is a counterfactual on the same
/// signals, not a second live book.
///
/// `day_limit` caps how many days are judged, which keeps a live demo's call
/// count — and bill — bounded. `None` runs the whole world.
pub async fn run_session(
    jev: &Jev,
    world: &FxWorld,
    params: &StrategyParams,
    day_limit: Option<u32>,
) -> Result<FxSession> {
    let horizon = day_limit.unwrap_or(world.days).min(world.days);

    // Every candidate from every pair, in time order.
    let mut queue: Vec<(Pair, TradeCandidate)> = Pair::ALL
        .iter()
        .flat_map(|pair| {
            signals(&world.series_for(*pair).bars, *pair, params)
                .into_iter()
                .map(move |c| (*pair, c))
        })
        .filter(|(_, c)| c.t.day < horizon)
        .collect();
    queue.sort_by_key(|(pair, c)| (c.t.index(), *pair));

    let mut book = RunningBook::new(world.book.clone());
    let mut decisions = Vec::with_capacity(queue.len());

    for (pair, candidate) in queue {
        let series = world.series_for(pair);
        let bars = &series.bars;
        book.expire(candidate.t);

        let input = build_input(world, bars, &candidate, params, &book.exposures());
        let mut pipeline = Pipeline::new(jev, "fx");
        let judgment = judge(&mut pipeline, &input, &candidate).await?;

        let ungated = simulate(&candidate, bars, 1.0, params);
        let gated = if judgment.gate.action.acts() {
            simulate(&candidate, bars, judgment.gate.size_factor as f64, params)
        } else {
            None
        };
        if let Some(fill) = gated {
            book.open(&candidate, fill.size_factor, fill.exited);
        }

        decisions.push(DecisionRecord {
            candidate,
            judgment,
            // Attached only after the judgment is made, never sent to Jev.
            true_regime: series.true_regime(candidate.bar_index),
            ungated,
            gated,
        });
    }

    let ungated = summarise(decisions.iter().filter_map(|d| d.ungated));
    let gated = summarise(decisions.iter().filter_map(|d| d.gated));
    Ok(FxSession { seed: world.seed, decisions, ungated, gated })
}

/// The book as it stands while a session runs: the starting exposures plus
/// whatever the gated session currently has open.
#[derive(Debug, Clone)]
pub struct RunningBook {
    starting: Vec<synth::Exposure>,
    open: Vec<(synth::Currency, f64, synth::Stamp)>,
}

impl RunningBook {
    /// A book holding only the starting exposures.
    pub fn new(starting: Vec<synth::Exposure>) -> Self {
        Self { starting, open: Vec::new() }
    }

    /// Drop positions that closed at or before `now`.
    pub fn expire(&mut self, now: synth::Stamp) {
        self.open.retain(|(_, _, exit)| exit.index() > now.index());
    }

    /// Record a position going on.
    pub fn open(&mut self, candidate: &TradeCandidate, size_factor: f64, exit: synth::Stamp) {
        let currency = features::added_currency(candidate.pair, candidate.side);
        self.open.push((currency, candidate.size_units * size_factor, exit));
    }

    /// The book's exposures, starting plus open.
    pub fn exposures(&self) -> Vec<synth::Exposure> {
        let mut out = self.starting.clone();
        for (currency, units, _) in &self.open {
            match out.iter_mut().find(|e| e.currency == *currency) {
                Some(entry) => entry.units += units,
                None => out.push(synth::Exposure { currency: *currency, units: *units }),
            }
        }
        out
    }

    /// How many positions are open.
    pub fn open_positions(&self) -> usize {
        self.open.len()
    }
}

/// Assemble the input for one candidate.
///
/// Takes `bars` rather than the series, so the generator's truth is out of
/// scope for everything that builds a Jev call.
pub fn build_input(
    world: &FxWorld,
    bars: &[synth::Bar],
    candidate: &TradeCandidate,
    params: &StrategyParams,
    book: &[synth::Exposure],
) -> FxDecisionInput {
    let headlines: Vec<String> = world
        .headlines_on(candidate.t.day)
        .iter()
        .map(|h| h.text.clone())
        .collect();
    let informative =
        world.headlines_on(candidate.t.day).iter().filter(|h| h.informative).count() as u32;

    FxDecisionInput {
        features: FxFeatures::compute(
            candidate,
            bars,
            &world.calendar,
            book,
            informative,
            params,
        ),
        candidate: CandidateView::of(candidate, world.start),
        headlines,
    }
}

/// The same world with every calendar entry on `day` removed.
///
/// The demo replays one CPI day twice — with the release present and with it
/// gone — and prints the two gate records side by side. Only the calendar
/// changes; the prices are identical, so the difference in the gate is the
/// judgment layer reacting to the event and nothing else.
pub fn without_events_on(world: &FxWorld, day: u32) -> FxWorld {
    let mut clone = world.clone();
    clone.calendar.retain(|e| e.t.day != day);
    clone.headlines.retain(|h| !(h.t.day == day && h.informative));
    clone
}
