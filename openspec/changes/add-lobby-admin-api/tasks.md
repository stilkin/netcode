# Tasks

Ordered so the workspace compiles and tests stay green after every task.
Each section names the requirements it implements.

## 1. Foundations *(enables the requirements below)*
- [x] 1.1 Add `serde_json` to `hub` (workspace dep, pure Rust).
- [x] 1.2 Backplane service: `Command::CloseSession` (drop worker link, close registry entry, detach clients) + `ServiceHandle::close_session` and public `detach` (kick). *(lobby-api: Session closure, Client kick)*
- [x] 1.3 Tests: close an active session (fake worker link drops, clients detached, id not reused); detach/kick semantics through a real attached client.

## 2. Configuration *(hub-runtime: Lobby configuration)*
- [x] 2.1 `[lobby]` section (`bind` default `0.0.0.0:8080`, `token` default empty) + `[[games]]` catalog (`game_type` unique, `display_name`, `max_players`, `payload_schema_version`, `worker`, `session_config_json`), strict rules throughout; empty catalog by default.
- [x] 2.2 Retire `client_edge.bootstrap_bind`: pre-scan emits "retired — use `[lobby] bind`"; remove the field from `ClientEdgeConfig`.
- [x] 2.3 Tests: defaults, valid sections, duplicate `game_type` rejected, unknown keys named, retired key message, invalid bind named.

## 3. Lobby service: HTTP plumbing *(lobby-api: all requirements' substrate)*
- [x] 3.1a *Fix surfaced during 3.4:* removing the bootstrap task left the client-edge accept loop's `JoinSet` empty — `join_next()` on an empty set is immediately ready, busy-looping the runtime. The reap arm is now conditional (`if !tasks.is_empty`).
- [x] 3.1 `hub::lobby` service: bind/start/shutdown/drain mirroring the other services; hand-rolled HTTP/1.1 request parsing (request line, headers, bounded `Content-Length` body) and JSON responses.
- [x] 3.2 Route table: `GET /cert-hash`, `GET /`, `GET /api/games`, `GET /api/sessions`, `POST /api/sessions`, `DELETE /api/sessions/{id}`, `DELETE /api/sessions/{id}/clients/{cid}`; 404 otherwise; 405 where the method mismatches.
- [x] 3.3 Move the test page out of `client_edge::bootstrap` (module removed); the page gains games list, sessions list, and a create-session button via the API.
- [x] 3.4 Wire into `shutdown_loop`: bind at boot, log line, drain on shutdown; edge keeps WebTransport only.

## 4. Lobby service: operations *(lobby-api: Game catalog, Session listing, Session creation, Session closure, Client kick, Token gate)*
- [x] 4.1 `GET /api/games` from the catalog; `GET /api/sessions` from `ServiceHandle::sessions()` as JSON.
- [x] 4.2 `POST /api/sessions`: catalog lookup → `spawn_session` → 201 `{session_id}`; unknown type 404, malformed body 400, unspawnable program 500 with nothing left behind.
- [x] 4.3 `DELETE /api/sessions/{id}` → `close_session` → 204 / 404; `DELETE .../clients/{cid}` → `detach` → 204 / 404.
- [x] 4.4 Token gate: mutations require `Authorization: Bearer` when `lobby.token` is set; 401 otherwise; reads and cert-hash/page open.
- [x] 4.5 Hub-level integration tests over real TCP covering every route's success and error paths (dummy catalog entries; no worker process needed).

## 5. End-to-end *(lobby-api: full loop)*
- [x] 5.1 `crates/sample-worker/tests/lobby_end_to_end.rs`: real hub (backplane + edge + lobby) + `sample-worker`: POST create → active → WebTransport join → `GET /api/sessions` shows the player → kick closes the browser connection → DELETE closes the session and the worker exits → listing empty.
- [ ] 5.2 *(owner: needs a real browser)* Manual pass: run the hub, open the test page, create a session from it, play echo arena.

## 6. Documentation
- [x] 6.1 `docs/rest_api.yaml` (OpenAPI 3) covering every route; smoke test asserting each documented route answers its documented status class.
- [x] 6.2 README (status, `[lobby]`/`[[games]]` config, run story), `docs/architecture.md` (lobby API to implemented), `docs/roadmap.md` (change 4 status), `docs/development.md` (layout, coverage re-measure), AGENTS.md status note.

## 7. Wrap-up
- [x] 7.1 `openspec validate add-lobby-admin-api --strict` green; full workspace gate green (fmt, clippy `-D warnings`, tests).
- [ ] 7.2 Archive the change, mark the roadmap, commit.
