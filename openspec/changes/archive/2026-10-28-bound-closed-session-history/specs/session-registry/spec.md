# Spec Delta

## ADDED Requirements

### Requirement: Closed session history

The hub MUST retain metadata for at most the 1,024 most recently closed sessions. Exceeding that history MUST evict the oldest closed entry; open sessions MUST NOT be evicted, and evicted IDs MUST NOT be reused.

#### Scenario: Oldest closed session is evicted

- **WHEN** closing a session pushes the closed history beyond 1,024 entries
- **THEN** the oldest closed ID behaves as unknown, while the newest retains closed metadata and remains not joinable

#### Scenario: Repeated close does not consume history

- **WHEN** an already closed session is closed again
- **THEN** no additional history entry is added and no other session is evicted
