# session-registry Specification

## Purpose
Defines the hub-side bookkeeping for live sessions — identity, metadata,
capacity, and lifecycle — independent of any particular transport, so both the
backplane (today) and the browser edge and admin API (later changes) observe
and manipulate the same truth.

## Requirements

### Requirement: Unique session ids

The hub MUST assign each session a unique u32 id, MUST NOT reuse an id for
the lifetime of the hub process, and MUST NOT assign duplicate ids to
concurrent registrations.

#### Scenario: Concurrent registrations

- **WHEN** two workers register sessions at the same time
- **THEN** they receive distinct session ids

### Requirement: Session metadata

The hub MUST track, per session: game type, display name, maximum player
count, payload schema version, and current player count. Player count MUST
increase by one per attach and decrease by one per detach.

#### Scenario: Player count tracks membership

- **WHEN** two clients attach and one detaches
- **THEN** the session's player count reads 1

### Requirement: Capacity enforcement

Attaching a client to a session that is already at its maximum player count
MUST fail, MUST NOT change the session's state, and the failing attach MUST
NOT affect other sessions.

#### Scenario: Full session

- **WHEN** a session with max 2 players already has 2 attached clients and a third attach is attempted
- **THEN** the third attach fails and the session still reports 2 players

### Requirement: Session lifecycle

A session MUST transition through: pending (spawned, awaiting its worker),
active (worker registered), and closed (worker gone or hub shutting down).
Only active sessions MUST accept attaches. Closing MUST detach all remaining
clients and MUST be idempotent — closing an already-closed session MUST have
no effect.

#### Scenario: Pending is not joinable

- **WHEN** a client attach targets a pending session whose worker has not registered
- **THEN** the attach fails and no client id is consumed

#### Scenario: Idempotent close

- **WHEN** a session is closed twice
- **THEN** the second close has no effect and no clients are re-detached

### Requirement: Closed session history

The hub MUST retain metadata for at most the 1,024 most recently closed sessions. Exceeding that history MUST evict the oldest closed entry; open sessions MUST NOT be evicted, and evicted IDs MUST NOT be reused.

#### Scenario: Oldest closed session is evicted

- **WHEN** closing a session pushes the closed history beyond 1,024 entries
- **THEN** the oldest closed ID behaves as unknown, while the newest retains closed metadata and remains not joinable

#### Scenario: Repeated close does not consume history

- **WHEN** an already closed session is closed again
- **THEN** no additional history entry is added and no other session is evicted

### Requirement: Live session capacity

The registry MUST reject creation that would exceed its configured limit of pending and active sessions. Closed sessions MUST NOT count toward the limit, a rejected creation MUST NOT allocate an id or change state, and closing a live session MUST free its capacity.

#### Scenario: Creation at the limit

- **WHEN** pending and active sessions already fill the configured limit and another session is requested
- **THEN** creation fails without allocating an id, while existing sessions remain unchanged

#### Scenario: Close frees capacity

- **WHEN** a live session closes after another creation was rejected at the limit
- **THEN** a subsequent creation succeeds and receives the next never-used id
