//! `jev-desk-server` — the demo's runs, served over HTTP.
//!
//! Offline by default: every judgment comes from the rule-based backend unless
//! `--live` is passed, which needs `TYPESAFE_API_KEY` in the environment. A
//! request may ask for either, so one process can serve both.

use anyhow::{Context, Result};
use clap::Parser;
use server::{AppState, Config};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "jev-desk-server",
    version,
    about = "jev-desk over HTTP: the same runs, the same judgments, as a typed JSON API."
)]
struct Cli {
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1:8787")]
    addr: SocketAddr,

    /// Default runs to the System One API rather than the offline backend.
    /// A request can still ask for either.
    #[arg(long)]
    live: bool,

    /// The built UI to serve. Skipped when the directory does not exist.
    #[arg(long, default_value = "ui/dist")]
    ui: PathBuf,

    /// Answer cross-origin requests, for the Vite dev server on another port.
    #[arg(long)]
    cors: bool,

    /// The largest `days` a request may ask for.
    #[arg(long, default_value_t = 365)]
    max_days: u32,

    /// How many runs to keep before dropping the oldest finished one.
    #[arg(long, default_value_t = 16)]
    run_capacity: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "server=info,tower_http=info".into()),
        )
        .init();

    let cli = Cli::parse();

    // A missing key is a configuration problem, not a fault in the program, so
    // it exits cleanly rather than unwinding an error with a backtrace.
    if cli.live && std::env::var_os("TYPESAFE_API_KEY").is_none() {
        eprintln!(
            "jev-desk-server: --live needs TYPESAFE_API_KEY in the environment.\n\n\
             Set one, or drop --live to serve the offline rule-based backend.\n"
        );
        std::process::exit(2);
    }

    let ui_dir = cli.ui.is_dir().then_some(cli.ui.clone());
    let config = Config {
        default_mock: !cli.live,
        max_days: cli.max_days,
        run_capacity: cli.run_capacity,
        ui_dir: ui_dir.clone(),
        permissive_cors: cli.cors,
    };
    let app = server::router(AppState::new(config));

    let listener = tokio::net::TcpListener::bind(cli.addr)
        .await
        .with_context(|| format!("binding {}", cli.addr))?;
    let bound = listener.local_addr().unwrap_or(cli.addr);

    eprintln!("jev-desk-server listening on http://{bound}");
    eprintln!("  api      http://{bound}/api");
    eprintln!("  backend  {}", if cli.live { "System One" } else { "offline (mock)" });
    match &ui_dir {
        Some(dir) => eprintln!("  ui       {}", dir.display()),
        None => eprintln!("  ui       not built; run `just ui-build`"),
    }

    axum::serve(listener, app).with_graceful_shutdown(shutdown()).await.context("serving")?;
    Ok(())
}

/// Stop on Ctrl-C, so a run in flight is not killed mid-write.
async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
    eprintln!("\njev-desk-server: shutting down");
}
