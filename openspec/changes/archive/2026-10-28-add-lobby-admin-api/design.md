# Design: add-lobby-admin-api

Grounding facts from the codebase (verified):

- `SessionInfo` (registry) already carries every field a directory needs:
  id, state, game type, display name, max players, schema version, player
  count; `ServiceHandle::sessions()` lists pending + active.
- `Command::Detach` → core `detach_client` already: registry detach, worker
  `ClientDetached`, **route dropped** → the client's outbound queue closes →
  the client-edge relay's `recv()` returns `None` → transport closed.
- Workers exit on hub loss by SDK contract (change 2: "clean return on hub
  loss"); dropping a worker's outbound channel closes the pump's TCP side.
- `spawn_session` exists (pending + child + supervision); `worker_lost`
  closes the session and detaches clients.
- The bootstrap responder (`client_edge::bootstrap`, 225 lines) already
  parses a request head and routes two GETs; the test page is embedded.
- `serde_json` is already in the workspace (sample-worker), pure Rust.

## D1 — Hand-rolled HTTP again; `serde_json` is the only new dependency

The lobby service is the bootstrap responder generalized: parse request
line + headers, read a bounded `Content-Length` body, route, answer JSON.
~300 lines in `hub::lobby`. No framework.

*Alternatives considered:* axum/hyper (rejected: the tower ecosystem for
seven routes — the same reasoning as change 3's D9, now with routing and
one POST decoder added); keeping the bootstrap endpoint separate from the
API (rejected: two HTTP ports serving one page's needs is operator
confusion, and change 3 already scheduled absorption).

## D2 — The game catalog is config: `[[games]]`

Session creation maps `game_type` → worker program + default spec +
session-config JSON. Adding a game stays a config entry plus a binary on
the box — the hub is still never rebuilt for games. The catalog is read at
boot; duplicates fail loudly.

*Alternatives considered:* directory auto-discovery of worker binaries
(rejected: implicit magic, platform-dependent); workers advertising game
types ahead of sessions (rejected: requires idle workers running before any
session exists, inverting the spawn model — revisit only with a concrete
hot-plug need).

## D3 — Close drops the worker link; no child-kill machinery

`DELETE /api/sessions/{id}` sends a new `Command::CloseSession`: the core
drops the worker's outbound channel (the pump exits, TCP closes, the worker
exits per its SDK contract) and closes the registry entry (detaching all
clients through the existing path). No child-handle map, no kill signals.

*Alternatives considered:* per-session child kill handles (rejected: extra
supervision state for the case the SDK contract already covers; a wedged
worker is an operator kill-by-pid, logged at spawn); a backplane `Close`
frame (rejected: a protocol change to reach the same outcome).

## D4 — Kick is the existing detach, made public

`ServiceHandle::detach(session_id, client_id)` wraps `Command::Detach`.
The semantics are already exactly right (worker notified, count drops,
transport closed via queue closure) — change 3's edge made them so. Zero
new core code.

## D5 — Auth: static bearer token on mutations, reads open

`[lobby] token` (empty = open LAN) gates `POST`/`DELETE` via
`Authorization: Bearer`. Reads and cert-hash/test-page stay open — a LAN
page must not need a login to render a server browser.

*Alternatives considered:* gating everything (rejected: reads are harmless
on a LAN and the page degrades); per-user identities (out of scope by
roadmap).

## D6 — Absorption: one port, loud retirement

`/cert-hash` and `/` move into the lobby service (the page gains a games
list, sessions list, and a create-session button — the manual story becomes
*run hub → open page → create → play*). `client_edge` loses
`bootstrap_bind`; a config pre-scan turns the strict-parse failure into
"retired — use `[lobby] bind`", so operators migrating read the fix in the
error itself.

## D7 — Testing split

Hub-level integration tests (real TCP, no worker process): catalog listing,
duplicate/unknown keys, session listing, unknown-game 404, malformed-body
400, auth 401, unknown-session 404, missing-worker 500, cert-hash and 404
routes. The full flow (`sample-worker` tests, where
`CARGO_BIN_EXE_sample-worker` lives): create over HTTP → `active` →
browser joins over WebTransport → listing shows the player → kick closes
the browser's connection → close ends the session and the worker process.

## D8 — OpenAPI

`docs/rest_api.yaml` documents the surface; a smoke test asserts every
documented route answers with the documented status classes, so the file
cannot silently drift from the implementation.
