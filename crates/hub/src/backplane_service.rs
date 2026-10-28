//! The backplane listener: accepts worker connections, authenticates them,
//! registers sessions, routes packets, and enforces liveness.
//!
//! All session state lives in one core task (see the design doc); everything
//! else — accept loop, per-worker connection pumps, client handles — talks to
//! it through [`Command`]s. Payloads are opaque: the core parses only client
//! packet headers for routing.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{info, warn};

use crate::config::BackplaneConfig;
use crate::registry::{
    AttachError, ClientId, Registry, RegistryError, SessionId, SessionInfo, SessionSpec,
};
use backplane::{Frame, conn};
use protocol;

/// How long a connection may take to produce `Hello` + `Register`.
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a pending spawned session waits for its worker's claim.
const PENDING_TIMEOUT: Duration = Duration::from_secs(10);
/// Silence longer than this closes an authenticated worker connection.
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(15);

/// Tunables; tests shorten these. Production uses [`Timeouts::default`].
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub registration: Duration,
    pub liveness: Duration,
    pub pending: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            registration: REGISTRATION_TIMEOUT,
            liveness: LIVENESS_TIMEOUT,
            pending: PENDING_TIMEOUT,
        }
    }
}

/// A client as seen by the session it is attached to: feed it packets from
/// the wire, drain packets destined to it. Dropping it detaches the client.
pub struct ClientHandle {
    pub id: ClientId,
    pub session_id: SessionId,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    cmd: mpsc::UnboundedSender<Command>,
}

impl ClientHandle {
    /// Packet bytes arriving for this client (already routed by the hub).
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        self.rx.recv().await
    }

    /// Feeds a raw client packet (header + payload) into the hub. The hub
    /// validates the header against this client's identity before forwarding.
    pub fn feed_packet(&self, packet: Vec<u8>) {
        let _ = self.cmd.send(Command::ClientPacket {
            session_id: self.session_id,
            client_id: self.id,
            packet,
        });
    }

    /// Packets queued for the relay to send, not yet drained. A client whose
    /// worker keeps sending while its transport stalls grows this without
    /// bound; relays close connections past their watermark.
    pub fn backlog(&self) -> usize {
        self.rx.len()
    }
}

impl Drop for ClientHandle {
    fn drop(&mut self) {
        let _ = self.cmd.send(Command::Detach {
            session_id: self.session_id,
            client_id: self.id,
        });
    }
}

/// Handle to the running backplane service; cheap to clone.
#[derive(Clone)]
pub struct ServiceHandle {
    local_addr: SocketAddr,
    token: String,
    cmd: mpsc::UnboundedSender<Command>,
    shutdown: watch::Sender<bool>,
    core: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl ServiceHandle {
    /// The address workers connect to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// The configured shared token (empty = authentication disabled).
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Attaches a client to a session.
    pub async fn attach(
        &self,
        session_id: SessionId,
        player_name: &str,
    ) -> Result<ClientHandle, AttachError> {
        let (resp, resp_rx) = oneshot::channel();
        self.cmd
            .send(Command::Attach {
                session_id,
                player_name: player_name.to_owned(),
                resp,
            })
            .expect("backplane core is alive");
        resp_rx.await.expect("backplane core answered")
    }

    /// Sessions a server browser would show (pending + active).
    pub async fn sessions(&self) -> Vec<SessionInfo> {
        let (resp, resp_rx) = oneshot::channel();
        self.cmd
            .send(Command::Sessions { resp })
            .expect("backplane core is alive");
        resp_rx.await.expect("backplane core answered")
    }

    /// Creates a pending session (spawn flows reserve their id this way).
    /// Returns `None` without consuming an id at the live-session limit.
    pub async fn create_pending(&self, spec: SessionSpec) -> Option<SessionId> {
        let (resp, resp_rx) = oneshot::channel();
        self.cmd
            .send(Command::CreatePending { spec, resp })
            .expect("backplane core is alive");
        resp_rx.await.expect("backplane core answered")
    }

    /// Admin close: drops the worker link (workers exit on hub loss) and
    /// closes the registry entry, detaching every client. `Err` carries the
    /// id when no open session has it.
    pub async fn close_session(&self, session_id: SessionId) -> Result<(), SessionId> {
        let (resp, resp_rx) = oneshot::channel();
        self.cmd
            .send(Command::CloseSession { session_id, resp })
            .expect("backplane core is alive");
        resp_rx.await.expect("backplane core answered")
    }

    /// Admin kick: detaches one client (worker notified, transport closed).
    /// Returns whether a live client was kicked.
    pub async fn kick(&self, session_id: SessionId, client_id: ClientId) -> bool {
        let (resp, resp_rx) = oneshot::channel();
        self.cmd
            .send(Command::Kick {
                session_id,
                client_id,
                resp,
            })
            .expect("backplane core is alive");
        resp_rx.await.expect("backplane core answered")
    }

    /// Notifies the core that the worker behind a session is gone.
    pub(crate) fn worker_lost(&self, session_id: SessionId) {
        let _ = self.cmd.send(Command::WorkerLost { session_id });
    }

    /// Begins shutdown: closes the listener, worker links, and client queues.
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    /// Waits for the core to finish draining.
    pub async fn drain(self) {
        let core = self
            .core
            .lock()
            .expect("backplane core handle mutex")
            .take();
        if let Some(core) = core {
            let _ = core.await;
        }
    }
}

enum Command {
    Attach {
        session_id: SessionId,
        player_name: String,
        resp: oneshot::Sender<Result<ClientHandle, AttachError>>,
    },
    Detach {
        session_id: SessionId,
        client_id: ClientId,
    },
    /// Admin kick: like `Detach`, but reports whether the client existed.
    Kick {
        session_id: SessionId,
        client_id: ClientId,
        resp: oneshot::Sender<bool>,
    },
    /// Admin close: drop the worker link and close the registry entry.
    CloseSession {
        session_id: SessionId,
        resp: oneshot::Sender<Result<(), SessionId>>,
    },
    Sessions {
        resp: oneshot::Sender<Vec<SessionInfo>>,
    },
    CreatePending {
        spec: SessionSpec,
        resp: oneshot::Sender<Option<SessionId>>,
    },
    Register {
        claimed_id: SessionId,
        spec: SessionSpec,
        outbound: mpsc::UnboundedSender<Frame>,
        resp: oneshot::Sender<Result<SessionId, RegisterRejection>>,
    },
    WorkerFrame {
        session_id: SessionId,
        frame: Frame,
    },
    WorkerLost {
        session_id: SessionId,
    },
    ClientPacket {
        session_id: SessionId,
        client_id: ClientId,
        packet: Vec<u8>,
    },
}

/// Why a `Register` claim was rejected.
#[derive(Debug)]
pub enum RegisterRejection {
    /// Claimed an id that is nobody's pending session.
    UnknownClaim(SessionId),
    /// The registry refused the transition (already active/closed).
    NotPending(SessionId),
    /// A new self-registered session would exceed the live-session limit.
    SessionLimitReached,
}

struct ClientRoute {
    session_id: SessionId,
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

/// The bound, not-yet-started service.
pub struct Service {
    listener: TcpListener,
    local_addr: SocketAddr,
    token: String,
    max_sessions: usize,
    timeouts: Timeouts,
}

impl Service {
    /// The bound address workers will connect to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Binds the backplane listener with production timeouts.
    pub async fn bind(cfg: &BackplaneConfig) -> std::io::Result<Service> {
        Self::bind_with_timeouts(cfg, Timeouts::default()).await
    }

    /// Binds the backplane listener with explicit timeouts (tests).
    pub async fn bind_with_timeouts(
        cfg: &BackplaneConfig,
        timeouts: Timeouts,
    ) -> std::io::Result<Service> {
        let addr: SocketAddr = cfg.bind.parse().map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("backplane.bind {}: {err}", cfg.bind),
            )
        })?;
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        Ok(Service {
            listener,
            local_addr,
            token: cfg.token.clone(),
            max_sessions: cfg.max_sessions,
            timeouts,
        })
    }

    /// Starts the service: spawns the accept loop and the hub core.
    pub fn start(self) -> ServiceHandle {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Accept loop: hands fresh connections to per-connection tasks.
        let mut accept_shutdown = shutdown_rx.clone();
        let accept_token = self.token.clone();
        let accept_cmd = cmd_tx.clone();
        let accept_timeouts = self.timeouts;
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accepted = self.listener.accept() => {
                        match accepted {
                            Ok((stream, peer)) => {
                                info!(%peer, "worker connection accepted");
                                let token = accept_token.clone();
                                let cmd = accept_cmd.clone();
                                let timeouts = accept_timeouts;
                                let mut shutdown = accept_shutdown.clone();
                                tokio::spawn(async move {
                                    handle_connection(stream, peer.to_string(), token, timeouts, cmd, &mut shutdown).await;
                                });
                            }
                            Err(err) => warn!(%err, "accept failed"),
                        }
                    }
                    _ = accept_shutdown.changed() => break,
                }
            }
            info!("backplane accept loop stopped");
        });

        let core_cmd_tx = cmd_tx.clone();
        let core = tokio::spawn(async move {
            Core {
                registry: Registry::with_session_limit(self.max_sessions),
                workers: HashMap::new(),
                clients: HashMap::new(),
                pendings: HashMap::new(),
                cmd_tx: core_cmd_tx,
            }
            .run(cmd_rx, shutdown_rx)
            .await;
        });

        ServiceHandle {
            local_addr: self.local_addr,
            token: self.token,
            cmd: cmd_tx.clone(),
            shutdown: shutdown_tx,
            core: Arc::new(Mutex::new(Some(core))),
        }
    }
}

/// Per-connection task: authentication, registration, then the link pump.
async fn handle_connection(
    stream: TcpStream,
    peer: String,
    token: String,
    timeouts: Timeouts,
    cmd: mpsc::UnboundedSender<Command>,
    shutdown: &mut watch::Receiver<bool>,
) {
    let (mut rd, mut wr) = stream.into_split();

    // --- registration phase (bounded) ---
    let hello = tokio::time::timeout(timeouts.registration, conn::read_frame(&mut rd)).await;
    let frame = match hello {
        Ok(Ok(Frame::Hello { token: got })) if token_is_valid(&token, &got) => got,
        Ok(Ok(frame)) => {
            warn!(%peer, frame = ?frame.opcode(), "first frame was not an acceptable Hello");
            return;
        }
        Ok(Err(err)) => {
            warn!(%peer, %err, "handshake failed");
            return;
        }
        Err(_) => {
            warn!(%peer, "handshake timed out before Hello");
            return;
        }
    };
    let _ = frame;

    let register = tokio::time::timeout(timeouts.registration, conn::read_frame(&mut rd)).await;
    let (claimed_id, spec) = match register {
        Ok(Ok(Frame::Register {
            claimed_id,
            game_type,
            display_name,
            max_players,
            payload_schema_version,
        })) => (
            claimed_id,
            SessionSpec {
                game_type,
                display_name,
                max_players,
                payload_schema_version,
            },
        ),
        Ok(Ok(frame)) => {
            warn!(%peer, frame = ?frame.opcode(), "expected Register");
            return;
        }
        Ok(Err(err)) => {
            warn!(%peer, %err, "registration read failed");
            return;
        }
        Err(_) => {
            warn!(%peer, "registration timed out");
            return;
        }
    };

    let (outbound, mut outbound_rx) = mpsc::unbounded_channel();
    let (resp_tx, resp_rx) = oneshot::channel();
    if cmd
        .send(Command::Register {
            claimed_id,
            spec,
            outbound,
            resp: resp_tx,
        })
        .is_err()
    {
        return;
    }
    let session_id = match resp_rx.await {
        Ok(Ok(session_id)) => session_id,
        Ok(Err(rejection)) => {
            warn!(%peer, ?rejection, "register rejected");
            return;
        }
        Err(_) => return,
    };
    if conn::write_frame(&mut wr, &Frame::Registered { session_id })
        .await
        .is_err()
    {
        let _ = cmd.send(Command::WorkerLost { session_id });
        return;
    }
    info!(%peer, session = session_id, "worker link established");

    // --- link pump: worker frames in, hub frames out, liveness enforced ---
    // The reader runs as its own task: `conn::read_frame` spans two reads
    // (length, body) and is therefore *not* cancel-safe, so it must never sit
    // inside a `select!` arm — an outbound write winning mid-body would drop
    // the length bytes and desynchronize the stream.
    let (frames_tx, mut frames_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            match conn::read_frame(&mut rd).await {
                Ok(frame) => {
                    if frames_tx.send(Ok(frame)).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    let _ = frames_tx.send(Err(err));
                    break;
                }
            }
        }
    });

    let mut last_seen = Instant::now();
    loop {
        let liveness = tokio::time::sleep_until(tokio::time::Instant::from_std(
            last_seen + timeouts.liveness,
        ));
        tokio::select! {
            _ = liveness => {
                warn!(%peer, session = session_id, "worker silent past liveness timeout");
                let _ = cmd.send(Command::WorkerLost { session_id });
                return;
            }
            msg = frames_rx.recv() => match msg {
                Some(Ok(frame)) => {
                    last_seen = Instant::now();
                    match frame {
                        Frame::Heartbeat => {}
                        frame => {
                            if cmd.send(Command::WorkerFrame { session_id, frame }).is_err() {
                                return;
                            }
                        }
                    }
                }
                Some(Err(err)) => {
                    info!(%peer, session = session_id, err = %err, "worker connection closed");
                    let _ = cmd.send(Command::WorkerLost { session_id });
                    return;
                }
                None => return,
            },
            out = outbound_rx.recv() => match out {
                Some(frame) => {
                    tracing::debug!(session = session_id, frame = ?frame.opcode(), "pump: outbound frame");
                    if conn::write_frame(&mut wr, &frame).await.is_err() {
                        let _ = cmd.send(Command::WorkerLost { session_id });
                        return;
                    }
                }
                // Hub core is gone.
                None => return,
            },
            _ = shutdown.changed() => return,
        }
    }
}

fn token_is_valid(configured: &str, provided: &str) -> bool {
    configured.is_empty() || configured == provided
}

/// Owns all session state; addressed exclusively through [`Command`]s.
struct Core {
    registry: Registry,
    workers: HashMap<SessionId, mpsc::UnboundedSender<Frame>>,
    clients: HashMap<ClientId, ClientRoute>,
    pendings: HashMap<SessionId, Instant>,
    cmd_tx: mpsc::UnboundedSender<Command>,
}

impl Core {
    async fn run(
        mut self,
        mut cmd_rx: mpsc::UnboundedReceiver<Command>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let mut sweep = tokio::time::interval(Duration::from_secs(1));
        sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = sweep.tick() => {
                    let now = Instant::now();
                    let expired: Vec<SessionId> = self
                        .pendings
                        .iter()
                        .filter(|(_, deadline)| **deadline <= now)
                        .map(|(id, _)| *id)
                        .collect();
                    for id in expired {
                        warn!(session = id, "pending session expired unclaimed");
                        self.pendings.remove(&id);
                        self.close_session(id);
                    }
                }
                cmd = cmd_rx.recv() => match cmd {
                    None => break,
                    Some(command) => {
                        if self.handle_command(command) {
                            break;
                        }
                    }
                },
                _ = shutdown.changed() => break,
            }
        }
        self.close_everything();
        info!("backplane core stopped");
    }

    /// Handles one command; returns `true` when the core should stop.
    fn handle_command(&mut self, command: Command) -> bool {
        match command {
            Command::Attach {
                session_id,
                player_name,
                resp,
            } => {
                let outcome = self
                    .registry
                    .attach(session_id, &player_name)
                    .map(|client_id| {
                        let (tx, rx) = mpsc::unbounded_channel();
                        self.clients
                            .insert(client_id, ClientRoute { session_id, tx });
                        self.notify_worker(session_id, Frame::ClientAttached { client_id });
                        ClientHandle {
                            id: client_id,
                            session_id,
                            rx,
                            cmd: self.cmd_tx.clone(),
                        }
                    });
                let _ = resp.send(outcome);
            }
            Command::Detach {
                session_id,
                client_id,
            } => {
                self.detach_client(session_id, client_id);
            }
            Command::Kick {
                session_id,
                client_id,
                resp,
            } => {
                let kicked = self.detach_client(session_id, client_id);
                let _ = resp.send(kicked);
            }
            Command::CloseSession { session_id, resp } => {
                let closed = self.close_session(session_id);
                self.workers.remove(&session_id);
                if closed {
                    info!(session = session_id, "session closed by admin request");
                }
                let _ = resp.send(if closed { Ok(()) } else { Err(session_id) });
            }
            Command::Sessions { resp } => {
                let _ = resp.send(self.registry.list());
            }
            Command::CreatePending { spec, resp } => {
                let created = self.registry.create_pending(&spec);
                if let Some(id) = created {
                    self.pendings.insert(id, Instant::now() + PENDING_TIMEOUT);
                    info!(session = id, game_type = %spec.game_type, "pending session created");
                }
                let _ = resp.send(created);
            }
            Command::Register {
                claimed_id,
                spec,
                outbound,
                resp,
            } => {
                let outcome = match claimed_id {
                    0 => self
                        .registry
                        .register_self(&spec)
                        .ok_or(RegisterRejection::SessionLimitReached),
                    id => match self.pendings.remove(&id) {
                        Some(_) => {
                            self.registry
                                .activate(id)
                                .map(|_| id)
                                .map_err(|err| match err {
                                    RegistryError::NotPending(_) => {
                                        RegisterRejection::NotPending(id)
                                    }
                                    RegistryError::NotFound(_) => {
                                        RegisterRejection::UnknownClaim(id)
                                    }
                                    other => {
                                        unreachable!("activate has no other errors: {other:?}")
                                    }
                                })
                        }
                        None => Err(RegisterRejection::UnknownClaim(id)),
                    },
                };
                match outcome {
                    Ok(session_id) => {
                        info!(session = session_id, game_type = %spec.game_type, "worker registered");
                        self.pendings.remove(&session_id);
                        self.workers.insert(session_id, outbound);
                        let _ = resp.send(Ok(session_id));
                    }
                    Err(rejection) => {
                        warn!(session = claimed_id, ?rejection, "register rejected");
                        let _ = resp.send(Err(rejection));
                    }
                }
            }
            Command::WorkerFrame { session_id, frame } => {
                self.handle_worker_frame(session_id, frame);
            }
            Command::WorkerLost { session_id } => {
                warn!(session = session_id, "worker lost; closing session");
                self.close_session(session_id);
                self.workers.remove(&session_id);
            }
            Command::ClientPacket {
                session_id,
                client_id,
                packet,
            } => self.handle_client_packet(session_id, client_id, packet),
        }
        false
    }

    /// Sends `ClientDetached` to the worker and forgets the client.
    /// Returns whether a live client was actually detached.
    fn detach_client(&mut self, session_id: SessionId, client_id: ClientId) -> bool {
        if self.clients.remove(&client_id).is_none() {
            return false; // already gone (e.g. the session closed first)
        }
        if let Err(err) = self.registry.detach(session_id, client_id) {
            warn!(session = session_id, client = client_id, %err, "detach ignored");
            return false;
        }
        self.notify_worker(session_id, Frame::ClientDetached { client_id });
        true
    }

    /// Closes the session if it is still open; returns whether it was.
    fn close_session(&mut self, session_id: SessionId) -> bool {
        let clients = self.registry.close(session_id);
        self.pendings.remove(&session_id);
        match clients {
            Some(client_ids) => {
                for client_id in client_ids {
                    self.clients.remove(&client_id);
                }
                true
            }
            None => false,
        }
    }

    fn close_everything(&mut self) {
        for info in self.registry.list() {
            self.close_session(info.id);
        }
        self.workers.clear();
    }

    fn handle_worker_frame(&mut self, session_id: SessionId, frame: Frame) {
        match frame {
            Frame::ToClient { packet } => self.route_packet(session_id, &packet),
            Frame::ToMany { client_ids, packet } => {
                for client_id in client_ids {
                    self.route_to(session_id, client_id, &packet);
                }
            }
            Frame::Broadcast { packet } => {
                // Copy the recipient list: routing mutates client queues.
                let client_ids: Vec<ClientId> = match self.registry.client_ids(session_id) {
                    Some(ids) => ids.to_vec(),
                    None => return,
                };
                for client_id in client_ids {
                    self.route_to(session_id, client_id, &packet);
                }
            }
            other => {
                warn!(session = session_id, frame = ?other.opcode(), "unexpected worker frame; dropped")
            }
        }
    }

    /// Routes one packet by its client-packet header. The payload is opaque.
    fn route_packet(&mut self, session_id: SessionId, packet: &[u8]) {
        let header = match protocol::Header::decode(packet) {
            Ok(header) => header,
            Err(err) => {
                warn!(session = session_id, %err, "worker packet with bad header; dropped");
                return;
            }
        };
        self.route_to(session_id, header.client_id, packet);
    }

    fn route_to(&mut self, session_id: SessionId, client_id: ClientId, packet: &[u8]) {
        match self.clients.get(&client_id) {
            Some(route) if route.session_id == session_id => {
                let _ = route.tx.send(packet.to_vec());
            }
            // Stale unicast to a detached client: expected, drop silently.
            _ => {}
        }
    }

    fn handle_client_packet(
        &mut self,
        session_id: SessionId,
        client_id: ClientId,
        packet: Vec<u8>,
    ) {
        let header_ok = protocol::Header::decode(&packet)
            .map(|header| header.client_id == client_id && header.session_id == session_id)
            .unwrap_or(false);
        if !header_ok {
            warn!(
                session = session_id,
                client = client_id,
                "client packet failed identity check; dropped"
            );
            return;
        }
        if let Some(worker) = self.workers.get(&session_id) {
            let _ = worker.send(Frame::FromClient { packet });
        }
    }

    fn notify_worker(&self, session_id: SessionId, frame: Frame) {
        if let Some(worker) = self.workers.get(&session_id) {
            let _ = worker.send(frame);
        }
    }
}
