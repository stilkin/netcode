# Design

## Context

Greenfield repo; no code exists. The hub architecture was agreed in prior
discussion: the hub terminates client connections, routes opaque packets by
header to per-session game-worker processes, and never inspects payloads. This
change lays only the foundation — workspace, wire codec, binary shell — with no
network I/O, so the wire contract can be reviewed in isolation before anything
depends on it.

## Goals / Non-Goals

**Goals:**

- A compiling, tested workspace on Ubuntu and Windows from the first commit.
- Wire header v1 implemented once, in `protocol`, pinned by golden vectors.
- A hub binary that starts, loads config, logs, and shuts down cleanly — the
  shell that later changes hang features on.

**Non-Goals:**

- Any socket I/O, backplane, game-worker, or client-facing behavior.
- Serde on the wire (serde is config-only).
- MSRV pinning, packaging/releases, container images.

## Decisions

### Workspace layout: `crates/protocol`, `crates/hub`
Two members now; `crates/backplane`, `crates/hub-sdk`, and worker fixtures are
added by their own changes rather than stubbed — empty crates rot and mislead.
Alternative (all four crates now) rejected: speculative scaffolding violates the
prime directive.

### Wire format: hand-rolled fixed-layout codec, little-endian
Header v1 is exactly 24 bytes: `magic: u32, version: u16, header_size: u16,
channel: u8, flags: u8, session_id: u32, client_id: u32, payload_schema_version:
u16, payload_len: u32` (all little-endian). Alternatives considered: postcard or
bincode (serde-coupled; hard to hand to a TypeScript client author as a spec),
protobuf (codegen + dependency weight for 24 bytes). Fixed-width LE wins because
the layout *is* the documentation, and LAN bandwidth makes byte-shaving tricks
(varints, packing) pointless complexity.

### Forward compatibility via `header_size` and version gate
Receivers locate the payload using `header_size`, accept larger headers than
they know, and ignore unknown header bytes; packets with unknown magic or a too-
new `version` are rejected before the payload is touched. This is the cheapest
possible evolution path: adding a field later bumps `header_size`, never breaks
old receivers.

### Channel and payload opacity
The codec validates nothing about `channel` and treats the payload as bytes.
Channel semantics are defined per link (client edge, backplane) in their own
specs — this is the rule that keeps the hub generic.

### `protocol` stays runtime-free and wasm-friendly
No tokio, no I/O, no allocator gymnastics: `std`-only encode/decode. The crate
must compile for `wasm32-unknown-unknown` eventually (browser clients will
embed it), so anything that would break that is rejected now.

### Errors: `thiserror` in libraries, `anyhow` in the binary
Typed errors at the codec/config boundary; context-rich but untyped at the
binary edge. No custom error framework.

### Config: clap-derive CLI + serde TOML, `deny_unknown_fields`
A typo'd config key must fail loudly, not silently default. Missing file means
all-defaults (the shell needs no config to boot). Later changes extend the same
structs (backplane address, WT port, worker paths).

### Logging: `tracing` + `tracing-subscriber` env-filter
Level from config, `RUST_LOG` overrides. One startup line carries version and
resolved config — enough to answer "what is running" from journald or a console.

### Shutdown: tokio signal handling, bounded grace
First SIGINT/SIGTERM/Ctrl-C starts graceful shutdown with a 5-second cap;
a second signal exits immediately. Clean shutdown exits 0; config errors exit 1;
fatal runtime errors exit 2. tokio enters now because every later change
(networking, watchdog) needs it; `protocol` still does not.

### Golden vectors: inline hex fixtures in one test file
Six vectors pin the format: valid v1 packet; future `header_size` (32); bad
magic; unsupported version; unknown channel; truncated buffer. Regenerating
them is a deliberate task tied to a protocol-version review — never a side
effect of refactoring.

### CI: GitHub Actions, matrix `ubuntu-latest` + `windows-latest`
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`. The pure-Rust dependency rule (see `openspec/config.yaml`) is what
makes one matrix suffice for both platforms.

## Risks / Trade-offs

- **The header is a forever contract.** Mitigated by the `header_size` escape
  hatch, the version gate, and golden vectors.
- **Golden vectors can drift silently** if regenerated casually. Mitigated by
  keeping them in one reviewed file with a comment stating the regeneration
  policy.
- **`deny_unknown_fields` config** fails hard on additive config drift between
  hub versions. Accepted: hub and config are co-deployed on one box; the error
  names the offending key.
- **Pure-Rust dependency rule may exclude convenient C crates later** (e.g.
  ENet). Accepted by design; the hub has no need for them on its planned paths.
