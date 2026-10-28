# Roadmap

Work is organized as [OpenSpec](https://openspec.dev) changes — one reviewable
unit each, drafted in `openspec/changes/<name>/`, implemented, then archived
into `openspec/specs/` as the source of truth. Each change leaves the workspace
green. See [development.md](development.md) for the workflow.

| # | Change | Status | Scope | Capabilities |
|---|--------|--------|-------|--------------|
| 1 | `add-hub-foundation` | ✅ implemented (2026-10-25) | Cargo workspace; wire header v1 codec pinned by golden vectors; hub binary shell (config, logging, graceful shutdown); CI matrix Ubuntu+Windows | `wire-protocol`, `hub-runtime`, `build-pipeline` |
| 2 | `add-backplane-game-workers` | ✅ implemented (2026-10-26) | Loopback-TCP backplane (register/attach/route/broadcast/heartbeat); `hub-sdk` with `GameSession` trait; session registry + process watchdog; sample worker; end-to-end test | `backplane`, `session-registry`, `hub-sdk` |
| 3 | `add-webtransport-client-edge` | ✅ implemented (2026-10-28) | WebTransport endpoint (self-signed cert + served hash), connect-token handshake, channel mapping (state→datagrams, events→stream), relay routing with backpressure | `client-edge`, `client-protocol` |
| 4 | `add-lobby-admin-api` | ✅ implemented (2026-10-28) | HTTP JSON API: list games/sessions, create/close session, kick client; session metadata for directory UIs. An OpenAPI 3 file (`docs/rest_api.yaml`) ships with it | `lobby-api` |
| 5 | `add-conformance-bots` | ✅ implemented (2026-10-28) | Headless bot client, deterministic scripted-run stream fixtures, load smoke profile (N bots × M sessions) | `conformance` |

## Post-roadmap changes

| Change | Status | Scope |
|---|---|---|
| `add-release-artifacts` | ✅ implemented (2026-10-28) | Local release pipeline: `xtask` (cross-builds, packaging, checksums), `FROM scratch` Docker image, bots-driven artifact self-validation, `gh release`/`docker push` publishing, pre-push gate |

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
