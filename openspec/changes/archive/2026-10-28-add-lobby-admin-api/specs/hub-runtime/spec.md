# Spec Delta

## Purpose

Extends the hub runtime with the lobby configuration (the `[lobby]` endpoint
and the `[[games]]` catalog) and the lobby service lifecycle. Existing
behavior is unchanged.

## ADDED Requirements

### Requirement: Lobby configuration

The `[[games]]` catalog MUST map a unique `game_type` to `display_name`,
`max_players`, `payload_schema_version`, `worker` (program path), and
`session_config_json`. The `[lobby]` section MUST define `bind` (default
`0.0.0.0:8080`) and `token` (default empty). Both MUST follow the strict
configuration rules: unknown keys fail startup naming the key, a missing
section means all defaults, and an invalid `bind` fails startup naming the
field. The hub MUST boot with an empty catalog.

#### Scenario: Unknown key

- **WHEN** `hub.toml` contains `[lobby]` with key `bind_adress`
- **THEN** startup fails with exit code 1 and the error names `bind_adress`

#### Scenario: Missing worker binary at creation time

- **WHEN** a `POST /api/sessions` names a game whose `worker` program cannot
  be executed
- **THEN** the creation request fails with `500`, no session remains in the
  registry, and the error is logged

### Requirement: Lobby lifecycle

The hub MUST bind the lobby HTTP service at boot, log its address, and treat
a bind failure as a fatal startup error. On shutdown the service stops
accepting and drains within the existing grace period. The retired
`client_edge.bootstrap_bind` key MUST fail startup naming the key, pointing
operators at `[lobby] bind`.

#### Scenario: Boot and log line

- **WHEN** the hub boots
- **THEN** the log names the lobby address alongside the backplane and
  client edge lines

#### Scenario: Retired key is loud

- **WHEN** `hub.toml` contains `[client_edge] bootstrap_bind = ...`
- **THEN** startup fails with exit code 1 and the error names
  `bootstrap_bind` as retired in favor of `[lobby] bind`

#### Scenario: Clean stop

- **WHEN** SIGTERM arrives while an HTTP request is in flight
- **THEN** the service stops accepting, open responses complete, and the hub
  exits 0
