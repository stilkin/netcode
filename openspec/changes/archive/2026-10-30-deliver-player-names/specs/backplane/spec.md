# Spec Delta

## MODIFIED Requirements

### Requirement: Client attach and detach notification

Whenever a client is attached to or detached from a worker's session, the hub
MUST send that worker a `ClientAttached` or `ClientDetached` frame carrying
the client id. `ClientAttached` MUST also carry the joining player's name
unchanged. Attach orders MUST arrive in the order the hub performed them.

#### Scenario: Attach then detach

- **WHEN** a client attaches to and then detaches from a session
- **THEN** the worker receives `ClientAttached { id, player_name }` followed by `ClientDetached { id }`, in that order

#### Scenario: Player name reaches the worker

- **WHEN** a client attaches with an empty or non-ASCII player name
- **THEN** the worker's `ClientAttached` frame contains that same name without substitution or normalization
