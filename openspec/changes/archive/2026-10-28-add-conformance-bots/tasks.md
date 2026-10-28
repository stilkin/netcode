# Tasks

Ordered so the workspace compiles and tests stay green after every task.
Each section names the requirements it implements.

## 1. Crate scaffold *(enables the requirements below)*
- [x] 1.1 Add `crates/conformance` to the workspace (lib + `netcode-bots` bin, `publish = false`); deps: `protocol`, `wtransport`, `tokio`, `tracing`, `serde`, `serde_json`, `toml`; dev-dep: `hub`.
- [x] 1.2 The scripted test worker as test-support (greeting per attach, one unicast echo per input, one broadcast counter per scripted tick), speaking the backplane protocol over real sockets.

## 2. Bot client *(conformance: Bot client)*
- [x] 2.1 Promote the `Browser` helper: lobby cert-hash fetch, hash-pinned WebTransport connect, join handshake, buffered event-stream parsing with strict framing checks, datagram send/receive — with every received packet verified against the client-protocol contract and violations recorded as failures.
- [x] 2.2 Tests against an in-process hub + scripted worker: join/exchange/verify; a deliberately malformed stream packet from a fake worker is reported as a violation.

## 3. Scenario runner *(conformance: Scenario runner, Run report)*
- [x] 3.1 Scenario model (TOML): endpoints, session sourcing (lobby-create or by-id), bot count/session spread, per-bot input scripts, duration, thresholds.
- [x] 3.2 Runner: drive bots, measure joins/packets/loss/ordering/latency percentiles, compare thresholds, emit the JSON report; exit 0/1 accordingly; unreachable hub and unknown sessions fail cleanly with reports.
- [x] 3.3 `netcode-bots run <scenario>` binary front end.
- [x] 3.4 Tests: a passing scenario exits 0 with measured values in the report; an unmeetable threshold exits 1 naming it.

## 4. Deterministic fixtures *(conformance: Deterministic fixtures)*
- [x] 4.1 Fixture format + recorder: per-bot ordered event payloads, ordered unicast payloads, broadcast counter series; loss-tolerant comparison bounded by the scenario.
- [x] 4.2 Recorded fixture for the scripted worker, pinned in the crate; replay test passes on every run.
- [x] 4.3 Mutation tests: altered payload, reordered events, and missing stream event each fail the comparison, naming bot/channel/position.

## 5. Load smoke profile *(conformance: Load smoke profile)*
- [x] 5.1 Churn support in the runner: staggered joins/leaves over the duration; declared N×M profiles.
- [x] 5.2 Default smoke scenario (dozens of bots, several sessions, bounded duration, gentle thresholds) as a committed file + test that it passes against the in-process hub.
- [x] 5.3 Isolation-under-load test: one bot stalls mid-run; every other bot still meets its thresholds.

## 6. Real-game end-to-end
- [x] 6.1 `crates/sample-worker/tests/conformance_end_to_end.rs`: the bot library against a real spawned echo arena via the lobby API (create → bots join → scripted inputs → echoes/broadcasts verified → report clean).

## 7. Documentation
- [x] 7.1 README (status, tooling section: `netcode-bots` usage + example scenario), `docs/architecture.md` (conformance to implemented), `docs/roadmap.md` (change 5 status), `docs/development.md` (layout, coverage), AGENTS.md status.

## 8. Wrap-up
- [x] 8.1 `openspec validate add-conformance-bots --strict` green; full workspace gate green (fmt, clippy `-D warnings`, tests).
- [x] 8.2 Archive the change, mark the roadmap, commit.
