//! Projecting a finished run onto the wire types.
//!
//! Nothing here judges anything or recomputes a number the solver owns; it
//! reshapes what the run already produced into the structs `dto` declares, so
//! the browser reads the same verdicts the terminal report does.

use crate::dto::*;
use crate::run::{BatteryRun, FxRun, Outcome};
use jev_core::{
    CallRecord, CheckOut, ClassifyOut, Evidence, ExplainOut, GateOut, RankOut, ScoreOut,
};
use serde::Serialize;

/// An enum's wire label, taken from its own serde representation so the API
/// and the audit log never disagree about what a label is called.
fn label_of<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

fn weights(evidence: &Evidence) -> Vec<WeightView> {
    evidence.distribution.iter().map(|w| WeightView { label: w.label.clone(), p: w.p }).collect()
}

fn classify_view<E: Serialize>(out: &ClassifyOut<E>) -> ClassifyView {
    ClassifyView {
        label: label_of(&out.label),
        confidence: out.confidence,
        reason: out.reason.clone(),
        distribution: weights(&out.evidence),
    }
}

fn check_views(out: &CheckOut) -> Vec<CheckView> {
    out.checks
        .iter()
        .map(|c| CheckView {
            name: c.name.clone(),
            ok: c.ok,
            note: c.note.clone(),
            p: c.p,
            source: match c.source {
                jev_core::CheckSource::Jev => "jev".to_owned(),
                jev_core::CheckSource::Rule => "rule".to_owned(),
            },
        })
        .collect()
}

fn score_view(out: &ScoreOut) -> ScoreView {
    ScoreView {
        score: out.score,
        drivers: out.drivers.clone(),
        reason: out.reason.clone(),
        confidence: out.evidence.confidence,
        distribution: weights(&out.evidence),
    }
}

fn gate_view(out: &GateOut) -> GateView {
    GateView {
        action: out.action.as_str().to_owned(),
        size_factor: out.size_factor,
        reason: out.reason.clone(),
        confidence: out.evidence.confidence,
        distribution: weights(&out.evidence),
    }
}

fn ranked_views<T: Serialize>(out: &RankOut<T>) -> Vec<RankedView> {
    out.ordered
        .iter()
        .map(|r| RankedView { id: label_of(&r.id), rationale: r.rationale.clone(), fit: r.fit })
        .collect()
}

/// An Explain output as the UI shows it.
pub fn explain_view(out: &ExplainOut) -> ExplainView {
    ExplainView {
        summary: out.summary.clone(),
        for_audience: label_of(&out.for_audience),
        confidence: out.evidence.confidence,
    }
}

/// The whole run, as a client reads it once it is done.
pub fn run_result(outcome: &Outcome, cost: Cost) -> RunResult {
    RunResult {
        fx: outcome.fx.as_ref().map(fx_result),
        battery: outcome.battery.as_ref().map(battery_result),
        compliance: outcome.compliance.as_ref().map(explain_view),
        cost,
    }
}

// ---------------------------------------------------------------------------
// Forex
// ---------------------------------------------------------------------------

fn fx_book(book: &fx::BookResult) -> FxBook {
    FxBook {
        trades: book.trades,
        pnl: book.pnl,
        wins: book.wins,
        losses: book.losses,
        hit_rate: book.hit_rate(),
        max_drawdown: book.max_drawdown,
        notional: book.notional,
        return_bps: book.return_bps(),
    }
}

fn fill_view(fill: &fx::Fill) -> FillView {
    FillView {
        entered_day: fill.entered.day,
        entered_hour: fill.entered.hour,
        exited_day: fill.exited.day,
        exited_hour: fill.exited.hour,
        exit: label_of(&fill.exit),
        entry_price: fill.entry_price,
        exit_price: fill.exit_price,
        size_factor: fill.size_factor,
        notional: fill.notional,
        pnl: fill.pnl,
        pnl_bps: fill.pnl_bps,
    }
}

/// One decision's row. `index` is what the detail endpoint takes.
fn fx_row(run: &FxRun, index: usize, record: &fx::DecisionRecord) -> FxDecisionRow {
    let c = &record.candidate;
    let j = &record.judgment;
    FxDecisionRow {
        index,
        day: c.t.day,
        hour: c.t.hour,
        date: run.world.start.plus_days(c.t.day as i64).to_string(),
        pair: c.pair.code().to_owned(),
        side: c.side.as_str().to_owned(),
        price: c.price,
        stop: c.stop,
        target: c.target,
        size_units: c.size_units,
        regime: label_of(&j.regime.label),
        regime_confidence: j.regime.confidence,
        true_regime: record.true_regime.as_str().to_owned(),
        regime_correct: record.regime_correct(),
        risk_score: j.risk.score,
        checks_failed: j.checks.failed().iter().map(|s| (*s).to_owned()).collect(),
        action: j.gate.action.as_str().to_owned(),
        size_factor: j.gate.size_factor,
        reason: j.gate.reason.clone(),
        ungated_pnl: record.ungated.map(|f| f.pnl),
        gated_pnl: record.gated.map(|f| f.pnl),
        pnl_delta: record.pnl_delta(),
        intervened: record.intervened(),
    }
}

/// The names of the checks this run ran, in the order the spec offers them.
fn check_names(first: Option<&CheckOut>) -> Vec<String> {
    first.map(|c| c.checks.iter().map(|x| x.name.clone()).collect()).unwrap_or_default()
}

/// Everything one forex run produced.
pub fn fx_result(run: &FxRun) -> FxResult {
    let session = &run.session;
    let decisions: Vec<FxDecisionRow> =
        session.decisions.iter().enumerate().map(|(i, d)| fx_row(run, i, d)).collect();

    let mut ungated_total = 0.0;
    let mut gated_total = 0.0;
    let equity: Vec<EquityPoint> = session
        .decisions
        .iter()
        .enumerate()
        .map(|(index, d)| {
            ungated_total += d.ungated.map(|f| f.pnl).unwrap_or(0.0);
            gated_total += d.gated.map(|f| f.pnl).unwrap_or(0.0);
            EquityPoint {
                index,
                day: d.candidate.t.day,
                ungated: ungated_total,
                gated: gated_total,
            }
        })
        .collect();

    let scorecard = check_names(session.decisions.first().map(|d| &d.judgment.checks))
        .iter()
        .map(|name| {
            let card = session.score_check(name);
            CheckScorecardView {
                name: card.name.clone(),
                failed_n: card.failed_n,
                failed_bps: card.failed_bps,
                failed_hit: card.failed_hit,
                passed_n: card.passed_n,
                passed_bps: card.passed_bps,
                passed_hit: card.passed_hit,
                edge_bps: card.edge_bps(),
                inverted: card.inverted(),
            }
        })
        .collect();

    FxResult {
        decisions_judged: session.decisions.len(),
        ungated: fx_book(&session.ungated),
        gated: fx_book(&session.gated),
        gate_distribution: session
            .gate_distribution()
            .into_iter()
            .map(|(action, count)| ActionCount { action: action.as_str().to_owned(), count })
            .collect(),
        regime_accuracy: session.regime_accuracy(),
        interventions: session.interventions(),
        escalations: session.escalations(),
        reviews: session.reviews(),
        failed_checks: session.decisions.iter().map(|d| d.judgment.checks.failed().len()).sum(),
        scorecard,
        equity,
        decisions,
        replay: fx_replay(run),
    }
}

/// One decision in full, including the features the judgment layer read.
pub fn fx_detail(run: &FxRun, index: usize) -> Option<FxDecisionDetail> {
    let record = run.session.decisions.get(index)?;
    let bars = &run.world.series_for(record.candidate.pair).bars;
    // The book the candidate was judged against is the running book, which is
    // not kept per decision; the starting book is what a reader can reproduce,
    // and is what the replay uses too.
    let input = fx::build_input(&run.world, bars, &record.candidate, &run.params, &run.world.book);
    let j = &record.judgment;
    Some(FxDecisionDetail {
        row: fx_row(run, index, record),
        features: input.features,
        headlines: input.headlines,
        regime: classify_view(&j.regime),
        checks: check_views(&j.checks),
        risk: score_view(&j.risk),
        gate: gate_view(&j.gate),
        review: j.review.clone(),
        ungated: record.ungated.as_ref().map(fill_view),
        gated: record.gated.as_ref().map(fill_view),
    })
}

fn fx_replay(run: &FxRun) -> Option<FxReplay> {
    let replay = run.replay.as_ref()?;
    let record = run.session.decisions.get(replay.index)?;
    let c = &record.candidate;
    let pane = |label: &str, event: &str, judgment: &fx::FxJudgment| FxReplayPane {
        label: label.to_owned(),
        event: event.to_owned(),
        risk_score: judgment.risk.score,
        gate: gate_view(&judgment.gate),
    };
    Some(FxReplay {
        pair: c.pair.code().to_owned(),
        date: run.world.start.plus_days(c.t.day as i64).to_string(),
        price: c.price,
        stop: c.stop,
        target: c.target,
        side: c.side.as_str().to_owned(),
        with_event: pane("CPI scheduled", &replay.with_event_label, &replay.with_event),
        without_event: pane("release removed", &replay.without_event_label, &replay.without_event),
    })
}

// ---------------------------------------------------------------------------
// Battery
// ---------------------------------------------------------------------------

fn battery_book(book: &battery::BookResult) -> BatteryBook {
    BatteryBook {
        days_run: book.days_run,
        days_stood_down: book.days_stood_down,
        margin: book.margin,
        reserve_payment: book.reserve_payment,
        degradation_cost: book.degradation_cost,
        reserve_breaches: book.reserve_breaches,
        days_with_breach: book.days_with_breach,
        cycles: book.cycles,
        margin_per_cycle: book.margin_per_cycle(),
        max_drawdown: book.max_drawdown,
    }
}

fn schedule_view(schedule: &battery::Schedule) -> ScheduleView {
    ScheduleView {
        kind: schedule.kind.as_str().to_owned(),
        power_mw: schedule.power_mw.clone(),
        soc_mwh: schedule.soc_mwh.clone(),
        expected_margin: schedule.expected_margin,
        cycles: schedule.cycles,
        degradation_cost: schedule.degradation_cost,
        min_soc_mwh: schedule.min_soc(),
    }
}

fn execution_view(execution: &battery::Execution) -> ExecutionView {
    ExecutionView {
        schedule: execution.schedule.clone(),
        size_factor: execution.size_factor,
        energy_margin: execution.energy_margin,
        reserve_payment: execution.reserve_payment,
        degradation_cost: execution.degradation_cost,
        realised_margin: execution.realised_margin,
        expected_margin: execution.expected_margin,
        soc_mwh: execution.soc_mwh.clone(),
        reserve_breaches: execution.reserve_breaches,
        cycles: execution.cycles,
    }
}

fn battery_row(record: &battery::DayRecord) -> BatteryDayRow {
    let j = &record.judgment;
    BatteryDayRow {
        day: record.day,
        date: record.date.clone(),
        regime: label_of(&j.regime.label),
        regime_confidence: j.regime.confidence,
        chosen: j.chosen.as_str().to_owned(),
        rank_margin: j.ranking.margin(),
        risk_score: j.risk.score,
        checks_failed: j.checks.failed().iter().map(|s| (*s).to_owned()).collect(),
        action: j.gate.action.as_str().to_owned(),
        size_factor: j.gate.size_factor,
        reason: j.gate.reason.clone(),
        solver_margin: record.solver_only.realised_margin,
        gated_margin: record.gated.realised_margin,
        margin_delta: record.margin_delta(),
        solver_breaches: record.solver_only.reserve_breaches,
        gated_breaches: record.gated.reserve_breaches,
        gated_cycles: record.gated.cycles,
        intervened: record.intervened(),
    }
}

/// Everything one battery run produced.
pub fn battery_result(run: &BatteryRun) -> BatteryResult {
    let session = &run.session;
    let mut solver_total = 0.0;
    let mut gated_total = 0.0;
    let margin_curve: Vec<MarginPoint> = session
        .days
        .iter()
        .map(|d| {
            solver_total += d.solver_only.realised_margin;
            gated_total += d.gated.realised_margin;
            MarginPoint { day: d.day, solver_only: solver_total, gated: gated_total }
        })
        .collect();

    BatteryResult {
        days_judged: session.days.len(),
        solver_only: battery_book(&session.solver_only),
        gated: battery_book(&session.gated),
        gate_distribution: session
            .gate_distribution()
            .into_iter()
            .map(|(action, count)| ActionCount { action: action.as_str().to_owned(), count })
            .collect(),
        rank_distribution: session
            .rank_distribution()
            .into_iter()
            .map(|(kind, count)| ScheduleCount { schedule: kind.as_str().to_owned(), count })
            .collect(),
        interventions: session.interventions(),
        escalations: session.escalations(),
        reviews: session.reviews(),
        failed_checks: session.days.iter().map(|d| d.judgment.checks.failed().len()).sum(),
        margin_curve,
        days: session.days.iter().map(battery_row).collect(),
        replay: battery_replay(run),
    }
}

/// One day in full: all three schedules, every stage, both executions.
pub fn battery_detail(run: &BatteryRun, day: u32) -> Option<BatteryDayDetail> {
    let record = run.session.days.iter().find(|d| d.day == day)?;
    Some(day_detail(record))
}

fn day_detail(record: &battery::DayRecord) -> BatteryDayDetail {
    let j = &record.judgment;
    BatteryDayDetail {
        row: battery_row(record),
        schedules: record.schedules.iter().map(schedule_view).collect(),
        regime: classify_view(&j.regime),
        ranking: ranked_views(&j.ranking),
        checks: check_views(&j.checks),
        risk: score_view(&j.risk),
        gate: gate_view(&j.gate),
        review: j.review.clone(),
        solver_only: execution_view(&record.solver_only),
        gated: execution_view(&record.gated),
    }
}

fn battery_replay(run: &BatteryRun) -> Option<BatteryReplay> {
    let replay = run.replay.as_ref()?;
    let pane = |label: &str, record: &battery::DayRecord| BatteryReplayPane {
        label: label.to_owned(),
        chosen: record.judgment.chosen.as_str().to_owned(),
        ranking: ranked_views(&record.judgment.ranking),
        risk_score: record.judgment.risk.score,
        checks: check_views(&record.judgment.checks),
        reserve_held: record.gated.reserve_held(),
        gate: gate_view(&record.judgment.gate),
    };
    Some(BatteryReplay {
        day: replay.day,
        date: replay.quiet.date.clone(),
        from_hour: replay.from_hour,
        to_hour: replay.to_hour,
        notice: replay.notice.clone(),
        quiet: pane("no notice", &replay.quiet),
        noticed: pane("notice added", &replay.noticed),
        flipped: replay.quiet.judgment.chosen != replay.noticed.judgment.chosen,
    })
}

// ---------------------------------------------------------------------------
// The audit log
// ---------------------------------------------------------------------------

/// One page of the audit log, without the payloads.
pub fn call_page(records: &[CallRecord], offset: usize, limit: usize) -> CallPage {
    let calls = records
        .iter()
        .enumerate()
        .skip(offset)
        .take(limit)
        .map(|(index, r)| CallRow {
            index,
            at_ms: r.at_ms,
            backend: r.backend.clone(),
            primitive: r.primitive.as_str().to_owned(),
            stage: r.stage.clone(),
            model: r.model.clone(),
            asks: r.asks.len(),
            input_tokens: r.input_tokens,
            output_tokens: r.output_tokens,
            latency_ms: r.latency_ms,
        })
        .collect();
    CallPage { total: records.len(), offset, calls }
}
