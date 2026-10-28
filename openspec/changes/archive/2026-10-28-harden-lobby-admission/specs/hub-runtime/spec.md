# Spec Delta

## ADDED Requirements

### Requirement: Backplane session limit

The `[backplane]` section MUST accept `max_sessions`, defaulting to 64. It MUST bound the combined pending and active session count; zero MUST disable new session creation rather than mean unlimited. The key MUST follow the existing strict configuration rules.

#### Scenario: Missing limit uses the default

- **WHEN** the backplane configuration omits `max_sessions`
- **THEN** the hub uses a limit of 64 live sessions

#### Scenario: Zero disables creation

- **WHEN** `max_sessions` is zero and either a spawned or self-registered session is requested
- **THEN** the request is rejected without creating a session
