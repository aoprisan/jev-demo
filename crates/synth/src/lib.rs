//! Seeded, deterministic synthetic markets.
//!
//! Everything here is a pure function of a `u64` seed: the same seed gives
//! byte-identical worlds, on any machine, forever. That is what makes a
//! gated-versus-ungated P&L comparison mean anything.
//!
//! Ground truth — the forex generator's regime — is deliberately kept in a
//! parallel array rather than on the bar, so a strategy or a judgment call
//! cannot read it by accident. It exists for evaluation only.

#![warn(missing_docs)]

pub mod battery;
pub mod calendar;
pub mod fx;
pub mod rng;

pub use battery::{
    AfrrWindow, BatterySpec, BatteryWorld, GridNote, GridNoteKind, PowerHeadline, TICKS_PER_HOUR,
};
pub use calendar::{Date, Stamp};
pub use fx::{
    Bar, CalendarEvent, Currency, EventKind, Exposure, FxWorld, Headline, Pair, PairSeries, Regime,
    BARS_PER_DAY, BAR_HOURS,
};
pub use rng::Rng;

/// The seed the demo uses unless told otherwise.
pub const DEFAULT_SEED: u64 = 20_250_106;
