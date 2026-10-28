//! Shared wire protocol for the netcode hub.
//!
//! Everything that talks to the hub — browser clients, game workers, the hub
//! itself — shares one packet header. This crate is deliberately dependency
//! light (std only, no I/O, no async runtime) so it can be embedded anywhere,
//! including `wasm32` browser clients.
//!
//! The byte layout is specified in [`docs/wire-format.md`]; the requirements it
//! implements live in the `wire-protocol` capability spec.

#![forbid(unsafe_code)]

use std::iter::repeat_n;

use thiserror::Error;

/// Magic bytes every packet starts with: `NHUB`.
pub const MAGIC: [u8; 4] = *b"NHUB";

/// The only protocol version this build speaks.
pub const PROTOCOL_VERSION: u16 = 1;

/// Size of the fixed portion of the header that every receiver understands.
pub const BASE_HEADER_SIZE: u16 = 24;

/// Sanity bound on payload size. Real per-link limits live with the links.
pub const MAX_PAYLOAD_LEN: usize = 64 * 1024;

/// The fixed wire header shared by every link (see `docs/wire-format.md`).
///
/// The header is the *only* thing the hub interprets; `channel` and the
/// payload are opaque to it by design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Protocol version; this build speaks [`PROTOCOL_VERSION`] only.
    pub version: u16,
    /// Total header size in bytes; `>= [`BASE_HEADER_SIZE`]`, unknown extra
    /// bytes belong to a future version of this format.
    pub header_size: u16,
    /// Routing-class hint owned by the link layers. Never validated here.
    pub channel: u8,
    /// Opaque per-link flag bits. Never validated here.
    pub flags: u8,
    /// Session the packet belongs to.
    pub session_id: u32,
    /// Client within the session the packet belongs to.
    pub client_id: u32,
    /// Version of the payload schema agreed between a game and its clients.
    pub payload_schema_version: u16,
    /// Payload size in bytes.
    pub payload_len: u32,
}

/// Everything that can go wrong while encoding or decoding.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Error {
    /// The packet does not start with [`MAGIC`].
    #[error("bad magic: not a netcode hub packet")]
    BadMagic,
    /// The packet declares a protocol version this build cannot speak.
    #[error("unsupported protocol version {got} (this build speaks {supported})")]
    UnsupportedVersion { got: u16, supported: u16 },
    /// The packet declares a header smaller than the fixed portion.
    #[error("header size {got} is smaller than the fixed header ({base} bytes)")]
    HeaderTooSmall { got: u16, base: u16 },
    /// The buffer ends before the header or payload does.
    #[error("truncated packet: need at least {needed} bytes, got {got}")]
    Truncated { needed: usize, got: usize },
    /// A payload exceeds [`MAX_PAYLOAD_LEN`].
    #[error("payload too large: {len} bytes exceeds the {max}-byte limit")]
    PayloadTooLarge { len: usize, max: usize },
    /// The header's `payload_len` disagrees with the payload handed to encode.
    #[error("payload length mismatch: header declares {declared} bytes, payload is {actual}")]
    PayloadLengthMismatch { declared: usize, actual: usize },
}

impl Header {
    /// A v1 header for a fresh packet with the given routing fields.
    pub fn new(
        channel: u8,
        session_id: u32,
        client_id: u32,
        payload_schema_version: u16,
        payload_len: u32,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            header_size: BASE_HEADER_SIZE,
            channel,
            flags: 0,
            session_id,
            client_id,
            payload_schema_version,
            payload_len,
        }
    }

    /// Bytes this header occupies on the wire (fixed part + unknown extras).
    pub fn wire_len(&self) -> usize {
        self.header_size as usize
    }

    /// Total packet size: header plus payload.
    pub fn packet_len(&self) -> usize {
        self.wire_len() + self.payload_len as usize
    }

    /// Appends the header to `out`.
    ///
    /// Bytes beyond the 24 this build knows are written as zero padding, which
    /// models how a future encoder would carry extension fields.
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        debug_assert!(self.header_size >= BASE_HEADER_SIZE);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.header_size.to_le_bytes());
        out.push(self.channel);
        out.push(self.flags);
        out.extend_from_slice(&self.session_id.to_le_bytes());
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&self.payload_schema_version.to_le_bytes());
        out.extend_from_slice(&self.payload_len.to_le_bytes());
        let padding = self.header_size as usize - BASE_HEADER_SIZE as usize;
        out.extend(repeat_n(0u8, padding));
    }

    /// Encodes header plus payload into a single packet buffer.
    pub fn encode_packet(&self, payload: &[u8]) -> Result<Vec<u8>, Error> {
        if payload.len() > MAX_PAYLOAD_LEN {
            return Err(Error::PayloadTooLarge {
                len: payload.len(),
                max: MAX_PAYLOAD_LEN,
            });
        }
        if payload.len() != self.payload_len as usize {
            return Err(Error::PayloadLengthMismatch {
                declared: self.payload_len as usize,
                actual: payload.len(),
            });
        }
        let mut out = Vec::with_capacity(self.packet_len());
        self.encode_into(&mut out);
        out.extend_from_slice(payload);
        Ok(out)
    }

    /// Encodes just the header.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.wire_len());
        self.encode_into(&mut out);
        out
    }

    /// Decodes the fixed portion from the start of `buf`.
    ///
    /// Magic, version, and header size are validated before anything else, so
    /// foreign or future packets are rejected without touching the payload.
    /// The payload does not need to be present; use [`Header::split_packet`]
    /// for that.
    pub fn decode(buf: &[u8]) -> Result<Self, Error> {
        if buf.len() < BASE_HEADER_SIZE as usize {
            return Err(Error::Truncated {
                needed: BASE_HEADER_SIZE as usize,
                got: buf.len(),
            });
        }
        let magic = [buf[0], buf[1], buf[2], buf[3]];
        if magic != MAGIC {
            return Err(Error::BadMagic);
        }
        let version = le_u16(&buf[4..6]);
        if version == 0 || version > PROTOCOL_VERSION {
            return Err(Error::UnsupportedVersion {
                got: version,
                supported: PROTOCOL_VERSION,
            });
        }
        let header_size = le_u16(&buf[6..8]);
        if header_size < BASE_HEADER_SIZE {
            return Err(Error::HeaderTooSmall {
                got: header_size,
                base: BASE_HEADER_SIZE,
            });
        }
        let header = Self {
            version,
            header_size,
            channel: buf[8],
            flags: buf[9],
            session_id: le_u32(&buf[10..14]),
            client_id: le_u32(&buf[14..18]),
            payload_schema_version: le_u16(&buf[18..20]),
            payload_len: le_u32(&buf[20..24]),
        };
        if header.payload_len as usize > MAX_PAYLOAD_LEN {
            return Err(Error::PayloadTooLarge {
                len: header.payload_len as usize,
                max: MAX_PAYLOAD_LEN,
            });
        }
        Ok(header)
    }

    /// Decodes a full packet, returning the header and its payload slice.
    /// Trailing bytes after the payload are ignored.
    pub fn split_packet(buf: &[u8]) -> Result<(Self, &[u8]), Error> {
        let header = Self::decode(buf)?;
        let start = header.wire_len();
        let end = start + header.payload_len as usize;
        if buf.len() < end {
            return Err(Error::Truncated {
                needed: end,
                got: buf.len(),
            });
        }
        Ok((header, &buf[start..end]))
    }
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

// ---------------------------------------------------------------------------
// Join frames — the handshake a client exchanges with the hub when joining a
// session. Frames are self-describing (kind byte + version) so replies can be
// decoded without knowing which answer to expect. Layout: `docs/wire-format.md`.
// ---------------------------------------------------------------------------

/// The only join-frame version this build speaks.
pub const JOIN_VERSION: u8 = 1;

/// Kind byte: a client's request to join a session.
pub const KIND_CONNECT: u8 = 0x01;
/// Kind byte: the hub's acceptance, carrying the assigned client id.
pub const KIND_ACCEPT: u8 = 0x02;
/// Kind byte: the hub's refusal, carrying a human-readable reason.
pub const KIND_REJECT: u8 = 0x03;

/// Sanity bound on the shared-secret token.
pub const MAX_TOKEN_LEN: usize = 256;
/// Sanity bound on player and session display names.
pub const MAX_NAME_LEN: usize = 64;
/// Sanity bound on reject reasons.
pub const MAX_REASON_LEN: usize = 256;

/// Everything that can go wrong while decoding a join frame.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JoinError {
    /// The buffer ends before the frame does.
    #[error("truncated join frame: need at least {needed} bytes, got {got}")]
    Truncated { needed: usize, got: usize },
    /// The kind byte names a frame this build does not speak.
    #[error("unknown join frame kind 0x{got:02x}")]
    UnknownKind { got: u8 },
    /// The frame declares a version this build cannot speak.
    #[error("unsupported join frame version {got} (this build speaks {supported})")]
    UnsupportedVersion { got: u8, supported: u8 },
    /// A string field is not valid UTF-8.
    #[error("join frame string field is not valid UTF-8")]
    BadUtf8,
    /// A length field exceeds its sanity bound.
    #[error("join frame field length {len} exceeds the {max}-byte limit")]
    FieldTooLarge { len: usize, max: usize },
}

/// A client's request to join a session (kind [`KIND_CONNECT`]).
///
/// Wire: `01 | version u8 | token_len u16 | token | session_id u32 |
/// name_len u16 | name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connect {
    /// Shared secret; empty when the hub runs open (LAN trust).
    pub token: String,
    /// The session to join.
    pub session_id: u32,
    /// The player's display name.
    pub name: String,
}

impl Connect {
    /// Encodes the frame; rejects over-long fields at the boundary.
    pub fn encode(&self) -> Result<Vec<u8>, JoinError> {
        let token = self.token.as_bytes();
        let name = self.name.as_bytes();
        if token.len() > MAX_TOKEN_LEN {
            return Err(JoinError::FieldTooLarge {
                len: token.len(),
                max: MAX_TOKEN_LEN,
            });
        }
        if name.len() > MAX_NAME_LEN {
            return Err(JoinError::FieldTooLarge {
                len: name.len(),
                max: MAX_NAME_LEN,
            });
        }
        let mut out = Vec::with_capacity(10 + token.len() + name.len());
        out.push(KIND_CONNECT);
        out.push(JOIN_VERSION);
        out.extend_from_slice(&(token.len() as u16).to_le_bytes());
        out.extend_from_slice(token);
        out.extend_from_slice(&self.session_id.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        Ok(out)
    }

    /// Decodes the frame from `buf` (trailing bytes are ignored).
    pub fn decode(buf: &[u8]) -> Result<Self, JoinError> {
        let mut cur = Cursor::new(buf);
        expect_header(&mut cur, KIND_CONNECT)?;
        let token = take_str(&mut cur, MAX_TOKEN_LEN)?;
        let session_id = cur.u32()?;
        let name = take_str(&mut cur, MAX_NAME_LEN)?;
        Ok(Self {
            token,
            session_id,
            name,
        })
    }
}

/// The hub's acceptance (kind [`KIND_ACCEPT`]): the client id it assigned.
///
/// Wire: `02 | version u8 | client_id u32`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accept {
    /// The assigned client id; valid within the joined session.
    pub client_id: u32,
}

impl Accept {
    /// Encodes the frame.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6);
        out.push(KIND_ACCEPT);
        out.push(JOIN_VERSION);
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out
    }

    /// Decodes the frame from `buf` (trailing bytes are ignored).
    pub fn decode(buf: &[u8]) -> Result<Self, JoinError> {
        let mut cur = Cursor::new(buf);
        expect_header(&mut cur, KIND_ACCEPT)?;
        Ok(Self {
            client_id: cur.u32()?,
        })
    }
}

/// The hub's refusal (kind [`KIND_REJECT`]): why the join failed.
///
/// Wire: `03 | version u8 | reason_len u16 | reason`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reject {
    /// Human-readable reason, shown by clients as-is.
    pub reason: String,
}

impl Reject {
    /// Encodes the frame.
    pub fn encode(&self) -> Vec<u8> {
        let reason = self.reason.as_bytes();
        let mut out = Vec::with_capacity(4 + reason.len());
        out.push(KIND_REJECT);
        out.push(JOIN_VERSION);
        out.extend_from_slice(&(reason.len() as u16).to_le_bytes());
        out.extend_from_slice(reason);
        out
    }

    /// Decodes the frame from `buf` (trailing bytes are ignored).
    pub fn decode(buf: &[u8]) -> Result<Self, JoinError> {
        let mut cur = Cursor::new(buf);
        expect_header(&mut cur, KIND_REJECT)?;
        Ok(Self {
            reason: take_str(&mut cur, MAX_REASON_LEN)?,
        })
    }
}

/// A hub reply: whichever of [`Accept`] or [`Reject`] the hub sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The join succeeded.
    Accepted(Accept),
    /// The join failed; the reason says why.
    Rejected(Reject),
}

impl Reply {
    /// Decodes either reply kind by inspecting the kind byte.
    pub fn decode(buf: &[u8]) -> Result<Self, JoinError> {
        match buf.first() {
            Some(&KIND_ACCEPT) => Ok(Self::Accepted(Accept::decode(buf)?)),
            Some(&KIND_REJECT) => Ok(Self::Rejected(Reject::decode(buf)?)),
            Some(&other) => Err(JoinError::UnknownKind { got: other }),
            None => Err(JoinError::Truncated { needed: 1, got: 0 }),
        }
    }
}

/// Minimal cursor so frame decoding stays allocation-free and std-only.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], JoinError> {
        let end = self.pos.checked_add(n).ok_or(JoinError::FieldTooLarge {
            len: usize::MAX,
            max: 0,
        })?;
        if end > self.buf.len() {
            return Err(JoinError::Truncated {
                needed: end,
                got: self.buf.len(),
            });
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, JoinError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u16(&mut self) -> Result<u16, JoinError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
}

/// Consumes the shared frame header: kind byte, version byte.
fn expect_header(cur: &mut Cursor<'_>, want_kind: u8) -> Result<(), JoinError> {
    let b = cur.take(2)?;
    if b[0] != want_kind {
        return Err(JoinError::UnknownKind { got: b[0] });
    }
    if b[1] != JOIN_VERSION {
        return Err(JoinError::UnsupportedVersion {
            got: b[1],
            supported: JOIN_VERSION,
        });
    }
    Ok(())
}

/// Consumes a `u16` length-prefixed UTF-8 string, bounded by `max`.
fn take_str(cur: &mut Cursor<'_>, max: usize) -> Result<String, JoinError> {
    let len = cur.u16()? as usize;
    if len > max {
        return Err(JoinError::FieldTooLarge { len, max });
    }
    let bytes = cur.take(len)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| JoinError::BadUtf8)
}
