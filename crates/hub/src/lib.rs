//! `hub` — the netcode hub: one server, many games.
//!
//! The library exposes the runtime (`run`), configuration, signal handling,
//! and — as later changes land — the session registry, backplane service, and
//! worker spawn APIs. The `hub` binary is a thin wrapper over this library.

#![forbid(unsafe_code)]

pub mod backplane_service;
pub mod client_edge;
pub mod config;
pub mod lobby;
pub mod registry;
pub mod signals;
pub mod spawn;

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
pub const EXIT_OK: u8 = 0;
/// Exit code for a configuration problem.
pub const EXIT_CONFIG: u8 = 1;
/// Exit code for an unexpected failure.
pub const EXIT_FATAL: u8 = 2;
/// Exit code for the impatient second signal.
const EXIT_IMPATIENT: i32 = 130;

/// One server, many games.
#[derive(Debug, Parser)]
#[command(name = "hub", version, about)]
pub struct Args {
    /// Path to the TOML configuration file.
    #[arg(long, default_value = "hub.toml")]
    pub config: PathBuf,
}

/// Why the hub stopped; maps to the process exit code.
#[derive(Debug)]
pub enum RunError {
    /// Configuration could not be loaded or parsed.
    Config(String),
    /// An unexpected failure.
    Fatal(anyhow::Error),
}

/// Runs the hub until shutdown. Entry point of the `hub` binary.
pub fn run(args: &Args) -> Result<(), RunError> {
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

    runtime
        .block_on(shutdown_loop(&config))
        .map_err(RunError::Fatal)?;

    info!("netcode hub stopped");
    Ok(())
}

/// Runs the hub until the first shutdown signal, then drains in-flight work
/// within [`GRACE_PERIOD`]. A second signal during the drain exits immediately.
async fn shutdown_loop(config: &Config) -> Result<(), anyhow::Error> {
    // Handlers are installed before the startup line is logged so that a
    // signal racing the boot still produces a clean stop.
    let mut signals = Signals::install();
    info!(
        version = env!("CARGO_PKG_VERSION"),
        log_level = %config.log_level,
        "netcode hub starting"
    );

    let service = backplane_service::Service::bind(&config.backplane)
        .await
        .context("failed to bind backplane listener")?;
    let service_handle = service.start();
    info!(backplane = %service_handle.local_addr(), "backplane listening");

    // The client edge routes browser traffic onto the backplane's attach
    // seam; a bind failure (port taken) is fatal, like the backplane's.
    let edge = client_edge::Service::bind(&config.client_edge, service_handle.clone())
        .await
        .context("failed to bind client edge")?;
    let edge_handle = edge.start();

    let lobby = lobby::Service::bind(
        &config.lobby,
        &config.games,
        service_handle.clone(),
        edge_handle.cert_sha256(),
    )
    .await
    .context("failed to bind lobby endpoint")?;
    let lobby_handle = lobby.start();

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

    // Stop accepting work, then drain within the grace period. Client
    // connections close first; workers see their clients leave, then the
    // backplane itself drains.
    lobby_handle.shutdown();
    let _ = tokio::time::timeout(GRACE_PERIOD, lobby_handle.drain()).await;
    edge_handle.shutdown();
    let _ = tokio::time::timeout(GRACE_PERIOD, edge_handle.drain()).await;
    service_handle.shutdown();
    let _ = tokio::time::timeout(GRACE_PERIOD, service_handle.drain()).await;
    Ok(())
}

/// Maps a [`RunError`] onto the process exit code.
pub fn exit_code(err: &RunError) -> ExitCode {
    match err {
        RunError::Config(msg) => {
            eprintln!("hub: {msg}");
            ExitCode::from(EXIT_CONFIG)
        }
        RunError::Fatal(err) => {
            eprintln!("hub: fatal: {err:#}");
            ExitCode::from(EXIT_FATAL)
        }
    }
}
