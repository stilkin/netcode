# Proposal: add-lobby-admin-api

## Why

The hub can host sessions and browsers can join them, but nothing can
discover, create, or manage sessions over HTTP: session creation exists only
as an in-process API used by tests, and the client edge's bootstrap endpoint
is a two-route stopgap. This change adds the lobby/admin HTTP JSON API — the
surface a server browser or admin page builds on — and absorbs the bootstrap
endpoint into a single coherent HTTP service.

## What Changes

- New `hub::lobby` module: a hand-rolled HTTP/1.1 + JSON service (the
  bootstrap responder generalized: request parsing, simple routing, JSON
  bodies via `serde_json` — no web framework).
- Routes (OpenAPI 3 document ships as `docs/rest_api.yaml`):
  - `GET /api/games` — the configured game catalog.
  - `GET /api/sessions` — live sessions with metadata (pending + active).
  - `POST /api/sessions` — `{ "game_type": "..." }` → spawn worker →
    session id (201); unknown game type → 404.
  - `DELETE /api/sessions/{id}` — close session (204): worker link dropped
    (workers exit on hub loss by SDK contract), registry closed, clients
    detached.
  - `DELETE /api/sessions/{id}/clients/{client_id}` — kick a client (204):
    the existing detach semantics — worker notified, player count drops,
    the client's transport closes.
  - Absorbed from the client edge: `GET /cert-hash`, `GET /` (test page,
    which gains game listing and a create-session button).
- `[[games]]` configuration: the game catalog mapping `game_type` to a
  worker program, default session spec, and session-config JSON. Session
  creation is config-driven; the hub still never needs rebuilding for new
  games — adding a game is a config entry plus a worker binary.
- `[lobby]` config section: `bind` (default `0.0.0.0:8080`), `token`
  (default empty = open LAN). When a token is configured, mutating
  operations require `Authorization: Bearer <token>`; reads stay open.
- Backplane service: `close_session` and public `detach` (kick) — thin
  wrappers over existing core commands; a `CloseSession` command drops the
  worker link and closes the registry entry.
- `client_edge.bootstrap_bind` is **removed** (strict config names the
  retired key loudly); the edge keeps only the WebTransport listener.
- Startup binds the lobby service and logs its address; shutdown drains it
  within the grace period.

## Capabilities

### New Capabilities

- `lobby-api` — the HTTP surface: game catalog, session listing/creation/
  closure, client kick, token gate, and hosting of the cert-hash + test
  page routes.

### Modified Capabilities

- `hub-runtime` — gains the `[lobby]` section and `[[games]]` catalog with
  the usual strict-config rules, the lobby lifecycle at boot/shutdown.
- `client-edge` — the cert-hash bootstrap endpoint requirement is removed
  (absorbed by `lobby-api`); the edge is WebTransport-only. Existing
  transport behavior is unchanged.

## Out of Scope

- Per-client connect-token issuance (the client edge's static token stays;
  issuance waits for a concrete need).
- The directory/admin UI itself — a separate project consuming this API.
- Live updates (websockets/SSE) — clients poll `GET /api/sessions`.
- Pagination, filtering, rate limiting, HTTPS on the lobby port (LAN).
- Hard-killing hung workers — well-behaved workers exit on hub loss (SDK
  contract); an operator can kill a wedged process by pid from the log.
- Restarting crashed workers, session persistence, host migration.
