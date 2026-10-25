# Proposal: add-hub-foundation

## Why

The netcode hub is greenfield. Before any networking can work, the repo needs
its Cargo workspace, the shared wire-format contract that every later link
(client-to-hub, hub-to-game) builds on, and a runnable hub binary skeleton with
configuration, logging, and graceful shutdown. Establishing the wire codec
first — pinned by golden vectors — is what keeps the hub
client-technology-agnostic later: anything that speaks the header can
interoperate without sharing code.

## What Changes

- New Cargo workspace with two crates: `protocol` (wire header codec) and
  `hub` (server binary skeleton).
- Wire header v1: fixed 24-byte little-endian layout, magic/version validation,
  forward-compatible header growth, opaque channel and payload handling.
- Golden-vector test fixtures that pin the byte format.
- Hub binary runtime: TOML config with strict unknown-key rejection, `tracing`
  logging to stdout, graceful shutdown on Ctrl-C/SIGTERM, `--version`.
- GitHub Actions CI matrix (Ubuntu + Windows): fmt, clippy `-D warnings`, tests.

## Impact

Affected specs (all new capabilities):

- `specs/wire-protocol/` — packet header and framing contract.
- `specs/hub-runtime/` — configuration, observability, shutdown, versioning.
- `specs/build-pipeline/` — cross-platform build and CI gates.

Out of scope: backplane and game-worker support, WebTransport/client
connections, lobby/admin API, discovery, direct-connect mode. Each arrives in
its own change (see README roadmap).
