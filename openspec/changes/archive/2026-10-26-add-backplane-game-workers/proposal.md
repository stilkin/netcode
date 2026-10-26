# Proposal: add-backplane-game-workers

## Why

The hub is a verified runtime shell with no games behind it. This change stands
up the architectural heart of the product: the hub-to-worker backplane, the
session registry, and the SDK contract that game developers write against.
After it lands, adding a game means implementing one trait in a separate
worker process — and the next change only has to bridge browsers onto the
already-working client-attach seam.

## What Changes

- New crate `backplane`: loopback-TCP framing (length-prefixed, opcode +
  body) with hand-rolled codecs and a format spec, plus connection wrappers.
- New crate `hub-sdk`: the `GameSession` trait, a `SessionCtx` with
  `to_client` / `to_many` / `broadcast` output helpers, and the worker runtime
  (connect, register, heartbeat, tick loop, clean exit on hub loss).
- New crate `sample-worker`: a minimal echo-arena game proving the full loop
  and serving as the test fixture.
- `hub` becomes a lib + bin: in-memory session registry (unique ids, capacity,
  player counts), backplane listener service (auth, registration, heartbeat
  timeouts, routing, fan-out, close-on-disconnect), client attach/detach with
  per-client outbound queues (the seam the browser edge will consume), and
  worker spawn + watchdog.
- `[backplane]` configuration section (bind address, optional shared token).
- Integration test: hub in-process + spawned `sample-worker` child process,
  covering register → attach → route → broadcast → detach → close-on-kill.

## Capabilities

### New Capabilities

- `backplane` — the wire protocol between hub and game workers (framing,
  authentication, registration, notifications, routing, liveness).
- `session-registry` — hub-side session bookkeeping (ids, metadata, capacity,
  lifecycle).
- `hub-sdk` — the worker-side contract: implement `GameSession`, and the SDK
  handles every networking concern.

### Modified Capabilities

- None. `hub-runtime` gains a configuration section; its existing observable
  behavior (boot, logging, shutdown, exit codes) is unchanged.
