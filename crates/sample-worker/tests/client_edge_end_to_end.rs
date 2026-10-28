//! The flagship end-to-end: a real WebTransport client (hash-pinned, the
//! browser trust path) joins a session served by the real `sample-worker`
//! process, through the in-process hub (backplane + client edge).
//!
//! browser protocol ⇄ QUIC ⇄ hub ⇄ backplane ⇄ echo arena.

use std::time::Duration;

use hub::backplane_service::{Service as Backplane, Timeouts};
use hub::client_edge::{Service as Edge, ServiceHandle as EdgeHandle};
use hub::config::{BackplaneConfig, ClientEdgeConfig};
use hub::registry::{SessionSpec, SessionState};
use hub::spawn::SpawnRequest;
use protocol::Header;

const TICK_HZ_JSON: &str = r#"{"tick_hz":60}"#;
const CHANNEL_STATE: u8 = 0;
const CHANNEL_EVENTS: u8 = 1;

async fn start_hub() -> (hub::backplane_service::ServiceHandle, EdgeHandle) {
    let backplane = Backplane::bind_with_timeouts(
        &BackplaneConfig {
            bind: "127.0.0.1:0".into(),
            token: String::new(),
        },
        Timeouts {
            registration: Duration::from_secs(2),
            liveness: Duration::from_secs(60),
            pending: Duration::from_secs(10),
        },
    )
    .await
    .expect("bind backplane");
    let backplane = backplane.start();

    let edge = Edge::bind(
        &ClientEdgeConfig {
            bind: "127.0.0.1:0".into(),
            bootstrap_bind: "127.0.0.1:0".into(),
            token: String::new(),
        },
        backplane.clone(),
    )
    .await
    .expect("bind edge");
    let edge = edge.start();
    (backplane, edge)
}

fn spawn_request() -> SpawnRequest {
    SpawnRequest {
        program: std::env!("CARGO_BIN_EXE_sample-worker").into(),
        spec: SessionSpec {
            game_type: "echo-arena".into(),
            display_name: "Echo Arena".into(),
            max_players: 8,
            payload_schema_version: 1,
        },
        session_config_json: TICK_HZ_JSON.into(),
        extra_args: Vec::new(),
    }
}

async fn poll_until(mut check: impl AsyncFnMut() -> bool, secs: u64, what: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if check().await {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out waiting for: {what}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn packet(channel: u8, session: u32, client: u32, payload: &[u8]) -> Vec<u8> {
    Header::new(channel, session, client, 1, payload.len() as u32)
        .encode_packet(payload)
        .expect("packet")
}

/// A browser-shaped client: WebTransport, hash-pinned, one control stream.
struct Browser {
    conn: wtransport::Connection,
    writer: wtransport::SendStream,
    reader: wtransport::RecvStream,
    buf: Vec<u8>,
    session: u32,
    client_id: u32,
}

async fn browser_join(edge: &EdgeHandle, session: u32, name: &str) -> Browser {
    let config = wtransport::ClientConfig::builder()
        .with_bind_default()
        .with_server_certificate_hashes([wtransport::tls::Sha256Digest::new(edge.cert_sha256())])
        .build();
    let endpoint = wtransport::Endpoint::client(config).expect("client endpoint");
    let conn = tokio::time::timeout(
        Duration::from_secs(5),
        endpoint.connect(format!("https://127.0.0.1:{}", edge.edge_addr().port())),
    )
    .await
    .expect("connect in time")
    .expect("connects");

    let opening = conn.open_bi().await.expect("open stream");
    let (mut writer, mut reader) = opening.await.expect("stream establishes");

    let connect = protocol::Connect {
        token: String::new(),
        session_id: session,
        name: name.into(),
    };
    writer
        .write_all(&connect.encode().expect("encode"))
        .await
        .expect("Connect sent");

    // Reply: kind, then the rest of the frame.
    let mut head = [0u8; 2];
    tokio::time::timeout(Duration::from_secs(3), reader.read_exact(&mut head))
        .await
        .expect("reply in time")
        .expect("reply bytes");
    assert_eq!(head[0], protocol::KIND_ACCEPT, "expected Accept");
    let mut rest = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(3), reader.read_exact(&mut rest))
        .await
        .expect("reply in time")
        .expect("reply bytes");
    let mut frame = head.to_vec();
    frame.extend_from_slice(&rest);
    let accept = protocol::Accept::decode(&frame).expect("accept parses");

    Browser {
        conn,
        writer,
        reader,
        buf: Vec::new(),
        session,
        client_id: accept.client_id,
    }
}

impl Browser {
    /// Sends an event packet on the control stream (client → hub events).
    async fn send_event(&mut self, payload: &[u8]) {
        self.writer
            .write_all(&packet(
                CHANNEL_EVENTS,
                self.session,
                self.client_id,
                payload,
            ))
            .await
            .expect("event sent");
    }

    fn send_input(&self, payload: &[u8]) {
        self.conn
            .send_datagram(packet(CHANNEL_STATE, self.session, self.client_id, payload))
            .expect("input datagram");
    }

    /// Next packet on either transport, routed by channel.
    async fn next(&mut self) -> (u8, u32, Vec<u8>) {
        loop {
            // Events on the control stream, buffered and self-delimiting.
            if self.buf.len() >= protocol::BASE_HEADER_SIZE as usize
                && let Ok((header, payload)) = Header::split_packet(&self.buf)
            {
                let consumed = header.packet_len();
                let out = (header.channel, header.client_id, payload.to_vec());
                self.buf.drain(..consumed);
                return out;
            }
            let mut chunk = [0u8; 4096];
            tokio::select! {
                read = self.reader.read(&mut chunk) => {
                    let n = read.expect("read ok").expect("stream open");
                    self.buf.extend_from_slice(&chunk[..n]);
                }
                datagram = self.conn.receive_datagram() => {
                    let datagram = datagram.expect("datagram arrives");
                    let (header, payload) = Header::split_packet(&datagram).expect("packet");
                    return (header.channel, header.client_id, payload.to_vec());
                }
            }
        }
    }
}

/// (client-edge + client-protocol: full loop) Spawn → active → browser join →
/// greeting on the stream → ordered datagram echoes → broadcasts → abrupt
/// loss detaches.
#[tokio::test]
async fn browser_plays_echo_arena_end_to_end() {
    let (backplane, edge) = start_hub().await;

    // Spawn the real game: pending → active once the worker claims it.
    let session = backplane
        .spawn_session(spawn_request())
        .await
        .expect("spawn");
    poll_until(
        || async {
            backplane
                .sessions()
                .await
                .iter()
                .any(|s| s.id == session && s.state == SessionState::Active)
        },
        5,
        "worker to claim its session",
    )
    .await;

    let mut player = browser_join(&edge, session, "browser").await;

    // The greeting arrives as an event on the control stream.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut greeting = None;
    while tokio::time::Instant::now() < deadline {
        let (channel, from, payload) = player.next().await;
        match channel {
            CHANNEL_EVENTS => {
                greeting = Some((from, payload));
                break;
            }
            _ => continue, // ticks may interleave
        }
    }
    let (from, payload) = greeting.expect("greeting event within deadline");
    assert_eq!(from, player.client_id, "greeting is unicast");
    assert_eq!(
        payload,
        format!("welcome {}", player.client_id).into_bytes()
    );

    // Ordered inputs come back as datagram echoes, in order.
    for i in 0..3 {
        let input = format!("input-{i}");
        player.send_input(input.as_bytes());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut echo = None;
        while tokio::time::Instant::now() < deadline {
            let (channel, from, payload) = player.next().await;
            if channel == CHANNEL_STATE && from == player.client_id {
                echo = Some(payload);
                break;
            }
        }
        assert_eq!(echo.expect("echo within deadline"), input.into_bytes());
    }

    // An input sent on the events stream round-trips too: the echo arena
    // answers every input with a state unicast, whatever channel it came on.
    player.send_event(b"streamed input").await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut echo = None;
    while tokio::time::Instant::now() < deadline {
        let (channel, from, payload) = player.next().await;
        if channel == CHANNEL_STATE && from == player.client_id {
            echo = Some(payload);
            break;
        }
    }
    assert_eq!(
        echo.expect("streamed echo within deadline"),
        b"streamed input".to_vec()
    );

    // Broadcast ticks arrive as datagrams addressed to client 0.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut ticks: u32 = 0;
    while tokio::time::Instant::now() < deadline && ticks < 2 {
        let (channel, from, payload) = player.next().await;
        if channel == CHANNEL_STATE && from == 0 {
            let tick = u64::from_le_bytes(payload.try_into().expect("tick counter"));
            ticks += 1;
            assert!(tick > 0, "tick counters increase");
        }
    }
    assert!(ticks >= 2, "broadcast ticks observed");

    // Abrupt loss (the tab closes): the session's player count drops to 0.
    drop(player);
    poll_until(
        || async {
            backplane
                .sessions()
                .await
                .iter()
                .find(|s| s.id == session)
                .is_some_and(|s| s.player_count == 0)
        },
        5,
        "player count to drop after disconnect",
    )
    .await;

    edge.shutdown();
    edge.drain().await;
    backplane.shutdown();
    backplane.clone().drain().await;
}
