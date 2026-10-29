//! The lobby/admin HTTP API: the surface directory UIs and admin pages build
//! on — game catalog, session listing/creation/closure, client kick — plus
//! the cert-hash and test-page routes absorbed from the client edge's
//! bootstrap stopgap.
//!
//! Hand-rolled HTTP/1.1 over tokio TCP (the bootstrap responder,
//! generalized): parse a request, route it, answer JSON. No framework; the
//! requirements live in the `lobby-api` capability spec, the routes in
//! `docs/rest_api.yaml`.

pub mod page;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::backplane_service::ServiceHandle as Backplane;
use crate::config::{GameConfig, LobbyConfig};
use crate::registry::{ClientId, SessionId, SessionState};
use crate::spawn::{SpawnError, SpawnRequest};

/// Largest request (head + body) we tolerate before giving up.
const MAX_REQUEST: usize = 64 * 1024;

/// Total time allowed for one request, including a trickled head or body.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A bound lobby service, ready to start.
pub struct Service {
    listener: TcpListener,
    state: Arc<State>,
    request_timeout: Duration,
}

struct State {
    token: String,
    games: Vec<GameConfig>,
    backplane: Backplane,
    cert_sha256: [u8; 32],
}

impl Service {
    /// Binds the lobby HTTP listener. `cert_sha256` is the live client-edge
    /// certificate hash, served at `/cert-hash`.
    pub async fn bind(
        cfg: &LobbyConfig,
        games: &[GameConfig],
        backplane: Backplane,
        cert_sha256: [u8; 32],
    ) -> Result<Self> {
        Self::bind_with_request_timeout(cfg, games, backplane, cert_sha256, REQUEST_TIMEOUT).await
    }

    /// Binds with an explicit total request timeout (test seams use a short
    /// deadline; production uses [`REQUEST_TIMEOUT`] through [`Self::bind`]).
    pub async fn bind_with_request_timeout(
        cfg: &LobbyConfig,
        games: &[GameConfig],
        backplane: Backplane,
        cert_sha256: [u8; 32],
        request_timeout: Duration,
    ) -> Result<Self> {
        let bind: SocketAddr = cfg.bind.parse().context("parse lobby.bind")?;
        let listener = TcpListener::bind(bind)
            .await
            .with_context(|| format!("bind lobby endpoint on {bind}"))?;
        Ok(Self {
            listener,
            state: Arc::new(State {
                token: cfg.token.clone(),
                games: games.to_vec(),
                backplane,
                cert_sha256,
            }),
            request_timeout,
        })
    }

    /// Runs the service until shutdown. (Lobby lifecycle.)
    pub fn start(self) -> ServiceHandle {
        let (shutdown, mut shutdown_rx) = watch::channel(false);
        let addr = self.listener.local_addr().expect("bound tcp listener");
        let state = self.state;
        let request_timeout = self.request_timeout;
        let join = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accepted = self.listener.accept() => {
                        let Ok((stream, _)) = accepted else { continue };
                        let state = state.clone();
                        tokio::spawn(async move {
                            match tokio::time::timeout(request_timeout, serve_one(stream, state)).await {
                                Ok(Ok(())) => {}
                                Ok(Err(err)) => warn!(%err, "lobby request failed"),
                                Err(_) => warn!("lobby request timed out"),
                            }
                        });
                    }
                    _ = shutdown_rx.changed() => break,
                }
            }
        });
        info!(address = %addr, "lobby api listening");
        ServiceHandle {
            addr,
            shutdown: Arc::new(shutdown),
            join: Arc::new(tokio::sync::Mutex::new(Some(join))),
        }
    }
}

/// Handle to the running lobby service; cheap to clone.
#[derive(Clone)]
pub struct ServiceHandle {
    addr: SocketAddr,
    shutdown: Arc<watch::Sender<bool>>,
    join: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl ServiceHandle {
    /// The lobby HTTP listener's local address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stops accepting; drains within the grace period.
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    /// Waits for the service to finish.
    pub async fn drain(&self) {
        let join = self.join.lock().await.take();
        if let Some(join) = join {
            let _ = tokio::time::timeout(Duration::from_secs(3), join).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Request handling
// ---------------------------------------------------------------------------

struct Request {
    method: String,
    path: String,
    authorization: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

enum RequestRead {
    Request(Request),
    Invalid(&'static str),
    Incomplete,
}

/// Reads and answers exactly one HTTP/1.1 request.
async fn serve_one(mut stream: TcpStream, state: Arc<State>) -> std::io::Result<()> {
    let response = match read_request(&mut stream).await? {
        RequestRead::Request(request) => route(&request, &state).await,
        RequestRead::Invalid(message) => error("400 Bad Request", message),
        RequestRead::Incomplete => return Ok(()),
    };
    let (status, content_type, body) = response;
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.flush().await
}

/// Reads a full request head plus a bounded `Content-Length` body.
async fn read_request(stream: &mut TcpStream) -> std::io::Result<RequestRead> {
    let mut buf = Vec::with_capacity(256);
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(RequestRead::Incomplete);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > MAX_REQUEST {
            return Ok(RequestRead::Incomplete);
        }
    };

    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();

    let mut authorization = None;
    let mut content_type = None;
    let mut content_length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("Authorization") {
            authorization = Some(value.to_owned());
        } else if name.eq_ignore_ascii_case("Content-Type") {
            content_type = Some(value.to_owned());
        } else if name.eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Ok(RequestRead::Invalid("duplicate Content-Length"));
            }
            let length = value
                .parse::<usize>()
                .ok()
                .filter(|length| *length <= MAX_REQUEST);
            let Some(length) = length else {
                return Ok(RequestRead::Invalid("invalid Content-Length"));
            };
            content_length = Some(length);
        } else if name.eq_ignore_ascii_case("Transfer-Encoding") {
            return Ok(RequestRead::Invalid("Transfer-Encoding is not supported"));
        }
    }

    let content_length = content_length.unwrap_or(0);
    let mut body = buf[head_end..].to_vec();
    if body.len() > content_length {
        return Ok(RequestRead::Invalid("request exceeds Content-Length"));
    }
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(RequestRead::Incomplete);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    if body.len() != content_length {
        return Ok(RequestRead::Invalid("request exceeds Content-Length"));
    }
    Ok(RequestRead::Request(Request {
        method,
        path,
        authorization,
        content_type,
        body,
    }))
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ErrorDto {
    error: String,
}

#[derive(Serialize)]
struct GameDto {
    game_type: String,
    display_name: String,
    max_players: u16,
    payload_schema_version: u16,
}

#[derive(Serialize)]
struct SessionDto {
    id: SessionId,
    state: &'static str,
    game_type: String,
    display_name: String,
    max_players: u16,
    payload_schema_version: u16,
    player_count: u16,
}

#[derive(Deserialize)]
struct CreateSession {
    game_type: String,
}

/// A routed response: status line, content type, body.
type Response = (String, &'static str, Vec<u8>);

fn json(status: &'static str, value: impl Serialize) -> Response {
    let body = serde_json::to_vec(&value).expect("serializable dto");
    (status.to_owned(), "application/json", body)
}

fn error(status: &'static str, message: impl Into<String>) -> Response {
    json(
        status,
        ErrorDto {
            error: message.into(),
        },
    )
}

/// Routes one request to its operation.
async fn route(request: &Request, state: &State) -> Response {
    let segments: Vec<&str> = request.path.split('/').filter(|s| !s.is_empty()).collect();

    // Absorbed bootstrap routes: open to everyone.
    if request.method == "GET" && request.path == "/cert-hash" {
        let hash = hex(&state.cert_sha256);
        return (
            "200 OK".to_owned(),
            "application/json",
            format!("{{\"sha256\":\"{hash}\"}}").into_bytes(),
        );
    }
    if request.method == "GET" && request.path == "/" {
        return (
            "200 OK".to_owned(),
            "text/html; charset=utf-8",
            page::TEST_PAGE.as_bytes().to_vec(),
        );
    }

    // Token gate on mutations (lobby-api: Token gate).
    if !state.token.is_empty() && request.method != "GET" {
        let expected = format!("Bearer {}", state.token);
        if request.authorization.as_deref() != Some(expected.as_str()) {
            return error("401 Unauthorized", "missing or wrong bearer token");
        }
    }

    match (request.method.as_str(), segments.as_slice()) {
        ("GET", ["api", "games"]) => json(
            "200 OK",
            state
                .games
                .iter()
                .map(|g| GameDto {
                    game_type: g.game_type.clone(),
                    display_name: g.display_name.clone(),
                    max_players: g.max_players,
                    payload_schema_version: g.payload_schema_version,
                })
                .collect::<Vec<_>>(),
        ),
        ("GET", ["api", "sessions"]) => {
            let sessions = state.backplane.sessions().await;
            json(
                "200 OK",
                sessions
                    .iter()
                    .map(|s| SessionDto {
                        id: s.id,
                        state: match s.state {
                            SessionState::Pending => "pending",
                            SessionState::Active => "active",
                            SessionState::Closed => "closed",
                        },
                        game_type: s.game_type.clone(),
                        display_name: s.display_name.clone(),
                        max_players: s.max_players,
                        payload_schema_version: s.payload_schema_version,
                        player_count: s.player_count,
                    })
                    .collect::<Vec<_>>(),
            )
        }
        ("POST", ["api", "sessions"]) => create_session(request, state).await,
        ("DELETE", ["api", "sessions", id]) => match id.parse::<SessionId>() {
            Ok(id) => match state.backplane.close_session(id).await {
                Ok(()) => ("204 No Content".to_owned(), "application/json", Vec::new()),
                Err(_) => error("404 Not Found", format!("no open session {id}")),
            },
            Err(_) => error(
                "404 Not Found",
                format!("session id {id:?} is not a number"),
            ),
        },
        ("DELETE", ["api", "sessions", id, "clients", cid]) => {
            match (id.parse::<SessionId>(), cid.parse::<ClientId>()) {
                (Ok(id), Ok(cid)) => {
                    if state.backplane.kick(id, cid).await {
                        ("204 No Content".to_owned(), "application/json", Vec::new())
                    } else {
                        error("404 Not Found", format!("no client {cid} in session {id}"))
                    }
                }
                _ => error("404 Not Found", "session or client id is not a number"),
            }
        }
        // Known shapes with a different method are 405s; everything else 404.
        (_, ["api", "games"] | ["api", "sessions"] | ["api", "sessions", _])
        | (_, ["api", "sessions", _, "clients", _]) => {
            error("405 Method Not Allowed", "method not allowed for this path")
        }
        _ => error("404 Not Found", format!("no route for {}", request.path)),
    }
}

/// `POST /api/sessions`: catalog lookup → spawn → session id.
async fn create_session(request: &Request, state: &State) -> Response {
    let is_json = request
        .content_type
        .as_deref()
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    if !is_json {
        return error(
            "415 Unsupported Media Type",
            "Content-Type must be application/json",
        );
    }
    let Ok(create) = serde_json::from_slice::<CreateSession>(&request.body) else {
        return error(
            "400 Bad Request",
            "body must be JSON with a game_type field",
        );
    };
    let Some(game) = state.games.iter().find(|g| g.game_type == create.game_type) else {
        return error(
            "404 Not Found",
            format!("unknown game_type {:?}", create.game_type),
        );
    };
    let spec = crate::registry::SessionSpec {
        game_type: game.game_type.clone(),
        display_name: game.display_name.clone(),
        max_players: game.max_players,
        payload_schema_version: game.payload_schema_version,
    };
    let request = SpawnRequest {
        program: game.worker.clone(),
        spec,
        session_config_json: game.session_config_json.clone(),
        extra_args: Vec::new(),
    };
    match state.backplane.spawn_session(request).await {
        Ok(session_id) => {
            info!(session = session_id, game = %game.game_type, "session created via lobby api");
            json(
                "201 Created",
                serde_json::json!({ "session_id": session_id }),
            )
        }
        Err(SpawnError::SessionLimitReached) => {
            error("503 Service Unavailable", "live-session limit reached")
        }
        Err(err) => error(
            "500 Internal Server Error",
            format!("failed to start worker: {err}"),
        ),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
