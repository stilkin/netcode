# Proposal: add-webtransport-client-edge

## Why

The hub terminates no client connections today: workers register and route
packets over the backplane, but the client-facing half of the system does not
exist. This change gives the hub its client edge — a WebTransport endpoint
desktop browsers connect to with no install — and completes the packet path
the architecture promises: browser ⇄ WebTransport ⇄ hub ⇄ backplane ⇄ worker.
After it lands, the lobby/admin API (change 4) and conformance bots (change 5)
have a real client surface to build on.

## What Changes

- `hub` gains a `client_edge` module: WebTransport listener (via `wtransport`,
  pure Rust on quinn/rustls), a self-signed certificate generated per boot
  (via `rcgen`) whose SHA-256 hash clients pin, and per-connection relay
  pumps between WebTransport and the existing `ClientHandle` seam
  (`attach` / `recv` / `feed_packet` / drop-detach).
- Handshake: the client's first bidirectional stream carries a `Connect`
  frame (token, session id, player name); the hub answers `Accept` (with the
  assigned client id) or `Reject` (with a reason), then that stream becomes
  the events channel.
- Channel mapping: state (channel 0) ⇄ unreliable WT datagrams; events
  (channel 1) ⇄ the reliable control stream, both directions, packets
  self-delimiting via the existing header length.
- Backpressure: datagram sends never block and may drop (state is
  newest-wins by design); stream writes obey QUIC flow control and a slow
  client stalls only itself.
- `protocol` gains the `Connect`/`Accept`/`Reject` codecs (std-only, so
  wasm32 browser clients can reuse them), pinned by golden vectors.
- A minimal bootstrap HTTP endpoint serves the current cert hash and a
  built-in test page — the stopgap that makes the edge manually verifiable
  from any browser before the lobby API exists (change 4 absorbs it).
- `[client_edge]` config section: `bind` (default `0.0.0.0:4433`),
  `bootstrap_bind` (default `0.0.0.0:8080`), `token` (default empty = open
  LAN trust, mirroring backplane semantics).
- Integration tests drive a real in-process WebTransport client against the
  hub; the flagship end-to-end test runs browser-protocol client ⇄ hub ⇄
  spawned `sample-worker`.

## Capabilities

### New Capabilities

- `client-edge` — the hub's browser-facing endpoint: WebTransport listener,
  certificate and hash pinning, bootstrap endpoint, connect handshake,
  token gate, session join rules, datagram/stream relay with backpressure,
  connection lifecycle.
- `client-protocol` — the contract a browser client codes against: join
  frame layout, channel-to-transport mapping, stream framing rules.

### Modified Capabilities

- `wire-protocol` — gains the `Connect`/`Accept`/`Reject` frame codecs and
  golden vectors (protocol v1 join extension).
- `hub-runtime` — gains the client edge lifecycle: binds the edge listener
  at boot, logs its address and cert hash, drains client connections within
  the grace period on shutdown. Existing behavior (backplane, exit codes)
  is unchanged.

## Out of Scope

- Lobby/admin HTTP API, session listing/creation, per-session token
  issuance (change 4; the bootstrap endpoint is a stopgap it replaces).
- Conformance bots, deterministic streams, load profiles (change 5).
- A supported browser-side JS/wasm SDK — the built-in test page is a
  fixture, not a product.
- Multiple sessions or client identities per connection: one connection
  joins one session as one client; joining another session is a new
  connection.
- Certificate persistence or stable server identity across restarts.
- Direct-connect mode, native UDP transports, host migration, persistence.
