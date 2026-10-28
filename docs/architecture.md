# Architecture

One server, many games. The **hub** is the single long-running service: it owns
all client connections, routes packets opaquely, and keeps the registry of game
types and live sessions. Game logic lives in **game-worker processes** — one
process per running session — which speak only to the hub and never see a
client socket.

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

## Design invariants (the prime directive)

**Keep it simple, elegant, maintainable.** Concretely:

1. **The hub routes by header and never parses payloads.** A fixed 24-byte
   header (magic, protocol version, channel, session/client id, payload schema
   version, length — see the [wire format](../crates/protocol/docs/wire-format.md))
   is the only thing the hub understands. Snapshot and event bytes are a
   contract between a game worker and its clients alone. This is what makes new
   games pluggable without rebuilding the hub.
2. **Game decisions are simulated server-side**, in workers. Clients stay dumb:
   connect, send normalized inputs, interpolate snapshots, render. On a LAN
   (< 5 ms RTT) there is no client prediction, no reconciliation — and it still
   feels instant. Dumb clients are cheap to write in any technology.
3. **Pure-Rust dependencies only.** Windows and Ubuntu build identically
   from one toolchain; no C/C++ build deps. One documented exception (change 3):
   the WebTransport transport tree (`wtransport` → quinn → rustls → `ring`),
   confined to the client edge; users receive prebuilt artifacts.
4. **Small surface.** The smallest crate set that works; no speculative layers.
   Crates appear when their roadmap change needs them.

## Components

### Today (implemented)

| Component | What it does |
|---|---|
| [`crates/protocol`](../crates/protocol/) | The shared packet header codec — the entire interoperability contract. std-only so it can also be embedded in `wasm32` browser clients later. Pinned by golden vectors. |
| [`crates/hub`](../crates/hub/) | The `hub` binary: TOML configuration (strict), `tracing` logging, graceful shutdown (bounded drain, impatient second signal), `--version`. Verified by an end-to-end boot/SIGINT test. |
| client edge (`hub::client_edge`) | The browser-facing WebTransport endpoint: per-boot ECDSA P-256 certificate pinned by SHA-256 hash, join handshake (`Connect`/`Accept`/`Reject`), state ⇄ datagrams / events ⇄ control-stream relay with per-connection backpressure, bootstrap endpoint (cert hash + test page). Verified end-to-end by a hash-pinned WT client against a spawned worker. |

### Planned (see the [roadmap](roadmap.md))

| Component | What it does |
|---|---|
| backplane | Loopback-TCP link between hub and workers: register, attach clients, route, broadcast, heartbeat. Payload-blind routing opcodes. |
| `hub-sdk` | The crate game developers build against: implement a `GameSession` trait (`join` / `leave` / `tick`), never touch transports. |
| lobby API | HTTP JSON registry: list games/sessions, create/close sessions, kick clients. |
| conformance | Headless bot client + deterministic recorded streams + load profile. |

## Cross-cutting rules

- **Version everything that crosses a boundary.** The header carries `version`
  and `payload_schema_version`; the backplane and admin API get the same
  treatment when they land.
- **Golden vectors are the wire contract.** Changing one is a deliberate
  protocol-evolution decision, never a refactoring side effect.
- **Unknown is rejected at the boundary.** Unknown config keys fail startup;
  unknown magic/versions drop the packet; unknown header bytes are skipped.
