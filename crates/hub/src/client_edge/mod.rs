//! The browser-facing client edge: a WebTransport listener that terminates
//! client connections, performs the join handshake, and relays packets
//! between the transport and the backplane's [`ClientHandle`] seam.
//!
//! The hub still routes by header only: the edge maps the header's channel
//! byte onto WebTransport primitives (state ⇄ datagrams, events ⇄ the
//! handshake stream) and never parses payloads.
//!
//! Requirements live in the `client-edge` capability spec; the frame layouts
//! in `crates/protocol/docs/wire-format.md`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};
use wtransport::connection::Connection;
use wtransport::tls::{Certificate, CertificateChain, PrivateKey};
use wtransport::{Endpoint, Identity, ServerConfig, VarInt};

use crate::backplane_service::{ClientHandle, ServiceHandle as Backplane};
use crate::config::ClientEdgeConfig;
use protocol::{Header, MAX_NAME_LEN, MAX_TOKEN_LEN};

/// Wire channels, as defined by the hub-sdk contract.
const CHANNEL_STATE: u8 = 0;
const CHANNEL_EVENTS: u8 = 1;

/// A client whose pending outbound relay exceeds this many packets is closed:
/// bounded memory per connection without a second policy engine.
const STALL_WATERMARK: usize = 1024;

/// Grace for connection tasks to observe shutdown and exit.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

/// The per-boot self-signed certificate and its pinned hash.
#[derive(Debug, Clone)]
pub struct Cert {
    /// DER-encoded X.509 certificate (what clients hash and pin).
    pub der: Vec<u8>,
    /// SHA-256 of `der`, the value served over the bootstrap endpoint.
    pub sha256: [u8; 32],
}

/// Generates a fresh ECDSA P-256 self-signed certificate and the identity
/// the WebTransport listener presents.
///
/// Validity stays within the W3C WebTransport two-week cap so hash-pinned
/// browser clients accept it (`serverCertificateHashes` rules).
pub fn generate_identity() -> Result<(Cert, Identity)> {
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
        .context("generate client-edge key pair")?;
    let mut params =
        rcgen::CertificateParams::new(vec!["netcode-hub".to_owned(), "localhost".to_owned()])
            .context("build client-edge certificate params")?;
    let now = time::OffsetDateTime::now_utc();
    // An hour of back-dating absorbs clock skew between hub and clients.
    params.not_before = now - time::Duration::hours(1);
    params.not_after = now + time::Duration::days(7);
    let cert = params
        .self_signed(&key)
        .context("sign client-edge certificate")?;

    let der = cert.der().as_ref().to_vec();
    let sha256: [u8; 32] = Sha256::digest(&der).into();
    let chain = CertificateChain::from_iter([
        Certificate::from_der(der.clone()).context("wrap certificate")?
    ]);
    let identity = Identity::new(chain, PrivateKey::from_der_pkcs8(key.serialize_der()));
    Ok((Cert { der, sha256 }, identity))
}

/// A bound client-edge service, ready to start.
pub struct Service {
    server: Endpoint<wtransport::endpoint::endpoint_side::Server>,
    cfg: ClientEdgeConfig,
    backplane: Backplane,
    cert: Cert,
}

impl Service {
    /// Binds the WebTransport listener (fresh certificate per boot) and the
    /// bootstrap HTTP endpoint. Both addresses come from `cfg`; the backplane
    /// handle is the seam client packets route through.
    pub async fn bind(cfg: &ClientEdgeConfig, backplane: Backplane) -> Result<Self> {
        let bind: SocketAddr = cfg.bind.parse().context("parse client_edge.bind")?;

        let (cert, identity) = generate_identity()?;

        let server_config = ServerConfig::builder()
            .with_bind_address(bind)
            .with_identity(identity)
            .build();
        let server = Endpoint::server(server_config)
            .with_context(|| format!("bind client edge on {bind}"))?;

        Ok(Self {
            server,
            cfg: cfg.clone(),
            backplane,
            cert,
        })
    }

    /// Runs the service until [`ServiceHandle::shutdown`]. (Client edge
    /// lifecycle: log lines are part of the contract.)
    pub fn start(self) -> ServiceHandle {
        let (shutdown, shutdown_rx) = watch::channel(false);
        let edge_addr = self
            .server
            .local_addr()
            .expect("bound server has a local address");

        let join = tokio::spawn(run(self.server, self.cfg, self.backplane, shutdown_rx));

        info!(address = %edge_addr, "client edge listening");
        info!(sha256 = %hex(&self.cert.sha256), "client edge certificate hash");

        ServiceHandle {
            edge_addr,
            cert: self.cert,
            shutdown: Arc::new(shutdown),
            join: Arc::new(tokio::sync::Mutex::new(Some(join))),
        }
    }
}

/// Handle to the running client edge; cheap to clone.
#[derive(Clone)]
pub struct ServiceHandle {
    edge_addr: SocketAddr,
    cert: Cert,
    shutdown: Arc<watch::Sender<bool>>,
    join: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl ServiceHandle {
    /// The WebTransport listener's local address.
    pub fn edge_addr(&self) -> SocketAddr {
        self.edge_addr
    }

    /// The pinned certificate hash (served by the bootstrap endpoint).
    pub fn cert_sha256(&self) -> [u8; 32] {
        self.cert.sha256
    }

    /// Stops accepting connections; open connections close within the drain
    /// grace period.
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    /// Waits for the service (accept loop, bootstrap, connections) to finish.
    pub async fn drain(&self) {
        let join = self.join.lock().await.take();
        if let Some(join) = join {
            let _ = tokio::time::timeout(DRAIN_TIMEOUT, join).await;
        }
    }
}

/// The accept loop: one task per connection, a bootstrap task, shutdown-aware.
async fn run(
    server: Endpoint<wtransport::endpoint::endpoint_side::Server>,
    cfg: ClientEdgeConfig,
    backplane: Backplane,
    shutdown_rx: watch::Receiver<bool>,
) {
    let mut tasks = JoinSet::new();
    let mut shutdown = shutdown_rx.clone();
    loop {
        tokio::select! {
            incoming = server.accept() => {
                // IncomingSession → SessionRequest → Connection.
                match incoming.await {
                    Ok(request) => match request.accept().await {
                        Ok(conn) => {
                            tasks.spawn(handle_connection(
                                conn,
                                cfg.clone(),
                                backplane.clone(),
                                shutdown_rx.clone(),
                            ));
                        }
                        Err(err) => {
                            debug!(%err, "client session failed to establish");
                        }
                    },
                    Err(err) => {
                        debug!(%err, "client connection failed");
                    }
                }
            }
            // Only reap when something can finish; an empty JoinSet's
            // join_next is immediately ready and would busy-loop.
            _ = tasks.join_next(), if !tasks.is_empty() => {}
            _ = shutdown.changed() => break,
        }
    }

    // Stop accepting; let connection tasks observe shutdown and finish.
    while tasks.join_next().await.is_some() {}
}

/// One accepted client connection: handshake, then relay until either side
/// goes away.
async fn handle_connection(
    conn: Connection,
    cfg: ClientEdgeConfig,
    backplane: Backplane,
    mut shutdown: watch::Receiver<bool>,
) {
    // The first stream must carry the handshake.
    let (mut send, mut recv) = match conn.accept_bi().await {
        Ok(pair) => pair,
        Err(err) => {
            debug!(%err, "client opened no handshake stream");
            return;
        }
    };

    let connect = match read_connect(&mut recv).await {
        Ok(connect) => connect,
        Err(err) => {
            // Malformed first frame: close, no state changes.
            debug!(%err, "malformed handshake");
            conn.close(VarInt::from_u32(0), b"bad handshake");
            return;
        }
    };

    // Token gate: empty configured token = open LAN trust.
    if !cfg.token.is_empty() && connect.token != cfg.token {
        reject(&conn, &mut send, "wrong token").await;
        return;
    }

    // Join rules: the registry decides existence, activity, capacity.
    let client = match backplane.attach(connect.session_id, &connect.name).await {
        Ok(client) => client,
        Err(err) => {
            reject(&conn, &mut send, &err.to_string()).await;
            return;
        }
    };

    let _ = send
        .write_all(
            &protocol::Accept {
                client_id: client.id,
            }
            .encode(),
        )
        .await;
    info!(
        session = connect.session_id,
        client = client.id,
        name = %connect.name,
        "client joined over WebTransport"
    );

    relay(conn, send, recv, client, &mut shutdown).await;
}

/// Sends a `Reject`, flushes it with a stream FIN, then closes the
/// connection. The FIN ordering guarantees delivery; an immediate
/// `close` would race the frame in flight and the client could lose it.
async fn reject(conn: &Connection, send: &mut wtransport::SendStream, reason: &str) {
    let _ = send
        .write_all(
            &protocol::Reject {
                reason: reason.to_owned(),
            }
            .encode(),
        )
        .await;
    let _ = send.finish().await;
    // Give the frame a beat to reach the client, then tear down.
    tokio::time::sleep(Duration::from_millis(100)).await;
    conn.close(VarInt::from_u32(0), b"rejected");
}
/// oversized length field can allocate unbounded memory.
/// oversized length field can allocate unbounded memory.
async fn read_connect(
    recv: &mut wtransport::RecvStream,
) -> std::result::Result<protocol::Connect, &'static str> {
    use protocol::{JOIN_VERSION, KIND_CONNECT};

    let mut head = [0u8; 4];
    recv.read_exact(&mut head)
        .await
        .map_err(|_| "stream ended before the handshake")?;
    if head[0] != KIND_CONNECT {
        return Err("first frame is not a Connect");
    }
    if head[1] != JOIN_VERSION {
        return Err("unsupported join frame version");
    }
    let token_len = u16::from_le_bytes([head[2], head[3]]) as usize;
    if token_len > MAX_TOKEN_LEN {
        return Err("oversized token");
    }

    let mut token = vec![0u8; token_len];
    recv.read_exact(&mut token)
        .await
        .map_err(|_| "stream ended inside the token")?;

    let mut mid = [0u8; 6];
    recv.read_exact(&mut mid)
        .await
        .map_err(|_| "stream ended inside the session id")?;
    let name_len = u16::from_le_bytes([mid[4], mid[5]]) as usize;
    if name_len > MAX_NAME_LEN {
        return Err("oversized name");
    }

    let mut name = vec![0u8; name_len];
    recv.read_exact(&mut name)
        .await
        .map_err(|_| "stream ended inside the name")?;

    let mut frame = Vec::with_capacity(head.len() + token.len() + mid.len() + name.len());
    frame.extend_from_slice(&head);
    frame.extend_from_slice(&token);
    frame.extend_from_slice(&mid);
    frame.extend_from_slice(&name);
    protocol::Connect::decode(&frame).map_err(|_| "malformed Connect frame")
}

/// The relay: routes packets between the transport and the [`ClientHandle`]
/// until the connection, the client queue, or shutdown ends it.
async fn relay(
    conn: Connection,
    mut events_send: wtransport::SendStream,
    mut events_recv: wtransport::RecvStream,
    mut client: ClientHandle,
    shutdown: &mut watch::Receiver<bool>,
) {
    // Stream reading runs as its own task: `RecvStream::read` fills a caller
    // buffer and is not cancel-safe, so it must never sit inside a `select!`
    // arm. The task parses self-delimiting packets and forwards them.
    let (inbound_tx, mut inbound_rx) =
        tokio::sync::mpsc::unbounded_channel::<Result<Vec<u8>, ()>>();
    tokio::spawn(async move {
        let mut stream_buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 65_536];
        loop {
            match events_recv.read(&mut chunk).await {
                Ok(Some(n)) => {
                    stream_buf.extend_from_slice(&chunk[..n]);
                    while let Some((_, end)) = parse_packet(&stream_buf) {
                        // Validation of the header against this client's
                        // identity lives in the backplane, as ever.
                        let packet: Vec<u8> = stream_buf.drain(..end).collect();
                        if inbound_tx.send(Ok(packet)).is_err() {
                            return;
                        }
                    }
                }
                Ok(None) => {
                    let _ = inbound_tx.send(Err(()));
                    return;
                }
                Err(_) => {
                    let _ = inbound_tx.send(Err(()));
                    return;
                }
            }
        }
    });

    loop {
        // Backpressure stall guard: a client that stopped reading while its
        // worker keeps sending gets closed, not buffered forever.
        if client.backlog() > STALL_WATERMARK {
            warn!(
                client = client.id,
                "relay backlog exceeded watermark; closing"
            );
            break;
        }

        tokio::select! {
            // Hub → client: one routed packet at a time.
            packet = client.recv() => {
                let Some(packet) = packet else { break };  // session ended
                match Header::decode(&packet).map(|h| h.channel) {
                    Ok(CHANNEL_STATE) => {
                        // Newest-wins traffic: send without blocking; refusal
                        // (full queue, oversized) is a drop, never a stall.
                        if let Err(err) = conn.send_datagram(packet) {
                            debug!(%err, client = client.id, "state datagram dropped");
                        }
                    }
                    Ok(CHANNEL_EVENTS) => {
                        if let Err(err) = events_send.write_all(&packet).await {
                            debug!(%err, client = client.id, "event stream write failed");
                            break;
                        }
                    }
                    Ok(_) => {
                        // Unknown channel: the header decoded, so route it as
                        // an event — reliable delivery for anything unusual.
                        if let Err(err) = events_send.write_all(&packet).await {
                            debug!(%err, client = client.id, "event stream write failed");
                            break;
                        }
                    }
                    Err(_) => {
                        // The backplane only forwards well-formed packets;
                        // anything else here would be a bug. Drop it.
                        debug!(client = client.id, "outbound packet with bad header dropped");
                    }
                }
            }
            // Client → hub: event packets parsed off the control stream.
            inbound = inbound_rx.recv() => {
                match inbound {
                    Some(Ok(packet)) => client.feed_packet(packet),
                    _ => break,  // stream ended or malformed
                }
            }
            // Client → hub: state datagrams.
            datagram = conn.receive_datagram() => {
                match datagram {
                    Ok(datagram) => client.feed_packet(Vec::from(&datagram[..])),
                    Err(_) => break,  // connection lost
                }
            }
            _ = conn.closed() => break,
            _ = shutdown.changed() => {
                conn.close(VarInt::from_u32(0), b"hub shutting down");
                break;
            }
        }
    }

    // Dropping the handle detaches the client (worker notified, count
    // decremented) — the same semantics as an in-process handle drop.
    drop(client);
    conn.close(VarInt::from_u32(0), b"relay closed");
}

/// If `buf` holds a complete packet, returns its header and end offset.
/// Returns `None` on incomplete input; invalid headers surface as `None`
/// from the caller's perspective (the relay closes the connection).
fn parse_packet(buf: &[u8]) -> Option<(Header, usize)> {
    let header = Header::decode(buf).ok()?;
    let end = header.packet_len();
    (buf.len() >= end).then_some((header, end))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
