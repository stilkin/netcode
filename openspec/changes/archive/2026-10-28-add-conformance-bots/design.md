# Design: add-conformance-bots

Grounding facts from the codebase (verified):

- The change-3/4 test clients (`Browser` in `client_edge_end_to_end.rs`
  and `lobby_end_to_end.rs`) already implement the full browser trust
  path: lobby `/cert-hash` fetch, hash-pinned WebTransport, `Connect`
  handshake, buffered stream parsing, datagram exchange. They are the
  bot's seed.
- The SDK drives ticks with `tokio::time::interval` at `tick_hz` —
  wall-clock. Byte-exact session recordings including timing are therefore
  impossible without worker-side replay support; fixtures must pin logical
  ordering.
- Datagrams are unreliable *by design* (state channel = newest-wins);
  fixtures and thresholds must tolerate bounded loss.
- The scripted fake-worker pattern (`crates/hub/tests/backplane_service.rs`,
  `client_edge.rs`) provides deterministic, input-driven worker behavior
  over real backplane sockets — no game binary needed for determinism.
- `sample-worker`'s own tests are the only place `CARGO_BIN_EXE_sample-worker`
  exists — the pattern every prior e2e followed.

## D1 — An external-client crate: `conformance` (lib + bin), no hub dependency

Bots exist to judge a hub from outside; the library must not link the hub.
`crates/conformance` provides the bot and runner as a library (reusable by
any game's tests) and the `netcode-bots` binary for release validation
against any running hub. Hub-dependent wiring lives only in the crate's
own tests (dev-dependency) and in `sample-worker`'s final e2e.

*Alternatives considered:* test-only helpers in each crate (rejected:
duplicated three times already, and release validation needs a binary);
bots linking the hub for convenience (rejected: a conformance tool that
shares code with the thing it judges judges nothing).

## D2 — Scenarios are TOML, fixtures are JSON

Scenario files are configuration — TOML, consistent with `hub.toml` and
the strict-key philosophy. Fixtures are recorded data compared at test
time — JSON, byte-stable and diffable, one file per recorded run.

*Alternative considered:* one format for both (rejected: different
lifetimes — scenarios are authored, fixtures are generated and pinned).

## D3 — Fixtures pin logical ordering, never timing; loss is bounded

A fixture records per bot: the ordered event payloads, the ordered unicast
payloads, and broadcast counters as a monotonicity series. Wall-clock
values never appear — the SDK's interval-driven ticks make them
meaningless. State-channel (datagram) entries may be absent up to the
scenario's loss bound; event-channel (stream) entries must be exact and
complete — the stream is reliable, so a missing event is a real failure.

*Alternatives considered:* wall-clock-exact recordings (rejected: needs
worker replay mode — a worker-feature change to serve a test concern);
requiring zero datagram loss (rejected: contradicts the channel's design).

## D4 — A scripted test worker for determinism; the real game once

The crate's tests run against an in-process hub whose sessions are served
by a scripted fake worker (the established pattern, promoted into the
conformance crate as test-support): greeting on attach, one unicast echo
per input, one broadcast counter per scripted tick. Deterministic because
responses are functions of inputs, not of wall time. The real echo arena
is exercised exactly once, from `sample-worker`'s tests, using the bot
library — real game, real spawn, real everything, no duplication.

*Alternatives considered:* making echo arena deterministic-with-seed
(rejected: complicates the reference game to serve testing); fake workers
only (rejected: one real-game run is the cheapest possible proof that the
bot library works against the SDK contract).

## D5 — Thresholds are declared per scenario, not baked into the spec

Join success, loss ratio, ordering violations, latency percentiles: a
scenario states its expectations; the runner measures and compares. The
spec fixes only that thresholds exist and are enforced — numbers belong to
scenario files, where a slower laptop can legitimately declare gentler
ones.

## D6 — Conformance verification lives in the bot's receive path

Every incoming packet is decoded through the same strict client-protocol
rules a browser client would apply; a malformed header on the stream, a
framing error, or a channel violation is recorded as a conformance failure
with position and bytes. The bots do not merely drive traffic — they
continuously judge the hub against the wire contracts.
