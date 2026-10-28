# client-protocol Specification

## Purpose
The contract a browser client implements against the hub: the join-frame
byte layout, how wire channels map onto WebTransport primitives, and the
framing rules of the control stream. Together with the packet header
(wire-protocol), this is everything a client needs to play a game.

## Requirements

### Requirement: Join frames

The handshake exchanges three little-endian binary frames — `Connect`
(client → hub), `Accept` and `Reject` (hub → client) — whose exact byte
layouts are specified in `crates/protocol/docs/wire-format.md` and
encoded/decoded by the `protocol` crate. A frame with an unknown version
MUST be treated as malformed by the receiver.

#### Scenario: Frame layouts

- **WHEN** `Connect` carries token "s3cret", session 7, name "p1"; `Accept`
  carries client id 2; `Reject` carries reason "session full"
- **THEN** each frame encodes to and decodes from exactly the bytes laid
  out in `wire-format.md`, pinned by golden vectors in
  `crates/protocol/tests/golden.rs`

#### Scenario: Unknown version

- **WHEN** a `Connect` frame arrives with version 9
- **THEN** the hub treats it as malformed and closes the connection

### Requirement: Channel-to-transport mapping

A client MUST map wire channels onto WebTransport primitives exactly as
follows: state (channel 0) ⇄ datagrams, one full packet per datagram, both
directions; events (channel 1) ⇄ the control stream after the handshake,
both directions. The client MUST NOT send state-channel packets on the
stream or event-channel packets as datagrams.

#### Scenario: Mapping is total

- **WHEN** a game sends both state snapshots and event messages
- **THEN** every state packet crosses the connection as exactly one
  datagram and every event packet crosses the control stream in order

### Requirement: Control stream framing

The control stream carries the `Connect`/`Accept` handshake first; from then
on it is a concatenation of full packets (24-byte header + payload), framed
by the header's length field — no additional framing. A receiver MUST
decode the stream incrementally and MUST discard the remainder if a header
fails validation (magic, version, length).

#### Scenario: Two events back to back

- **WHEN** a worker unicasts event packets of 3 and 300 payload bytes to one
  client
- **THEN** the client's stream reader yields exactly two packets, using only
  the headers' length fields to find the boundaries

#### Scenario: Junk on the stream

- **WHEN** garbage follows a valid packet on the control stream
- **THEN** the stream reader detects the invalid header and the connection
  is closed
