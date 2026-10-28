# Spec Delta

## Purpose

Extends the shared protocol crate with the handshake frames a client
exchanges with the hub when joining a session. The header (24 bytes,
unchanged) routes every packet; these frames open the conversation.

## ADDED Requirements

### Requirement: Join frame codecs

The `protocol` crate MUST encode and decode the three little-endian join
frames — `Connect` (version, token, session id, player name), `Accept`
(version, client id), `Reject` (version, reason) — exactly as specified in
`crates/protocol/docs/wire-format.md`, and MUST reject a frame whose
version byte is not 1. The codecs MUST remain std-only so wasm32 browser
clients can reuse them, and their byte layouts MUST be pinned by golden
vectors alongside the header vectors.

#### Scenario: Golden vectors

- **WHEN** the golden vector test runs
- **THEN** encoding each sample join frame reproduces the recorded bytes and
  decoding yields the recorded fields, and a vector is never edited without
  a protocol-evolution decision in the change's design doc

#### Scenario: Truncated frame

- **WHEN** a `Connect` buffer is cut short of its declared token length
- **THEN** decoding fails with an error and no partial frame is produced
