//! The battery report: terminal and Markdown.

use crate::explain::{self, Counts};
use crate::fmt::{bar, bold, dim, heading, markdown_table, signed, table, thousands, wrap};
use crate::DomainReport;
use battery::{BatterySession, BookResult, ScheduleKind};
use jev_core::{Audience, Jev, Result};
use synth::BatteryWorld;

/// Build the battery report, running the Explain stage for each audience.
pub async fn render(
    jev: &Jev,
    world: &BatteryWorld,
    session: &BatterySession,
) -> Result<DomainReport> {
    let failed_checks: usize = session.days.iter().map(|d| d.judgment.checks.failed().len()).sum();
    let counts = Counts {
        domain: "battery",
        decisions: session.days.len(),
        interventions: session.interventions(),
        escalations: session.escalations(),
        jev_calls: jev.audit().len(),
        failed_checks,
        headlines: vec![
            format!(
                "the reserve was missed {} times against {} without the judgment layer",
                session.gated.reserve_breaches, session.solver_only.reserve_breaches,
            ),
            format!(
                "the ranking moved {} of {} days off the default schedule",
                session.days.iter().filter(|d| d.judgment.chosen != ScheduleKind::Balanced).count(),
                session.days.len()
            ),
        ],
    };
    let summaries = explain::for_audiences(
        jev,
        &counts,
        &[Audience::Trader, Audience::Compliance],
        "the battery session",
    )
    .await?;

    let terminal = terminal(world, session, jev, &summaries);
    let markdown = markdown(world, session, jev, &summaries);
    Ok(DomainReport { name: "battery".into(), terminal, markdown })
}

fn book_rows(solver_only: &BookResult, gated: &BookResult) -> Vec<Vec<String>> {
    vec![
        vec!["".into(), "solver-only".into(), "gated".into()],
        vec!["days run".into(), solver_only.days_run.to_string(), gated.days_run.to_string()],
        vec![
            "days stood down".into(),
            solver_only.days_stood_down.to_string(),
            gated.days_stood_down.to_string(),
        ],
        vec!["realised margin".into(), signed(solver_only.margin), signed(gated.margin)],
        vec![
            "reserve payments".into(),
            thousands(solver_only.reserve_payment),
            thousands(gated.reserve_payment),
        ],
        vec![
            "reserve breaches".into(),
            format!("{} h on {} days", solver_only.reserve_breaches, solver_only.days_with_breach),
            format!("{} h on {} days", gated.reserve_breaches, gated.days_with_breach),
        ],
        vec![
            "equivalent cycles".into(),
            format!("{:.1}", solver_only.cycles),
            format!("{:.1}", gated.cycles),
        ],
        vec![
            "margin per cycle".into(),
            format!("{:.1}", solver_only.margin_per_cycle()),
            format!("{:.1}", gated.margin_per_cycle()),
        ],
        vec![
            "max drawdown".into(),
            thousands(solver_only.max_drawdown),
            thousands(gated.max_drawdown),
        ],
    ]
}

fn rank_rows(session: &BatterySession) -> Vec<Vec<String>> {
    let max = session.rank_distribution().iter().map(|(_, n)| *n).max().unwrap_or(0);
    let mut rows = vec![vec!["schedule".into(), "ranked first".into(), "".into()]];
    for (kind, count) in session.rank_distribution() {
        rows.push(vec![kind.as_str().to_owned(), count.to_string(), bar(count, max, 24)]);
    }
    rows
}

fn gate_rows(session: &BatterySession) -> Vec<Vec<String>> {
    let max = session.gate_distribution().iter().map(|(_, n)| *n).max().unwrap_or(0);
    let mut rows = vec![vec!["action".into(), "days".into(), "".into()]];
    for (action, count) in session.gate_distribution() {
        rows.push(vec![action.as_str().to_owned(), count.to_string(), bar(count, max, 24)]);
    }
    rows
}

fn check_rows(session: &BatterySession) -> Vec<Vec<String>> {
    let total = session.days.len();
    let mut rows = vec![vec!["check".into(), "decided by".into(), "failed".into(), "of".into()]];
    for name in ["reserve_ok", "margin_plausible", "cycle_budget_ok"] {
        let failed = session
            .days
            .iter()
            .filter(|d| d.judgment.checks.get(name).is_some_and(|c| !c.ok))
            .count();
        rows.push(vec![
            name.to_owned(),
            crate::fx_report::check_source(session.days.iter().map(|d| &d.judgment.checks), name),
            failed.to_string(),
            total.to_string(),
        ]);
    }
    rows
}

fn candidate_rows(session: &BatterySession) -> Vec<Vec<String>> {
    let days = session.days.len().max(1) as f64;
    let mut rows = vec![vec![
        "schedule".into(),
        "mean margin".into(),
        "mean cycles".into(),
        "ranked first".into(),
    ]];
    for kind in ScheduleKind::ALL {
        let of_kind: Vec<_> = session
            .days
            .iter()
            .filter_map(|d| d.schedules.iter().find(|s| s.kind == kind))
            .collect();
        let margin: f64 = of_kind.iter().map(|s| s.expected_margin).sum::<f64>() / days;
        let cycles: f64 = of_kind.iter().map(|s| s.cycles).sum::<f64>() / days;
        let first = session.days.iter().filter(|d| d.judgment.chosen == kind).count();
        rows.push(vec![
            kind.as_str().to_owned(),
            format!("{margin:.1}"),
            format!("{cycles:.2}"),
            first.to_string(),
        ]);
    }
    rows
}

fn terminal(
    world: &BatteryWorld,
    session: &BatterySession,
    jev: &Jev,
    summaries: &[jev_core::ExplainOut],
) -> String {
    let (tokens, latency) = jev.audit().totals();
    let mut s = String::new();
    s.push_str(&bold("BATTERY"));
    s.push_str(&dim(&format!(
        "  {} days from {}, {:.0} MW / {:.0} MWh, seed {}, backend {}\n",
        world.days,
        world.start,
        world.asset.power_mw,
        world.asset.capacity_mwh,
        world.seed,
        jev.backend()
    )));

    s.push_str(&heading("Candidate schedules"));
    s.push_str(&dim("  three complete, valid plans per day from the same solver\n"));
    s.push_str(&table(&candidate_rows(session)));

    s.push_str(&heading("Ranking"));
    s.push_str(&table(&rank_rows(session)));

    s.push_str(&heading("Gate distribution"));
    s.push_str(&table(&gate_rows(session)));

    s.push_str(&heading("Checks"));
    s.push_str(&table(&check_rows(session)));
    s.push_str(&dim(
        "  a rule is a comparison the battery crate makes itself; a jev check is a judgment
",
    ));

    s.push_str(&heading("Review"));
    s.push_str(&format!(
        "  {} of {} days flagged for a second look on thin certainty (the gate stands)
",
        session.reviews(),
        session.days.len()
    ));

    s.push_str(&heading("Books"));
    s.push_str(&table(&book_rows(&session.solver_only, &session.gated)));

    s.push_str(&heading("Ten most consequential decisions"));
    let top = session.most_consequential(10);
    if top.is_empty() {
        s.push_str("  the judgment layer changed nothing\n");
    }
    for d in &top {
        s.push_str(&format!(
            "  {} day {:<3} chose {:<14} {}\n",
            d.date,
            d.day,
            d.judgment.chosen.as_str(),
            bold(&format!("margin {}", signed(d.margin_delta()))),
        ));
        s.push_str(&format!("    {}\n", wrap(&d.judgment.gate.reason, 92, "    ")));
        let failed = d.judgment.checks.failed();
        if !failed.is_empty() {
            s.push_str(&dim(&format!("    failed: {}\n", failed.join(", "))));
        }
    }

    s.push_str(&heading("Explain"));
    for out in summaries {
        s.push_str(&format!("  {}\n", bold(out.for_audience.as_str())));
        s.push_str(&format!("    {}\n\n", wrap(&out.summary, 92, "    ")));
    }

    s.push_str(&dim(&format!(
        "{} calls, {} tokens, {} ms\n",
        jev.audit().len(),
        thousands(tokens as f64),
        latency
    )));
    s
}

fn markdown(
    world: &BatteryWorld,
    session: &BatterySession,
    jev: &Jev,
    summaries: &[jev_core::ExplainOut],
) -> String {
    let (tokens, _) = jev.audit().totals();
    let mut s = String::new();
    s.push_str("# Battery session\n\n");
    s.push_str(&format!(
        "{} days from {}, a {:.0} MW / {:.0} MWh battery, seed `{}`, backend `{}`.\n\n",
        world.days,
        world.start,
        world.asset.power_mw,
        world.asset.capacity_mwh,
        world.seed,
        jev.backend(),
    ));

    s.push_str("## Candidate schedules\n\n");
    s.push_str(&markdown_table(&candidate_rows(session)));
    s.push_str(
        "\nAll three respect the state-of-charge bounds. They differ in how much headroom \
         they keep and how hard they cycle.\n",
    );

    s.push_str("\n## Ranking\n\n");
    let plain = |rows: Vec<Vec<String>>| -> Vec<Vec<String>> {
        rows.into_iter()
            .map(|mut r| {
                r.truncate(2);
                r
            })
            .collect()
    };
    s.push_str(&markdown_table(&plain(rank_rows(session))));

    s.push_str("\n## Gate distribution\n\n");
    s.push_str(&markdown_table(&plain(gate_rows(session))));

    s.push_str("\n## Checks\n\n");
    s.push_str(&markdown_table(&check_rows(session)));
    s.push_str(
        "\n`reserve_ok` rarely fails because the ranking has already moved the day onto a \
         schedule that holds the reserve. The check is what would catch it if it had not.\n",
    );

    s.push_str("\n## Books\n\n");
    s.push_str(&markdown_table(&book_rows(&session.solver_only, &session.gated)));
    s.push_str(
        "\n`solver-only` runs the balanced schedule every day, whole. `gated` runs whichever \
         schedule the ranking chose, at the size the gate allowed, on the same prices.\n",
    );

    s.push_str("\n## Ten most consequential decisions\n\n");
    let mut rows = vec![vec![
        "date".into(),
        "chose".into(),
        "action".into(),
        "size".into(),
        "margin effect".into(),
        "reason".into(),
    ]];
    for d in session.most_consequential(10) {
        rows.push(vec![
            d.date.clone(),
            d.judgment.chosen.as_str().to_owned(),
            d.judgment.gate.action.as_str().to_owned(),
            format!("{:.0}%", d.judgment.gate.size_factor * 100.0),
            signed(d.margin_delta()),
            d.judgment.gate.reason.clone(),
        ]);
    }
    s.push_str(&markdown_table(&rows));

    s.push_str("\n## Explain\n\n");
    for out in summaries {
        s.push_str(&format!("**{}** — {}\n\n", out.for_audience.as_str(), out.summary));
    }

    s.push_str(&format!(
        "\n---\n\n{} typed calls, {} tokens, backend `{}`. Every call and its output is in \
         `decisions.jsonl`.\n",
        jev.audit().len(),
        thousands(tokens as f64),
        jev.backend(),
    ));
    s
}
