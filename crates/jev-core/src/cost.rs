//! What the judgment layer cost: tokens, latency and an estimate in dollars,
//! totalled over a run and over each decision in it.
//!
//! The tokens and the latency are measured — every [`CallRecord`] carries what
//! the backend reported. The dollars are an *estimate*: System One's price list
//! is not part of this repository, so [`Rates::ASSUMED`] stands in until a desk
//! sets its own (see [`Rates::from_env`]). Every rendering of the figure says
//! so, and carries the rates it used.
//!
//! A mock run is priced the same way a live one is. Nothing was billed, but the
//! calls, the state and the questions are the ones a live run would have made,
//! so the estimate answers the question the demo is actually asked: what would
//! this desk pay to run its judgment layer for a quarter?

use crate::client::CallRecord;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// The environment variable naming the input-token price.
pub const INPUT_RATE_ENV: &str = "JEV_USD_PER_MTOK_IN";
/// The environment variable naming the output-token price.
pub const OUTPUT_RATE_ENV: &str = "JEV_USD_PER_MTOK_OUT";

/// What a million tokens cost, in US dollars.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rates {
    /// USD per million input tokens.
    pub input_usd_per_mtok: f64,
    /// USD per million output tokens.
    pub output_usd_per_mtok: f64,
}

impl Rates {
    /// The stand-in prices, used when the environment names none.
    ///
    /// These are an assumption, not a quote: a placeholder for the rate a desk
    /// has contracted, at the order of magnitude a frontier model is priced at.
    /// Anything that prints a dollar figure derived from them says "est.".
    pub const ASSUMED: Rates = Rates { input_usd_per_mtok: 3.0, output_usd_per_mtok: 15.0 };

    /// Free: what a run costs when nothing is billed for it.
    pub const FREE: Rates = Rates { input_usd_per_mtok: 0.0, output_usd_per_mtok: 0.0 };

    /// The rates named by [`INPUT_RATE_ENV`] and [`OUTPUT_RATE_ENV`], falling
    /// back to [`Rates::ASSUMED`] for either one that is absent or unreadable.
    pub fn from_env() -> Self {
        fn rate(name: &str, fallback: f64) -> f64 {
            std::env::var(name).ok().and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(fallback)
        }
        Rates {
            input_usd_per_mtok: rate(INPUT_RATE_ENV, Self::ASSUMED.input_usd_per_mtok),
            output_usd_per_mtok: rate(OUTPUT_RATE_ENV, Self::ASSUMED.output_usd_per_mtok),
        }
    }

    /// What those token counts come to.
    pub fn usd(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        (input_tokens as f64 * self.input_usd_per_mtok
            + output_tokens as f64 * self.output_usd_per_mtok)
            / 1_000_000.0
    }

    /// Whether these are the stand-in prices rather than a desk's own.
    pub fn assumed(&self) -> bool {
        *self == Self::ASSUMED
    }
}

impl Default for Rates {
    fn default() -> Self {
        Self::ASSUMED
    }
}

/// What some set of calls cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct CostEstimate {
    /// How many calls were made.
    pub calls: usize,
    /// Input tokens reported across them.
    pub input_tokens: u64,
    /// Output tokens reported across them.
    pub output_tokens: u64,
    /// Wall-clock milliseconds across them.
    pub latency_ms: u64,
    /// What they come to at the ledger's rates. An estimate, never a bill.
    pub usd: f64,
}

impl CostEstimate {
    /// Input plus output.
    pub fn tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// Total the given records at `rates`.
    pub fn of<'a>(records: impl IntoIterator<Item = &'a CallRecord>, rates: Rates) -> Self {
        let mut out = CostEstimate::default();
        for record in records {
            out.add(record);
        }
        out.usd = rates.usd(out.input_tokens, out.output_tokens);
        out
    }

    /// Add one call, leaving [`CostEstimate::usd`] to the caller.
    fn add(&mut self, record: &CallRecord) {
        self.calls += 1;
        self.input_tokens += record.input_tokens.unwrap_or(0);
        self.output_tokens += record.output_tokens.unwrap_or(0);
        self.latency_ms += record.latency_ms;
    }
}

/// A run's cost, split by the decision each call was tagged with.
///
/// This is what [`CallRecord::decision`] buys: a trade is judged by more than
/// one call — forex takes two, battery three — so "what did this trade cost?"
/// is a question about a group of calls, and the tag is what groups them.
#[derive(Debug, Clone)]
pub struct CostLedger {
    rates: Rates,
    total: CostEstimate,
    by_decision: IndexMap<String, CostEstimate>,
    untagged: CostEstimate,
}

impl CostLedger {
    /// Total `records` at `rates`, keeping each decision's share.
    pub fn of(records: &[CallRecord], rates: Rates) -> Self {
        let mut total = CostEstimate::default();
        let mut by_decision: IndexMap<String, CostEstimate> = IndexMap::new();
        let mut untagged = CostEstimate::default();
        for record in records {
            total.add(record);
            match record.decision.as_deref() {
                Some(id) => by_decision.entry(id.to_owned()).or_default().add(record),
                None => untagged.add(record),
            }
        }
        let price = |e: &mut CostEstimate| e.usd = rates.usd(e.input_tokens, e.output_tokens);
        price(&mut total);
        price(&mut untagged);
        for estimate in by_decision.values_mut() {
            price(estimate);
        }
        Self { rates, total, by_decision, untagged }
    }

    /// The prices this ledger was totalled at.
    pub fn rates(&self) -> Rates {
        self.rates
    }

    /// Every call in the run.
    pub fn total(&self) -> CostEstimate {
        self.total
    }

    /// What one decision's calls cost. A decision nothing was recorded for
    /// costs nothing, which is what a zeroed estimate says.
    pub fn of_decision(&self, id: &str) -> CostEstimate {
        self.by_decision.get(id).copied().unwrap_or_default()
    }

    /// Each decision's share, in the order the decisions were first judged.
    pub fn by_decision(&self) -> impl Iterator<Item = (&str, CostEstimate)> {
        self.by_decision.iter().map(|(id, e)| (id.as_str(), *e))
    }

    /// How many distinct decisions the run tagged.
    pub fn decisions(&self) -> usize {
        self.by_decision.len()
    }

    /// The calls that belong to no decision — the report's `Explain` calls.
    pub fn untagged(&self) -> CostEstimate {
        self.untagged
    }

    /// The mean cost of a decision, over the decisions that made calls.
    pub fn usd_per_decision(&self) -> f64 {
        if self.by_decision.is_empty() {
            return 0.0;
        }
        let tagged: f64 = self.by_decision.values().map(|e| e.usd).sum();
        tagged / self.by_decision.len() as f64
    }
}
