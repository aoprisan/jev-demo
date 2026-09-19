//! The forex report: terminal and Markdown.

use crate::explain::{self, Counts};
use crate::fmt::{bar, bold, dim, heading, markdown_table, signed, table, thousands, wrap};
use crate::DomainReport;
use fx::{BookResult, FxSession};
use jev_core::{Action, Audience, Jev, Result};
use synth::{FxWorld, Pair};

/// Build the forex report, running the Explain stage for each audience.
pub async fn render(
    jev: &Jev,
    world: &FxWorld,
    session: &FxSession,
    params: &fx::StrategyParams,
) -> Result<DomainReport> {
    let failed_checks: usize =
        session.decisions.iter().map(|d| d.judgment.checks.failed().len()).sum();
    let counts = Counts {
        domain: "forex",
        decisions: session.decisions.len(),
        interventions: session.interventions(),
        escalations: session.escalations(),
        jev_calls: jev.audit().len(),
        failed_checks,
        headlines: vec![
            format!(
                "gating moved the book from {} to {}, at {:.1} against {:.1} bp of return",
                signed(session.ungated.pnl),
                signed(session.gated.pnl),
                session.gated.return_bps(),
                session.ungated.return_bps(),
            ),
            format!(
                "the hit rate went from {:.0}% to {:.0}% on {} fewer trades",
                session.ungated.hit_rate() * 100.0,
                session.gated.hit_rate() * 100.0,
                session.ungated.trades.saturating_sub(session.gated.trades),
            ),
        ],
    };
    let summaries = explain::for_audiences(
        jev,
        &counts,
        &[Audience::Trader, Audience::Compliance],
        "the forex session",
    )
    .await?;

    let terminal = terminal(world, session, params, jev, &summaries);
    let markdown = markdown(world, session, params, jev, &summaries);
    Ok(DomainReport { name: "fx".into(), terminal, markdown })
}

fn book_rows(ungated: &BookResult, gated: &BookResult) -> Vec<Vec<String>> {
    vec![
        vec!["".into(), "ungated".into(), "gated".into()],
        vec!["trades".into(), ungated.trades.to_string(), gated.trades.to_string()],
        vec!["P&L".into(), signed(ungated.pnl), signed(gated.pnl)],
        vec![
            "return on notional".into(),
            format!("{:.2} bp", ungated.return_bps()),
            format!("{:.2} bp", gated.return_bps()),
        ],
        vec![
            "hit rate".into(),
            format!("{:.0}%", ungated.hit_rate() * 100.0),
            format!("{:.0}%", gated.hit_rate() * 100.0),
        ],
        vec!["max drawdown".into(), thousands(ungated.max_drawdown), thousands(gated.max_drawdown)],
        vec!["notional deployed".into(), thousands(ungated.notional), thousands(gated.notional)],
    ]
}

fn gate_rows(session: &FxSession) -> Vec<Vec<String>> {
    let max = session.gate_distribution().iter().map(|(_, n)| *n).max().unwrap_or(0);
    let mut rows = vec![vec!["action".into(), "count".into(), "".into()]];
    for (action, count) in session.gate_distribution() {
        rows.push(vec![action.as_str().to_owned(), count.to_string(), bar(count, max, 24)]);
    }
    rows
}

fn check_rows(session: &FxSession) -> Vec<Vec<String>> {
    let total = session.decisions.len();
    let mut rows = vec![vec!["check".into(), "decided by".into(), "failed".into(), "of".into()]];
    for name in CHECKS {
        let failed = session
            .decisions
            .iter()
            .filter(|d| d.judgment.checks.get(name).is_some_and(|c| !c.ok))
            .count();
        rows.push(vec![
            name.to_owned(),
            check_source(session.decisions.iter().map(|d| &d.judgment.checks), name),
            failed.to_string(),
            total.to_string(),
        ]);
    }
    rows
}

/// Who decided a named check: a rule in the domain's code, or Jev.
pub(crate) fn check_source<'a>(
    mut checks: impl Iterator<Item = &'a jev_core::CheckOut>,
    name: &str,
) -> String {
    match checks.find_map(|c| c.get(name)).map(|c| c.source) {
        Some(jev_core::CheckSource::Rule) => "rule".into(),
        Some(jev_core::CheckSource::Jev) => "jev".into(),
        None => "—".into(),
    }
}

/// Each named check held up against the outcomes it flagged.
fn scorecard_rows(session: &FxSession) -> Vec<Vec<String>> {
    let mut rows = vec![vec![
        "check".into(),
        "flagged".into(),
        "mean bp".into(),
        "hit".into(),
        "passed".into(),
        "mean bp".into(),
        "hit".into(),
        "edge".into(),
    ]];
    for name in CHECKS {
        let card = session.score_check(name);
        rows.push(vec![
            name.to_owned(),
            card.failed_n.to_string(),
            format!("{:+.1}", card.failed_bps),
            format!("{:.0}%", card.failed_hit * 100.0),
            card.passed_n.to_string(),
            format!("{:+.1}", card.passed_bps),
            format!("{:.0}%", card.passed_hit * 100.0),
            format!("{:+.1}", card.edge_bps()),
        ]);
    }
    rows
}

/// The checks this domain runs, in report order.
const CHECKS: [&str; 3] = ["stop_sane", "signal_valid_in_regime", "correlated_exposure_ok"];

fn candidate_rows(session: &FxSession) -> Vec<Vec<String>> {
    let mut rows = vec![vec!["pair".into(), "candidates".into(), "executed".into()]];
    for pair in Pair::ALL {
        let of_pair: Vec<_> =
            session.decisions.iter().filter(|d| d.candidate.pair == pair).collect();
        let executed = of_pair.iter().filter(|d| d.judgment.gate.action.acts()).count();
        rows.push(vec![pair.code().to_owned(), of_pair.len().to_string(), executed.to_string()]);
    }
    rows
}

fn terminal(
    world: &FxWorld,
    session: &FxSession,
    params: &fx::StrategyParams,
    jev: &Jev,
    summaries: &[jev_core::ExplainOut],
) -> String {
    let (tokens, latency) = jev.audit().totals();
    let mut s = String::new();
    s.push_str(&bold("FOREX"));
    s.push_str(&dim(&format!(
        "  {} days from {}, {} pairs, seed {}, backend {}\n",
        world.days,
        world.start,
        Pair::ALL.len(),
        world.seed,
        jev.backend()
    )));

    s.push_str(&heading("Candidates"));
    s.push_str(&dim(&format!(
        "  Bollinger({}, {:.1}) mean reversion on 4h bars, stop at {:.1} sd\n",
        params.period, params.k, params.stop_sd
    )));
    s.push_str(&table(&candidate_rows(session)));

    s.push_str(&heading("Gate distribution"));
    s.push_str(&table(&gate_rows(session)));

    s.push_str(&heading("Checks"));
    s.push_str(&table(&check_rows(session)));
    s.push_str(&dim(
        "  a rule is a comparison the fx crate makes itself; a jev check is a judgment
",
    ));

    s.push_str(&heading("Review"));
    s.push_str(&format!(
        "  {} of {} decisions flagged for a second look on thin certainty (the gate stands)
",
        session.reviews(),
        session.decisions.len()
    ));

    s.push_str(&heading("Regime classification"));
    s.push_str(&format!(
        "  agreed with the generator on {:.1}% of {} decisions\n",
        session.regime_accuracy() * 100.0,
        session.decisions.len()
    ));
    s.push_str(&dim(
        "  the generator's regime is never sent to Jev; this is measured after the fact\n",
    ));

    s.push_str(&heading("Judgment scorecard"));
    s.push_str(&dim(
        "  each named check against the outcomes it flagged, measured on the ungated fills\n\
         \x20 so the check is scored on its own merits, not on the gate's response to it\n",
    ));
    s.push_str(&table(&scorecard_rows(session)));
    for name in CHECKS {
        let card = session.score_check(name);
        if card.inverted() {
            s.push_str(&format!("\n  {} {}\n", bold("INVERTED"), name));
            s.push_str(&format!(
                "    {}\n",
                wrap(
                    &format!(
                        "it flags the better decisions, not the worse ones: {:+.1} bp \
                         flagged against {:+.1} bp passed. Acting on it costs money in \
                         this market. The rule is implemented as specified and the regime \
                         call behind it is accurate; it is the inference from the regime \
                         that does not hold here.",
                        card.failed_bps, card.passed_bps
                    ),
                    88,
                    "    "
                )
            ));
        }
    }

    s.push_str(&heading("Books"));
    s.push_str(&table(&book_rows(&session.ungated, &session.gated)));

    s.push_str(&heading("Ten most consequential decisions"));
    let top = session.most_consequential(10);
    if top.is_empty() {
        s.push_str("  the gate changed nothing\n");
    }
    for d in &top {
        s.push_str(&format!(
            "  {} {:<7} {:<5} {}\n",
            d.candidate.t,
            d.candidate.pair.code(),
            d.candidate.side.as_str(),
            bold(&format!("P&L {}", signed(d.pnl_delta()))),
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
    world: &FxWorld,
    session: &FxSession,
    params: &fx::StrategyParams,
    jev: &Jev,
    summaries: &[jev_core::ExplainOut],
) -> String {
    let (tokens, _) = jev.audit().totals();
    let mut s = String::new();
    s.push_str("# Forex session\n\n");
    s.push_str(&format!(
        "{} days from {}, {} pairs, seed `{}`, backend `{}`. \
         Bollinger({}, {:.1}) mean reversion on 4h bars.\n\n",
        world.days,
        world.start,
        Pair::ALL.len(),
        world.seed,
        jev.backend(),
        params.period,
        params.k,
    ));

    s.push_str("## Candidates\n\n");
    s.push_str(&markdown_table(&candidate_rows(session)));

    s.push_str("\n## Gate distribution\n\n");
    let plain: Vec<Vec<String>> = gate_rows(session)
        .into_iter()
        .map(|mut r| {
            r.truncate(2);
            r
        })
        .collect();
    s.push_str(&markdown_table(&plain));

    s.push_str("\n## Checks\n\n");
    s.push_str(&markdown_table(&check_rows(session)));

    s.push_str(&format!(
        "\nThe classifier agreed with the generator's regime on **{:.1}%** of {} \
         decisions. That regime is never sent to Jev; the comparison is made \
         afterwards, which is why it is not 100%.\n",
        session.regime_accuracy() * 100.0,
        session.decisions.len()
    ));

    s.push_str("\n## Judgment scorecard\n\n");
    s.push_str(
        "Each named check held up against what actually happened. The outcomes are the \
         **ungated** fills, so a check is scored on its own merits rather than on the \
         gate's response to it. A positive `edge` means the check flagged the worse \
         decisions, which is what it is for.\n\n",
    );
    s.push_str(&markdown_table(&scorecard_rows(session)));
    for name in CHECKS {
        let card = session.score_check(name);
        if card.inverted() {
            s.push_str(&format!(
                "\n> **`{name}` is inverted in this market.** It flags decisions averaging \
                 {:+.1} bp while passing ones averaging {:+.1} bp, so acting on it costs \
                 money. The rule is implemented exactly as specified — an excursion outside \
                 the band should not be bought into a trend — and the regime call behind it \
                 is accurate. What fails is the inference: at a band excursion, the same \
                 sharpness that marks a trend also marks an overshoot that reverts hard, and \
                 the second effect dominates. This is the kind of thing a typed, logged \
                 judgment layer exists to make visible.\n",
                card.failed_bps, card.passed_bps
            ));
        }
    }

    s.push_str("\n## Books\n\n");
    s.push_str(&markdown_table(&book_rows(&session.ungated, &session.gated)));
    s.push_str(
        "\n`ungated` runs every candidate at the size the solver chose. `gated` runs \
         what the judgment layer allowed, on the same signals.\n",
    );

    s.push_str("\n## Ten most consequential decisions\n\n");
    let mut rows = vec![vec![
        "when".into(),
        "pair".into(),
        "side".into(),
        "action".into(),
        "size".into(),
        "P&L effect".into(),
        "reason".into(),
    ]];
    for d in session.most_consequential(10) {
        rows.push(vec![
            d.candidate.t.to_string(),
            d.candidate.pair.code().to_owned(),
            d.candidate.side.as_str().to_owned(),
            d.judgment.gate.action.as_str().to_owned(),
            format!("{:.0}%", d.judgment.gate.size_factor * 100.0),
            signed(d.pnl_delta()),
            d.judgment.gate.reason.clone(),
        ]);
    }
    s.push_str(&markdown_table(&rows));

    s.push_str("\n## Explain\n\n");
    for out in summaries {
        s.push_str(&format!("**{}** — {}\n\n", out.for_audience.as_str(), out.summary));
    }

    s.push_str(&format!(
        "\n---\n\n{} typed calls, {} tokens, backend `{}`. Every call and its output is \
         in `decisions.jsonl`.\n",
        jev.audit().len(),
        thousands(tokens as f64),
        jev.backend(),
    ));
    s
}

/// The gate's distribution, for the `demo-all` summary line.
pub fn gate_counts(session: &FxSession) -> Vec<(Action, usize)> {
    session.gate_distribution()
}
