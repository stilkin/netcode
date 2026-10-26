# Tasks

## 1. Workspace growth *(enables the requirements below)*
- [x] 1.1 Add crates `backplane`, `hub-sdk`, `sample-worker` to the workspace (publish = false); skeletons compile.
- [x] 1.2 Split `hub` into lib + thin bin (`src/lib.rs` re-exports config/run; `main.rs` parses args); existing tests green untouched.

## 2. backplane — framing and codecs *(backplane: Backplane framing)*
- [x] 2.1 Frame codec: length-prefixed read/write over tokio TCP halves, 256 KiB cap, unknown-opcode error; opcode enum v1 per design table.
- [x] 2.2 Hand-rolled body codecs for every opcode (strings u16-length-prefixed); `docs/backplane-format.md` byte tables + rules.
- [x] 2.3 Unit tests: round-trip every opcode, cap rejection, string edge cases (empty, non-ASCII).

## 3. hub config: backplane section *(hub-runtime: Configuration loading)*
- [x] 3.1 `[backplane]` table: `bind` (default `127.0.0.1:24000`), `token` (optional); nested `deny_unknown_fields`; defaults boot without file.
- [x] 3.2 Unit tests: defaults, valid section, unknown nested key rejected.

## 4. hub: session registry *(session-registry: all requirements)*
- [x] 4.1 Registry types: session states (pending/active/closed), metadata, monotonic non-reused ids; attach/detach with capacity check; per-client outbound queues (`ClientHandle { id, rx }`).
- [x] 4.2 Unit tests: distinct concurrent ids, player-count tracking, full-session refusal, pending not joinable, idempotent close, id non-reuse.

## 5. hub: backplane service *(backplane: Authentication, registration, notifications, delivery, fan-out, liveness, closure)*
- [x] 5.1 Accept loop + auth (token compare, `Hello`-first enforcement) + registration/claim paths incl. pending-entry timeout.
- [x] 5.2 Per-connection pump: heartbeat/liveness tracking, `ClientAttached/Detached` delivery, `FromClient` routing into worker input path, `ToClient`/`ToMany`/`Broadcast` fan-out into client queues (stale unicast dropped silently), close-on-disconnect cleanup.
- [x] 5.3 Integration tests over real 127.0.0.1 sockets with scripted fake workers: wrong token rejected; self-registration; silent worker reaped; broadcast fan-out; stale unicast dropped; disconnect closes session.

## 6. hub: worker spawn + watchdog *(backplane: Session closure on worker loss)*
- [x] 6.1 Spawn API: build argv (`--backplane`, `--token`, `--session-config` JSON), pre-create pending session, supervise child; child exit ⇒ registry close.
- [x] 6.2 Test: spawned `sample-worker --exit-immediately` produces pending → closed, id not reused.

## 7. hub-sdk: the worker contract *(hub-sdk: all requirements)*
- [x] 7.1 `GameSession` trait (`on_attach`, `on_detach`, `tick`) + `SessionCtx` (`to_client`, `to_many`, `broadcast`); `SessionConfig` (tick_hz) parse from JSON argv.
- [x] 7.2 Runtime: connect → `Hello` → `Register` (claim or fresh) → input buffering → tick loop at `tick_hz` → heartbeats on its own schedule → clean return on hub loss.
- [x] 7.3 Test against an in-process stub hub TCP server: register observed, idle worker heartbeats past liveness, hub drop ⇒ run returns.

## 8. sample-worker: the fixture game *(hub-sdk: Game session runtime, Output helpers)*
- [x] 8.1 Echo arena: on attach assigns player slot and greets; per tick echoes each client's input back via `to_client` and broadcasts a tick counter snapshot; `--exit-immediately` flag for the watchdog test.

## 9. End-to-end: hub + spawned worker *(backplane: full loop; session-registry: lifecycle)*
- [x] 9.1 `crates/sample-worker/tests/backplane_end_to_end.rs`: in-process hub + spawned `sample-worker`; assert pending → active; attach 2 clients; inputs delivered in order; per-client echoes + shared broadcast each exactly once; detach decrements; kill child ⇒ session closed, clients detached, id not reused.

## 10. Wrap-up
- [x] 10.1 `openspec validate add-backplane-game-workers --strict` green; full workspace gate (fmt, clippy -D warnings, tests) green.
- [x] 10.2 Mark change implemented in README roadmap + `docs/roadmap.md`; `openspec archive add-backplane-game-workers --yes`.
