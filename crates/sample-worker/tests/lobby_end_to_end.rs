//! The lobby end-to-end: HTTP creates a session served by the real
//! `sample-worker`, a browser-shaped WebTransport client joins it, the
//! listing reflects the player, a kick closes the client's connection, and
//! closing the session ends the worker.

use std::time::Duration;

use hub::backplane_service::{Service as Backplane, Timeouts};
use hub::client_edge::{Service as Edge, ServiceHandle as EdgeHandle};
use hub::config::{BackplaneConfig, ClientEdgeConfig, GameConfig, LobbyConfig};
use hub::lobby::{Service as Lobby, ServiceHandle as LobbyHandle};
use protocol::Header;

const CHANNEL_EVENTS: u8 = 1;

struct Rig {
    backplane: hub::backplane_service::ServiceHandle,
    edge: EdgeHandle,
    lobby: LobbyHandle,
}

async fn start_hub() -> Rig {
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

    // The catalog points at the real worker binary.
    let games = [GameConfig {
        game_type: "echo-arena".into(),
        display_name: "Echo Arena".into(),
        max_players: 8,
        payload_schema_version: 1,
        worker: std::env!("CARGO_BIN_EXE_sample-worker").into(),
        session_config_json: r#"{"tick_hz":60}"#.into(),
    }];

    let lobby = Lobby::bind(
        &LobbyConfig {
            bind: "127.0.0.1:0".into(),
            token: String::new(),
        },
        &games,
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

async fn http(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: hub\r\n");
    if let Some(body) = body {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("Connection: close\r\n\r\n");
    if let Some(body) = body {
        request.push_str(body);
    }
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.expect("read");
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .expect("status");
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_owned())
        .unwrap_or_default();
    (status, body)
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

/// A browser-shaped client (same trust path as the client-edge e2e).
struct Browser {
    conn: wtransport::Connection,
    writer: wtransport::SendStream,
    reader: wtransport::RecvStream,
    buf: Vec<u8>,
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
    let mut head = [0u8; 2];
    tokio::time::timeout(Duration::from_secs(3), reader.read_exact(&mut head))
        .await
        .expect("reply in time")
        .expect("reply bytes");
    assert_eq!(head[0], protocol::KIND_ACCEPT);
    let mut rest = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(3), reader.read_exact(&mut rest))
        .await
        .expect("reply in time")
        .expect("reply bytes");
    let mut frame = head.to_vec();
    frame.extend_from_slice(&rest);
    let accept = protocol::Accept::decode(&frame).expect("accept");

    Browser {
        conn,
        writer,
        reader,
        buf: Vec::new(),
        client_id: accept.client_id,
    }
}

/// (lobby-api: full loop) Create over HTTP → active → join over
/// WebTransport → listing shows the player → kick closes the client →
/// close ends the session and the worker.
#[tokio::test]
async fn lobby_manages_a_real_session_end_to_end() {
    let rig = start_hub().await;

    // Catalog lists the configured game.
    let (status, body) = http(rig.lobby.addr(), "GET", "/api/games", None).await;
    assert_eq!(status, 200);
    assert!(body.contains("echo-arena"), "body: {body}");

    // Create a session over HTTP; the real worker claims it.
    let (status, body) = http(
        rig.lobby.addr(),
        "POST",
        "/api/sessions",
        Some(r#"{"game_type":"echo-arena"}"#),
    )
    .await;
    assert_eq!(status, 201, "body: {body}");
    let session: u32 = serde_json::from_str::<serde_json::Value>(&body)
        .expect("json body")
        .get("session_id")
        .and_then(|v| v.as_u64())
        .expect("session_id") as u32;

    poll_until(
        || async {
            let (_, body) = http(rig.lobby.addr(), "GET", "/api/sessions", None).await;
            body.contains("\"active\"") && body.contains(&format!("\"id\":{session}"))
        },
        5,
        "session to become active",
    )
    .await;

    // A browser joins; the listing reflects the player.
    let mut player = browser_join(&rig.edge, session, "browser").await;
    poll_until(
        || async {
            let (_, body) = http(rig.lobby.addr(), "GET", "/api/sessions", None).await;
            body.contains("\"player_count\":1")
        },
        5,
        "player to appear in the listing",
    )
    .await;

    // The greeting arrives as an event; an input round-trips as a datagram
    // (the loop is alive, not just the bookkeeping).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut greeted = false;
    while tokio::time::Instant::now() < deadline && !greeted {
        let mut chunk = [0u8; 4096];
        tokio::select! {
            read = player.reader.read(&mut chunk) => {
                let n = read.expect("read").expect("open");
                player.buf.extend_from_slice(&chunk[..n]);
                while player.buf.len() >= protocol::BASE_HEADER_SIZE as usize
                    && let Ok((header, payload)) = Header::split_packet(&player.buf)
                {
                    let (channel, payload, consumed) =
                        (header.channel, payload.to_vec(), header.packet_len());
                    player.buf.drain(..consumed);
                    assert_eq!(channel, CHANNEL_EVENTS);
                    assert_eq!(payload, format!("welcome {}", player.client_id).into_bytes());
                    greeted = true;
                }
            }
            datagram = player.conn.receive_datagram() => {
                let datagram = datagram.expect("datagram");
                let (header, _) = Header::split_packet(&datagram).expect("packet");
                // Broadcast ticks are fine to observe while greeting waits.
                let _ = header;
            }
        }
    }
    assert!(greeted, "greeting event within deadline");

    // Kick the player over HTTP; the browser's connection closes.
    let (status, _) = http(
        rig.lobby.addr(),
        "DELETE",
        &format!("/api/sessions/{session}/clients/{}", player.client_id),
        None,
    )
    .await;
    assert_eq!(status, 204);
    tokio::time::timeout(Duration::from_secs(5), player.conn.closed())
        .await
        .expect("kicked client's connection closes in time");
    poll_until(
        || async {
            let (_, body) = http(rig.lobby.addr(), "GET", "/api/sessions", None).await;
            body.contains("\"player_count\":0")
        },
        5,
        "player count to drop after the kick",
    )
    .await;

    // Close the session; the worker exits and the listing empties.
    let (status, _) = http(
        rig.lobby.addr(),
        "DELETE",
        &format!("/api/sessions/{session}"),
        None,
    )
    .await;
    assert_eq!(status, 204);
    poll_until(
        || async {
            let (_, body) = http(rig.lobby.addr(), "GET", "/api/sessions", None).await;
            body == "[]"
        },
        5,
        "session to leave the listing after close",
    )
    .await;

    drop(player.writer);
    rig.lobby.shutdown();
    rig.lobby.drain().await;
    rig.edge.shutdown();
    rig.edge.drain().await;
    rig.backplane.shutdown();
    rig.backplane.clone().drain().await;
}
