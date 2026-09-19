//! Typed judgment primitives over TypeSafe's Jev model.
//!
//! The deterministic side of a trading system owns the numbers: prices, sizes,
//! stops, schedules, P&L. This crate is the layer above it, where the judgments
//! live — whether a signal is valid in this regime, whether a stop is sane,
//! whether today's conditions deserve the size the solver asked for.
//!
//! Six primitives cover that layer:
//!
//! | Primitive | Jev decides | The caller supplies |
//! |---|---|---|
//! | [`Gate`] | act / reduce / hold / escalate, and how much size | the actions' meanings, a size rubric |
//! | [`Classify`] | which label of a domain enum | the enum and its descriptions |
//! | [`Score`] | a position on a rubric, and which drivers hold | the rubric and the driver vocabulary |
//! | [`Check`] | each named plausibility claim | the claims |
//! | [`Rank`] | the ordering of solver-produced candidates | the candidates |
//! | [`Explain`] | framing, severity, which facts belong | the prose for each |
//!
//! Every one is a generic typed call — input struct in, fixed output struct out,
//! validated before it returns. A schema violation or an out-of-range value is a
//! hard error; nothing ever degrades to [`Action::Execute`].
//!
//! # Backends
//!
//! [`JevClient`] is the seam. [`LiveJev`] talks to the System One API through
//! the `typesafe-ai-sdk` crate; [`MockJev`] answers by rule, offline, reading
//! only the observable state. The primitives are identical either way.
//!
//! # Composition
//!
//! [`Pipeline`] threads each stage's typed output into the state of every later
//! stage, and [`pipeline::Batch`] fans independent stages into one call, so
//! `[Classify, Check, Score] -> Gate` is two calls with a real data dependency
//! between them rather than four unrelated ones.

#![warn(missing_docs)]

pub mod ask;
pub mod audit;
pub mod client;
pub mod error;
mod jev;
pub mod live;
pub mod mock;
pub mod pipeline;
pub mod policy;
pub mod primitives;
pub mod prompts;
pub mod schema;

pub use ask::{Ask, Asks, Usage, Verdict};
pub use audit::Audit;
pub use client::{CallRecord, JevCall, JevClient, JevReply, Primitive};
pub use error::{JevError, Result};
pub use jev::Jev;
pub use live::LiveJev;
pub use mock::MockJev;
pub use pipeline::{Batch, BatchOut, Pipeline, PriorEntry, Priors, Slot, Staged};
pub use policy::ReviewPolicy;
pub use primitives::{
    Action, Audience, Candidate, CandidateId, Check, CheckItem, CheckOut, CheckResult, CheckSource,
    CheckSpec, Classify, ClassifyOut, ClassifySpec, DriverSpec, Evidence, Explain, ExplainOut,
    ExplainSpec, FactSpec, Framing, Gate, GateOut, GateSpec, JevInput, Label, Rank, RankOut,
    RankSpec, Ranked, Score, ScoreOut, ScoreSpec, Weight,
};

use std::sync::Arc;

/// Build a [`Jev`] handle over the mock backend or the live one.
///
/// `--mock` is the only switch in the demo; this is where it lands.
pub fn connect(mock: bool, audit: Arc<Audit>) -> Result<Jev> {
    let client: Arc<dyn JevClient> =
        if mock { Arc::new(MockJev::new()) } else { Arc::new(LiveJev::from_env()?) };
    Ok(Jev::with_audit(client, audit))
}
