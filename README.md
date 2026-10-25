# netcode hub

One server, many games. A LAN-party game server written in Rust: players join
from any desktop browser — no install — and the hub routes their packets to
per-session game-worker processes, one process per running lobby. Adding a new
game means dropping a new worker binary on the box; the hub itself is never
rebuilt for new games.

> **Status:** foundation implemented (wire protocol + runtime shell).
> Browser transports and the first playable sessions land in the next roadmap
> changes — see [docs/roadmap.md](docs/roadmap.md).

## What is the project?

The hub is the single long-running service of a LAN party's game infrastructure:

- it terminates all client connections (desktop browsers over WebTransport) and
  routes packets **opaquely, by header only** — it never parses game payloads;
- it hosts many concurrent sessions (lobbies) of many different game types,
  each simulated by a separate game-worker process;
- it exposes a lobby/admin HTTP API so browsers can discover and join sessions.

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
| CI | GitHub Actions — fmt, clippy `-D warnings`, tests on Ubuntu **and** Windows |
| Process | [OpenSpec](https://openspec.dev) spec-driven development ([docs/development.md](docs/development.md)) |

**Policy:** pure-Rust dependencies only — no C/C++ build deps — so Windows and
Ubuntu build identically with one toolchain. WebTransport ([wtransport](https://docs.rs/wtransport))
and the HTTP layer are planned, not yet integrated (roadmap changes 3–4).

## Installation

Prerequisite: a current stable Rust toolchain (edition 2024 requires ≥ 1.85):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Build from source:

```sh
git clone <this-repo> && cd netcode
cargo build --release          # artifacts in target/release/
```

Or install the binary onto your PATH:

```sh
cargo install --path crates/hub   # provides the `hub` command
```

There are no system dependencies to install.

## Configuration

The hub reads an optional TOML file (`hub.toml` in the working directory by
default; override with `--config`). A missing file means all defaults.
**Unknown keys are rejected** with an error naming the key — typos fail loudly.

```toml
# hub.toml — all keys optional
log_level = "info"   # trace | debug | info | warn | error
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

Today the binary is the verified runtime shell: it loads config, logs a startup
line (`netcode hub starting`), and idles until Ctrl-C/SIGTERM for a clean stop.
Network transports arrive with roadmap change 3.

## REST API and database

- **REST API:** none yet. The lobby/admin HTTP API arrives with roadmap change
  4 (`add-lobby-admin-api`); an OpenAPI 3 document (`docs/rest_api.yaml`) will be added at that time.
- **Database:** none, by design. Session and game state live in memory in the
  game-worker processes; persistence is explicitly out of scope for now
  ([docs/roadmap.md](docs/roadmap.md)).

## Documentation

| Document | Contents |
|---|---|
| [docs/architecture.md](docs/architecture.md) | system design, components, design invariants |
| [docs/roadmap.md](docs/roadmap.md) | planned changes, statuses, out-of-scope list |
| [docs/development.md](docs/development.md) | spec-driven workflow, repo conventions, local checks |
| [crates/protocol/docs/wire-format.md](crates/protocol/docs/wire-format.md) | the packet header byte layout (protocol v1) |
| [openspec/specs/](openspec/specs/) | behavioral specs — the source of truth per capability |

## Contributing / development

```sh
cargo test --workspace                              # all tests
cargo fmt --all -- --check                          # CI: formatting
cargo clippy --workspace --all-targets -- -D warnings  # CI: lints
```

A version-controlled pre-commit hook (`.githooks/pre-commit`) runs the same
gates before every commit — activate once per clone with
`git config core.hooksPath .githooks`.

Changes to behavior go through the spec workflow described in
[docs/development.md](docs/development.md) — requirements first, then code.
