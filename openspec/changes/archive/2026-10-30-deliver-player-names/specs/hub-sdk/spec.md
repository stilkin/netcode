# Spec Delta

## MODIFIED Requirements

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
