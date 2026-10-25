# wire-protocol Specification

## Purpose
Establishes the shared packet header and framing contract used by every link in
the netcode system (client-to-hub and hub-to-game), so that any client
technology or game process can interoperate with the hub by implementing this
byte format alone, without sharing code with it.

## Requirements

### Requirement: Canonical packet header

Every packet MUST begin with a fixed-size, little-endian 24-byte header consisting
of: magic (u32), protocol version (u16), header size in bytes (u16), channel
(u8), flags (u8), session id (u32), client id (u32), payload schema version
(u16), payload length in bytes (u32). The header MUST be followed by exactly
payload-length bytes of opaque payload.

#### Scenario: Round trip

- **WHEN** a header with any valid field values is encoded and then decoded
- **THEN** all fields compare equal and exactly 24 bytes are consumed

#### Scenario: Payload boundary

- **WHEN** a buffer contains a header, its payload, and trailing bytes
- **THEN** decoding yields a payload slice of exactly payload-length bytes and ignores the trailing bytes

### Requirement: Magic and version rejection

Parsers MUST reject packets whose magic is not the hub magic, or whose declared
protocol version is greater than the implementation's maximum supported
version, before interpreting any other field or payload byte. Rejection MUST
NOT affect receiver state.

#### Scenario: Foreign packet

- **WHEN** a packet arrives whose magic is not the hub magic
- **THEN** it is rejected with a bad-magic error and no state changes

#### Scenario: Newer protocol version

- **WHEN** a packet declares a protocol version the receiver does not support
- **THEN** it is rejected with an unsupported-version error and the receiver continues operating

### Requirement: Forward-compatible header growth

Receivers MUST use the header-size field to locate the payload, MUST accept
headers larger than the 24 bytes they know, and MUST treat unknown header bytes
as opaque.

#### Scenario: Header from the future

- **WHEN** a packet arrives with header size 32
- **THEN** the known fields parse correctly, the 8 unknown bytes are ignored, and the payload is read from byte offset 32

### Requirement: Channel and payload opacity

The codec MUST NOT interpret or validate the channel field and MUST deliver
channel and payload to the caller unchanged. Channel semantics SHALL be owned
by the link-layer specs, not the codec.

#### Scenario: Unknown channel round trip

- **WHEN** a packet with an unassigned channel value is encoded and decoded
- **THEN** the channel value is returned unchanged and no error occurs

### Requirement: Golden vector stability

The wire format SHALL be pinned by committed golden vectors: fixed byte
sequences with expected decoded fields, kept in a single reviewed test file.
Changing a vector MUST require a deliberate protocol-evolution decision, never
a refactoring side effect.

#### Scenario: Vectors pass

- **WHEN** the protocol test suite runs
- **THEN** every committed golden vector encodes to, and decodes from, exactly its committed bytes
