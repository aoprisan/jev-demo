//! The counterfactual replays.
//!
//! Each domain's demo ends by running one day twice, changing exactly one thing
//! and printing the two judgment records side by side. The prices are identical
//! across both runs, so whatever moves is the judgment layer reacting to the
//! one thing that changed — and nothing else.

use crate::Cli;
use anyhow::{Context, Result};
use battery::ScheduleKind;
use jev_core::{GateOut, Jev};
use report::fmt::{bold, dim, heading, table};
use synth::{BatteryWorld, EventKind, FxWorld, Pair};

/// Column width for the side-by-side panes.
const PANE: usize = 44;

/// Replay one CPI day with the release present and with it removed.
pub async fn fx_event_day(
    jev: &Jev,
    world: &FxWorld,
    session: &fx::FxSession,
    params: &fx::StrategyParams,
) -> Result<()> {
    // Pick the candidate that best exercises the rule under test: a release
    // within three hours, and a stop tighter than the window's own movement.
    // Falling back to whatever sits nearest a CPI would show a pair of records
    // that differ in no way a reader could act on.
    let cpi_days: Vec<u32> =
        world.calendar.iter().filter(|e| e.kind == EventKind::Cpi).map(|e| e.t.day).collect();
    if cpi_days.is_empty() {
        println!("{}", dim("no CPI day in this world; skipping the replay"));
        return Ok(());
    }

    let scored = |d: &&fx::DecisionRecord| -> Option<(bool, f64)> {
        let bars = &world.series_for(d.candidate.pair).bars;
        let input = fx::build_input(world, bars, &d.candidate, params, &world.book);
        let f = &input.features;
        let exercises = f.hours_to_event <= 3.0 && f.stop_atr_multiple < 1.2;
        Some((exercises, f.hours_to_event))
    };
    let Some(record) = session
        .decisions
        .iter()
        .filter(|d| cpi_days.contains(&d.candidate.t.day))
        .filter(|d| scored(d).is_some_and(|(exercises, _)| exercises))
        .min_by(|a, b| scored(a).unwrap().1.total_cmp(&scored(b).unwrap().1))
        .or_else(|| {
            // Nothing meets both halves; show the candidate closest to a release.
            session
                .decisions
                .iter()
                .filter(|d| cpi_days.contains(&d.candidate.t.day))
                .min_by(|a, b| scored(a).unwrap().1.total_cmp(&scored(b).unwrap().1))
        })
    else {
        println!("{}", dim("no candidate on a CPI day; skipping the replay"));
        return Ok(());
    };
    let candidate = record.candidate;
    let pair = candidate.pair;

    print!(
        "{}",
        heading(&format!(
            "Replay — {} on {}, with and without the CPI release",
            pair.code(),
            world.start.plus_days(candidate.t.day as i64)
        ))
    );

    let with_event = judge_fx(jev, world, &candidate, params).await?;
    let without = fx::without_events_on(world, candidate.t.day);
    let without_event = judge_fx(jev, &without, &candidate, params).await?;

    // The prices are untouched, so the solver's proposal is identical.
    println!(
        "{}",
        dim(&format!(
            "  same bars, same candidate: {} {} at {:.5}, stop {:.5}, target {:.5}",
            pair.code(),
            candidate.side.as_str(),
            candidate.price,
            candidate.stop,
            candidate.target,
        ))
    );
    println!();
    print_side_by_side(
        "CPI scheduled",
        &with_event.0,
        "release removed",
        &without_event.0,
        &[
            ("hours to event", with_event.1.clone(), without_event.1.clone()),
            ("event risk", format!("{}/100", with_event.2), format!("{}/100", without_event.2)),
        ],
    );
    Ok(())
}

/// Judge one fx candidate in a given world; returns the gate, the event
/// distance as rendered, and the risk score.
async fn judge_fx(
    jev: &Jev,
    world: &FxWorld,
    candidate: &fx::TradeCandidate,
    params: &fx::StrategyParams,
) -> Result<(GateOut, String, u8)> {
    let bars = &world.series_for(candidate.pair).bars;
    let input = fx::build_input(world, bars, candidate, params, &world.book);
    let hours = match input.features.next_event_kind.as_deref() {
        Some(kind) if input.features.hours_to_event < 72.0 => {
            format!("{kind} in {:.1}h", input.features.hours_to_event)
        }
        _ => "none scheduled".to_owned(),
    };
    let mut pipeline = jev_core::Pipeline::new(jev, "fx-replay");
    let judgment = fx::judge(&mut pipeline, &input, candidate)
        .await
        .context("judging the replayed fx candidate")?;
    Ok((judgment.gate, hours, judgment.risk.score))
}

/// Replay one quiet day with a grid notice added over the discharge block.
pub async fn battery_notice_day(jev: &Jev, world: &BatteryWorld, cli: &Cli) -> Result<()> {
    let horizon = cli.limit.unwrap_or(world.days).min(world.days);
    let Some(day) = (0..horizon).find(|d| world.grid_notes_on(*d).is_empty()) else {
        println!("{}", dim("every day already carries a grid note; skipping the replay"));
        return Ok(());
    };

    let quiet_world = battery::without_grid_notices(world, day);
    let quiet = battery::judge_day(jev, &quiet_world, day)
        .await
        .context("judging the quiet replay day")?;

    let Some((from, to)) = quiet.chosen().discharge_block() else {
        println!("{}", dim("the chosen schedule does not discharge; skipping the replay"));
        return Ok(());
    };
    let noticed_world = battery::with_grid_notice(world, day, from, to);
    let noticed = battery::judge_day(jev, &noticed_world, day)
        .await
        .context("judging the noticed replay day")?;

    print!(
        "{}",
        heading(&format!(
            "Replay — day {day} ({}), with and without a grid notice over {from:02}:00-{to:02}:00",
            quiet.date
        ))
    );
    println!(
        "{}",
        dim("  same prices, same three schedules; only the grid operator's note differs")
    );
    println!();

    // The ranking, side by side.
    let order = |r: &battery::DayRecord| -> Vec<String> {
        r.judgment
            .ranking
            .ordered
            .iter()
            .enumerate()
            .map(|(i, x)| format!("{}. {} ({:.2})", i + 1, x.id.as_str(), x.p))
            .collect()
    };
    let quiet_order = order(&quiet);
    let noticed_order = order(&noticed);
    let mut rows = vec![vec!["rank".into(), "no notice".into(), "notice added".into()]];
    for i in 0..quiet_order.len().max(noticed_order.len()) {
        rows.push(vec![
            format!("{}", i + 1),
            quiet_order.get(i).cloned().unwrap_or_default(),
            noticed_order.get(i).cloned().unwrap_or_default(),
        ]);
    }
    println!("{}", table(&rows));

    if quiet.judgment.chosen != noticed.judgment.chosen {
        println!(
            "  {}\n",
            bold(&format!(
                "the ranking flipped: {} -> {}",
                quiet.judgment.chosen.as_str(),
                noticed.judgment.chosen.as_str()
            ))
        );
    } else {
        println!("  {}\n", dim("the ranking held"));
    }

    let checks = |r: &battery::DayRecord| -> String {
        r.judgment
            .checks
            .checks
            .iter()
            .map(|c| format!("{} {}", if c.ok { "ok  " } else { "FAIL" }, c.name))
            .collect::<Vec<_>>()
            .join("\n")
    };
    print_side_by_side(
        "no notice",
        &quiet.judgment.gate,
        "notice added",
        &noticed.judgment.gate,
        &[
            (
                "chose",
                quiet.judgment.chosen.as_str().to_owned(),
                noticed.judgment.chosen.as_str().to_owned(),
            ),
            (
                "reserve held",
                yes_no(quiet.gated.reserve_held()),
                yes_no(noticed.gated.reserve_held()),
            ),
            ("risk", format!("{}/100", quiet.judgment.risk.score), format!("{}/100", noticed.judgment.risk.score)),
        ],
    );

    println!("  {}", bold("checks"));
    print_columns(&checks(&quiet), &checks(&noticed));
    println!();
    let _ = ScheduleKind::ALL;
    Ok(())
}

fn yes_no(value: bool) -> String {
    if value { "yes".into() } else { "no".into() }
}

/// Print two gate records beside each other, with extra labelled rows above.
fn print_side_by_side(
    left_title: &str,
    left: &GateOut,
    right_title: &str,
    right: &GateOut,
    extra: &[(&str, String, String)],
) {
    let mut rows = vec![vec!["".into(), left_title.to_owned(), right_title.to_owned()]];
    for (label, l, r) in extra {
        rows.push(vec![(*label).to_owned(), l.clone(), r.clone()]);
    }
    rows.push(vec![
        "action".into(),
        left.action.as_str().to_owned(),
        right.action.as_str().to_owned(),
    ]);
    rows.push(vec![
        "size factor".into(),
        format!("{:.0}%", left.size_factor * 100.0),
        format!("{:.0}%", right.size_factor * 100.0),
    ]);
    rows.push(vec![
        "confidence".into(),
        format!("{:.2}", left.evidence.confidence),
        format!("{:.2}", right.evidence.confidence),
    ]);
    println!("{}", table(&rows));

    println!("  {}", bold("reason"));
    print_columns(&left.reason, &right.reason);
    println!();
}

/// Print two blocks of text in two columns.
fn print_columns(left: &str, right: &str) {
    let left_lines = wrap_lines(left, PANE);
    let right_lines = wrap_lines(right, PANE);
    for i in 0..left_lines.len().max(right_lines.len()) {
        let l = left_lines.get(i).cloned().unwrap_or_default();
        let r = right_lines.get(i).cloned().unwrap_or_default();
        println!("  {:<width$}  {}", l, r, width = PANE);
    }
}

fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    text.lines()
        .flat_map(|line| {
            report::fmt::wrap(line, width, "")
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Kept so `Pair` stays referenced if the fx replay is trimmed.
#[allow(dead_code)]
fn pairs() -> [Pair; 3] {
    Pair::ALL
}
