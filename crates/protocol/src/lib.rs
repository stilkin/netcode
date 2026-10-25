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
