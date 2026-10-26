# Spec Delta

## Purpose

Defines the worker-side contract that game developers implement: a single
`GameSession` trait plus output helpers, with the SDK owning every networking
concern — connection, registration, heartbeats, input buffering, and the tick
loop — so a new game requires no transport code.

## ADDED Requirements

### Requirement: Game session runtime

The SDK MUST connect to the hub, authenticate, register or claim its session,
and then drive the game by invoking, in order: `on_attach` per attached
client, `tick` once per configured tick interval with all inputs received
since the previous tick, and `on_detach` per detached client. The tick rate
MUST come from the session configuration.

#### Scenario: Inputs reach the game

- **WHEN** a client sends two inputs between two ticks of a 30 Hz session
- **THEN** the next `tick` call observes both inputs

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
