# Spec Delta

## ADDED Requirements

### Requirement: Live session capacity

The registry MUST reject creation that would exceed its configured limit of pending and active sessions. Closed sessions MUST NOT count toward the limit, a rejected creation MUST NOT allocate an id or change state, and closing a live session MUST free its capacity.

#### Scenario: Creation at the limit

- **WHEN** pending and active sessions already fill the configured limit and another session is requested
- **THEN** creation fails without allocating an id, while existing sessions remain unchanged

#### Scenario: Close frees capacity

- **WHEN** a live session closes after another creation was rejected at the limit
- **THEN** a subsequent creation succeeds and receives the next never-used id
