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
| [`crates/protocol`](../crates/protocol/) | The shared packet header + join-frame codecs — the interoperability contract. std-only so it can also be embedded in `wasm32` browser clients later. Pinned by golden vectors. |
| [`crates/backplane`](../crates/backplane/) | The hub↔worker wire protocol over loopback TCP: framing, registration, notifications, routing, liveness. Payload-blind opcodes; [format spec](../crates/backplane/docs/backplane-format.md). |
| [`crates/hub`](../crates/hub/) | The `hub` binary and library: strict TOML configuration, session registry, backplane service (auth, routing, fan-out, watchdog), worker spawn/supervision, the client edge, and the lobby API. Graceful shutdown with bounded drain; exit codes are part of the contract. |
| [`crates/hub-sdk`](../crates/hub-sdk/) | The crate game developers build against: implement a `GameSession` trait (`on_attach` with client id and display name / `on_detach` / `tick`), never touch transports. |
| [`crates/sample-worker`](../crates/sample-worker/) | Echo arena — the reference game and integration-test fixture. |
| client edge (`hub::client_edge`) | The browser-facing WebTransport endpoint: per-boot ECDSA P-256 certificate pinned by SHA-256 hash, join handshake (`Connect`/`Accept`/`Reject`), state ⇄ datagrams / events ⇄ control-stream relay with bounded inbound rates and an outbound stall watermark. |
| lobby API (`hub::lobby`) | The HTTP surface: game catalog (`[[games]]`), session listing/creation/closure, client kick, token gate on mutations, and the cert-hash + test-page routes. Hand-rolled HTTP/1.1 + JSON; documented in [rest_api.yaml](rest_api.yaml). |
| [`crates/conformance`](../crates/conformance/) | `netcode-bots`: headless protocol-verified WebTransport clients, a TOML scenario runner with thresholds and JSON reports, deterministic fixtures (logical ordering, loss-bounded), and load smoke profiles with stall isolation. Hub-free by design — it judges from outside. |
| [`crates/xtask`](../crates/xtask/) | The local release pipeline: cross-builds, packaging + checksums, the Docker image, and bots-driven artifact self-validation. No hosted CI, by decision. |

### Runtime bounds

Resource limits are deliberately small and local: the registry admits 64
pending/active sessions by default and retains at most 1,024 closed-session
tombstones. Each client shares a 4,096 packets/sec refill and 512-packet burst
across datagrams and stream packets; incomplete stream input is capped at 256
KiB and outbound pending packets at 1,024. Lobby requests are capped at 64 KiB
and 30 seconds. Only the session limit is configurable; the other limits are
abuse circuit breakers, not gameplay fairness throttles.

Nothing is currently planned beyond the out-of-scope list in the
[roadmap](roadmap.md); future work (direct-connect mode, native UDP
clients, a directory UI) is deliberately deferred until needed.

## Cross-cutting rules

- **Version everything that crosses a boundary.** The packet header carries
  `version` and `payload_schema_version`; the backplane protocol and the
  lobby API carry versions of their own.
- **Golden vectors and exact-layout tests are the wire contract.** Changing
  one is a deliberate protocol-evolution decision, never a refactoring side
  effect.
- **Unknown is rejected at the boundary.** Unknown config keys fail startup;
  unknown magic/versions drop the packet; unknown header bytes are skipped.
