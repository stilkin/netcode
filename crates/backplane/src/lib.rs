//! Backplane wire protocol between the hub and game workers.
//!
//! Framing is `[u32 len][u16 opcode][body]`, little-endian, where `len`
//! counts the opcode plus body and is capped at [`MAX_FRAME_LEN`]. Client
//! packets (see the `protocol` crate) ride as opaque bodies of the routing
//! frames; the full byte layout is specified in `docs/backplane-format.md`.

#![forbid(unsafe_code)]

pub mod conn;

use thiserror::Error;

/// Hard cap on one frame, inclusive of opcode bytes.
pub const MAX_FRAME_LEN: usize = 256 * 1024;

/// Backplane opcodes (protocol v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Hello,
    Register,
    Registered,
    Heartbeat,
    ClientAttached,
    ClientDetached,
    ToClient,
    ToMany,
    Broadcast,
    FromClient,
}

impl Opcode {
    pub fn to_u16(self) -> u16 {
        match self {
            Opcode::Hello => 1,
            Opcode::Register => 2,
            Opcode::Registered => 3,
            Opcode::Heartbeat => 4,
            Opcode::ClientAttached => 5,
            Opcode::ClientDetached => 6,
            Opcode::ToClient => 7,
            Opcode::ToMany => 8,
            Opcode::Broadcast => 9,
            Opcode::FromClient => 10,
        }
    }

    pub fn from_u16(value: u16) -> Option<Self> {
        Some(match value {
            1 => Opcode::Hello,
            2 => Opcode::Register,
            3 => Opcode::Registered,
            4 => Opcode::Heartbeat,
            5 => Opcode::ClientAttached,
            6 => Opcode::ClientDetached,
            7 => Opcode::ToClient,
            8 => Opcode::ToMany,
            9 => Opcode::Broadcast,
            10 => Opcode::FromClient,
            _ => return None,
        })
    }
}

/// One backplane frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// w→h, first frame: shared token (empty when the hub has none).
    Hello { token: String },
    /// w→h: claim a pending session (`claimed_id != 0`) or ask for one (0).
    Register {
        claimed_id: u32,
        game_type: String,
        display_name: String,
        max_players: u16,
        payload_schema_version: u16,
    },
    /// h→w: the hub accepted the session under this id.
    Registered { session_id: u32 },
    /// both: liveness, empty body.
    Heartbeat,
    /// h→w: a client joined the worker's session.
    ClientAttached { client_id: u32 },
    /// h→w: a client left the worker's session.
    ClientDetached { client_id: u32 },
    /// w→h: packet to the client named in the packet header.
    ToClient { packet: Vec<u8> },
    /// w→h: packet to each listed client.
    ToMany {
        client_ids: Vec<u32>,
        packet: Vec<u8>,
    },
    /// w→h: packet to every attached client.
    Broadcast { packet: Vec<u8> },
    /// h→w: a client packet for the worker's session.
    FromClient { packet: Vec<u8> },
}

/// Codec-level failures. The connection is closed on any of these.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum Error {
    #[error("frame too large: {len} bytes exceeds the {max}-byte limit")]
    TooLarge { len: usize, max: usize },
    #[error("unknown backplane opcode {0}")]
    UnknownOpcode(u16),
    #[error("frame shorter than its opcode")]
    Truncated { needed: usize, got: usize },
    #[error("malformed body for opcode {0}")]
    MalformedBody(u16),
    #[error("string field is not valid UTF-8")]
    InvalidUtf8,
}

impl Frame {
    /// The frame's opcode.
    pub fn opcode(&self) -> Opcode {
        match self {
            Frame::Hello { .. } => Opcode::Hello,
            Frame::Register { .. } => Opcode::Register,
            Frame::Registered { .. } => Opcode::Registered,
            Frame::Heartbeat => Opcode::Heartbeat,
            Frame::ClientAttached { .. } => Opcode::ClientAttached,
            Frame::ClientDetached { .. } => Opcode::ClientDetached,
            Frame::ToClient { .. } => Opcode::ToClient,
            Frame::ToMany { .. } => Opcode::ToMany,
            Frame::Broadcast { .. } => Opcode::Broadcast,
            Frame::FromClient { .. } => Opcode::FromClient,
        }
    }

    /// Encodes the frame (length prefix included) onto the end of `out`.
    pub fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), Error> {
        let start = out.len();
        out.extend_from_slice(&0u32.to_le_bytes()); // length placeholder
        out.extend_from_slice(&self.opcode().to_u16().to_le_bytes());
        let mut body = BodyWriter::default();
        match self {
            Frame::Hello { token } => body.write_str(token)?,
            Frame::Register {
                claimed_id,
                game_type,
                display_name,
                max_players,
                payload_schema_version,
            } => {
                body.write_u32(*claimed_id);
                body.write_str(game_type)?;
                body.write_str(display_name)?;
                body.write_u16(*max_players);
                body.write_u16(*payload_schema_version);
            }
            Frame::Registered { session_id } => body.write_u32(*session_id),
            Frame::Heartbeat => {}
            Frame::ClientAttached { client_id } => body.write_u32(*client_id),
            Frame::ClientDetached { client_id } => body.write_u32(*client_id),
            Frame::ToClient { packet }
            | Frame::Broadcast { packet }
            | Frame::FromClient { packet } => {
                body.write_bytes(packet);
            }
            Frame::ToMany { client_ids, packet } => {
                body.write_u16(client_ids.len().try_into().map_err(|_| Error::TooLarge {
                    len: client_ids.len(),
                    max: u16::MAX as usize,
                })?);
                for id in client_ids {
                    body.write_u32(*id);
                }
                body.write_bytes(packet);
            }
        }
        out.extend_from_slice(&body.finish());
        // The length prefix covers opcode + body only — never itself.
        let len = out.len() - start - 4;
        if len > MAX_FRAME_LEN {
            out.truncate(start);
            return Err(Error::TooLarge {
                len,
                max: MAX_FRAME_LEN,
            });
        }
        out[start..start + 4].copy_from_slice(&(len as u32).to_le_bytes());
        Ok(())
    }

    /// Encodes the frame into a fresh buffer.
    pub fn encode_to_vec(&self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        Ok(out)
    }

    /// Decodes a frame from `bytes`, which must be the opcode plus body
    /// (i.e. exactly what the length prefix counted).
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < 2 {
            return Err(Error::Truncated {
                needed: 2,
                got: bytes.len(),
            });
        }
        let opcode = Opcode::from_u16(u16::from_le_bytes([bytes[0], bytes[1]])).ok_or(
            Error::UnknownOpcode(u16::from_le_bytes([bytes[0], bytes[1]])),
        )?;
        let mut body = BodyReader::new(&bytes[2..], opcode);
        Ok(match opcode {
            Opcode::Hello => Frame::Hello {
                token: body.read_str()?,
            },
            Opcode::Register => Frame::Register {
                claimed_id: body.read_u32()?,
                game_type: body.read_str()?,
                display_name: body.read_str()?,
                max_players: body.read_u16()?,
                payload_schema_version: body.read_u16()?,
            },
            Opcode::Registered => Frame::Registered {
                session_id: body.read_u32()?,
            },
            Opcode::Heartbeat => Frame::Heartbeat,
            Opcode::ClientAttached => Frame::ClientAttached {
                client_id: body.read_u32()?,
            },
            Opcode::ClientDetached => Frame::ClientDetached {
                client_id: body.read_u32()?,
            },
            Opcode::ToClient => Frame::ToClient {
                packet: body.read_rest().to_vec(),
            },
            Opcode::ToMany => {
                let count = body.read_u16()? as usize;
                let mut client_ids = Vec::with_capacity(count);
                for _ in 0..count {
                    client_ids.push(body.read_u32()?);
                }
                Frame::ToMany {
                    client_ids,
                    packet: body.read_rest().to_vec(),
                }
            }
            Opcode::Broadcast => Frame::Broadcast {
                packet: body.read_rest().to_vec(),
            },
            Opcode::FromClient => Frame::FromClient {
                packet: body.read_rest().to_vec(),
            },
        })
    }
}

/// Cursor over a frame body that fails as [`Error::MalformedBody`] on any
/// out-of-bounds read.
struct BodyReader<'a> {
    bytes: &'a [u8],
    pos: usize,
    opcode: Opcode,
}

impl<'a> BodyReader<'a> {
    fn new(bytes: &'a [u8], opcode: Opcode) -> Self {
        Self {
            bytes,
            pos: 0,
            opcode,
        }
    }

    fn fail<T>(&self) -> Result<T, Error> {
        Err(Error::MalformedBody(self.opcode.to_u16()))
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let Some(end) = self.pos.checked_add(n) else {
            return Err(Error::MalformedBody(self.opcode.to_u16()));
        };
        if end > self.bytes.len() {
            return self.fail();
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_str(&mut self) -> Result<String, Error> {
        let len = self.read_u16()? as usize;
        let raw = self.take(len)?;
        String::from_utf8(raw.to_vec()).map_err(|_| Error::InvalidUtf8)
    }

    fn read_rest(&mut self) -> &'a [u8] {
        let rest = &self.bytes[self.pos..];
        self.pos = self.bytes.len();
        rest
    }
}

/// Accumulates a frame body.
#[derive(Default)]
struct BodyWriter {
    buf: Vec<u8>,
}

impl BodyWriter {
    fn write_u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    fn write_str(&mut self, value: &str) -> Result<(), Error> {
        let len = value.len();
        let len = u16::try_from(len).map_err(|_| Error::TooLarge {
            len,
            max: u16::MAX as usize,
        })?;
        self.write_u16(len);
        self.buf.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn write_bytes(&mut self, value: &[u8]) {
        self.buf.extend_from_slice(value);
    }

    fn finish(self) -> Vec<u8> {
        self.buf
    }
}
