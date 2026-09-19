//! Running a desk, and the counterfactual replay that follows it.
//!
//! This is the CLI's `fx`, `battery` and `all` commands with the printing taken
//! out: the same sessions, the same reports, the same replays, kept as values
//! so a handler can project whatever slice of them a client asked for.

use crate::dto::{Domain, RunSpec};
use jev_core::{Audience, ExplainOut, Jev, Result};
use report::DomainReport;

/// Everything a finished run holds on to.
///
/// The worlds are kept alongside the sessions because the detail endpoints
/// recompute the features a decision was judged on, and features are a function
/// of the world the candidate came from.
#[derive(Debug)]
pub struct Outcome {
    /// The forex desk, for the `fx` and `all` domains.
    pub fx: Option<FxRun>,
    /// The battery desk, for the `battery` and `all` domains.
    pub battery: Option<BatteryRun>,
    /// One Explain call over both desks, for the `all` domain.
    pub compliance: Option<ExplainOut>,
}

/// One forex run.
#[derive(Debug)]
pub struct FxRun {
    /// The world it ran on.
    pub world: synth::FxWorld,
    /// The strategy parameters the solver used.
    pub params: fx::StrategyParams,
    /// Every candidate, its judgment and both books' outcomes.
    pub session: fx::FxSession,
    /// The Markdown and terminal report.
    pub report: DomainReport,
    /// The CPI day judged twice.
    pub replay: Option<FxReplay>,
}

/// One CPI day judged with the release and without it.
#[derive(Debug)]
pub struct FxReplay {
    /// Which decision was replayed, by index into the session.
    pub index: usize,
    /// The next release as the features rendered it, with the event present.
    pub with_event_label: String,
    /// The judgment with the event present.
    pub with_event: fx::FxJudgment,
    /// The same, with every event on that day removed.
    pub without_event_label: String,
    /// The judgment without it.
    pub without_event: fx::FxJudgment,
}

/// One battery run.
#[derive(Debug)]
pub struct BatteryRun {
    /// The world it ran on.
    pub world: synth::BatteryWorld,
    /// Every day, in order.
    pub session: battery::BatterySession,
    /// The Markdown and terminal report.
    pub report: DomainReport,
    /// The quiet day judged twice.
    pub replay: Option<BatteryReplay>,
}

/// One day judged quiet and then with a grid notice over its discharge block.
#[derive(Debug)]
pub struct BatteryReplay {
    /// Which day.
    pub day: u32,
    /// First hour the notice covers.
    pub from_hour: u32,
    /// Last hour it covers.
    pub to_hour: u32,
    /// The notice, as the judgment layer read it.
    pub notice: String,
    /// The day with every grid note removed.
    pub quiet: battery::DayRecord,
    /// The same day with the notice added.
    pub noticed: battery::DayRecord,
}

/// Run what `spec` asks for, judging every call through `jev`.
pub async fn execute(spec: RunSpec, jev: &Jev) -> Result<Outcome> {
    let fx_run = match spec.domain {
        Domain::Fx | Domain::All => Some(run_fx(&spec, jev).await?),
        Domain::Battery => None,
    };
    let battery_run = match spec.domain {
        Domain::Battery | Domain::All => Some(run_battery(&spec, jev).await?),
        Domain::Fx => None,
    };
    let compliance = match (&fx_run, &battery_run) {
        (Some(f), Some(b)) if spec.domain == Domain::All => {
            Some(compliance_summary(jev, &f.session, &b.session).await?)
        }
        _ => None,
    };
    Ok(Outcome { fx: fx_run, battery: battery_run, compliance })
}

async fn run_fx(spec: &RunSpec, jev: &Jev) -> Result<FxRun> {
    let world = synth::FxWorld::generate(spec.seed, spec.days);
    let params = fx::StrategyParams::default();
    let session = fx::run_session(jev, &world, &params, spec.limit).await?;
    let report = report::fx_report::render(jev, &world, &session, &params).await?;
    let replay = fx_replay(jev, &world, &session, &params).await?;
    Ok(FxRun { world, params, session, report, replay })
}

/// Judge one CPI candidate twice: with the release on the calendar, and with
/// every event on that day removed.
///
/// The bars are untouched between the two runs, so the solver's proposal is
/// identical and whatever moves is the judgment layer reacting to the calendar.
async fn fx_replay(
    jev: &Jev,
    world: &synth::FxWorld,
    session: &fx::FxSession,
    params: &fx::StrategyParams,
) -> Result<Option<FxReplay>> {
    let Some(record) = fx::replay_decision(world, &session.decisions, params) else {
        return Ok(None);
    };
    let index = session
        .decisions
        .iter()
        .position(|d| std::ptr::eq(d, record))
        .expect("the record came from this session");
    let candidate = record.candidate;

    let without_world = fx::without_events_on(world, candidate.t.day);
    let (with_event_label, with_event) = judge_once(jev, world, &candidate, params).await?;
    let (without_event_label, without_event) =
        judge_once(jev, &without_world, &candidate, params).await?;

    Ok(Some(FxReplay { index, with_event_label, with_event, without_event_label, without_event }))
}

/// Judge one candidate in one world, returning how the release read and the
/// four stages it produced.
async fn judge_once(
    jev: &Jev,
    world: &synth::FxWorld,
    candidate: &fx::TradeCandidate,
    params: &fx::StrategyParams,
) -> Result<(String, fx::FxJudgment)> {
    let bars = &world.series_for(candidate.pair).bars;
    let input = fx::build_input(world, bars, candidate, params, &world.book);
    let label = fx::features::event_label(&input.features);
    let mut pipeline = jev_core::Pipeline::new(jev, "fx-replay");
    let judgment = fx::judge(&mut pipeline, &input, candidate).await?;
    Ok((label, judgment))
}

async fn run_battery(spec: &RunSpec, jev: &Jev) -> Result<BatteryRun> {
    let world = synth::BatteryWorld::generate(spec.seed, spec.days);
    let session = battery::run_session(jev, &world, spec.limit).await?;
    let report = report::battery_report::render(jev, &world, &session).await?;
    let replay = battery_replay(jev, &world, spec.limit).await?;
    Ok(BatteryRun { world, session, report, replay })
}

/// Judge one quiet day twice: once as it stands, once with a grid notice laid
/// over the hours the chosen schedule discharges in.
async fn battery_replay(
    jev: &Jev,
    world: &synth::BatteryWorld,
    limit: Option<u32>,
) -> Result<Option<BatteryReplay>> {
    let horizon = limit.unwrap_or(world.days).min(world.days);
    let Some(day) = battery::first_quiet_day(world, horizon) else {
        return Ok(None);
    };

    let quiet_world = battery::without_grid_notices(world, day);
    let quiet = battery::judge_day(jev, &quiet_world, day).await?;
    let Some((from_hour, to_hour)) = quiet.chosen().discharge_block() else {
        return Ok(None);
    };

    let noticed_world = battery::with_grid_notice(world, day, from_hour, to_hour);
    let notice =
        noticed_world.grid_notes_on(day).first().map(|n| n.text.clone()).unwrap_or_default();
    let noticed = battery::judge_day(jev, &noticed_world, day).await?;

    Ok(Some(BatteryReplay { day, from_hour, to_hour, notice, quiet, noticed }))
}

/// One Explain call over both desks, for Compliance. The `all` domain's close.
async fn compliance_summary(
    jev: &Jev,
    fx_session: &fx::FxSession,
    battery_session: &battery::BatterySession,
) -> Result<ExplainOut> {
    let counts = report::explain::Counts {
        domain: "trading desk",
        decisions: fx_session.decisions.len() + battery_session.days.len(),
        interventions: fx_session.interventions() + battery_session.interventions(),
        escalations: fx_session.escalations() + battery_session.escalations(),
        jev_calls: fx_session.decisions.len() * 4 + battery_session.days.len() * 5,
        failed_checks: fx_session
            .decisions
            .iter()
            .map(|d| d.judgment.checks.failed().len())
            .sum::<usize>()
            + battery_session.days.iter().map(|d| d.judgment.checks.failed().len()).sum::<usize>(),
        headlines: vec![
            format!(
                "forex gating took the book from {} to {}; the battery desk missed the \
                 reserve {} times against {}",
                report::fmt::signed(fx_session.ungated.pnl),
                report::fmt::signed(fx_session.gated.pnl),
                battery_session.gated.reserve_breaches,
                battery_session.solver_only.reserve_breaches,
            ),
            format!(
                "{} decisions across both desks were held or sized down",
                fx_session.interventions() + battery_session.interventions()
            ),
        ],
    };
    report::explain::for_audience(
        jev,
        &counts,
        Audience::Compliance,
        "the trading day across both desks",
    )
    .await
}
