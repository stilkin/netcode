//! Shared test rig: an in-process hub (backplane + client edge + lobby) and
//! the scripted deterministic worker that serves its sessions.

use std::time::Duration;

use backplane::{Frame, conn};
use hub::backplane_service::{Service as Backplane, ServiceHandle, Timeouts};
use hub::client_edge::{Service as Edge, ServiceHandle as EdgeHandle};
use hub::config::{BackplaneConfig, ClientEdgeConfig, LobbyConfig};
use hub::lobby::{Service as Lobby, ServiceHandle as LobbyHandle};
use hub::registry::SessionSpec;
use protocol::Header;

/// Relaxed liveness: these tests exercise the bots, not the watchdog.
pub const SLOW: Timeouts = Timeouts {
    registration: Duration::from_secs(2),
    liveness: Duration::from_secs(60),
    pending: Duration::from_secs(30),
};

const CHANNEL_STATE: u8 = 0;
const CHANNEL_EVENTS: u8 = 1;

/// The in-process hub with all three listeners on ephemeral ports.
pub struct Rig {
    pub backplane: ServiceHandle,
    pub edge: EdgeHandle,
    pub lobby: LobbyHandle,
}

impl Rig {
    pub async fn start() -> Rig {
        let backplane = Backplane::bind_with_timeouts(
            &BackplaneConfig {
                bind: "127.0.0.1:0".into(),
                token: String::new(),
                max_sessions: 64,
            },
            SLOW,
        )
        .await
        .expect("bind backplane")
        .start();

        let edge = Edge::bind(
            &ClientEdgeConfig {
                bind: "127.0.0.1:0".into(),
                token: String::new(),
            },
            backplane.clone(),
        )
        .await
        .expect("bind edge")
        .start();

        let lobby = Lobby::bind(
            &LobbyConfig {
                bind: "127.0.0.1:0".into(),
                token: String::new(),
            },
            &[],
            backplane.clone(),
            edge.cert_sha256(),
        )
        .await
        .expect("bind lobby")
        .start();

        Rig {
            backplane,
            edge,
            lobby,
        }
    }

    /// Creates a pending session and activates it with the scripted
    /// worker, whose behavior is a pure function of its inputs:
    /// greet on attach, echo each input, broadcast one counter per input.
    pub async fn scripted_session(&self) -> u32 {
        let spec = SessionSpec {
            game_type: "scripted".into(),
            display_name: "Scripted".into(),
            max_players: 64,
            payload_schema_version: 1,
        };
        let pending = self.backplane.create_pending(spec).await.expect("pending");

        let stream = tokio::net::TcpStream::connect(self.backplane.local_addr())
            .await
            .expect("connect worker");
        let (mut rd, mut wr) = stream.into_split();
        conn::write_frame(
            &mut wr,
            &Frame::Hello {
                token: String::new(),
            },
        )
        .await
        .expect("hello");
        conn::write_frame(
            &mut wr,
            &Frame::Register {
                claimed_id: pending,
                game_type: "scripted".into(),
                display_name: "Scripted".into(),
                max_players: 64,
                payload_schema_version: 1,
            },
        )
        .await
        .expect("register");
        match tokio::time::timeout(Duration::from_secs(2), conn::read_frame(&mut rd))
            .await
            .expect("registered in time")
            .expect("frame parses")
        {
            Frame::Registered { session_id } => assert_eq!(session_id, pending),
            other => panic!("expected Registered, got {other:?}"),
        }

        // Reader task (cancel-safety lesson learned in change 3): frames in,
        // commands out; the handler below is the only writer.
        let (commands_tx, mut commands_rx) = tokio::sync::mpsc::unbounded_channel::<Frame>();
        tokio::spawn(async move {
            while let Ok(frame) = conn::read_frame(&mut rd).await {
                if commands_tx.send(frame).is_err() {
                    break;
                }
            }
        });

        let session = pending;
        tokio::spawn(async move {
            let mut tick: u64 = 0;
            while let Some(frame) = commands_rx.recv().await {
                match frame {
                    Frame::ClientAttached { client_id } => {
                        // Constant greeting: client ids are never reused
                        // across runs, so fixtures must not embed them.
                        let greeting = packet(CHANNEL_EVENTS, session, client_id, b"welcome");
                        let _ =
                            conn::write_frame(&mut wr, &Frame::ToClient { packet: greeting }).await;
                    }
                    Frame::FromClient { packet: bytes } => {
                        let Ok((header, payload)) = Header::split_packet(&bytes) else {
                            continue;
                        };
                        // Echo, then one broadcast per input — a deterministic
                        // "tick" that is a function of the input stream.
                        let echo = packet(CHANNEL_STATE, session, header.client_id, payload);
                        if conn::write_frame(&mut wr, &Frame::ToClient { packet: echo })
                            .await
                            .is_err()
                        {
                            break;
                        }
                        tick += 1;
                        let broadcast = packet(CHANNEL_STATE, session, 0, &tick.to_le_bytes());
                        if conn::write_frame(&mut wr, &Frame::Broadcast { packet: broadcast })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Frame::Heartbeat => {}
                    _ => {}
                }
            }
        });

        pending
    }

    pub async fn stop(&self) {
        self.lobby.shutdown();
        self.lobby.drain().await;
        self.edge.shutdown();
        self.edge.drain().await;
        self.backplane.shutdown();
        self.backplane.clone().drain().await;
    }
}

fn packet(channel: u8, session: u32, client: u32, payload: &[u8]) -> Vec<u8> {
    Header::new(channel, session, client, 1, payload.len() as u32)
        .encode_packet(payload)
        .expect("packet")
}
