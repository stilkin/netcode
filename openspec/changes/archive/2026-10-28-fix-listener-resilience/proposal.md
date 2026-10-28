# Proposal

## Why

One misbehaving client can currently stall WebTransport admission for everyone, grow an inbound stream buffer without bound, or hold a lobby HTTP request open forever. These are availability bugs at the hub's LAN-facing boundaries and should be fixed before shipping.

## What Changes

- Move WebTransport session negotiation into the existing supervised connection-task lifecycle so a stalled peer cannot block other joins or shutdown.
- Bound incomplete client-to-hub control-stream buffering to 256 KiB and disconnect only the offending client when it exceeds the cap.
- Apply a total 30-second deadline to each lobby HTTP request, closing expired connections while keeping the service available.
- Add focused regressions for all three behaviors, using a hash-pinned raw-QUIC fixture and Tokio paused time.

Out of scope: connection-count or inbound-rate limits, lobby drain redesign, new runtime configuration, public API or wire changes, and unrelated security or simplification findings.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `client-edge`: WebTransport negotiation must be isolated per peer, and incomplete inbound control-stream data must be bounded.
- `lobby-api`: every HTTP request must have a total deadline and expire without affecting other requests.

## Impact

- `crates/hub/src/client_edge/mod.rs`: supervised negotiation and inbound stream cap.
- `crates/hub/src/lobby/mod.rs`: total request deadline.
- `crates/hub/tests/{client_edge,lobby}.rs`: three regression tests and fixtures.
- `crates/hub/Cargo.toml`: dev-only quinn and Tokio `test-util` support; no production dependency changes.
- `openspec/specs/{client-edge,lobby-api}/spec.md`: three new observable requirements.
