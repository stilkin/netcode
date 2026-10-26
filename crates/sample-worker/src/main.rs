//! Echo arena — the sample game worker and integration-test fixture.
//!
//! Game loop: greet each player on join, echo every input back to its sender
//! (state channel), and broadcast a tick counter (so clients can observe
//! smooth per-tick world state).

use clap::Parser;
use hub_sdk::{
    CHANNEL_EVENTS, CHANNEL_STATE, ClientInput, GameInfo, GameSession, SessionConfig, SessionCtx,
    WorkerConfig,
};

/// Echo arena — the reference netcode hub game.
#[derive(Debug, Parser)]
#[command(name = "sample-worker", version, about)]
struct Args {
    /// Backplane address, e.g. 127.0.0.1:24000.
    #[arg(long)]
    backplane: String,
    /// Shared backplane token (empty = none).
    #[arg(long, default_value = "")]
    token: String,
    /// Session configuration as JSON, e.g. '{"tick_hz":30}'.
    #[arg(long, default_value = "{}")]
    session_config: String,
    /// Pending session id to claim (set by the hub when it spawns us).
    #[arg(long)]
    session_id: Option<u32>,
    /// Test hook: exit 0 immediately (exercises the hub watchdog).
    #[arg(long)]
    exit_immediately: bool,
    /// Test hook: exit 0 after this many seconds.
    #[arg(long)]
    exit_after: Option<f32>,
}

struct EchoArena {
    tick: u64,
}

impl GameSession for EchoArena {
    fn on_attach(&mut self, ctx: &SessionCtx, client_id: u32, _player_name: &str) {
        ctx.to_client(
            CHANNEL_EVENTS,
            client_id,
            format!("welcome {client_id}").as_bytes(),
        );
    }

    fn on_detach(&mut self, _ctx: &SessionCtx, _client_id: u32) {}

    fn tick(&mut self, ctx: &SessionCtx, _dt: f32, inputs: &[ClientInput]) {
        for input in inputs {
            ctx.to_client(CHANNEL_STATE, input.client_id, &input.payload);
        }
        self.tick += 1;
        ctx.broadcast(CHANNEL_STATE, &self.tick.to_le_bytes());
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();
    if args.exit_immediately {
        std::process::exit(0);
    }
    let session_config: SessionConfig =
        serde_json::from_str(&args.session_config).expect("valid --session-config JSON");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    runtime.block_on(async move {
        // Test hook: a scheduled clean exit (must run inside the runtime).
        if let Some(secs) = args.exit_after {
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs_f32(secs)).await;
                std::process::exit(0);
            });
        }
        let config = WorkerConfig {
            backplane_addr: args.backplane,
            token: args.token,
            claim: args.session_id,
            info: GameInfo {
                game_type: "echo-arena".to_owned(),
                display_name: "Echo Arena".to_owned(),
                max_players: 8,
                payload_schema_version: 1,
            },
            session: session_config,
            heartbeat_interval: std::time::Duration::from_secs(5),
        };
        match hub_sdk::run(config, EchoArena { tick: 0 }).await {
            Ok(()) => tracing::info!("echo arena stopped"),
            Err(err) => {
                eprintln!("sample-worker: {err}");
                std::process::exit(2);
            }
        }
    });
}
