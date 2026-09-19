//! The fx judgment pipeline:
//! `Classify(regime) -> Check(sanity) -> Score(event risk) -> Gate`.

use crate::features::FxFeatures;
use crate::strategy::TradeCandidate;
use jev_core::{
    CheckOut, CheckSpec, ClassifyOut, ClassifySpec, GateOut, GateSpec, JevInput, Label, Pipeline,
    Result, ScoreOut, ScoreSpec,
};
use serde::{Deserialize, Serialize};

/// The regime a forex decision is being taken in, as Jev reads it.
///
/// Deliberately the same partition the generator uses, so that the demo can
/// report how often the judgment layer agrees with ground truth — without ever
/// letting it see that truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FxRegime {
    /// Prices are turning back on themselves. Mean reversion belongs here.
    Ranging,
    /// Prices are going one way and staying there.
    Trending,
    /// A scheduled release dominates the window.
    EventDriven,
}

impl Label for FxRegime {
    fn labels() -> &'static [Self] {
        &[FxRegime::Ranging, FxRegime::Trending, FxRegime::EventDriven]
    }

    fn name(&self) -> &'static str {
        match self {
            FxRegime::Ranging => "ranging",
            FxRegime::Trending => "trending",
            FxRegime::EventDriven => "event_driven",
        }
    }

    fn describe(&self) -> &'static str {
        match self {
            FxRegime::Ranging => {
                "Prices are oscillating around a level: excursions outside the band come back. \
                 Directional persistence is low and no release dominates the window."
            }
            FxRegime::Trending => {
                "Prices are moving persistently one way. Excursions outside the band extend \
                 rather than revert. Directional persistence is high."
            }
            FxRegime::EventDriven => {
                "A scheduled macroeconomic release dominates the window. Ranges are wide, \
                 moves gap, and the level before the release carries little information."
            }
        }
    }
}

impl FxRegime {
    /// The matching generator regime, for scoring the classifier against truth.
    pub fn from_truth(regime: synth::Regime) -> Self {
        match regime {
            synth::Regime::Ranging => FxRegime::Ranging,
            synth::Regime::Trending => FxRegime::Trending,
            synth::Regime::EventDriven => FxRegime::EventDriven,
        }
    }
}

/// One forex decision, as Jev sees it.
///
/// `features` is the observable state; `candidate` is what the solver built.
/// Neither carries the generator's regime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FxDecisionInput {
    /// The observable state.
    pub features: FxFeatures,
    /// The solver's proposal. Jev may judge it; it may not change it.
    pub candidate: CandidateView,
    /// Today's headlines, mostly noise.
    pub headlines: Vec<String>,
}

/// The candidate as Jev sees it: prices, not instructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateView {
    /// Which pair.
    pub pair: String,
    /// Which way.
    pub side: String,
    /// Entry.
    pub price: f64,
    /// Stop.
    pub stop: f64,
    /// Target.
    pub target: f64,
    /// Notional units, in thousands.
    pub size_units: f64,
    /// When the signal printed.
    pub at: String,
}

impl CandidateView {
    /// Render a candidate for Jev.
    pub fn of(c: &TradeCandidate, start: synth::Date) -> Self {
        Self {
            pair: c.pair.code().to_owned(),
            side: c.side.as_str().to_owned(),
            price: round(c.price, 5),
            stop: round(c.stop, 5),
            target: round(c.target, 5),
            size_units: c.size_units,
            at: c.t.on(start),
        }
    }
}

fn round(v: f64, places: u32) -> f64 {
    let f = 10f64.powi(places as i32);
    (v * f).round() / f
}

impl JevInput for FxDecisionInput {
    fn context_block(&self) -> String {
        let f = &self.features;
        let event = match &f.next_event_kind {
            Some(kind) if f.hours_to_event < 48.0 => {
                format!("{kind} in {:.1}h", f.hours_to_event)
            }
            _ => "no release scheduled nearby".to_owned(),
        };
        format!(
            "Domain: spot forex, 4-hour bars. A deterministic Bollinger mean-reversion solver \
             has proposed a {} in {} at {:.5}, stopping at {:.5} and targeting {:.5} — a stop \
             {:.2} ATRs away for {:.2}:1 reward-to-risk on {:.0}k units.\n\
             Conditions: {event}. Directional persistence {:.2} of 1. Volatility at the {:.0}th \
             percentile of its recent history. The trade would take {} from {:.0}% to {:.0}% of \
             the book.\n\
             The solver owns those numbers. You are judging whether, and how much of, this \
             should be acted on.",
            f.side,
            f.pair,
            self.candidate.price,
            self.candidate.stop,
            self.candidate.target,
            f.stop_atr_multiple,
            f.reward_risk,
            self.candidate.size_units,
            f.trend_strength,
            f.volatility_percentile * 100.0,
            f.exposure_currency(),
            f.pre_trade_exposure_share * 100.0,
            f.post_trade_exposure_share * 100.0,
        )
    }
}

impl FxFeatures {
    /// The leg of the pair this trade adds to, for the context block.
    fn exposure_currency(&self) -> &str {
        // The pair's leg the trade adds to, derived from the rendered side.
        if self.side == "long" {
            self.pair.split('/').next().unwrap_or("the base")
        } else {
            self.pair.split('/').nth(1).unwrap_or("the quote")
        }
    }
}

/// Everything the pipeline decided about one candidate.
#[derive(Debug, Clone, Serialize)]
pub struct FxJudgment {
    /// Which regime Jev read.
    pub regime: ClassifyOut<FxRegime>,
    /// The sanity checks.
    pub checks: CheckOut,
    /// Event risk, 0..=100.
    pub risk: ScoreOut,
    /// The gate.
    pub gate: GateOut,
}

/// The classify stage.
pub fn regime_spec() -> ClassifySpec {
    ClassifySpec::new(
        "the price window",
        "Which regime are these 4-hour bars in? Judge from the price action and the \
         calendar, not from what the trade would prefer.",
    )
}

/// The three sanity checks, exactly as the brief names them.
pub fn sanity_spec() -> CheckSpec {
    CheckSpec::new("the proposed trade")
        .item(
            "stop_sane",
            "The stop is far enough from entry to survive ordinary noise in this market.",
            "the stop sits beyond the window's normal swing, at or above roughly 1.2 ATRs",
            "the stop is inside the range this market moves in a single bar",
        )
        .item(
            "signal_valid_in_regime",
            "A mean-reversion signal is valid in the regime this window is in.",
            "prices are turning back on themselves, so an excursion outside the band reverts",
            "prices are persisting one way, so an excursion outside the band extends",
        )
        .item(
            "correlated_exposure_ok",
            "Adding this trade leaves the book's currency exposure acceptably balanced.",
            "the currency stays a reasonable share of the book",
            "the trade roughly doubles a currency already held, concentrating the book",
        )
}

/// The event-risk score and its driver vocabulary.
pub fn risk_spec() -> ScoreSpec {
    ScoreSpec::new(
        "event risk",
        "How much risk does the calendar and the state of this market put on a trade \
         entered now and held for a day or so?",
        [
            "Nothing scheduled and nothing unusual in the tape.",
            "Minor: something on the calendar, but distant or low impact.",
            "Real: a release within the holding period, or a market already moving.",
            "High: a major release close at hand, or conditions already disorderly.",
            "Severe: a top-tier release imminent into a market that cannot absorb it.",
        ],
    )
    .driver("imminent_event", "A scheduled release lands within the next few hours.")
    .driver("high_impact_event", "The release in question is a top-tier one (CPI, NFP, FOMC).")
    .driver("tight_stop", "The stop is tight relative to how far this market moves.")
    .driver("trend_pressure", "The market has been persisting in one direction.")
    .driver("crowded_book", "The book is already concentrated in the currency this adds to.")
}

/// The gate.
pub fn gate_spec(candidate: &TradeCandidate) -> GateSpec {
    GateSpec::new(
        format!("{} {}", candidate.pair.code(), candidate.side.as_str()),
        "Should this trade go on as the solver sized it, and if not, what instead?",
    )
}

/// Run the full fx pipeline over one candidate.
pub async fn judge(
    pipeline: &mut Pipeline,
    input: &FxDecisionInput,
    candidate: &TradeCandidate,
) -> Result<FxJudgment> {
    let regime: ClassifyOut<FxRegime> =
        pipeline.classify("regime", input, &regime_spec()).await?;
    let checks = pipeline.check("sanity", input, &sanity_spec()).await?;
    let risk = pipeline.score("risk", input, &risk_spec()).await?;
    let gate = pipeline.gate("gate", input, &gate_spec(candidate)).await?;
    Ok(FxJudgment { regime, checks, risk, gate })
}
