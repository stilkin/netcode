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
