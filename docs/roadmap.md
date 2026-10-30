# Roadmap

Work is organized as [OpenSpec](https://openspec.dev) changes — one reviewable
unit each, drafted in `openspec/changes/<name>/`, implemented, then archived
into `openspec/specs/` as the source of truth. Each change leaves the workspace
green. See [development.md](development.md) for the workflow.

| # | Change | Status | Scope | Capabilities |
|---|--------|--------|-------|--------------|
| 1 | `add-hub-foundation` | ✅ implemented (2026-10-25) | Cargo workspace; wire header v1 codec pinned by golden vectors; hub binary shell (config, logging, graceful shutdown); initial CI matrix Ubuntu+Windows (later removed) | `wire-protocol`, `hub-runtime`, `build-pipeline` |
| 2 | `add-backplane-game-workers` | ✅ implemented (2026-10-26) | Loopback-TCP backplane (register/attach/route/broadcast/heartbeat); `hub-sdk` with `GameSession` trait; session registry + process watchdog; sample worker; end-to-end test | `backplane`, `session-registry`, `hub-sdk` |
| 3 | `add-webtransport-client-edge` | ✅ implemented (2026-10-28) | WebTransport endpoint (self-signed cert + served hash), connect-token handshake, channel mapping (state→datagrams, events→stream), relay routing with backpressure | `client-edge`, `client-protocol` |
| 4 | `add-lobby-admin-api` | ✅ implemented (2026-10-28) | HTTP JSON API: list games/sessions, create/close session, kick client; session metadata for directory UIs. An OpenAPI 3 file (`docs/rest_api.yaml`) ships with it | `lobby-api` |
| 5 | `add-conformance-bots` | ✅ implemented (2026-10-28) | Headless bot client, deterministic scripted-run stream fixtures, load smoke profile (N bots × M sessions) | `conformance` |

## Post-roadmap changes

| Change | Status | Scope |
|---|---|---|
| `replace-ci-with-local-gates` | ✅ implemented (2026-10-26) | Removed hosted CI by owner decision; version-controlled pre-commit/pre-push fmt, clippy, and test gates |
| `add-release-artifacts` | ✅ implemented (2026-10-28) | Local release pipeline: `xtask` (cross-builds, packaging, checksums), `FROM scratch` Docker image, bots-driven artifact self-validation, `gh release`/`docker push` publishing, pre-push gate |
| `fix-worker-frame-reader` | ✅ implemented (2026-10-28) | Dedicated ordered SDK frame reader prevents ticks/heartbeats from cancelling partial TCP frames |
| `fix-listener-resilience` | ✅ implemented (2026-10-28) | Shutdown-aware WebTransport negotiation, 256 KiB incomplete-stream cap, 30-second lobby request deadline |
| `fix-startup-and-release-validation` | ✅ implemented (2026-10-28) | Early invalid tick-rate errors; release validation builds a current host bot binary first |
| `bound-closed-session-history` | ✅ implemented (2026-10-28) | Bounded oldest-first retention of 1,024 closed-session tombstones without evicting live sessions |
| `harden-lobby-admission` | ✅ implemented (2026-10-28) | JSON content-type and unambiguous HTTP framing checks; atomic pending/active session limit (default 64) |
| `limit-client-input-rate` | ✅ implemented (2026-10-29) | Shared per-client datagram/stream packet budget (4,096/sec, 512 burst); offender-only disconnect without throttling healthy clients |
| `deliver-player-names` | ✅ implemented (2026-10-30) | `ClientAttached` carries the joining player's display name; the SDK passes it to `GameSession::on_attach` |

## Deliberately out of scope (for now)

- **Direct-connect mode** — the hub stepping out of the packet path for extreme
  twitchy sessions. The wire header already leaves room for it; it stays a
  config flag, not a rewrite, when it happens.
- **Native UDP transport** for non-browser clients (e.g. a Rust desktop client
  via renet). The protocol is transport-agnostic; this is additive.
- **Landing-page / directory UI** — a separate project that consumes the lobby
  API (QR codes, server browser). Not part of this repo.
- **Host migration** — sessions end when their worker ends.
- **Persistence / database** — state lives in worker memory; no DB in the near
  term. Revisit only with a concrete need (e.g. player profiles).
