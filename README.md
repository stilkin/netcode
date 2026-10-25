# netcode hub

One server, many games. A LAN-party netcode hub written in Rust: clients
(any desktop browser, no install) connect to the hub; the hub routes their
packets — **opaquely, by header only** — to per-session game-worker processes,
and routes replies back. Adding a game means dropping a new worker binary on
the box. The hub is never rebuilt for new games.

## Prime directive

**Keep it simple, elegant, maintainable.**

- Smallest crate set that works; no speculative abstraction layers.
- Pure-Rust dependencies only — Windows and Ubuntu build identically, one CI matrix.
- Game decisions are simulated server-side (in workers); clients stay dumb:
  connect, send inputs, interpolate snapshots, render. That is the whole client
  contract, which is what makes clients cheap to write in any technology.

## Architecture

```
 game clients (browsers, WebTransport)      game logic (separate processes)
        │                                            │
        │  client protocol (WT datagrams + streams)  │  backplane (localhost TCP)
        ▼                                            ▼
 ┌───────────────────────────────────────────────────────────┐
 │                      HUB  (this repo)                     │
 │  transports · connections · router · game/session registry│
 │  watchdog · admin HTTP API                                │
 └───────────────────────────────────────────────────────────┘
```

The one inviolable rule: **the hub routes by header and never parses payloads.**
A fixed 24-byte header (magic, protocol version, channel, session/client id,
payload schema version, length) is the only thing the hub understands. Snapshot
and event bytes are a contract between a game worker and its clients alone.

## Roadmap

Implemented as OpenSpec changes, in order. Each change is independently
reviewable and leaves the workspace green.

| # | Change | Scope | Capabilities created |
|---|--------|-------|----------------------|
| 1 | `add-hub-foundation` **(drafted)** | Cargo workspace; wire header v1 codec pinned by golden vectors; hub binary shell (config, logging, graceful shutdown); CI matrix Ubuntu+Windows | `wire-protocol`, `hub-runtime`, `build-pipeline` |
| 2 | `add-backplane-game-workers` | Loopback-TCP backplane (register/attach/route/broadcast/heartbeat opcodes); `hub-sdk` with `GameSession` trait; session registry + process watchdog; sample worker; end-to-end loopback test | `backplane`, `session-registry`, `hub-sdk` |
| 3 | `add-webtransport-client-edge` | WebTransport endpoint (self-signed cert + served cert hash), client handshake with connect tokens, channel mapping (state→datagrams, events→stream), relay routing with backpressure policy | `client-edge`, `client-protocol` |
| 4 | `add-lobby-admin-api` | HTTP JSON API: list games/sessions, create/close session (spawns worker), kick client; session metadata for directory UIs | `lobby-api` |
| 5 | `add-conformance-bots` | Headless bot client, deterministic scripted-run stream fixtures, load smoke profile (N bots × M sessions) | `conformance` |

Deliberately out of scope (future changes or other repos): direct-connect mode
(hub steps out of the packet path), native UDP transport for non-browser
clients, the landing-page/directory UI (separate project; consumes the lobby
API), host migration, persistence.

## How we work

Spec-driven via [OpenSpec](https://openspec.dev): `proposal.md` (why/what) →
delta `specs/` (required behavior) → `design.md` (how, alternatives weighed) →
`tasks.md` (checklist) → implement → `openspec archive` merges deltas into
`openspec/specs/` as the source of truth.

```
openspec/changes/<change>/   proposal.md · design.md · tasks.md · specs/<capability>/spec.md
openspec/specs/<capability>/ merged, current behavior (source of truth)
```

With the CLI installed (`npm i -g @fission-ai/openspec`):
`openspec validate --changes` before implementing; `openspec archive` when the
last task is checked.
