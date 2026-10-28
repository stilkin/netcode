# netcode hub

One server, many games. A LAN-party game server written in Rust: players join
from any desktop browser — no install — and the hub routes their packets to
per-session game-worker processes, one process per running lobby. Adding a new
game means dropping a new worker binary on the box and a config entry; the hub
itself is never rebuilt for new games.

> **Status:** feature-complete — wire protocol, hub runtime, worker backplane
> with game SDK, WebTransport client edge, lobby/admin HTTP API, conformance
> bots, and a self-validating local release pipeline. 102 tests, version-
> controlled commit/push gates, no hosted CI (by decision).

## What is the project?

The hub is the single long-running service of a LAN party's game
infrastructure:

- it terminates all client connections (desktop browsers over WebTransport)
  and routes packets **opaquely, by header only** — it never parses game
  payloads;
- it hosts many concurrent sessions (lobbies) of many different game types,
  each simulated by a separate game-worker process on the same machine;
- it exposes a lobby/admin HTTP API so browsers discover, create, join, and
  manage sessions.

Game logic lives exclusively in game workers; game decisions are made
server-side. Clients stay dumb: connect, send inputs, interpolate snapshots,
render. The full design — including the one inviolable rule that keeps games
pluggable — is in [docs/architecture.md](docs/architecture.md).

## Technologies

| Layer | Choice |
|---|---|
| Language | Rust (edition 2024, stable toolchain ≥ 1.85) |
| Async runtime | [tokio](https://toki.rs) (multi-thread, signals, timers) |
| Client transport | WebTransport ([wtransport](https://docs.rs/wtransport)) over QUIC, [rcgen](https://docs.rs/rcgen) self-signed certs (see policy note below) |
| Lobby API | hand-rolled HTTP/1.1 + [serde_json](https://docs.rs/serde_json) — [OpenAPI](docs/rest_api.yaml) |
| Wire & backplane codecs | hand-rolled, zero-dependency ([wire format](crates/protocol/docs/wire-format.md) · [backplane format](crates/backplane/docs/backplane-format.md)) |
| Game SDK | `hub-sdk`: implement a `GameSession` trait, the SDK does all networking |
| Conformance | `netcode-bots`: protocol-verified headless clients, deterministic fixtures, load smoke profiles |
| CLI / config / logging / errors | [clap](https://docs.rs/clap) · [serde](https://serde.rs) + [toml](https://docs.rs/toml) (strict: typos fail loudly) · [tracing](https://docs.rs/tracing) (`RUST_LOG`) · [thiserror](https://docs.rs/thiserror)/[anyhow](https://docs.rs/anyhow) |
| Release pipeline | `xtask`: cross-build, package, Docker image, bots-validated — fully local ([runbook](docs/development.md#releasing)) |
| Gates | `.githooks/pre-commit` + `.githooks/pre-push`: fmt, clippy `-D warnings`, tests |
| Process | [OpenSpec](https://openspec.dev) spec-driven development ([docs/development.md](docs/development.md)) |

**Dependency policy:** pure-Rust only — no C/C++ build deps — so Windows and
Ubuntu build identically with one toolchain. **One documented exception:** the
WebTransport transport tree (`wtransport` → quinn → rustls → `ring`), confined
to the client edge; users receive prebuilt artifacts and never compile it.

## Installation

**Run it with docker** (lowest entry bar — no toolchain at all):

```sh
docker build -t netcode-hub .
docker run --rm -p 4433:4433/udp -p 8080:8080 netcode-hub
# then open http://localhost:8080/ and click "create Echo Arena"
```

**Or run a release binary** (from the GitHub releases page): `hub`,
`sample-worker`, and `netcode-bots` in one archive with an example
`hub.toml` — static Linux (musl) and Windows builds, plus `SHA256SUMS`.

**Or build from source.** You need a current stable Rust toolchain and a C
compiler (the platform's default toolchain — `gcc`/clang or VS Build Tools —
required by `ring` in the transport tree above):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # if needed
git clone <this-repo> && cd netcode
cargo build --release
```

## Configuration

The hub reads an optional TOML file (`hub.toml` in the working directory;
override with `--config`). A missing file means all defaults. **Unknown keys
are rejected** with an error naming the key — typos fail loudly.

```toml
# hub.toml — all keys optional
log_level = "info"   # trace | debug | info | warn | error

[backplane]
bind = "127.0.0.1:24000"   # loopback by design: trusted control plane
token = ""                 # shared token workers must present; empty = off

[client_edge]
bind = "0.0.0.0:4433"      # WebTransport (UDP) — LAN-facing
token = ""                 # shared token Connect frames must present; empty = open LAN

[lobby]
bind = "0.0.0.0:8080"      # HTTP: lobby API + cert hash + test page
token = ""                 # gates mutations; empty = open LAN

# One entry per installable game: adding a game is a binary + a config entry.
[[games]]
game_type = "echo-arena"
display_name = "Echo Arena"
max_players = 8
payload_schema_version = 1
worker = "target/debug/sample-worker"
session_config_json = '{"tick_hz":60}'
```

`RUST_LOG` always overrides `log_level`. Exit codes: `0` clean stop after
Ctrl-C/SIGTERM · `1` invalid configuration (error names the key) · `2` fatal
error · `130` second signal during shutdown.

## Running

```sh
cargo run -p hub                        # dev run with defaults
cargo run -p hub -- --config hub.toml   # explicit config
hub --version                           # → hub 0.1.0
```

At boot the hub binds all three listeners — backplane (loopback TCP),
client edge (WebTransport/UDP), lobby (HTTP) — and logs each address plus the
per-boot certificate hash. From a browser, open `http://<host>:8080/`: the
built-in test page lists the catalog and live sessions, creates a session
with one click, pins the served certificate hash, and plays echo arena.

Session without the page, manually:

```sh
curl -X POST localhost:8080/api/sessions \
     -H 'Content-Type: application/json' -d '{"game_type":"echo-arena"}'
# → {"session_id":1}  (the hub spawns and supervises the worker)
```

**Conformance / load testing** against a running hub:

```sh
cargo run -p conformance --bin netcode-bots -- run crates/conformance/scenarios/smoke.toml
```

Protocol-verified bots join over WebTransport, drive their scripts, and exit
0/1 against the scenario's thresholds. Write your own scenarios for bigger
profiles ([docs/development.md](docs/development.md)).

## REST API & database

- **REST API:** the lobby/admin API is specified in
  [`docs/rest_api.yaml`](docs/rest_api.yaml) (OpenAPI 3): game catalog,
  session listing/creation/closure, client kick, plus cert-hash and test-page
  routes — all on the `[lobby]` listener. A drift-guard test keeps the file
  honest against the implementation.
- **Database:** none, by design. Session and game state live in memory in the
  game-worker processes; persistence is explicitly out of scope
  ([docs/roadmap.md](docs/roadmap.md)).

## Documentation

| Document | Contents |
|---|---|
| [docs/architecture.md](docs/architecture.md) | system design, component map, design invariants |
| [docs/development.md](docs/development.md) | spec-driven workflow, conventions, gates, release runbook |
| [docs/roadmap.md](docs/roadmap.md) | change history, out-of-scope list |
| [docs/rest_api.yaml](docs/rest_api.yaml) | lobby/admin API (OpenAPI 3) |
| [crates/protocol/docs/wire-format.md](crates/protocol/docs/wire-format.md) | client-packet header + join-frame byte layouts (protocol v1) |
| [crates/backplane/docs/backplane-format.md](crates/backplane/docs/backplane-format.md) | hub↔worker frame layout (backplane v1) |
| [openspec/specs/](openspec/specs/) | behavioral specs — the source of truth per capability |

## Releasing (maintainers)

One command, fully local — the release **validates itself** with the
conformance bots before anything is published:

```sh
cargo run -p xtask -- release [--publish]
```

See the [release runbook](docs/development.md#releasing) for the full flow
(preflight, cross-builds, packaging, Docker image, self-validation,
publishing).

## Contributing / development

```sh
cargo test --workspace                                  # 102 tests
cargo fmt --all -- --check                              # gate: formatting
cargo clippy --workspace --all-targets -- -D warnings   # gate: lints
```

Activate the version-controlled hooks once per clone —
`git config core.hooksPath .githooks` — and behavior changes go through the
spec workflow in [docs/development.md](docs/development.md): requirements
first, then code.
