# netcode hub

One server, many games. A LAN-party game server written in Rust: players join
from any desktop browser — no install — and the hub routes their packets to
per-session game-worker processes, one process per running lobby. Adding a new
game means dropping a new worker binary on the box; the hub itself is never
rebuilt for new games.

> **Status:** roadmap changes 1–5 implemented — wire protocol, hub runtime,
> worker backplane with game SDK, WebTransport client edge, lobby/admin HTTP
> API, and conformance bots (96 tests, local pre-commit gates). The roadmap
> is complete; see [docs/roadmap.md](docs/roadmap.md).

## What is the project?

The hub is the single long-running service of a LAN party's game infrastructure:

- it terminates all client connections (desktop browsers over WebTransport —
  landing in roadmap change 3) and routes packets **opaquely, by header only**
  — it never parses game payloads;
- it hosts many concurrent sessions (lobbies) of many different game types,
  each simulated by a separate game-worker process on the same machine;
- it exposes a lobby/admin HTTP API so browsers can discover and join sessions
  (roadmap change 4).

Game logic lives exclusively in game workers; game decisions are made
server-side. Clients stay dumb: connect, send inputs, interpolate snapshots,
render. The full design — including the one inviolable rule that keeps games
pluggable — is in [docs/architecture.md](docs/architecture.md).

## Technologies

| Layer | Choice |
|---|---|
| Language | Rust (edition 2024, stable toolchain) |
| Async runtime | [tokio](https://toki.rs) (multi-thread, signals, timers) |
| CLI | [clap](https://docs.rs/clap) (derive) |
| Configuration | [serde](https://serde.rs) + [toml](https://docs.rs/toml) |
| Logging | [tracing](https://docs.rs/tracing) + tracing-subscriber (`RUST_LOG`) |
| Errors | [thiserror](https://docs.rs/thiserror) (libraries), [anyhow](https://docs.rs/anyhow) (binary) |
| Wire codec | hand-rolled, zero-dependency ([spec](crates/protocol/docs/wire-format.md)) |
| Backplane codec | hand-rolled, zero-dependency ([spec](crates/backplane/docs/backplane-format.md)) |
| Client transport | [WebTransport](https://docs.rs/wtransport) over QUIC + [rcgen](https://docs.rs/rcgen) self-signed certs (see policy note) |
| Lobby API | hand-rolled HTTP/1.1 + [serde_json](https://docs.rs/serde_json) — [OpenAPI](docs/rest_api.yaml) |
| Conformance | `netcode-bots`: protocol-verified headless clients, deterministic fixtures, load smoke profiles |
| Game SDK | `hub-sdk`: implement `GameSession`, the SDK does all networking |
| Commit gates | version-controlled pre-commit hook — fmt, clippy `-D warnings`, tests before every commit |
| Process | [OpenSpec](https://openspec.dev) spec-driven development ([docs/development.md](docs/development.md)) |

**Policy:** pure-Rust dependencies only — no C/C++ build deps — so Windows
and Ubuntu build identically with one toolchain, with **one documented
exception**: the WebTransport transport tree (`wtransport` → quinn → rustls
→ `ring`), confined to the client edge (change 3). Users receive prebuilt
artifacts and never compile it. The HTTP layer is planned (roadmap change 4).

## Installation

**Run it with docker** (lowest entry bar — no toolchain at all):

```sh
docker build -t netcode-hub .
docker run --rm -p 4433:4433/udp -p 8080:8080 netcode-hub
# open http://localhost:8080/ and click "create Echo Arena"
```

**Or run a release binary** (static, from the GitHub releases page):
`hub`, `sample-worker`, and `netcode-bots` in one archive with an example
`hub.toml` — Linux (musl, fully static) and Windows, plus `SHA256SUMS`.

**Or build from source.** Prerequisite: a current stable Rust toolchain
(edition 2024 requires ≥ 1.85):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Because the WebTransport transport tree compiles `ring` (the one
documented exception to the pure-Rust rule), source builds also need a C
compiler: the platform's default toolchain (`gcc`/clang or VS Build
Tools) — the same one linking requires anyway.

```sh
git clone <this-repo> && cd netcode
cargo build --release          # artifacts in target/release/
```

There are no other system dependencies.

## Configuration

The hub reads an optional TOML file (`hub.toml` in the working directory by
default; override with `--config`). A missing file means all defaults.
**Unknown keys are rejected** with an error naming the key — typos fail loudly.

```toml
# hub.toml — all keys optional
log_level = "info"   # trace | debug | info | warn | error

[backplane]
bind = "127.0.0.1:24000"   # loopback by design: trusted control plane
token = ""                 # shared token workers must present; empty = off

[client_edge]
bind = "0.0.0.0:4433"        # WebTransport (UDP) — LAN-facing
token = ""                   # shared token Connect frames must present; empty = open LAN

[lobby]
bind = "0.0.0.0:8080"        # HTTP: lobby API + cert hash + test page
token = ""                   # gates mutations; empty = open LAN

# One entry per installable game: adding a game is a binary + a config entry.
[[games]]
game_type = "echo-arena"
display_name = "Echo Arena"
max_players = 8
payload_schema_version = 1
worker = "target/debug/sample-worker"
session_config_json = '{"tick_hz":60}'
```

The `RUST_LOG` environment variable always overrides `log_level`.

| Exit code | Meaning |
|---:|---|
| 0 | clean stop after Ctrl-C / SIGTERM |
| 1 | invalid configuration (error names the offending key) |
| 2 | unexpected fatal error |
| 130 | second signal received during shutdown (immediate exit) |

## Running

```sh
cargo run -p hub                    # dev run with defaults
cargo run -p hub -- --config /etc/hub.toml
hub --version                       # → hub 0.1.0
```

The hub binds its backplane listener (`netcode hub starting`, then
`backplane listening <addr>`) and accepts worker registrations until
Ctrl-C/SIGTERM for a clean stop. See it end to end:

```sh
# terminal 1: the hub
cargo run -p hub
# terminal 2: a sample game session
cargo run -p sample-worker -- --backplane 127.0.0.1:24000
# the hub log now shows: worker registered (session = 1)
```

Browsers join over WebTransport: open `http://<host>:8080/` for the built-in
test client — it lists the game catalog and live sessions, creates sessions
with one click, fetches the hub's per-boot certificate hash, and plays echo
arena. The same routes are the documented REST API
([OpenAPI](docs/rest_api.yaml)): `GET /api/games`, `GET /api/sessions`,
`POST /api/sessions`, `DELETE /api/sessions/{id}`,
`DELETE /api/sessions/{id}/clients/{id}`.

## REST API and database

- **Conformance / load**: `cargo run -p conformance --bin netcode-bots -- run crates/conformance/scenarios/smoke.toml`
  drives a running hub with protocol-verified bots and exits 0/1; write your
  own scenarios for bigger profiles.
- **REST API:** the lobby/admin API ([`docs/rest_api.yaml`](docs/rest_api.yaml))
  serves the game catalog, session listing/creation/closure, and client kick,
  plus the cert-hash and test-page routes on the `[lobby]` listener.
- **Database:** none, by design. Session and game state live in memory in the
  game-worker processes; persistence is explicitly out of scope for now
  ([docs/roadmap.md](docs/roadmap.md)).

## Documentation

| Document | Contents |
|---|---|
| [docs/architecture.md](docs/architecture.md) | system design, components, design invariants |
| [docs/roadmap.md](docs/roadmap.md) | planned changes, statuses, out-of-scope list |
| [docs/development.md](docs/development.md) | spec-driven workflow, repo conventions, local checks |
| [crates/protocol/docs/wire-format.md](crates/protocol/docs/wire-format.md) | client-packet header byte layout (protocol v1) |
| [crates/backplane/docs/backplane-format.md](crates/backplane/docs/backplane-format.md) | hub-to-worker frame layout (backplane v1) |
| [openspec/specs/](openspec/specs/) | behavioral specs — the source of truth per capability |

## Releasing (maintainers)

One command, fully local (no hosted CI by decision):

```sh
cargo run -p xtask -- release            # preflight → build → package → image → bots-validate
cargo run -p xtask -- release --publish  # additionally: git tag, gh release, docker push
```

The release **validates itself**: the freshly built hub is booted and the
conformance bots must pass their smoke scenario against it before anything
is published. Cross-builds need `musl-tools` and `mingw-w64`; preflight
names anything missing with install hints.

## Contributing / development

```sh
cargo test --workspace                              # all tests
cargo fmt --all -- --check                          # gate: formatting
cargo clippy --workspace --all-targets -- -D warnings  # gate: lints
```

A version-controlled pre-commit hook (`.githooks/pre-commit`) runs the same
gates before every commit — activate once per clone with
`git config core.hooksPath .githooks`.

Changes to behavior go through the spec workflow described in
[docs/development.md](docs/development.md) — requirements first, then code.
