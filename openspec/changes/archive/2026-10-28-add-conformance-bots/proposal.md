# Proposal: add-conformance-bots

## Why

Changes 1–4 are verified by construction-level tests: tens of clients,
scripted frame exchanges, one flagship flow each. Nothing answers the
operational questions the product makes: does a hub build conform to the
wire contracts under sustained load, do packets arrive in order at scale,
does the stall watermark actually bound memory, does a wedged player
degrade only themselves? This change builds the measuring instrument —
headless protocol-speaking bots — and the recorded streams and load
profiles that turn those questions into repeatable pass/fail checks. It is
also the release-validation tool: point it at any running hub.

## What Changes

- New crate `conformance` (lib + `netcode-bots` binary; no dependency on
  the hub — bots are external clients by definition).
- **Bot client**: the change-3/4 test clients productized — fetch the cert
  hash from the lobby API, WebTransport connect with hash pinning, join
  handshake, state datagrams and event streams — with every received
  packet *verified* against the client-protocol contract (header decode,
  stream framing, channel mapping). A violation is a conformance failure,
  not just a dropped packet.
- **Scenario runner**: `netcode-bots run <scenario.toml>` declares hub
  endpoints, session creation (via the lobby API) or existing session ids,
  N bots × M sessions, per-bot input scripts, duration, and thresholds
  (join success, bounded datagram loss, zero ordering violations, latency
  bounds). Exits 0 on pass, 1 with a JSON report on failure.
- **Deterministic fixtures**: scripted runs against a scripted test worker
  produce recorded streams (per-bot event payloads in order, unicast
  payloads in order, monotonic broadcast counters). A fixture replays and
  compares on every test run; mutating a fixture must fail the comparison.
  Fixtures pin *logical ordering*, never wall-clock timing — and tolerate
  bounded loss on the state channel, which is unreliable by design.
- **Load smoke profile**: the runner drives declared N×M profiles (e.g.
  dozens of bots across sessions with join/leave churn), asserting the
  declared thresholds and the isolation guarantee: a deliberately stalled
  bot must not affect any other bot's traffic.
- Tests: the crate's own suite runs against an in-process hub with the
  scripted worker; one end-to-end run in `sample-worker`'s tests drives
  the real echo arena with the bot library.

## Capabilities

### New Capabilities

- `conformance` — the bot client, scenario runner, deterministic fixtures,
  load smoke profiles, and the run report: everything that turns "it seems
  to work" into a repeatable verdict.

### Modified Capabilities

- None. This change is purely additive; no hub behavior changes.

## Out of Scope

- Browser automation — bots speak the protocol directly; a browser's
  WebTransport stack is the browser's conformance problem.
- Performance benchmarking beyond smoke thresholds (no flamegraphs,
  throughput records, or tuning).
- Game AI or realistic gameplay models — bots replay scripts.
- Fuzzing and adversarial-input generation.
- Hosted CI integration (no hosted CI by owner decision); the bots are a
  local/release tool, runnable by anyone against any hub.
- Worker-side replay determinism (wall-clock-exact session recordings) —
  revisit only if a game needs it.
