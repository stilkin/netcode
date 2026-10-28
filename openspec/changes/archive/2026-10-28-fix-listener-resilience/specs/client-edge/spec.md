# Spec Delta

## ADDED Requirements

### Requirement: Connection admission isolation

The hub MUST negotiate WebTransport sessions independently. A peer stalled during negotiation MUST NOT prevent other clients from joining or delay shutdown beyond the drain grace period.

#### Scenario: Stalled negotiation is isolated

- **WHEN** a peer establishes QUIC but stops before completing WebTransport negotiation
- **THEN** another client can still join, and the edge shuts down within its drain grace period

### Requirement: Inbound stream bounds

The hub MUST bound incomplete client-to-hub control-stream data to 256 KiB. A client exceeding that bound MUST be disconnected and detached without affecting other clients.

#### Scenario: Unframed stream flood

- **WHEN** an accepted client sends more than 256 KiB without completing a valid packet
- **THEN** only that connection is closed and its client is detached
