# hub-sdk Specification

## Purpose
Defines the worker-side contract that game developers implement: a single
`GameSession` trait plus output helpers, with the SDK owning every networking
concern — connection, registration, heartbeats, input buffering, and the tick
loop — so a new game requires no transport code.

## Requirements

### Requirement: Game session runtime

The SDK MUST connect to the hub, authenticate, register or claim its session,
and then drive the game by invoking, in order: `on_attach` per attached
client, `tick` once per configured tick interval with all inputs received
since the previous tick, and `on_detach` per detached client. `on_attach`
MUST receive the client id and player name from the attachment notification.
The tick rate MUST come from the session configuration.

#### Scenario: Inputs reach the game

- **WHEN** a client sends two inputs between two ticks of a 30 Hz session
- **THEN** the next `tick` call observes both inputs

#### Scenario: Attachment name reaches the game

- **WHEN** the SDK receives a client attachment carrying a player name
- **THEN** it invokes `on_attach` once with that client id and the same player name

#### Scenario: Fragmented frames survive worker scheduling

- **WHEN** a backplane frame arrives in fragments while ticks or heartbeats become ready
- **THEN** the SDK delivers the complete frame once, preserves frame order, and keeps the worker connected

### Requirement: Output helpers

The SDK MUST provide the game a context with `to_client`, `to_many`, and
`broadcast` operations that wrap game payloads in client packets and map onto
the backplane fan-out frames. Payloads MUST NOT be interpreted or modified by
the SDK.

#### Scenario: Game broadcasts state

- **WHEN** the game calls `broadcast` once per tick with its snapshot bytes
- **THEN** every attached client's packet stream contains those bytes in tick order

### Requirement: Automatic heartbeats

The SDK MUST send backplane heartbeats on its own schedule, without game-code
involvement, such that an idle game that produces no output remains connected
past the hub's liveness timeout.

#### Scenario: Idle game stays connected

- **WHEN** a worker's game emits nothing for several liveness timeouts
- **THEN** the hub has not closed the connection

### Requirement: Clean exit on hub loss

When the hub closes the connection (or was never reachable at startup), the
SDK MUST stop the game loop and return, allowing the worker process to exit —
never retry in the background or leak the session.

#### Scenario: Hub shutdown

- **WHEN** the hub process stops while a worker is connected
- **THEN** the worker's run call returns and the worker process exits

### Requirement: Tick rate validation

The SDK MUST validate `tick_hz` before connecting. A zero rate or a rate whose tick period rounds to zero MUST fail with an error naming `tick_hz`, never panic.

#### Scenario: Zero tick rate

- **WHEN** a worker starts with `tick_hz = 0`
- **THEN** the SDK returns a configuration error naming `tick_hz` without opening a connection

#### Scenario: Unrepresentable tick period

- **WHEN** a configured rate produces a zero-length tick duration
- **THEN** the SDK returns the same configuration error without opening a connection
