//! Client-edge integration tests: a real in-process hub (backplane + client
//! edge on ephemeral ports) driven by a real WebTransport client that pins
//! the hub's certificate hash — the same trust path a browser takes.
//!
//! Sessions are activated by scripted fake workers over real backplane
//! sockets (the pattern of `backplane_service.rs` tests); the flagship
//! browser-loop e2e with the real sample worker lives in
//! `crates/sample-worker/tests/client_edge_end_to_end.rs`.

use std::net::SocketAddr;
use std::time::Duration;

use backplane::{Frame, conn};
use hub::backplane_service::{Service as Backplane, ServiceHandle as BackplaneHandle, Timeouts};
use hub::client_edge::{Service as Edge, ServiceHandle as EdgeHandle};
use hub::config::{BackplaneConfig, ClientEdgeConfig};
use hub::registry::SessionSpec;
use protocol::Header;

/// Relaxed liveness: these tests exercise the edge, not the watchdog.
const SLOW: Timeouts = Timeouts {
    registration: Duration::from_secs(2),
    liveness: Duration::from_secs(60),
    pending: Duration::from_secs(30),
};

const CHANNEL_STATE: u8 = 0;
const CHANNEL_EVENTS: u8 = 1;

type Rd = tokio::net::tcp::OwnedReadHalf;
type Wr = tokio::net::tcp::OwnedWriteHalf;

struct Rig {
    backplane: BackplaneHandle,
    edge: EdgeHandle,
}

impl Rig {
    /// Shuts both services down (drain clones the owning handles).
    async fn stop(&self) {
        self.edge.shutdown();
        self.edge.drain().await;
        self.backplane.shutdown();
        self.backplane.clone().drain().await;
    }
}

async fn start(token: &str) -> Rig {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();
    let backplane_cfg = BackplaneConfig {
        bind: "127.0.0.1:0".into(),
        token: String::new(),
        max_sessions: 64,
    };
    let backplane = Backplane::bind_with_timeouts(&backplane_cfg, SLOW)
        .await
        .expect("bind backplane");
    let backplane = backplane.start();

    let edge_cfg = ClientEdgeConfig {
        bind: "127.0.0.1:0".into(),
        token: token.into(),
    };
    let edge = Edge::bind(&edge_cfg, backplane.clone())
        .await
        .expect("bind edge");
    let edge = edge.start();
    Rig { backplane, edge }
}

fn spec(max_players: u16) -> SessionSpec {
    SessionSpec {
        game_type: "echo-arena".into(),
        display_name: "Echo Arena".into(),
        max_players,
        payload_schema_version: 1,
    }
}

/// Creates a pending session and activates it with a fake worker claim.
async fn active_session(rig: &Rig, max_players: u16) -> (u32, Rd, Wr) {
    let spec = spec(max_players);
    let pending = rig
        .backplane
        .create_pending(spec.clone())
        .await
        .expect("pending");
    let (rd, wr, id) = fake_worker(rig.backplane.local_addr(), pending, &spec).await;
    assert_eq!(id, pending, "worker claims the pending session");
    (id, rd, wr)
}

/// Connects, registers, claims `claim`; returns the link + session id.
async fn fake_worker(addr: SocketAddr, claim: u32, spec: &SessionSpec) -> (Rd, Wr, u32) {
    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (mut rd, mut wr) = stream.into_split();
    conn::write_frame(
        &mut wr,
        &Frame::Hello {
            token: String::new(),
        },
    )
    .await
    .expect("Hello");
    conn::write_frame(
        &mut wr,
        &Frame::Register {
            claimed_id: claim,
            game_type: spec.game_type.clone(),
            display_name: spec.display_name.clone(),
            max_players: spec.max_players,
            payload_schema_version: spec.payload_schema_version,
        },
    )
    .await
    .expect("Register");
    let registered = tokio::time::timeout(Duration::from_secs(2), conn::read_frame(&mut rd))
        .await
        .expect("Registered in time")
        .expect("Registered parses");
    match registered {
        Frame::Registered { session_id } => (rd, wr, session_id),
        other => panic!("expected Registered, got {other:?}"),
    }
}

async fn next_frame(rd: &mut Rd) -> Frame {
    tokio::time::timeout(Duration::from_secs(3), conn::read_frame(rd))
        .await
        .expect("frame in time")
        .expect("frame parses")
}

fn packet(channel: u8, session: u32, client: u32, payload: &[u8]) -> Vec<u8> {
    Header::new(channel, session, client, 1, payload.len() as u32)
        .encode_packet(payload)
        .expect("packet")
}

/// A WebTransport test client joined to a session.
struct Client {
    conn: wtransport::Connection,
    writer: wtransport::SendStream,
    reader: wtransport::RecvStream,
    buf: Vec<u8>,
    session: u32,
    client_id: u32,
}

enum Join {
    Ok(Client),
    Rejected(String),
}

/// Connects with the hub's pinned hash and performs the handshake.
async fn join(edge: &EdgeHandle, session: u32, name: &str, token: &str) -> Join {
    let hash = edge.cert_sha256();
    let config = wtransport::ClientConfig::builder()
        .with_bind_default()
        .with_server_certificate_hashes([wtransport::tls::Sha256Digest::new(hash)])
        .build();
    let endpoint = wtransport::Endpoint::client(config).expect("client endpoint");
    let addr = edge.edge_addr();
    let conn = tokio::time::timeout(
        Duration::from_secs(5),
        endpoint.connect(format!("https://127.0.0.1:{}", addr.port())),
    )
    .await
    .expect("connect in time")
    .expect("connects");

    let opening = tokio::time::timeout(Duration::from_secs(3), conn.open_bi())
        .await
        .expect("open in time")
        .expect("opens stream");
    let (mut writer, mut reader) = tokio::time::timeout(Duration::from_secs(3), opening)
        .await
        .expect("establish in time")
        .expect("stream establishes");

    let connect = protocol::Connect {
        token: token.into(),
        session_id: session,
        name: name.into(),
    };
    writer
        .write_all(&connect.encode().expect("encode"))
        .await
        .expect("send Connect");

    // Reply: kind byte first, then the frame's remaining bytes.
    let mut kind = [0u8; 1];
    read_exact(&mut reader, &mut kind).await;
    match kind[0] {
        protocol::KIND_ACCEPT => {
            let mut rest = [0u8; 5];
            read_exact(&mut reader, &mut rest).await;
            let mut frame = kind.to_vec();
            frame.extend_from_slice(&rest);
            let accept = protocol::Accept::decode(&frame).expect("accept parses");
            Join::Ok(Client {
                conn,
                writer,
                reader,
                buf: Vec::new(),
                session,
                client_id: accept.client_id,
            })
        }
        protocol::KIND_REJECT => {
            let mut len = [0u8; 3];
            read_exact(&mut reader, &mut len).await;
            let reason_len = u16::from_le_bytes([len[1], len[2]]) as usize;
            let mut reason = vec![0u8; reason_len];
            read_exact(&mut reader, &mut reason).await;
            Join::Rejected(String::from_utf8(reason).expect("reason is UTF-8"))
        }
        other => panic!("unexpected reply kind {other:#x}"),
    }
}

async fn read_exact(stream: &mut wtransport::RecvStream, buf: &mut [u8]) {
    tokio::time::timeout(Duration::from_secs(3), stream.read_exact(buf))
        .await
        .expect("read in time")
        .expect("stream has the bytes");
}

/// A peer that completes QUIC but deliberately never starts WebTransport.
/// The endpoint must stay alive alongside the connection.
struct StalledQuic {
    _endpoint: quinn::Endpoint,
    _connection: quinn::Connection,
}

async fn stall_before_webtransport(edge: &EdgeHandle) -> StalledQuic {
    let verifier =
        wtransport::tls::client::ServerHashVerification::new([wtransport::tls::Sha256Digest::new(
            edge.cert_sha256(),
        )]);
    let tls = wtransport::tls::client::build_default_tls_config(
        std::sync::Arc::new(wtransport::tls::build_native_cert_store()),
        Some(std::sync::Arc::new(verifier)),
    );
    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .expect("WebTransport TLS is QUIC-compatible");
    let config = quinn::ClientConfig::new(std::sync::Arc::new(crypto));
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().expect("bind address"))
        .expect("raw QUIC client endpoint");
    endpoint.set_default_client_config(config);
    let connection = tokio::time::timeout(
        Duration::from_secs(3),
        endpoint
            .connect(edge.edge_addr(), "localhost")
            .expect("raw QUIC connect"),
    )
    .await
    .expect("QUIC connection completes before WebTransport stalls")
    .expect("QUIC connection");
    StalledQuic {
        _endpoint: endpoint,
        _connection: connection,
    }
}

impl Client {
    /// Sends a state-channel input as a datagram.
    async fn send_input(&self, payload: &[u8]) {
        let packet = packet(CHANNEL_STATE, self.session, self.client_id, payload);
        self.conn.send_datagram(packet).expect("datagram sent");
    }

    /// Receives the next state datagram, parsed as (client_id, payload).
    async fn recv_datagram(&self) -> (u32, Vec<u8>) {
        let datagram = tokio::time::timeout(Duration::from_secs(3), self.conn.receive_datagram())
            .await
            .expect("datagram in time")
            .expect("datagram arrives");
        let (header, payload) = Header::split_packet(&datagram).expect("packet parses");
        (header.client_id, payload.to_vec())
    }

    /// Receives the next event packet from the control stream.
    async fn recv_event(&mut self) -> Vec<u8> {
        loop {
            if self.buf.len() >= protocol::BASE_HEADER_SIZE as usize
                && let Ok((header, payload)) = Header::split_packet(&self.buf)
            {
                let consumed = header.packet_len();
                let payload = payload.to_vec();
                self.buf.drain(..consumed);
                return payload;
            }
            let mut chunk = [0u8; 4096];
            let read = tokio::time::timeout(Duration::from_secs(3), self.reader.read(&mut chunk))
                .await
                .expect("stream read in time");
            let n = match read {
                Ok(Some(n)) => n,
                Ok(None) => panic!("stream ended before an event arrived"),
                Err(_) => panic!("stream read failed"),
            };
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    async fn send_event(&mut self, payload: &[u8]) {
        let packet = packet(CHANNEL_EVENTS, self.session, self.client_id, payload);
        self.writer.write_all(&packet).await.expect("event sent");
    }
}

/// (client-edge: Connect handshake) Join an active session; the worker sees
/// the attach and its greeting reaches the client over the events stream.
#[tokio::test]
async fn handshake_accepts_and_greets_over_the_stream() {
    let rig = start("").await;
    let (session, mut wrd, mut wwr) = active_session(&rig, 8).await;

    let mut client = match join(&rig.edge, session, "p1", "").await {
        Join::Ok(client) => client,
        Join::Rejected(reason) => panic!("expected accept, got: {reason}"),
    };

    // The worker observes the attach.
    match next_frame(&mut wrd).await {
        Frame::ClientAttached { client_id } => assert_eq!(client_id, client.client_id),
        other => panic!("expected ClientAttached, got {other:?}"),
    }

    // Worker greets on the events channel; it lands on the control stream.
    let greeting = packet(CHANNEL_EVENTS, session, client.client_id, b"welcome 1");
    conn::write_frame(&mut wwr, &Frame::ToClient { packet: greeting })
        .await
        .expect("greeting");
    assert_eq!(client.recv_event().await, b"welcome 1".to_vec());

    rig.stop().await;
}

/// (client-edge: Token gate) A wrong token is rejected without attaching.
#[tokio::test]
async fn wrong_token_is_rejected() {
    let rig = start("s3cret").await;
    let (session, mut wrd, _wwr) = active_session(&rig, 8).await;

    match join(&rig.edge, session, "p1", "nope").await {
        Join::Ok(_) => panic!("wrong token must not join"),
        Join::Rejected(reason) => assert_eq!(reason, "wrong token"),
    }

    // No attach reached the worker: silence for a beat, not a timeout panic.
    let silence =
        tokio::time::timeout(Duration::from_millis(500), conn::read_frame(&mut wrd)).await;
    assert!(silence.is_err(), "worker must see nothing, got {silence:?}");
}

/// (client-edge: Session join rules) Unknown, pending, and full sessions
/// are rejected with reasons; a rejected client never attaches.
#[tokio::test]
async fn join_rules_reject_with_reasons() {
    let rig = start("").await;

    // Unknown session.
    match join(&rig.edge, 4242, "p", "").await {
        Join::Rejected(reason) => assert!(reason.contains("4242"), "reason: {reason}"),
        Join::Ok(_) => panic!("unknown session must not join"),
    }

    // Pending (never claimed by a worker): not joinable.
    let pending = rig
        .backplane
        .create_pending(spec(8))
        .await
        .expect("pending");
    match join(&rig.edge, pending, "p", "").await {
        Join::Rejected(reason) => assert!(reason.contains("not accepting"), "reason: {reason}"),
        Join::Ok(_) => panic!("pending session must not join"),
    }

    // Full session: capacity 1, one client already in.
    let (session, _wrd, _wwr) = active_session(&rig, 1).await;
    let _first = match join(&rig.edge, session, "p1", "").await {
        Join::Ok(client) => client,
        Join::Rejected(reason) => panic!("first client must join: {reason}"),
    };
    match join(&rig.edge, session, "p2", "").await {
        Join::Rejected(reason) => assert!(reason.contains("full"), "reason: {reason}"),
        Join::Ok(_) => panic!("full session must not join"),
    }

    rig.stop().await;
}

/// (client-edge: Connect handshake) Anything but a Connect as the first
/// stream frame closes the connection without touching session state.
#[tokio::test]
async fn garbage_first_frame_closes_the_connection() {
    let rig = start("").await;
    let (session, _wrd, _wwr) = active_session(&rig, 8).await;

    let hash = rig.edge.cert_sha256();
    let config = wtransport::ClientConfig::builder()
        .with_bind_default()
        .with_server_certificate_hashes([wtransport::tls::Sha256Digest::new(hash)])
        .build();
    let endpoint = wtransport::Endpoint::client(config).expect("client endpoint");
    let conn = endpoint
        .connect(format!("https://127.0.0.1:{}", rig.edge.edge_addr().port()))
        .await
        .expect("connects");
    let (mut writer, _reader) = conn.open_bi().await.expect("open").await.expect("stream");
    writer
        .write_all(b"definitely not a Connect frame")
        .await
        .expect("junk");
    drop(writer);

    // The connection is closed by the hub.
    let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed())
        .await
        .expect("connection closed in time");

    // And no attach reached the worker: the session still has zero players.
    let info = rig
        .backplane
        .sessions()
        .await
        .into_iter()
        .find(|s| s.id == session)
        .expect("session still exists");
    assert_eq!(info.player_count, 0, "no state change from garbage");

    rig.stop().await;
}

/// (client-edge: Connection admission isolation) A peer stalled before the
/// WebTransport session request must not serialize every other peer behind it.
#[tokio::test]
async fn stalled_negotiation_does_not_block_joins() {
    let rig = start("").await;
    let (session, mut wrd, _wwr) = active_session(&rig, 8).await;

    let stalled = stall_before_webtransport(&rig.edge).await;
    // Let the edge accept the raw QUIC peer and enter its negotiation wait.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = match join(&rig.edge, session, "healthy", "").await {
        Join::Ok(client) => client,
        Join::Rejected(reason) => panic!("healthy client must join: {reason}"),
    };
    match next_frame(&mut wrd).await {
        Frame::ClientAttached { client_id } => assert_eq!(client_id, client.client_id),
        other => panic!("expected ClientAttached, got {other:?}"),
    }

    tokio::time::timeout(Duration::from_secs(5), rig.stop())
        .await
        .expect("edge drains with the stalled peer still present");
    drop(stalled);
}

/// (client-edge: State relay over datagrams) Inputs flow client → worker as
/// FromClient and worker unicast/broadcast flow back as datagrams.
#[tokio::test]
async fn state_relays_over_datagrams_both_ways() {
    let rig = start("").await;
    let (session, mut wrd, mut wwr) = active_session(&rig, 8).await;

    let client = match join(&rig.edge, session, "p1", "").await {
        Join::Ok(client) => client,
        Join::Rejected(reason) => panic!("expected accept, got: {reason}"),
    };
    match next_frame(&mut wrd).await {
        Frame::ClientAttached { .. } => {}
        other => panic!("expected ClientAttached, got {other:?}"),
    }

    // Client input arrives at the worker as a routed packet.
    client.send_input(b"hello").await;
    let echo_source = packet(CHANNEL_STATE, session, client.client_id, b"hello");
    match next_frame(&mut wrd).await {
        Frame::FromClient { packet } => assert_eq!(packet, echo_source),
        other => panic!("expected FromClient, got {other:?}"),
    }

    // Worker unicast reaches its client as a datagram.
    conn::write_frame(
        &mut wwr,
        &Frame::ToClient {
            packet: packet(CHANNEL_STATE, session, client.client_id, b"echo"),
        },
    )
    .await
    .expect("echo");
    assert_eq!(
        client.recv_datagram().await,
        (client.client_id, b"echo".to_vec())
    );

    // And so does a broadcast.
    conn::write_frame(
        &mut wwr,
        &Frame::Broadcast {
            packet: packet(CHANNEL_STATE, session, 0, b"tick"),
        },
    )
    .await
    .expect("broadcast");
    assert_eq!(client.recv_datagram().await, (0, b"tick".to_vec()));

    rig.stop().await;
}

/// (client-edge: Event relay over the control stream) Events flow in order
/// in both directions; a client that stops reading its stream stalls only
/// itself — the other client's traffic keeps flowing.
#[tokio::test]
async fn slow_reader_stalls_only_itself() {
    let rig = start("").await;
    let (session, mut wrd, mut wwr) = active_session(&rig, 8).await;

    let mut fast = match join(&rig.edge, session, "fast", "").await {
        Join::Ok(client) => client,
        Join::Rejected(reason) => panic!("fast must join: {reason}"),
    };
    let slow = match join(&rig.edge, session, "slow", "").await {
        Join::Ok(client) => client,
        Join::Rejected(reason) => panic!("slow must join: {reason}"),
    };
    for _ in 0..2 {
        match next_frame(&mut wrd).await {
            Frame::ClientAttached { .. } => {}
            other => panic!("expected ClientAttached, got {other:?}"),
        }
    }

    // Bury the slow client's stream: enough events to fill QUIC flow-control
    // windows several times over. `slow` never reads them.
    let big = vec![0x42u8; 4096];
    for _ in 0..256 {
        conn::write_frame(
            &mut wwr,
            &Frame::ToClient {
                packet: packet(CHANNEL_EVENTS, session, slow.client_id, &big),
            },
        )
        .await
        .expect("bury slow");
    }

    // The fast client still round-trips while slow is stalled.
    for i in 0..5 {
        let payload = format!("ping {i}");
        fast.send_input(payload.as_bytes()).await;
        let expected = packet(CHANNEL_STATE, session, fast.client_id, payload.as_bytes());
        match next_frame(&mut wrd).await {
            Frame::FromClient { packet } => assert_eq!(packet, expected),
            other => panic!("expected FromClient, got {other:?}"),
        }
        conn::write_frame(
            &mut wwr,
            &Frame::ToClient {
                packet: packet(CHANNEL_STATE, session, fast.client_id, payload.as_bytes()),
            },
        )
        .await
        .expect("pong");
        let (from, got) = fast.recv_datagram().await;
        assert_eq!(from, fast.client_id);
        assert_eq!(got, payload.into_bytes());
    }

    // The slow client's datagram path works even with a buried stream.
    conn::write_frame(
        &mut wwr,
        &Frame::ToClient {
            packet: packet(CHANNEL_STATE, session, slow.client_id, b"still here"),
        },
    )
    .await
    .expect("datagram to slow");
    let (from, got) = slow.recv_datagram().await;
    assert_eq!(from, slow.client_id);
    assert_eq!(got, b"still here".to_vec());

    // Client → hub events on the stream reach the worker, in order.
    fast.send_event(b"event one").await;
    fast.send_event(b"event two").await;
    for expected in [b"event one".to_vec(), b"event two".to_vec()] {
        match next_frame(&mut wrd).await {
            Frame::FromClient { packet } => {
                let (_, payload) = Header::split_packet(&packet).expect("packet");
                assert_eq!(payload, expected);
            }
            other => panic!("expected FromClient, got {other:?}"),
        }
    }

    rig.stop().await;
}

/// (client-edge: Inbound stream bounds) Invalid incomplete data cannot grow
/// the relay buffer forever or affect another client's session.
#[tokio::test]
async fn unframed_stream_flood_closes_only_that_client() {
    let rig = start("").await;
    let (session, mut wrd, _wwr) = active_session(&rig, 8).await;

    let mut bad = match join(&rig.edge, session, "bad", "").await {
        Join::Ok(client) => client,
        Join::Rejected(reason) => panic!("bad client must join: {reason}"),
    };
    let healthy = match join(&rig.edge, session, "healthy", "").await {
        Join::Ok(client) => client,
        Join::Rejected(reason) => panic!("healthy client must join: {reason}"),
    };
    for _ in 0..2 {
        match next_frame(&mut wrd).await {
            Frame::ClientAttached { .. } => {}
            other => panic!("expected ClientAttached, got {other:?}"),
        }
    }

    let garbage = vec![0u8; 256 * 1024 + 1];
    tokio::time::timeout(Duration::from_secs(5), bad.writer.write_all(&garbage))
        .await
        .expect("flood write completes")
        .expect("flood reaches the hub");
    tokio::time::timeout(Duration::from_secs(3), bad.conn.closed())
        .await
        .expect("over-limit connection closes");
    match next_frame(&mut wrd).await {
        Frame::ClientDetached { client_id } => assert_eq!(client_id, bad.client_id),
        other => panic!("expected ClientDetached, got {other:?}"),
    }

    healthy.send_input(b"still alive").await;
    match next_frame(&mut wrd).await {
        Frame::FromClient { packet } => {
            let (_, payload) = Header::split_packet(&packet).expect("packet");
            assert_eq!(payload, b"still alive");
        }
        other => panic!("expected healthy FromClient, got {other:?}"),
    }
    assert_eq!(poll_player_count(&rig, session, 1).await, 1);

    rig.stop().await;
}

/// (client-edge: Connection lifecycle) Losing the client connection detaches
/// the client: the worker sees it go and the player count drops.
#[tokio::test]
async fn disconnect_detaches_the_client() {
    let rig = start("").await;
    let (session, mut wrd, _wwr) = active_session(&rig, 8).await;

    let client = match join(&rig.edge, session, "p1", "").await {
        Join::Ok(client) => client,
        Join::Rejected(reason) => panic!("expected accept, got: {reason}"),
    };
    let client_id = client.client_id;
    match next_frame(&mut wrd).await {
        Frame::ClientAttached { client_id: id } => assert_eq!(id, client_id),
        other => panic!("expected ClientAttached, got {other:?}"),
    }

    // Abrupt loss: drop the whole transport.
    drop(client);

    match next_frame(&mut wrd).await {
        Frame::ClientDetached { client_id: id } => assert_eq!(id, client_id),
        other => panic!("expected ClientDetached, got {other:?}"),
    }
    let count = poll_player_count(&rig, session, 0).await;
    assert_eq!(count, 0);

    rig.stop().await;
}

async fn poll_player_count(rig: &Rig, session: u32, want: u16) -> u16 {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let count = rig
            .backplane
            .sessions()
            .await
            .into_iter()
            .find(|s| s.id == session)
            .map(|s| s.player_count)
            .unwrap_or(u16::MAX);
        if count == want {
            return count;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("player count never reached {want}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// (client-edge: Connection lifecycle) Hub shutdown closes connected
/// clients within the drain grace period.
#[tokio::test]
async fn shutdown_closes_connected_clients() {
    let rig = start("").await;
    let (session, _wrd, _wwr) = active_session(&rig, 8).await;

    let client = match join(&rig.edge, session, "p1", "").await {
        Join::Ok(client) => client,
        Join::Rejected(reason) => panic!("expected accept, got: {reason}"),
    };

    rig.edge.shutdown();
    tokio::time::timeout(Duration::from_secs(5), client.conn.closed())
        .await
        .expect("connection closed within grace");
    rig.edge.drain().await;
    rig.backplane.shutdown();
    rig.backplane.drain().await;
}

/// (client-edge: WebTransport endpoint) The certificate rotates per boot.
#[tokio::test]
async fn certificate_rotates_per_boot() {
    let rig_a = start("").await;
    let rig_b = start("").await;
    assert_ne!(rig_a.edge.cert_sha256(), rig_b.edge.cert_sha256());

    for rig in [&rig_a, &rig_b] {
        rig.stop().await;
    }
}
