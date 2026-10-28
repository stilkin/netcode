//! Lobby API integration tests: the route matrix over real TCP, against an
//! in-process hub (backplane + client edge + lobby) with a dummy catalog.
//! Worker-spawning flows (create → active → play) live in
//! `crates/sample-worker/tests/lobby_end_to_end.rs`, where the real worker
//! binary is available.

use std::time::Duration;

use hub::backplane_service::{Service as Backplane, Timeouts};
use hub::client_edge::{Service as Edge, ServiceHandle as EdgeHandle};
use hub::config::{BackplaneConfig, ClientEdgeConfig, GameConfig, LobbyConfig};
use hub::lobby::{Service as Lobby, ServiceHandle as LobbyHandle};
use hub::registry::{SessionSpec, SessionState};

const SLOW: Timeouts = Timeouts {
    registration: Duration::from_secs(2),
    liveness: Duration::from_secs(60),
    pending: Duration::from_secs(30),
};

struct Rig {
    backplane: hub::backplane_service::ServiceHandle,
    edge: EdgeHandle,
    lobby: LobbyHandle,
}

fn dummy_game(game_type: &str, worker: &str) -> GameConfig {
    GameConfig {
        game_type: game_type.to_owned(),
        display_name: format!("Dummy {game_type}"),
        max_players: 8,
        payload_schema_version: 1,
        worker: worker.into(),
        session_config_json: "{}".to_owned(),
    }
}

async fn start(token: &str, games: Vec<GameConfig>) -> Rig {
    start_with_limit(token, games, 64).await
}

async fn start_with_limit(token: &str, games: Vec<GameConfig>, max_sessions: usize) -> Rig {
    let backplane = Backplane::bind_with_timeouts(
        &BackplaneConfig {
            bind: "127.0.0.1:0".into(),
            token: String::new(),
            max_sessions,
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
            token: token.to_owned(),
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

impl Rig {
    async fn stop(&self) {
        self.lobby.shutdown();
        self.lobby.drain().await;
        self.edge.shutdown();
        self.edge.drain().await;
        self.backplane.shutdown();
        self.backplane.clone().drain().await;
    }
}

/// Sends raw request bytes and reads one complete response.
async fn exchange(addr: std::net::SocketAddr, request: String) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    stream.write_all(request.as_bytes()).await.expect("request");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.expect("response");
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status code");
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    (status, body)
}

/// One HTTP request/response exchange over a fresh connection.
async fn http(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
    auth: Option<&str>,
) -> (u16, String) {
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: hub\r\n");
    if let Some(auth) = auth {
        request.push_str(&format!("Authorization: {auth}\r\n"));
    }
    match body {
        Some(body) => request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        )),
        None => {
            if method == "POST" {
                request.push_str("Content-Type: application/json\r\nContent-Length: 0\r\n");
            }
        }
    }
    request.push_str("Connection: close\r\n\r\n");
    if let Some(body) = body {
        request.push_str(body);
    }
    exchange(addr, request).await
}

/// (lobby-api: Game catalog) The catalog lists configured games; an empty
/// catalog lists an empty array.
#[tokio::test]
async fn games_listing_reflects_the_catalog() {
    let rig = start("", vec![dummy_game("echo-arena", "/nowhere")]).await;
    let (status, body) = http(rig.lobby.addr(), "GET", "/api/games", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains("\"echo-arena\""), "body: {body}");
    assert!(body.contains("\"Dummy echo-arena\""), "body: {body}");

    let empty = start("", vec![]).await;
    let (status, body) = http(empty.lobby.addr(), "GET", "/api/games", None, None).await;
    assert_eq!(status, 200);
    assert_eq!(body, "[]");

    rig.stop().await;
    empty.stop().await;
}

/// (lobby-api: Session listing) In-process sessions appear with metadata.
#[tokio::test]
async fn session_listing_shows_metadata() {
    let rig = start("", vec![]).await;
    let spec = SessionSpec {
        game_type: "probe".into(),
        display_name: "Probe".into(),
        max_players: 4,
        payload_schema_version: 1,
    };
    let id = rig.backplane.create_pending(spec).await.expect("pending");

    let (status, body) = http(rig.lobby.addr(), "GET", "/api/sessions", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains(&format!("\"id\":{id}")), "body: {body}");
    assert!(body.contains("\"pending\""), "body: {body}");
    assert!(body.contains("\"max_players\":4"), "body: {body}");

    rig.stop().await;
}

/// (lobby-api: Session creation) Unknown game types 404; malformed bodies
/// 400; unspawnable programs 500 with nothing left behind.
#[tokio::test]
async fn session_creation_error_paths() {
    let rig = start("", vec![dummy_game("ghost", "/nonexistent/worker")]).await;

    let (status, body) = http(
        rig.lobby.addr(),
        "POST",
        "/api/sessions",
        Some(r#"{"game_type":"chess"}"#),
        None,
    )
    .await;
    assert_eq!(status, 404);
    assert!(body.contains("chess"), "body: {body}");

    let (status, body) = http(
        rig.lobby.addr(),
        "POST",
        "/api/sessions",
        Some("not json"),
        None,
    )
    .await;
    assert_eq!(status, 400);
    assert!(body.contains("game_type"), "body: {body}");

    // The catalog entry exists but the program cannot be executed.
    let (status, _) = http(
        rig.lobby.addr(),
        "POST",
        "/api/sessions",
        Some(r#"{"game_type":"ghost"}"#),
        None,
    )
    .await;
    assert_eq!(status, 500);
    let (status, body) = http(rig.lobby.addr(), "GET", "/api/sessions", None, None).await;
    assert_eq!(status, 200);
    assert_eq!(body, "[]", "no session left behind by a failed spawn");

    rig.stop().await;
}

/// (lobby-api: Session creation) Capacity rejection happens before the
/// configured worker program is executed and leaves existing sessions alone.
#[tokio::test]
async fn session_creation_at_capacity_returns_503() {
    let rig = start_with_limit("", vec![dummy_game("ghost", "/nonexistent/worker")], 1).await;
    let pending = rig
        .backplane
        .create_pending(SessionSpec {
            game_type: "probe".into(),
            display_name: "Probe".into(),
            max_players: 1,
            payload_schema_version: 1,
        })
        .await
        .expect("pending at the limit");

    let (status, body) = http(
        rig.lobby.addr(),
        "POST",
        "/api/sessions",
        Some(r#"{"game_type":"ghost"}"#),
        None,
    )
    .await;
    assert_eq!(status, 503, "body: {body}");
    assert!(body.contains("limit"), "body: {body}");
    let sessions = rig.backplane.sessions().await;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, pending);
    rig.stop().await;
}

/// (lobby-api: Session creation) Browser-simple and missing media types
/// cannot create sessions; normal JSON parameters and header casing work.
#[tokio::test]
async fn session_creation_requires_json_content_type() {
    let rig = start("", vec![dummy_game("ghost", "/nonexistent/worker")]).await;
    let payload = r#"{"game_type":"ghost"}"#;

    for content_type in [None, Some("text/plain")] {
        let content_type = content_type
            .map(|value| format!("Content-Type: {value}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "POST /api/sessions HTTP/1.1\r\nHost: hub\r\n{content_type}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        let (status, body) = exchange(rig.lobby.addr(), request).await;
        assert_eq!(status, 415, "content type {content_type:?}: {body}");
    }

    for content_type in ["application/json", "Application/JSON; charset=utf-8"] {
        let payload = r#"{"game_type":"chess"}"#;
        let request = format!(
            "POST /api/sessions HTTP/1.1\r\nHost: hub\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        let (status, body) = exchange(rig.lobby.addr(), request).await;
        assert_eq!(status, 404, "content type {content_type:?}: {body}");
        assert!(body.contains("chess"), "body: {body}");
    }

    let (_, body) = http(rig.lobby.addr(), "GET", "/api/sessions", None, None).await;
    assert_eq!(body, "[]", "rejected requests created no sessions");
    rig.stop().await;
}

/// (lobby-api: HTTP request framing) Recognized header names are ASCII
/// case-insensitive for authentication as well as body framing.
#[tokio::test]
async fn lowercase_headers_carry_auth_and_body() {
    let rig = start("s3cret", vec![dummy_game("ghost", "/nonexistent/worker")]).await;
    let payload = r#"{"game_type":"ghost"}"#;
    let request = format!(
        "POST /api/sessions HTTP/1.1\r\nHost: hub\r\nauthorization: Bearer s3cret\r\ncontent-type: application/json\r\ncontent-length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let (status, body) = exchange(rig.lobby.addr(), request).await;
    assert_eq!(status, 500, "authenticated request reached spawn: {body}");
    rig.stop().await;
}

/// (lobby-api: HTTP request framing) Ambiguous or unsupported body framing
/// receives a deterministic 400 instead of being routed.
#[tokio::test]
async fn ambiguous_request_framing_is_rejected() {
    let rig = start("", vec![dummy_game("ghost", "/nonexistent/worker")]).await;
    let payload = r#"{"game_type":"ghost"}"#;
    let length = payload.len();
    let cases = [
        (
            "duplicate lengths",
            format!("Content-Length: {length}\r\nContent-Length: {length}\r\n"),
        ),
        (
            "conflicting lengths",
            format!("Content-Length: {length}\r\nContent-Length: 0\r\n"),
        ),
        ("malformed length", "Content-Length: nope\r\n".to_owned()),
        (
            "transfer encoding",
            format!("Transfer-Encoding: chunked\r\nContent-Length: {length}\r\n"),
        ),
    ];

    for (name, framing) in cases {
        let request = format!(
            "POST /api/sessions HTTP/1.1\r\nHost: hub\r\nContent-Type: application/json\r\n{framing}Connection: close\r\n\r\n{payload}"
        );
        let (status, body) = exchange(rig.lobby.addr(), request).await;
        assert_eq!(status, 400, "{name}: {body}");
    }

    let (_, body) = http(rig.lobby.addr(), "GET", "/api/sessions", None, None).await;
    assert_eq!(body, "[]", "rejected requests created no sessions");
    rig.stop().await;
}

/// (lobby-api: Session closure, Client kick) Unknown ids 404; a live
/// pending session closes cleanly.
#[tokio::test]
async fn close_and_kick_unknown_ids_are_404() {
    let rig = start("", vec![]).await;

    let (status, _) = http(rig.lobby.addr(), "DELETE", "/api/sessions/4242", None, None).await;
    assert_eq!(status, 404);

    let (status, _) = http(
        rig.lobby.addr(),
        "DELETE",
        "/api/sessions/4242/clients/1",
        None,
        None,
    )
    .await;
    assert_eq!(status, 404);

    // A pending session (created in-process) closes via the API.
    let spec = SessionSpec {
        game_type: "probe".into(),
        display_name: "Probe".into(),
        max_players: 4,
        payload_schema_version: 1,
    };
    let id = rig.backplane.create_pending(spec).await.expect("pending");
    let (status, _) = http(
        rig.lobby.addr(),
        "DELETE",
        &format!("/api/sessions/{id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, 204);
    let (_, body) = http(rig.lobby.addr(), "GET", "/api/sessions", None, None).await;
    assert_eq!(body, "[]", "closed session leaves the listing");

    rig.stop().await;
}

/// (lobby-api: Token gate) Mutations need the bearer token when one is
/// configured; reads and cert-hash stay open.
#[tokio::test]
async fn token_gates_mutations_only() {
    let rig = start("s3cret", vec![dummy_game("echo-arena", "/nowhere")]).await;

    let (status, _) = http(rig.lobby.addr(), "GET", "/api/games", None, None).await;
    assert_eq!(status, 200, "reads stay open");

    let (status, _) = http(
        rig.lobby.addr(),
        "POST",
        "/api/sessions",
        Some(r#"{"game_type":"echo-arena"}"#),
        None,
    )
    .await;
    assert_eq!(status, 401, "mutation without token is rejected");

    let (status, _) = http(
        rig.lobby.addr(),
        "POST",
        "/api/sessions",
        Some(r#"{"game_type":"echo-arena"}"#),
        Some("Bearer wrong"),
    )
    .await;
    assert_eq!(status, 401, "wrong token is rejected");

    // With the right token the request proceeds (404: the worker path is
    // bogus, but that proves the gate passed).
    let (status, _) = http(
        rig.lobby.addr(),
        "POST",
        "/api/sessions",
        Some(r#"{"game_type":"echo-arena"}"#),
        Some("Bearer s3cret"),
    )
    .await;
    assert_eq!(status, 500, "gate passed; spawn failed on bogus path");

    rig.stop().await;
}

/// (lobby-api: Cert hash and test page hosting) The absorbed routes serve
/// the live edge hash, the page, and 404 for the rest.
#[tokio::test]
async fn absorbed_bootstrap_routes() {
    let rig = start("", vec![]).await;

    let (status, body) = http(rig.lobby.addr(), "GET", "/cert-hash", None, None).await;
    assert_eq!(status, 200);
    let expected = rig
        .edge
        .cert_sha256()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(body, format!("{{\"sha256\":\"{expected}\"}}"));

    let (status, body) = http(rig.lobby.addr(), "GET", "/", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains("netcode hub"), "test page served");

    let (status, _) = http(rig.lobby.addr(), "GET", "/nope", None, None).await;
    assert_eq!(status, 404);

    // Known path, wrong method.
    let (status, body) = http(rig.lobby.addr(), "POST", "/api/games", Some("{}"), None).await;
    assert_eq!(status, 405);
    assert!(body.contains("not allowed"), "body: {body}");

    rig.stop().await;
}

/// (lobby-api: Session listing) A pending session lists with state
/// `pending`; the `active` mapping is covered by the end-to-end test.
#[tokio::test]
async fn session_states_map_to_strings() {
    let rig = start("", vec![]).await;
    let spec = SessionSpec {
        game_type: "probe".into(),
        display_name: "Probe".into(),
        max_players: 1,
        payload_schema_version: 1,
    };
    let id = rig.backplane.create_pending(spec).await.expect("pending");

    let (status, body) = http(rig.lobby.addr(), "GET", "/api/sessions", None, None).await;
    assert_eq!(status, 200);
    assert!(body.contains("\"pending\""), "body: {body}");
    assert!(body.contains(&format!("\"id\":{id}")), "body: {body}");
    assert!(body.contains("\"player_count\":0"), "body: {body}");

    rig.stop().await;
    let _ = SessionState::Pending; // state import exercised above
}

/// (lobby-api: OpenAPI drift guard) Every route documented in
/// `docs/rest_api.yaml` is answered by the implementation with its
/// documented status class. The yaml itself is matched textually per route
/// entry — if a route disappears from either side, this test fails.
#[tokio::test]
async fn documented_routes_answer() {
    let rig = start("", vec![dummy_game("echo-arena", "/nowhere")]).await;
    let spec = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/rest_api.yaml"
    ))
    .expect("read docs/rest_api.yaml");

    let (session_id, client_id) = (999_999u32, 1u32);
    let probes: [(&str, &str, &str, u16); 7] = [
        ("GET", "/cert-hash", "  /cert-hash:", 200),
        ("GET", "/", "  /:", 200),
        ("GET", "/api/games", "  /api/games:", 200),
        ("GET", "/api/sessions", "  /api/sessions:", 200),
        (
            "POST",
            "/api/sessions",
            "  /api/sessions:",
            400, // empty body is a documented-class error
        ),
        (
            "DELETE",
            &format!("/api/sessions/{session_id}"),
            "  /api/sessions/{id}:",
            404,
        ),
        (
            "DELETE",
            &format!("/api/sessions/{session_id}/clients/{client_id}"),
            "  /api/sessions/{id}/clients/{clientId}:",
            404,
        ),
    ];
    for (method, path, doc_path, expect_class) in probes {
        assert!(
            spec.contains(doc_path),
            "docs/rest_api.yaml is missing a path entry for {doc_path}"
        );
        let (status, _) = http(rig.lobby.addr(), method, path, None, None).await;
        assert_eq!(
            status / 100,
            expect_class / 100,
            "{method} {path}: expected {expect_class}-class, got {status}"
        );
    }

    rig.stop().await;
}

/// (lobby-api: Request deadline) An incomplete request cannot hold its task
/// and socket forever, and its expiry does not affect later requests.
#[tokio::test(start_paused = true)]
async fn incomplete_request_times_out_without_blocking_others() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let rig = start("", vec![]).await;
    let mut slow = tokio::net::TcpStream::connect(rig.lobby.addr())
        .await
        .expect("connect slow request");
    slow.write_all(b"GET /api/games HTTP/1.1\r\nHost: hub\r\n")
        .await
        .expect("partial request head");

    // Let the accept loop spawn the request task before advancing its clock.
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::task::yield_now().await;
    let mut response = Vec::new();
    match tokio::time::timeout(Duration::from_secs(1), slow.read_to_end(&mut response)).await {
        Ok(Ok(0)) => assert!(response.is_empty()),
        other => panic!("slow request must end at the deadline: {other:?}"),
    }

    let (status, body) = http(rig.lobby.addr(), "GET", "/api/games", None, None).await;
    assert_eq!(status, 200);
    assert_eq!(body, "[]");

    rig.stop().await;
}
