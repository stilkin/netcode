//! Worker-side SDK for the netcode hub.
//!
//! Implement [`GameSession`], hand it to [`run`] with a [`WorkerConfig`], and
//! the SDK owns every networking concern: connecting, authenticating,
//! registering, heartbeats, input buffering, and the tick loop. Game payloads
//! are opaque bytes; the SDK never interprets them.

#![forbid(unsafe_code)]

use std::time::Duration;

use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{info, warn};

use backplane::{Frame, conn};
use protocol::Header;

/// Channel used for per-tick world state (the latest-wins channel).
pub const CHANNEL_STATE: u8 = 0;
/// Channel used for reliable events (chat, scores, round transitions).
pub const CHANNEL_EVENTS: u8 = 1;

/// How long the hub may take to answer registration.
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Session-level game configuration (parsed from `--session-config` JSON).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SessionConfig {
    /// Simulation ticks per second the worker will run.
    pub tick_hz: u32,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self { tick_hz: 30 }
    }
}

/// What the session is, for the hub's registry and server browsers.
#[derive(Debug, Clone)]
pub struct GameInfo {
    pub game_type: String,
    pub display_name: String,
    pub max_players: u16,
    pub payload_schema_version: u16,
}

/// One player input, decoded from a client packet.
#[derive(Debug, Clone)]
pub struct ClientInput {
    pub client_id: u32,
    pub payload: Vec<u8>,
}

/// Everything a game needs to talk back to its players.
pub struct SessionCtx {
    session_id: u32,
    payload_schema_version: u16,
    out: mpsc::UnboundedSender<Frame>,
}

impl SessionCtx {
    /// The session id the hub assigned.
    pub fn session_id(&self) -> u32 {
        self.session_id
    }

    fn packet(&self, channel: u8, client_id: u32, payload: &[u8]) -> Option<Vec<u8>> {
        let header = Header::new(
            channel,
            self.session_id,
            client_id,
            self.payload_schema_version,
            payload.len() as u32,
        );
        match header.encode_packet(payload) {
            Ok(packet) => Some(packet),
            Err(err) => {
                warn!(%err, "output payload rejected; dropped");
                None
            }
        }
    }

    /// Sends a packet to one client.
    pub fn to_client(&self, channel: u8, client_id: u32, payload: &[u8]) {
        if let Some(packet) = self.packet(channel, client_id, payload) {
            let _ = self.out.send(Frame::ToClient { packet });
        }
    }

    /// Sends a packet to an explicit list of clients.
    pub fn to_many(&self, channel: u8, client_ids: &[u32], payload: &[u8]) {
        if let Some(packet) = self.packet(channel, 0, payload) {
            let _ = self.out.send(Frame::ToMany {
                client_ids: client_ids.to_vec(),
                packet,
            });
        }
    }

    /// Sends a packet to every attached client.
    pub fn broadcast(&self, channel: u8, payload: &[u8]) {
        if let Some(packet) = self.packet(channel, 0, payload) {
            let _ = self.out.send(Frame::Broadcast { packet });
        }
    }
}

/// The entire networking-facing surface of a game.
pub trait GameSession {
    /// A client joined. Called before any of that client's inputs.
    fn on_attach(&mut self, ctx: &SessionCtx, client_id: u32, player_name: &str);
    /// A client left.
    fn on_detach(&mut self, ctx: &SessionCtx, client_id: u32);
    /// One simulation step: apply `inputs`, emit outputs through `ctx`.
    fn tick(&mut self, ctx: &SessionCtx, dt: f32, inputs: &[ClientInput]);
}

/// Worker startup configuration.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Backplane address, from `--backplane`.
    pub backplane_addr: String,
    /// Shared token, from `--token`.
    pub token: String,
    /// Pending session to claim; `Some` for hub-spawned workers.
    pub claim: Option<u32>,
    pub info: GameInfo,
    pub session: SessionConfig,
    /// Heartbeat cadence; production default is 5 s (tests shorten it).
    pub heartbeat_interval: Duration,
}

#[derive(Debug, Error)]
pub enum SdkError {
    /// The hub was unreachable at startup.
    #[error("cannot reach hub at {addr}")]
    Connect {
        addr: String,
        #[source]
        source: std::io::Error,
    },
    /// Registration did not complete as specified.
    #[error("registration failed: {0}")]
    Registration(String),
}

/// Runs the worker until the hub disconnects. Returns cleanly when the hub
/// goes away; only startup problems produce errors.
pub async fn run(config: WorkerConfig, mut game: impl GameSession) -> Result<(), SdkError> {
    let stream = TcpStream::connect(&config.backplane_addr)
        .await
        .map_err(|source| SdkError::Connect {
            addr: config.backplane_addr.clone(),
            source,
        })?;
    let (mut rd, mut wr) = stream.into_split();

    conn::write_frame(
        &mut wr,
        &Frame::Hello {
            token: config.token.clone(),
        },
    )
    .await
    .map_err(|err| SdkError::Registration(err.to_string()))?;
    conn::write_frame(
        &mut wr,
        &Frame::Register {
            claimed_id: config.claim.unwrap_or(0),
            game_type: config.info.game_type.clone(),
            display_name: config.info.display_name.clone(),
            max_players: config.info.max_players,
            payload_schema_version: config.info.payload_schema_version,
        },
    )
    .await
    .map_err(|err| SdkError::Registration(err.to_string()))?;

    let registered = tokio::time::timeout(REGISTRATION_TIMEOUT, conn::read_frame(&mut rd))
        .await
        .map_err(|_| SdkError::Registration("timed out waiting for Registered".into()))?
        .map_err(|err| SdkError::Registration(err.to_string()))?;
    let session_id = match registered {
        Frame::Registered { session_id } => session_id,
        other => {
            return Err(SdkError::Registration(format!(
                "expected Registered, got {:?}",
                other.opcode()
            )));
        }
    };
    info!(session = session_id, "registered with hub");

    // All worker-to-hub frames flow through this queue, heartbeats included.
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let ctx = SessionCtx {
        session_id,
        payload_schema_version: config.info.payload_schema_version,
        out: out_tx.clone(),
    };

    let pump = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            if conn::write_frame(&mut wr, &frame).await.is_err() {
                break;
            }
        }
    });

    let mut inputs: Vec<ClientInput> = Vec::new();
    let tick_period = Duration::from_secs_f64(1.0 / f64::from(config.session.tick_hz));
    let mut tick_interval = tokio::time::interval(tick_period);
    tick_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut heartbeat = tokio::time::interval(config.heartbeat_interval);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick_interval.tick().await; // consume the immediate first tick
    heartbeat.tick().await;
    let mut last_tick = tokio::time::Instant::now();

    loop {
        tokio::select! {
            read = conn::read_frame(&mut rd) => match read {
                Ok(Frame::ClientAttached { client_id }) => {
                    game.on_attach(&ctx, client_id, "");
                }
                Ok(Frame::ClientDetached { client_id }) => {
                    game.on_detach(&ctx, client_id);
                }
                Ok(Frame::FromClient { packet }) => {
                    if let Ok((header, payload)) = Header::split_packet(&packet) {
                        inputs.push(ClientInput {
                            client_id: header.client_id,
                            payload: payload.to_vec(),
                        });
                    }
                }
                Ok(_) => {}
                // Hub loss is a clean stop, never an error.
                Err(_) => break,
            },
            _ = tick_interval.tick() => {
                let dt = last_tick.elapsed().as_secs_f32();
                last_tick = tokio::time::Instant::now();
                game.tick(&ctx, dt, &inputs);
                inputs.clear();
            }
            _ = heartbeat.tick() => {
                let _ = out_tx.send(Frame::Heartbeat);
            }
        }
    }

    pump.abort();
    Ok(())
}
