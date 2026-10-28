//! The bot: a headless browser-shaped client that joins a session over
//! WebTransport and *verifies* everything it receives against the
//! client-protocol contract.

use std::net::SocketAddr;
use std::time::Duration;

use protocol::{BASE_HEADER_SIZE, Header, Reply};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Wire channels, as defined by the hub-sdk contract.
const CHANNEL_STATE: u8 = 0;
const CHANNEL_EVENTS: u8 = 1;

/// Where to find the hub.
#[derive(Debug, Clone)]
pub struct BotConfig {
    /// The lobby HTTP endpoint (serves `/cert-hash`).
    pub lobby: SocketAddr,
    /// The WebTransport endpoint.
    pub edge: SocketAddr,
    /// Shared connect token (empty on an open hub).
    pub token: String,
}

/// A protocol or ordering violation observed by a bot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Violation {
    /// Short machine-readable kind, e.g. `bad_stream_header`.
    pub kind: String,
    /// Human-readable detail, including position where known.
    pub detail: String,
}

/// One verified packet, already routed by channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// An events-channel packet (reliable, ordered stream).
    Event {
        /// Sending client id from the header (0 = broadcast).
        from: u32,
        /// Payload bytes.
        payload: Vec<u8>,
    },
    /// A state-channel packet (unreliable, unordered datagram).
    State {
        /// Sending client id from the header (0 = broadcast).
        from: u32,
        /// Payload bytes.
        payload: Vec<u8>,
    },
}

/// A joined, verified bot.
pub struct Bot {
    conn: wtransport::Connection,
    writer: wtransport::SendStream,
    reader: wtransport::RecvStream,
    stream_buf: Vec<u8>,
    /// The session this bot joined.
    pub session: u32,
    /// The assigned client id.
    pub client_id: u32,
    /// Violations observed so far (protocol + ordering).
    pub violations: Vec<Violation>,
}

impl Bot {
    /// Joins a session exactly as a browser does: fetch the certificate
    /// hash from the lobby, connect with it pinned, perform the handshake.
    /// `Err` carries the reason (unreachable hub, rejected join, …).
    pub async fn join(cfg: &BotConfig, session: u32, name: &str) -> Result<Bot, String> {
        let hash = fetch_cert_hash(cfg.lobby).await?;
        let config = wtransport::ClientConfig::builder()
            .with_bind_default()
            .with_server_certificate_hashes([wtransport::tls::Sha256Digest::new(hash)])
            .build();
        let endpoint = wtransport::Endpoint::client(config).map_err(|e| e.to_string())?;
        let conn = tokio::time::timeout(
            Duration::from_secs(5),
            endpoint.connect(format!("https://{}", cfg.edge)),
        )
        .await
        .map_err(|_| "connect timed out".to_string())?
        .map_err(|e| format!("connect failed: {e}"))?;

        let opening = conn
            .open_bi()
            .await
            .map_err(|e| format!("stream open failed: {e}"))?;
        let (mut writer, mut reader) = opening
            .await
            .map_err(|e| format!("stream establish failed: {e}"))?;

        let connect = protocol::Connect {
            token: cfg.token.clone(),
            session_id: session,
            name: name.to_owned(),
        };
        writer
            .write_all(&connect.encode().map_err(|e| e.to_string())?)
            .await
            .map_err(|e| format!("connect send failed: {e}"))?;

        // Reply: read more bytes as the frame demands, then decode strictly.
        let mut reply = Vec::with_capacity(8);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline
                .checked_duration_since(tokio::time::Instant::now())
                .ok_or_else(|| "join reply timed out".to_string())?;
            let mut chunk = [0u8; 64];
            let read = tokio::time::timeout(remaining, reader.read(&mut chunk))
                .await
                .map_err(|_| "join reply timed out".to_string())?;
            match read.map_err(|e| format!("reply read failed: {e}"))? {
                Some(n) if n > 0 => reply.extend_from_slice(&chunk[..n]),
                _ => return Err("connection closed before the join reply".into()),
            }
            match Reply::decode(&reply) {
                Ok(Reply::Accepted(accept)) => {
                    return Ok(Bot {
                        conn,
                        writer,
                        reader,
                        stream_buf: Vec::new(),
                        session,
                        client_id: accept.client_id,
                        violations: Vec::new(),
                    });
                }
                Ok(Reply::Rejected(reject)) => return Err(format!("rejected: {}", reject.reason)),
                Err(protocol::JoinError::Truncated { .. }) => continue, // need more bytes
                Err(err) => return Err(format!("malformed reply: {err}")),
            }
        }
    }

    /// Sends one events-channel packet on the control stream.
    pub async fn send_event(&mut self, payload: &[u8]) -> Result<(), String> {
        let packet = Header::new(
            CHANNEL_EVENTS,
            self.session,
            self.client_id,
            1,
            payload.len() as u32,
        )
        .encode_packet(payload)
        .map_err(|e| e.to_string())?;
        self.writer
            .write_all(&packet)
            .await
            .map_err(|e| format!("event send failed: {e}"))
    }

    /// Sends one state-channel input as a datagram.
    pub fn send_input(&self, payload: &[u8]) -> Result<(), String> {
        let packet = Header::new(
            CHANNEL_STATE,
            self.session,
            self.client_id,
            1,
            payload.len() as u32,
        )
        .encode_packet(payload)
        .map_err(|e| e.to_string())?;
        self.conn
            .send_datagram(packet)
            .map_err(|e| format!("datagram send failed: {e}"))
    }

    /// Receives the next packet on either transport, verifying it against
    /// the client-protocol contract. `None` means the connection ended (or
    /// a stream violation made further parsing meaningless).
    pub async fn next(&mut self) -> Option<Packet> {
        loop {
            // Drain complete packets already buffered from the stream.
            if let Some(packet) = self.parse_stream_packet() {
                return Some(packet);
            }
            let mut chunk = [0u8; 4096];
            tokio::select! {
                read = self.reader.read(&mut chunk) => {
                    match read {
                        Ok(Some(n)) if n > 0 => self.stream_buf.extend_from_slice(&chunk[..n]),
                        Ok(_) => return None, // stream ended
                        Err(err) => {
                            self.violations.push(Violation {
                                kind: "stream_read_failed".into(),
                                detail: err.to_string(),
                            });
                            return None;
                        }
                    }
                }
                datagram = self.conn.receive_datagram() => {
                    match datagram {
                        Ok(datagram) => match Header::split_packet(&datagram[..]) {
                            Ok((header, payload)) => {
                                return Some(classify(header, payload.to_vec()));
                            }
                            Err(err) => {
                                self.violations.push(Violation {
                                    kind: "bad_datagram".into(),
                                    detail: err.to_string(),
                                });
                                // Unparseable datagrams are skipped, not fatal.
                            }
                        },
                        Err(_) => return None, // connection lost
                    }
                }
                _ = self.conn.closed() => return None,
            }
        }
    }

    /// Parses one complete packet out of the buffered stream bytes, with
    /// strict framing: a bad header is a recorded violation and fatal.
    fn parse_stream_packet(&mut self) -> Option<Packet> {
        parse_stream(&mut self.stream_buf, &mut self.violations)
    }

    /// Stops reading (simulates a wedged client) and waits out the run.
    pub async fn stall(&self, until: tokio::time::Instant) {
        tokio::time::sleep_until(until).await;
    }
}

/// Parses one packet out of `buf`, recording violations instead of
/// panicking. Public for tests: this is the contract-enforcement core.
#[doc(hidden)]
pub fn parse_stream(buf: &mut Vec<u8>, violations: &mut Vec<Violation>) -> Option<Packet> {
    if buf.len() < BASE_HEADER_SIZE as usize {
        return None;
    }
    match Header::split_packet(buf) {
        Ok((header, payload)) => {
            let total = header.packet_len();
            let packet = classify(header, payload.to_vec());
            buf.drain(..total);
            Some(packet)
        }
        Err(protocol::Error::Truncated { .. }) => None, // need more bytes
        Err(err) => {
            violations.push(Violation {
                kind: "bad_stream_header".into(),
                detail: format!("at stream offset {}: {err}", buf.len()),
            });
            None // fatal: framing is lost
        }
    }
}

fn classify(header: Header, payload: Vec<u8>) -> Packet {
    match header.channel {
        CHANNEL_EVENTS => Packet::Event {
            from: header.client_id,
            payload,
        },
        _ => Packet::State {
            from: header.client_id,
            payload,
        },
    }
}

/// Fetches and parses `/cert-hash` from the lobby endpoint.
async fn fetch_cert_hash(lobby: SocketAddr) -> Result<[u8; 32], String> {
    let mut stream = tokio::net::TcpStream::connect(lobby)
        .await
        .map_err(|e| format!("lobby unreachable: {e}"))?;
    stream
        .write_all(b"GET /cert-hash HTTP/1.1\r\nHost: hub\r\nConnection: close\r\n\r\n")
        .await
        .map_err(|e| format!("lobby request failed: {e}"))?;
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut buf))
        .await
        .map_err(|_| "lobby response timed out".to_string())?
        .map_err(|e| format!("lobby response failed: {e}"))?;
    let text = String::from_utf8_lossy(&buf);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .ok_or("malformed lobby response")?;
    #[derive(serde::Deserialize)]
    struct HashBody {
        sha256: String,
    }
    let parsed: HashBody =
        serde_json::from_str(body.trim()).map_err(|e| format!("bad cert-hash body: {e}"))?;
    let mut hash = [0u8; 32];
    let bytes = parsed.sha256.as_bytes();
    if bytes.len() != 64 {
        return Err("cert-hash is not 64 hex chars".into());
    }
    for (i, slot) in hash.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&parsed.sha256[i * 2..i * 2 + 2], 16)
            .map_err(|_| "cert-hash is not hex".to_string())?;
    }
    Ok(hash)
}
