//! `jev-desk` — a demo of TypeSafe's Jev model as a typed judgment layer over
//! deterministic trading and optimisation code.
//!
//! Two domains, one set of primitives. `--mock` is the only switch: with it,
//! every judgment comes from the offline rule-based backend; without it, from
//! the System One API.

mod replay;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use jev_core::{Audience, Jev};
use report::fmt::{bold, dim};
use std::path::PathBuf;
use synth::{BatteryWorld, FxWorld, DEFAULT_SEED};

#[derive(Parser, Debug)]
#[command(
    name = "jev-desk",
    version,
    about = "The solver owns the numbers, Jev owns the judgment, the schema owns the contract."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Use the offline rule-based backend instead of the System One API.
    #[arg(long, global = true)]
    mock: bool,

    /// Seed for the synthetic worlds.
    #[arg(long, global = true, default_value_t = DEFAULT_SEED)]
    seed: u64,

    /// Days to generate.
    #[arg(long, global = true, default_value_t = 90)]
    days: u32,

    /// Judge only the first N days. Keeps a live run's call count bounded.
    #[arg(long, global = true)]
    limit: Option<u32>,

    /// Where reports and audit logs go.
    #[arg(long, global = true, default_value = "out")]
    out: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Forex: a full run, then one CPI day replayed with and without the event.
    Fx,
    /// Battery: a full run, then one day replayed with a grid notice added.
    Battery,
    /// Both, then one Explain call summarising the day for Compliance.
    All,
    /// Print each primitive's standing instructions.
    Prompts,
    /// Print the JSON Schema of each primitive output.
    Schemas,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Prompts => {
            print_prompts();
            return Ok(());
        }
        Command::Schemas => {
            print_schemas()?;
            return Ok(());
        }
        _ => {}
    }

    // A missing key is a configuration problem, not a fault in the program, so
    // it exits cleanly rather than unwinding an error with a backtrace.
    if !cli.mock && std::env::var_os("TYPESAFE_API_KEY").is_none() {
        eprintln!(
            "jev-desk: no TYPESAFE_API_KEY in the environment.\n\n\
             Set one to run against the System One API, or pass --mock to use the offline\n\
             rule-based backend:\n\n    \
             just demo-fx-mock\n    just demo-battery-mock\n    just demo-all-mock\n"
        );
        std::process::exit(2);
    }

    match cli.command {
        Command::Fx => {
            run_fx(&cli).await?;
        }
        Command::Battery => {
            run_battery(&cli).await?;
        }
        Command::All => run_all(&cli).await?,
        Command::Prompts | Command::Schemas => unreachable!("handled above"),
    }
    Ok(())
}

/// Build a Jev handle logging to `<out>/<domain>/decisions.jsonl`.
fn connect(cli: &Cli, domain: &str) -> Result<Jev> {
    let audit = report::audit_for(&cli.out, domain)
        .with_context(|| format!("opening the audit log for {domain}"))?;
    jev_core::connect(cli.mock, audit).map_err(Into::into)
}

async fn run_fx(cli: &Cli) -> Result<fx::FxSession> {
    let jev = connect(cli, "fx")?;
    let world = FxWorld::generate(cli.seed, cli.days);
    let params = fx::StrategyParams::default();

    eprintln!("{}", dim("running the forex session..."));
    let session = fx::run_session(&jev, &world, &params, cli.limit).await?;

    let report = report::fx_report::render(&jev, &world, &session, &params).await?;
    println!("{}", report.terminal);

    let path = report.write(report::out_dir(&cli.out, "fx"))?;
    println!("{}", dim(&format!("report: {}", path.display())));
    println!(
        "{}",
        dim(&format!(
            "audit:  {}",
            report::out_dir(&cli.out, "fx").join("decisions.jsonl").display()
        ))
    );

    replay::fx_event_day(&jev, &world, &session, &params).await?;
    Ok(session)
}

async fn run_battery(cli: &Cli) -> Result<battery::BatterySession> {
    let jev = connect(cli, "battery")?;
    let world = BatteryWorld::generate(cli.seed, cli.days);

    eprintln!("{}", dim("running the battery session..."));
    let session = battery::run_session(&jev, &world, cli.limit).await?;

    let report = report::battery_report::render(&jev, &world, &session).await?;
    println!("{}", report.terminal);

    let path = report.write(report::out_dir(&cli.out, "battery"))?;
    println!("{}", dim(&format!("report: {}", path.display())));
    println!(
        "{}",
        dim(&format!(
            "audit:  {}",
            report::out_dir(&cli.out, "battery").join("decisions.jsonl").display()
        ))
    );

    replay::battery_notice_day(&jev, &world, cli).await?;
    Ok(session)
}

async fn run_all(cli: &Cli) -> Result<()> {
    let fx_session = run_fx(cli).await?;
    println!();
    let battery_session = run_battery(cli).await?;

    // One closing Explain call over both domains, for Compliance.
    print!("{}", report::fmt::heading("The day, for Compliance"));
    let jev = connect(cli, "all")?;
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
    let out = report::explain::for_audience(
        &jev,
        &counts,
        Audience::Compliance,
        "the trading day across both desks",
    )
    .await?;
    println!("  {}\n", report::fmt::wrap(&out.summary, 92, "  "));
    Ok(())
}

fn print_prompts() {
    for (primitive, text) in jev_core::prompts::all() {
        println!("{}", bold(&format!("=== {primitive} ===")));
        println!("{text}");
    }
}

fn print_schemas() -> Result<()> {
    let mut all = serde_json::Map::new();
    for (name, schema) in jev_core::schema::all() {
        all.insert(name.to_owned(), schema);
    }
    println!("{}", serde_json::to_string_pretty(&all)?);
    Ok(())
}
