//! `hub` — the netcode hub binary: one server, many games.
//!
//! Today this is the runtime shell: configuration, logging, and graceful
//! shutdown. Networking lands in later changes on top of this loop.

#![forbid(unsafe_code)]

mod config;
mod signals;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use tracing::info;

use crate::config::Config;
use crate::signals::Signals;

/// Grace period granted to in-flight work after a shutdown signal.
const GRACE_PERIOD: Duration = Duration::from_secs(5);

/// Exit code for a clean stop.
const EXIT_OK: u8 = 0;
/// Exit code for a configuration problem.
const EXIT_CONFIG: u8 = 1;
/// Exit code for an unexpected failure.
const EXIT_FATAL: u8 = 2;
/// Exit code for the impatient second signal.
const EXIT_IMPATIENT: i32 = 130;

/// One server, many games.
#[derive(Debug, Parser)]
#[command(name = "hub", version, about)]
struct Args {
    /// Path to the TOML configuration file.
    #[arg(long, default_value = "hub.toml")]
    config: PathBuf,
}

/// Why the hub stopped; maps to the process exit code.
#[derive(Debug)]
enum RunError {
    /// Configuration could not be loaded or parsed.
    Config(String),
    /// An unexpected failure.
    Fatal(anyhow::Error),
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::from(EXIT_OK),
        Err(RunError::Config(msg)) => {
            eprintln!("hub: {msg}");
            ExitCode::from(EXIT_CONFIG)
        }
        Err(RunError::Fatal(err)) => {
            eprintln!("hub: fatal: {err:#}");
            ExitCode::from(EXIT_FATAL)
        }
    }
}

fn run(args: &Args) -> Result<(), RunError> {
    let config = Config::load(&args.config).map_err(|err| RunError::Config(err.to_string()))?;

    // RUST_LOG always wins over the config file.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(config.log_level.clone()));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")
        .map_err(RunError::Fatal)?;

    runtime.block_on(shutdown_loop(&config));

    info!("netcode hub stopped");
    Ok(())
}

/// Runs the hub until the first shutdown signal, then drains in-flight work
/// within [`GRACE_PERIOD`]. A second signal during the drain exits immediately.
async fn shutdown_loop(config: &Config) {
    // Handlers are installed before the startup line is logged so that a
    // signal racing the boot still produces a clean stop.
    let mut signals = Signals::install();
    info!(
        version = env!("CARGO_PKG_VERSION"),
        log_level = %config.log_level,
        "netcode hub starting"
    );

    signals.first().await;
    info!(
        grace_secs = GRACE_PERIOD.as_secs(),
        "shutdown signal received; draining"
    );

    // An impatient operator gets an immediate exit on the second signal.
    tokio::spawn(async {
        let mut impatient = Signals::install();
        impatient.first().await;
        std::process::exit(EXIT_IMPATIENT);
    });

    // In-flight work drains here. Today there is none, so this completes
    // immediately; the timeout exists to bound the drain once it does not.
    let _ = tokio::time::timeout(GRACE_PERIOD, std::future::ready(())).await;
}
