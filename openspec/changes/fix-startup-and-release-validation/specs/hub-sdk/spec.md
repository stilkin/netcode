# Spec Delta

## ADDED Requirements

### Requirement: Tick rate validation

The SDK MUST validate `tick_hz` before connecting. A zero rate or a rate whose tick period rounds to zero MUST fail with an error naming `tick_hz`, never panic.

#### Scenario: Zero tick rate

- **WHEN** a worker starts with `tick_hz = 0`
- **THEN** the SDK returns a configuration error naming `tick_hz` without opening a connection

#### Scenario: Unrepresentable tick period

- **WHEN** a configured rate produces a zero-length tick duration
- **THEN** the SDK returns the same configuration error without opening a connection
