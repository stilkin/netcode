# lobby-api Specification

## Purpose
The HTTP surface of the hub: how directory UIs and admin pages discover
games and sessions, create and close sessions, and kick clients — plus the
cert-hash and test-page routes absorbed from the client edge's stopgap
bootstrap endpoint.

## Requirements

### Requirement: Game catalog

The hub MUST expose the configured game catalog — one entry per `[[games]]`
table (`game_type`, `display_name`, `max_players`, `payload_schema_version`,
worker program, session-config JSON) — at `GET /api/games` as a JSON array.
A hub with no `[[games]]` entries MUST serve an empty array, and a
`game_type` value MUST be unique across the catalog.

#### Scenario: Empty catalog

- **WHEN** the hub boots with no `[[games]]` entries and a client requests
  `GET /api/games`
- **THEN** the response is `200` with body `[]`

#### Scenario: Duplicate game type

- **WHEN** `hub.toml` declares two `[[games]]` entries with the same
  `game_type`
- **THEN** startup fails with exit code 1 and the error names the duplicate

### Requirement: Session listing

`GET /api/sessions` MUST return the live sessions (pending and active) as a
JSON array, each carrying id, state, game type, display name, max players,
payload schema version, and current player count.

#### Scenario: Session appears and fills

- **WHEN** a session is created and a browser client joins it
- **THEN** `GET /api/sessions` lists the session, first `pending`, then
  `active`, with a player count that reflects joins and leaves

### Requirement: Session creation

`POST /api/sessions` with body `{ "game_type": "<type>" }` MUST look up the
catalog entry, spawn its worker program with the entry's defaults, and
respond `201` with the new session id. An unknown `game_type` MUST respond
`404` with a JSON error naming the type; a malformed body MUST respond `400`.

#### Scenario: Create an echo-arena session

- **WHEN** the catalog contains `echo-arena` and a client POSTs
  `{"game_type": "echo-arena"}`
- **THEN** the response is `201` with a session id, and the session reaches
  `active` once the spawned worker claims it

#### Scenario: Unknown game type

- **WHEN** a client POSTs `{"game_type": "chess"}`
- **THEN** the response is `404` and the error body names `chess`

### Requirement: Session closure

`DELETE /api/sessions/{id}` MUST close the session: the worker link is
dropped (the worker exits on hub loss by SDK contract), the registry entry
closes, and every attached client is detached and its transport closed.
An unknown session id MUST respond `404`. Closed ids are never reused.

#### Scenario: Close an active session

- **WHEN** an operator DELETEs an active session with a browser client
  attached
- **THEN** the response is `204`, the worker process exits, the client's
  connection closes, and `GET /api/sessions` no longer lists the session

#### Scenario: Close an unknown session

- **WHEN** a client DELETEs `/api/sessions/4242`
- **THEN** the response is `404` with a JSON error naming the id

### Requirement: Client kick

`DELETE /api/sessions/{id}/clients/{client_id}` MUST detach the named
client: the worker observes the detach, the player count drops, and the
client's transport connection closes. An unknown session or client MUST
respond `404`.

#### Scenario: Kick a player

- **WHEN** an operator DELETEs an attached client
- **THEN** the response is `204`, the worker sees `ClientDetached` for that
  id, and the kicked browser's connection closes

### Requirement: Token gate on mutations

When `lobby.token` is empty, all operations MUST be accepted (open LAN
trust, mirroring the backplane and client edge). When a token is configured,
every mutating request (`POST`, `DELETE`) MUST present
`Authorization: Bearer <token>`; a missing or wrong token MUST respond
`401`. Reads and the absorbed cert-hash/test-page routes stay open.

#### Scenario: Open LAN

- **WHEN** no token is configured
- **THEN** a plain `POST /api/sessions` succeeds

#### Scenario: Wrong bearer token

- **WHEN** a token is configured and a POST arrives with a different token
- **THEN** the response is `401` and no session is created

### Requirement: Cert hash and test page hosting

The lobby service MUST serve `GET /cert-hash` (the client edge certificate's
SHA-256 as JSON, CORS `*` — unchanged behavior) and `GET /` (the built-in
test page, which now also lists games and sessions and offers a
create-session button). Any other path MUST respond `404`.

#### Scenario: Hash matches the edge

- **WHEN** a browser fetches `/cert-hash` while a WebTransport client pins
  the served hash
- **THEN** the connection succeeds — the served hash is the live edge
  certificate's hash

#### Scenario: Unknown path

- **WHEN** a request arrives for an undefined path
- **THEN** the response status is 404

### Requirement: Request deadline

The lobby MUST impose a total 30-second deadline on each HTTP request. An incomplete request exceeding that deadline MUST be closed without affecting other requests.

#### Scenario: Slow request expires

- **WHEN** a client leaves its request incomplete beyond the deadline
- **THEN** its connection closes and subsequent requests are served normally
