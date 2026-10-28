# AGENTS.md

**netcode hub** — a LAN-party game server in Rust (edition 2024). One long-running
hub process terminates all client connections and routes packets **payload-blind,
by header only** to game-worker processes — one process per running lobby. Game
logic lives exclusively in workers; clients stay dumb (connect, send inputs,
interpolate snapshots, render). Adding a game means dropping a new worker binary
on the box; the hub is never rebuilt for games.

## Non-negotiable invariants

Violating any of these breaks the project's core design — hold them through every edit:

1. **The hub routes by header, never parses payloads.** Snapshot/event bytes are a
   contract between a game worker and its clients alone. This is what keeps games pluggable.
2. **Game decisions are simulated server-side**, in workers. On a LAN there is no
   client prediction, no reconciliation.
3. **Pure-Rust dependencies only** — no C/C++ build deps; Windows and Ubuntu must
   build identically. **One documented exception (2026-10-27, change 3):** the
   WebTransport transport tree (`wtransport` → quinn → rustls → `ring`), confined
   to the client edge — users receive Docker images / static binaries and never
   compile it; everything else stays pure Rust. (`libc` is allowed as a
   dev-dependency where a test needs a raw signal.)
4. **Unknown input is rejected at the boundary** — config keys, packet magic,
   protocol versions.
5. **Golden vectors** (`crates/protocol/tests/golden.rs`) pin the wire format.
   Regenerating one is a deliberate protocol-evolution decision recorded in the
   change's design doc — treat a failing golden test as a signal to stop, not to
   re-pin.

## Workflow: spec first, code second

Any behavior change goes through [OpenSpec](https://openspec.dev) — proposal →
delta specs → design → tasks → implement → archive. Start a change with
`openspec new change "add-<capability>"` (or the propose flow in `.claude/skills/`);
artifacts land in `openspec/changes/<name>/`. `openspec/specs/` is the source of
truth for current behavior; `openspec/changes/archive/` holds applied changes.
Requirements are observable behavior with ≥ 1 WHEN/THEN scenario; every task
traces to a requirement. Full workflow and conventions:
[docs/development.md](docs/development.md).

## Gates

The hooks are the enforcement point — there is no hosted CI. Activate once per
clone: `git config core.hooksPath .githooks` (both `pre-commit` and `pre-push`
live in `.githooks/`). The push gate closes the `--no-verify` commit bypass;
`git push --no-verify` remains for deliberate WIP. Releases:
`cargo run -p xtask -- release` (self-validates with the bots; see
docs/development.md).

```sh
cargo fmt --all -- --check                             # formatting
cargo clippy --workspace --all-targets -- -D warnings  # lints
cargo test --workspace                                 # unit + golden + integration
```

Every change leaves the workspace green.

## Read before touching…

| Material | Reach for it when… |
|---|---|
| [docs/architecture.md](docs/architecture.md) | touching hub routing, the backplane, or making any design decision |
| [docs/roadmap.md](docs/roadmap.md) | proposing features — statuses of changes 1–5 and the out-of-scope list |
| [docs/development.md](docs/development.md) | needing workflow detail, coverage conventions, or commit conventions |
| [crates/protocol/docs/wire-format.md](crates/protocol/docs/wire-format.md) | touching the client-packet header codec |
| [crates/backplane/docs/backplane-format.md](crates/backplane/docs/backplane-format.md) | touching the hub↔worker frame codec |
| [docs/development.md](docs/development.md#coverage) | measuring coverage (`cargo-llvm-cov`; ~91% lines overall, `protocol` at 100%) |

## Repo map

```
crates/protocol        client-packet header codec + golden vectors (std-only; wasm32-ready)
crates/backplane       hub↔worker frame codec
crates/hub             the `hub` binary: config (strict TOML incl. [[games]]), registry,
                       backplane service, worker spawn/watchdog, client edge
                       (WebTransport), lobby (HTTP API), graceful shutdown
crates/hub-sdk         game-side contract: implement GameSession, SDK does the networking
crates/sample-worker   echo arena — reference game + test fixture
crates/conformance/    netcode-bots: protocol-verified bots, scenarios, fixtures
openspec/specs/        current behavior — source of truth per capability
docs/                  architecture · roadmap · development
```

## Gotchas

- Edition 2024 requires Rust ≥ 1.85 (stable via rustup).
- Windows is not exercised automatically: `cfg(unix)` is confined to signal
  handling and gated tests; if you touch platform-specific code, build on a
  Windows machine before merging.
- Commits: small, conventional-ish prefixes (`feat`, `fix`, `chore`, `docs`);
  the message names the change/tasks it implements.
- A crate appears in the workspace when its roadmap change needs it — a crate
  skeleton ahead of need is a design smell here.
- Status (2026-10): roadmap complete — changes 1–5 implemented (wire
  protocol, hub runtime, backplane + SDK + sample worker, WebTransport
  client edge, lobby/admin HTTP API, conformance bots) plus the local
  release pipeline (xtask + Docker image + push gates).
