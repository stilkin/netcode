# Spec Delta

## MODIFIED Requirements

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
