//! A synthetic forex world: regime-switching prices, an economic calendar, a
//! mostly-noise headline feed, and a starting book.

use crate::calendar::{Date, Stamp};
use crate::rng::Rng;
use serde::{Deserialize, Serialize};

/// Bars are four hours long, so six to a day.
pub const BARS_PER_DAY: u32 = 6;
/// Hours per bar.
pub const BAR_HOURS: u32 = 24 / BARS_PER_DAY;

/// The currencies in play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Currency {
    /// Euro.
    Eur,
    /// US dollar.
    Usd,
    /// Pound sterling.
    Gbp,
    /// Japanese yen.
    Jpy,
}

impl Currency {
    /// ISO code.
    pub fn code(&self) -> &'static str {
        match self {
            Currency::Eur => "EUR",
            Currency::Usd => "USD",
            Currency::Gbp => "GBP",
            Currency::Jpy => "JPY",
        }
    }
}

impl std::fmt::Display for Currency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

/// The traded pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Pair {
    /// EUR/USD.
    EurUsd,
    /// GBP/USD.
    GbpUsd,
    /// USD/JPY.
    UsdJpy,
}

impl Pair {
    /// All three, in a fixed order.
    pub const ALL: [Pair; 3] = [Pair::EurUsd, Pair::GbpUsd, Pair::UsdJpy];

    /// Display code.
    pub fn code(&self) -> &'static str {
        match self {
            Pair::EurUsd => "EUR/USD",
            Pair::GbpUsd => "GBP/USD",
            Pair::UsdJpy => "USD/JPY",
        }
    }

    /// The base currency.
    pub fn base(&self) -> Currency {
        match self {
            Pair::EurUsd => Currency::Eur,
            Pair::GbpUsd => Currency::Gbp,
            Pair::UsdJpy => Currency::Usd,
        }
    }

    /// The quote currency.
    pub fn quote(&self) -> Currency {
        match self {
            Pair::EurUsd | Pair::GbpUsd => Currency::Usd,
            Pair::UsdJpy => Currency::Jpy,
        }
    }

    /// The starting level.
    fn anchor(&self) -> f64 {
        match self {
            Pair::EurUsd => 1.0850,
            Pair::GbpUsd => 1.2720,
            Pair::UsdJpy => 148.40,
        }
    }

    /// One pip, in price units.
    pub fn pip(&self) -> f64 {
        match self {
            Pair::UsdJpy => 0.01,
            _ => 0.0001,
        }
    }

    /// Per-bar volatility as a fraction of price.
    fn base_vol(&self) -> f64 {
        match self {
            Pair::EurUsd => 0.0016,
            Pair::GbpUsd => 0.0021,
            Pair::UsdJpy => 0.0019,
        }
    }
}

impl std::fmt::Display for Pair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

/// The generator's regime. **Ground truth**, for evaluation only.
///
/// Kept in a parallel array rather than on [`Bar`], so that handing a bar to a
/// strategy — or to Jev — cannot leak it by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Regime {
    /// Mean-reverting around a slow anchor.
    Ranging,
    /// Persistent directional drift.
    Trending,
    /// Wide and jumpy around a scheduled event.
    EventDriven,
}

impl Regime {
    /// All three.
    pub const ALL: [Regime; 3] = [Regime::Ranging, Regime::Trending, Regime::EventDriven];

    /// Lower-case name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Regime::Ranging => "ranging",
            Regime::Trending => "trending",
            Regime::EventDriven => "event_driven",
        }
    }
}

/// One four-hour candle. Carries no ground truth.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    /// When the bar opened.
    pub t: Stamp,
    /// Open.
    pub open: f64,
    /// High.
    pub high: f64,
    /// Low.
    pub low: f64,
    /// Close.
    pub close: f64,
}

/// A scheduled macroeconomic release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    /// Consumer price index.
    Cpi,
    /// Non-farm payrolls.
    Nfp,
    /// European Central Bank decision.
    Ecb,
    /// Federal Open Market Committee decision.
    Fomc,
}

impl EventKind {
    /// Short code, as the fx layer passes it to Jev.
    pub fn code(&self) -> &'static str {
        match self {
            EventKind::Cpi => "CPI",
            EventKind::Nfp => "NFP",
            EventKind::Ecb => "ECB",
            EventKind::Fomc => "FOMC",
        }
    }

    /// The currency the release moves.
    pub fn currency(&self) -> Currency {
        match self {
            EventKind::Ecb => Currency::Eur,
            _ => Currency::Usd,
        }
    }

    /// How hard it hits, as a multiplier on the bar's volatility.
    pub fn impact(&self) -> f64 {
        match self {
            EventKind::Cpi => 3.4,
            EventKind::Nfp => 3.1,
            EventKind::Fomc => 3.8,
            EventKind::Ecb => 2.6,
        }
    }
}

/// One entry in the economic calendar.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// When it lands.
    pub t: Stamp,
    /// What it is.
    pub kind: EventKind,
    /// The currency it moves.
    pub currency: Currency,
}

/// A headline. Most are noise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Headline {
    /// When it printed.
    pub t: Stamp,
    /// The text.
    pub text: String,
    /// Whether it carries any information at all.
    pub informative: bool,
}

/// A currency position on the starting book.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Exposure {
    /// Which currency.
    pub currency: Currency,
    /// Signed units, in thousands.
    pub units: f64,
}

/// One pair's price history, with its ground truth kept alongside.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairSeries {
    /// The pair.
    pub pair: Pair,
    /// The bars.
    pub bars: Vec<Bar>,
    /// `truth[i]` is the regime that generated `bars[i]`. Evaluation only:
    /// nothing that reaches a strategy or a judgment call may read this.
    pub truth: Vec<Regime>,
}

impl PairSeries {
    /// The regime that generated bar `i`.
    pub fn true_regime(&self, i: usize) -> Regime {
        self.truth[i]
    }
}

/// A whole synthetic forex world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxWorld {
    /// The seed it was generated from.
    pub seed: u64,
    /// The first day.
    pub start: Date,
    /// How many days.
    pub days: u32,
    /// One series per pair, in [`Pair::ALL`] order.
    pub series: Vec<PairSeries>,
    /// Scheduled releases, in time order.
    pub calendar: Vec<CalendarEvent>,
    /// The headline feed, in time order.
    pub headlines: Vec<Headline>,
    /// The starting book.
    pub book: Vec<Exposure>,
}

impl FxWorld {
    /// Generate a world from `seed` over `days` days.
    pub fn generate(seed: u64, days: u32) -> Self {
        let start = Date::new(2025, 1, 6); // a Monday
        let calendar = generate_calendar(seed, days, start);
        let series =
            Pair::ALL.iter().map(|pair| generate_series(seed, *pair, days, &calendar)).collect();
        FxWorld {
            seed,
            start,
            days,
            series,
            headlines: generate_headlines(seed, days, &calendar),
            book: starting_book(seed),
            calendar,
        }
    }

    /// The default 90-day world.
    pub fn default_world(seed: u64) -> Self {
        Self::generate(seed, 90)
    }

    /// One pair's series.
    pub fn series_for(&self, pair: Pair) -> &PairSeries {
        self.series.iter().find(|s| s.pair == pair).expect("every pair is generated")
    }

    /// The next scheduled event at or after `t` that moves either leg of `pair`.
    pub fn next_event(&self, pair: Pair, t: Stamp) -> Option<&CalendarEvent> {
        self.calendar.iter().find(|e| {
            e.t.index() >= t.index() && (e.currency == pair.base() || e.currency == pair.quote())
        })
    }

    /// Every event on `day`.
    pub fn events_on(&self, day: u32) -> Vec<&CalendarEvent> {
        self.calendar.iter().filter(|e| e.t.day == day).collect()
    }

    /// The headlines printed on `day`.
    pub fn headlines_on(&self, day: u32) -> Vec<&Headline> {
        self.headlines.iter().filter(|h| h.t.day == day).collect()
    }

    /// The first day carrying an event of `kind`, for the demo replay.
    pub fn first_day_with(&self, kind: EventKind) -> Option<u32> {
        self.calendar.iter().find(|e| e.kind == kind).map(|e| e.t.day)
    }
}

/// Releases land on a recognisable monthly rhythm, always on a weekday.
fn generate_calendar(seed: u64, days: u32, start: Date) -> Vec<CalendarEvent> {
    let mut rng = Rng::stream(seed, "fx.calendar");
    let mut events = Vec::new();
    for day in 0..days {
        let date = start.plus_days(day as i64);
        if date.is_weekend() {
            continue;
        }
        let dom = date.day;
        // CPI mid-month, NFP on the first Friday, central banks around the 20th.
        // CPI is the first weekday of its window, so a weekend does not
        // produce three of them in a row.
        let cpi_window = 11..=13;
        let cpi_today = cpi_window.contains(&dom)
            && (11..dom).all(|earlier| date.plus_days(earlier as i64 - dom as i64).is_weekend());
        let kind = if cpi_today {
            Some(EventKind::Cpi)
        } else if dom <= 7 && date.weekday() == 4 {
            Some(EventKind::Nfp)
        } else if (19..=21).contains(&dom) && date.weekday() == 3 {
            Some(EventKind::Ecb)
        } else if (18..=20).contains(&dom) && date.weekday() == 2 {
            Some(EventKind::Fomc)
        } else {
            None
        };
        if let Some(kind) = kind {
            // Releases print in the European or US morning.
            let hour = *rng.pick(&[8u32, 12, 14]);
            events.push(CalendarEvent {
                t: Stamp::new(day, hour),
                kind,
                currency: kind.currency(),
            });
        }
    }
    events.sort_by_key(|e| e.t.index());
    events
}

/// A regime-switching random walk, with event bars widened and jumped.
fn generate_series(seed: u64, pair: Pair, days: u32, calendar: &[CalendarEvent]) -> PairSeries {
    let mut rng = Rng::stream(seed, &format!("fx.series.{}", pair.code()));
    let total = (days * BARS_PER_DAY) as usize;
    let mut bars = Vec::with_capacity(total);
    let mut truth = Vec::with_capacity(total);

    let anchor_start = pair.anchor();
    let mut price = anchor_start;
    let mut anchor = anchor_start;
    let mut regime = Regime::Ranging;
    let mut regime_left = rng.int(8, 30) as u32;
    let mut drift = 0.0;

    for i in 0..total {
        let t = Stamp::new(i as u32 / BARS_PER_DAY, (i as u32 % BARS_PER_DAY) * BAR_HOURS);

        // An event within this bar forces the event-driven regime for its duration.
        let event = calendar.iter().find(|e| {
            e.t.index() >= t.index()
                && e.t.index() < t.index() + BAR_HOURS
                && (e.currency == pair.base() || e.currency == pair.quote())
        });

        if event.is_some() {
            regime = Regime::EventDriven;
            regime_left = regime_left.max(2);
        } else if regime_left == 0 {
            // Ranging is the resting state; trends are shorter and rarer.
            let next = rng.weighted(&[0.62, 0.30, 0.08]);
            regime = Regime::ALL[next];
            regime_left = match regime {
                Regime::Ranging => rng.int(10, 34) as u32,
                Regime::Trending => rng.int(8, 22) as u32,
                Regime::EventDriven => rng.int(2, 5) as u32,
            };
            if regime == Regime::Trending {
                drift = rng.range(0.0006, 0.0018) * if rng.chance(0.5) { 1.0 } else { -1.0 };
            }
        }
        regime_left = regime_left.saturating_sub(1);

        let vol = pair.base_vol();
        let open = price;
        let shock = rng.normal();
        let step = match regime {
            // Pull back toward a slowly drifting anchor.
            Regime::Ranging => {
                let pull = (anchor - price) / price * 0.18;
                pull + shock * vol
            }
            Regime::Trending => drift + shock * vol * 1.15,
            Regime::EventDriven => {
                let jump = match event {
                    Some(e) => rng.normal() * vol * e.kind.impact(),
                    None => 0.0,
                };
                jump + shock * vol * 1.8
            }
        };
        price *= 1.0 + step;

        // The anchor tracks price slowly, so ranging does not mean flat forever.
        anchor += (price - anchor) * 0.03;

        // Intrabar extremes, always containing both open and close.
        let wick = vol * price * rng.range(0.4, 1.6);
        let high = open.max(price) + wick * rng.unit();
        let low = open.min(price) - wick * rng.unit();

        bars.push(Bar { t, open, high, low, close: price });
        truth.push(regime);
    }

    PairSeries { pair, bars, truth }
}

const NOISE: [&str; 10] = [
    "Analysts split on the medium-term outlook",
    "Bank revises quarterly forecast, cites little new",
    "Market commentary: positioning looks balanced",
    "Weekly flows survey shows nothing unusual",
    "Strategist repeats year-end target",
    "Liquidity thin ahead of the European open",
    "Options desk reports routine hedging",
    "Broker note: no change to recommendation",
    "Retail sentiment gauge little changed",
    "Cross-asset correlation within normal range",
];

fn generate_headlines(seed: u64, days: u32, calendar: &[CalendarEvent]) -> Vec<Headline> {
    let mut rng = Rng::stream(seed, "fx.headlines");
    let mut out = Vec::new();
    for day in 0..days {
        for _ in 0..rng.int(2, 6) {
            let hour = rng.int(6, 20) as u32;
            out.push(Headline {
                t: Stamp::new(day, hour),
                text: (*rng.pick(&NOISE)).to_owned(),
                informative: false,
            });
        }
        // A release gets one genuinely informative headline, on the day.
        for e in calendar.iter().filter(|e| e.t.day == day) {
            out.push(Headline {
                t: Stamp::new(day, e.t.hour.saturating_sub(1)),
                text: format!("{} due today at {:02}:00 ({})", e.kind.code(), e.t.hour, e.currency),
                informative: true,
            });
        }
    }
    out.sort_by_key(|h| h.t.index());
    out
}

/// A desk that runs mostly flat, carrying modest residual positions.
///
/// The scale matters: residuals of a few tens of units are ones a single
/// 100k-unit candidate can genuinely concentrate, which is what makes the
/// "this trade doubles an existing exposure" judgment a live condition rather
/// than an unreachable one.
fn starting_book(seed: u64) -> Vec<Exposure> {
    let mut rng = Rng::stream(seed, "fx.book");
    [Currency::Eur, Currency::Usd, Currency::Gbp, Currency::Jpy]
        .iter()
        .map(|c| Exposure { currency: *c, units: (rng.range(-150.0, 150.0) / 10.0).round() * 10.0 })
        .collect()
}
