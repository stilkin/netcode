# backplane Specification

## Purpose
Defines the wire protocol between the hub and game-worker processes: how
workers authenticate, register sessions, learn about clients, exchange opaque
packets, and demonstrate liveness. Any worker — regardless of implementation
language — can participate by speaking this protocol alone.

## Requirements

### Requirement: Backplane framing

The backplane carries length-prefixed frames over TCP: a u32 little-endian
byte count, then a u16 little-endian opcode, then the opcode body. The byte
count MUST include the opcode bytes and MUST NOT exceed 256 KiB. A frame that
exceeds the cap, or a first frame that is not `Hello`, MUST cause the hub to
close the connection.

#### Scenario: Oversized frame

- **WHEN** a connection declares a frame larger than 256 KiB
- **THEN** the hub closes the connection without reading the body and no session state changes

#### Scenario: Valid frame

- **WHEN** a connection sends a well-formed frame within the cap
- **THEN** the hub parses the opcode and dispatches it

### Requirement: Authentication

The first frame on every backplane connection MUST be `Hello` carrying a
token. When no token is configured the token MAY be empty and is accepted.
When a token is configured, a connection presenting a wrong or missing token
MUST be rejected and closed, and MUST NOT create or modify session state.

#### Scenario: Wrong token

- **WHEN** a token is configured and a connection sends `Hello` with a different token
- **THEN** the connection is closed and no session is created

#### Scenario: No token configured

- **WHEN** no token is configured and a connection sends `Hello` with an empty token
- **THEN** the connection proceeds to registration

### Requirement: Worker registration and session claiming

After authentication, a worker MUST send `Register` carrying a game type, display name, maximum player count, payload schema version, and a claimed session id. If the claimed id is zero the hub MUST assign a fresh unique id and reply `Registered { session_id }`, unless the live-session limit is full. If the claimed id names a pending spawned session it MUST be accepted, including at the live-session limit; claiming an unknown or non-pending id MUST close the connection.

#### Scenario: Self-registered session

- **WHEN** a worker registers with claimed id 0 below the live-session limit
- **THEN** the hub assigns a fresh id, replies `Registered`, and the session appears in the registry

#### Scenario: Self-registration at the limit

- **WHEN** a worker registers with claimed id 0 while the live-session limit is full
- **THEN** the hub closes the connection without allocating an id or changing existing sessions

#### Scenario: Spawned session claimed

- **WHEN** the hub pre-created a pending session with id 7 and the spawned worker registers claiming id 7
- **THEN** the pending session becomes active for that worker connection, including when the limit is full

#### Scenario: Bad claim

- **WHEN** a worker claims a session id that is not pending
- **THEN** the connection is closed and no session is created

### Requirement: Client attach and detach notification

Whenever a client is attached to or detached from a worker's session, the hub
MUST send that worker a `ClientAttached` or `ClientDetached` frame carrying
the client id. Attach orders MUST arrive in the order the hub performed them.

#### Scenario: Attach then detach

- **WHEN** a client attaches to and then detaches from a session
- **THEN** the worker receives `ClientAttached { id }` followed by `ClientDetached { id }`, in that order

### Requirement: In-order packet delivery

Packets between a worker and the hub MUST be delivered in the order they were
sent, per direction, per session. The hub MUST NOT interpret or modify packet
payload bytes; only the client packet header is read for routing.

#### Scenario: Ordered inputs

- **WHEN** one client sends three input packets in order 1, 2, 3
- **THEN** the session worker receives them in the order 1, 2, 3

### Requirement: Packet fan-out

A worker MUST be able to send a client packet three ways: `ToClient` (body is
the packet; the header's client id selects the recipient), `ToMany` (an
explicit client id list followed by the packet), and `Broadcast` (the packet
to every attached client). Broadcast MUST deliver the packet exactly once per
attached client. A `ToClient` addressed to a currently detached client MUST be
dropped silently — stale sends are expected, not errors.

#### Scenario: Broadcast reaches everyone once

- **WHEN** a session has three attached clients and the worker broadcasts a packet
- **THEN** each of the three clients receives exactly one copy

#### Scenario: Stale unicast

- **WHEN** the worker sends `ToClient` for a client that has already detached
- **THEN** the packet is dropped, no error is raised, and the connection stays open

### Requirement: Heartbeat liveness

Workers MUST send `Heartbeat` frames periodically. If the hub receives no
frame at all from an authenticated connection for more than the liveness
timeout, the hub MUST close the connection and apply the session-closure
behavior. Heartbeats MUST NOT be required of the pre-registration phase,
which has its own shorter registration timeout.

#### Scenario: Silent worker

- **WHEN** an authenticated worker sends nothing for longer than the liveness timeout
- **THEN** the hub closes the connection and the worker's sessions are closed

#### Scenario: Idle but alive

- **WHEN** a worker sends only heartbeats (no game output) at the heartbeat interval
- **THEN** the connection stays open indefinitely

### Requirement: Session closure on worker loss

When a worker's backplane connection closes — by disconnect, timeout, or, for
a spawned worker, child-process exit — the hub MUST close that worker's
sessions: detach all clients, remove the sessions from the registry, and free
their capacity. The session ids MUST NOT be reused while the hub process runs.

#### Scenario: Worker crash

- **WHEN** a spawned worker process exits unexpectedly
- **THEN** its session is removed, its attached clients are detached, and the id is not reused
