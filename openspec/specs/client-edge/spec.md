# client-edge Specification

## Purpose
The hub's browser-facing endpoint: how desktop browsers reach the hub over
WebTransport, prove they may join, and exchange state/event packets with a
game session — with the hub still routing by header only.

## Requirements

### Requirement: WebTransport endpoint

The hub MUST accept client connections over WebTransport on the configured
`client_edge.bind` address, authenticated by TLS with a self-signed
certificate generated at boot. The certificate MUST be ECDSA P-256 (browser
WebTransport compatibility) and the endpoint MUST log its listen address and
the SHA-256 hash of the certificate DER at startup.

#### Scenario: Startup

- **WHEN** the hub boots with the default `[client_edge]` configuration
- **THEN** a WebTransport listener is bound to `0.0.0.0:4433` and the startup
  log names the address and the current certificate hash

#### Scenario: Certificate rotates per boot

- **WHEN** the hub restarts
- **THEN** a fresh certificate is generated and the logged hash differs from
  the previous boot's

### Requirement: Connect handshake

A client MUST open exactly one bidirectional stream immediately after the
WebTransport session is established and send a `Connect` frame on it. The
hub MUST answer `Accept` (carrying the assigned client id) or `Reject`
(carrying a human-readable reason) and MUST close the session after a
`Reject`. A session that sends anything other than a well-formed `Connect`
as its first stream frame MUST be closed. One connection joins at most one
session as one client.

#### Scenario: Successful join

- **WHEN** a client sends `Connect` naming an active session with a free slot
- **THEN** the hub replies `Accept` with the client id, the client counts
  toward the session's player count, and the worker observes the attach

#### Scenario: Garbage first frame

- **WHEN** the first frame on the stream is not a well-formed `Connect`
- **THEN** the hub closes the connection and no session state changes

### Requirement: Token gate

When `client_edge.token` is empty the endpoint MUST accept any `Connect`
(open LAN trust, mirroring the backplane). When a token is configured, a
`Connect` presenting a wrong or missing token MUST be rejected with a
`Reject` frame, and no session state may change.

#### Scenario: Open LAN

- **WHEN** no token is configured and a client sends `Connect` with an empty
  token
- **THEN** the handshake proceeds normally

#### Scenario: Wrong token

- **WHEN** a token is configured and a client presents a different token
- **THEN** the hub replies `Reject` and closes the session without attaching

### Requirement: Session join rules

The hub MUST reject a `Connect` whose session does not exist, is not active
(no worker has claimed it), or is at capacity, with a `Reject` naming the
reason. A rejected client never appears in the session's player count.

#### Scenario: Unknown session

- **WHEN** a client connects with session id 4242 and no such session exists
- **THEN** the reply is `Reject` with a reason naming the session

#### Scenario: Full session

- **WHEN** a session is at its configured player capacity and a new client
  connects
- **THEN** the reply is `Reject` naming capacity, and existing players are
  unaffected

### Requirement: State relay over datagrams

After a successful handshake, packets on the state channel (channel 0) MUST
be carried as WebTransport datagrams in both directions, each datagram
containing exactly one full packet (header + payload). A hub-to-client state
packet larger than the transport's datagram limit MUST be dropped and logged,
never queued or split. Datagram sends MUST never block the sending of other
clients' packets.

#### Scenario: Input round trip

- **WHEN** an accepted client sends a state datagram containing a valid
  packet for its own session and client id
- **THEN** the worker receives that input, and a reply the worker unicasts
  to that client arrives as a datagram on the client's connection

#### Scenario: Oversized unicast

- **WHEN** a worker unicasts a state packet exceeding the datagram limit
- **THEN** the packet is dropped for that client, a warning is logged, and
  other clients' traffic is unaffected

### Requirement: Event relay over the control stream

After a successful handshake, packets on the events channel (channel 1) MUST
flow over the handshake's bidirectional stream in both directions, reliable
and ordered, as a concatenation of self-delimiting packets (the header's
length field frames the stream). A slow-reading client MUST stall only its
own connection's relay — other clients' packets continue flowing.

#### Scenario: Event delivery in order

- **WHEN** a worker sends two event packets to one client
- **THEN** the client receives both on the control stream, in order, exactly
  once

#### Scenario: Slow reader isolation

- **WHEN** one client stops reading its control stream while others are
  active
- **THEN** only that client's relay stalls; the others' datagrams and events
  continue

### Requirement: Connection lifecycle

When a client's WebTransport session closes or breaks — for any reason — the
hub MUST detach the client exactly as an in-process handle drop does: the
worker observes the detach and the session's player count decrements. On hub
shutdown, the hub MUST stop accepting connections and close all client
sessions within the drain grace period.

#### Scenario: Browser tab closed

- **WHEN** an accepted client's connection is closed abruptly
- **THEN** the worker observes the detach and the session's player count
  returns to its prior value

#### Scenario: Hub shutdown

- **WHEN** the hub receives SIGTERM while clients are connected
- **THEN** all client sessions are closed cleanly and the process still
  exits 0
