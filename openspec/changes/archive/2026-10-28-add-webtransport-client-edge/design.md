# Design: add-webtransport-client-edge

Grounding facts from the codebase (verified, not assumed):

- `ServiceHandle::attach(session_id, player_name) -> ClientHandle` is the
  client seam; `ClientHandle` has `recv()` (outbound packets), `feed_packet()`
  (inbound), and **Drop = detach**. The in-process e2e tests already drive
  it exactly this way — the client edge is a WebTransport adapter around
  this handle, nothing more.
- `feed_packet` routes through `Command::ClientPacket`, and the backplane
  service already validates the header against the client's identity before
  forwarding (per its own contract). There is exactly one validation point
  and it stays where it is.
- `shutdown_loop` (hub lib) binds the backplane, logs, drains on signal —
  the edge plugs in symmetrically.
- `session-registry` set the precedent: capability ⇒ module in `hub`, not a
  crate.

## D1 — Module in `hub`, not a new crate

The client edge terminates browser connections; only the hub does that. No
worker ever links it, so it has no second consumer — the bar `backplane`
cleared to become a crate (shared by hub and hub-sdk) is not met. Cost of a
module: `hub` gains the wtransport dependency tree; acceptable because the
hub binary is the only thing that builds it and clippy/test already run
workspace-wide.

*Alternative considered:* a `client-edge` crate keeps the QUIC tree out of
`hub`. Rejected: a crate whose only consumer is `hub` is a speculative
layer; the smallest crate set wins.

## D2 — Transport: `wtransport` (+ `ring`), a documented exception to the pure-Rust rule

**Decision (owner, with deployment strategy):** the hub ships as artifacts
(Docker image + static binaries) — users never compile — and the project is
open-sourced with contributors as the only source builders. On Windows the
default `msvc` toolchain needs VS Build Tools for the linker regardless, and
Linux contributors have gcc by default, so a C crypto provider costs the
actual population approximately nothing. The invariant is amended (see
below) rather than silently eroded.

`wtransport` 0.7 (default features: `self-signed`, `ring`) provides the
complete WebTransport implementation — server **and** client. The client
half matters: integration tests drive a real WT client over loopback UDP
with certificate-hash pinning (`ClientConfig::with_server_certificate_hashes`),
the same trust path browsers use. `rcgen` (default features) generates the
self-signed certificate.

**The amendment (recorded in `openspec/config.yaml`, AGENTS.md, README):**
default builds stay rustup-only; the single documented exception is the
WebTransport transport tree (`wtransport` → quinn → rustls → `ring`),
confined to the client-edge dependency tree, exercised only by contributors
and release builds. The release pipeline (a later change) compiles the C in
containers/on the release machine; users receive artifacts.

*Alternatives considered:* a vendored, patched `quinn-proto` with pure-Rust
QUIC header protection (rejected: a permanent ~40k-line fork with
security-critical code we own, re-patched on every upstream security update —
a bigger violation of the prime directive's spirit than this documented
exception is of its letter); the h3 stack (rejected: even with `ring` it
requires hand-rolling the client half of the WT handshake in tests);
WebSocket transport (rejected: no unreliable datagrams).

## D3 — Certificate: generated per boot via `rcgen`, pinned by hash

At boot the hub generates an ECDSA P-256 self-signed certificate
(Ed25519 has spotty browser WebTransport support) with a validity window
within the W3C WebTransport two-week cap (issued now, ~7 days), and logs the
SHA-256 of its DER. Clients pin that hash when connecting — the standard
`serverCertificateHashes` pattern — and fetch it from the bootstrap endpoint
(D9). Restarting rotates the certificate; pages fetch the hash per load, so
rotation is invisible. No disk persistence.

*Alternative considered:* persisting the certificate for stable identity.
Rejected for this change: identity continuity matters only once the lobby
API issues long-lived tokens — change 4's problem, solved there if needed.

## D4 — Handshake: one bidirectional control stream, binary frames in `protocol`

The client opens exactly one bidi stream on session establishment and sends
a `Connect` frame (kind byte, version, token, session id, player name); the
hub answers `Accept` (client id) or `Reject` (reason) and closes the session
after a reject. Each frame carries a kind byte (`0x01` Connect, `0x02`
Accept, `0x03` Reject) so replies are self-describing; exact layouts live in
`crates/protocol/docs/wire-format.md`. After `Accept`, **the same stream
becomes the events channel** — one stream per connection, total. The frame
codecs live in `protocol` (std-only): that crate exists precisely so wasm32
browser clients can share byte-level code with the hub, and golden vectors
pin the layout like the header.

*Alternatives considered:* a JSON handshake (rejected: inconsistent with the
hand-rolled binary codecs and drags a JSON dep into wasm clients for no
gain); a separate events stream after the handshake (rejected: an extra
stream and an extra framing negotiation with no benefit — the handshake
frame and event packets are self-delimiting on the same stream).

## D5 — Token gate mirrors the backplane

`client_edge.token` empty ⇒ open LAN trust; set ⇒ a shared secret the
`Connect` must present. Identical semantics to `[backplane] token`, so
operators learn one rule. Static compare, no expiry, no issuance.

*Alternative considered:* per-session signed tokens now. Rejected: issuance
is the lobby API's job (change 4); building a signer before an issuer is a
speculative layer. The gate is one function; change 4 refactors it when
tokens get structure.

## D6 — One connection = one client = one session

A connection joins one session as one client. Joining another session is a
new connection. Browser tabs do this naturally, and it keeps the
connection's identity (session id, client id) a pair of fields rather than
a routing table.

*Alternative considered:* multiplexing several sessions per connection.
Rejected: speculative — no roadmap change needs it, and it complicates
header validation and backpressure for a hypothetical benefit.

## D7 — Channel mapping: state ⇄ datagrams, events ⇄ control stream

The header's channel byte (already reserved: 0 = state, 1 = events, opaque
to the hub) selects the transport primitive: state crosses as WebTransport
datagrams (unreliable — snapshots are newest-wins, dropping one is correct
behavior), events cross the control stream (reliable, ordered — joins and
game events must not be lost). The hub maps by channel byte; payloads stay
opaque. The prime directive holds.

Stream framing is free: packets are self-delimiting (the header's length
field), so the control stream is a plain concatenation after the handshake
frame. No length prefixes, no extra framing layer.

## D8 — Backpressure: per-connection pump with a drop policy and a stall guard

Each accepted connection runs one relay pump draining its `ClientHandle`:

- **Datagram sends** are non-blocking; when the transport refuses (queue
  full), the packet is dropped and logged at debug. Correct for state
  (newest-wins), and droppability is decided by the channel byte — no
  payload peeking.
- **Stream writes** await QUIC flow control. A client that stops reading
  stalls its own stream writes and therefore its own pump — other clients'
  pumps are separate tasks and are unaffected. The stall is self-inflicted
  and isolated.
- **Stall guard:** if a pump's pending outbound backlog exceeds a fixed
  watermark (1024 packets), the connection is closed as abusive — bounded
  memory per connection without a second policy engine.

Inbound (client → worker) needs no new policy: WT gives datagram frames and
ordered stream bytes, both feed `feed_packet`, and the existing
service-side identity validation is the boundary.

## D9 — Bootstrap endpoint: hand-rolled HTTP/1.1, two routes

Before the lobby API exists, the page needs the cert hash from somewhere.
A ~100-line tokio TCP responder answers `GET /cert-hash` (JSON, CORS `*`)
and `GET /` (an embedded single-file test page: connect, join, watch ticks,
send inputs). Anything else is 404. Change 4's lobby API absorbs or retires
it.

*Alternatives considered:* axum/hyper (rejected: the tower ecosystem for two
GET routes is exactly the heavy dependency the project refuses for small
needs — same reasoning as the hand-rolled codecs); logging the hash only
(rejected: untestable from a browser without a fetchable endpoint).

## D10 — Testing: real WebTransport client in-process

`wtransport::Client` connects over real loopback UDP with a custom rustls
verifier that pins the hub's cert hash — mirroring the browser's
`serverCertificateHashes` behavior, so tests exercise the same trust path
players do. Unit tests: join-frame codecs (round-trip + golden vectors),
config section. Integration: handshake accept/reject matrix (wrong token,
unknown session, full session, garbage first frame), datagram + stream
relay, slow-reader isolation, disconnect-detach. Flagship e2e mirrors
`backplane_end_to_end.rs`: WT client ⇄ hub ⇄ spawned `sample-worker`,
inputs echoed, broadcasts observed, tab-close detaches.
