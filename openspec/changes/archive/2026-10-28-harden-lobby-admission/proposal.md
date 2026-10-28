# Proposal

## Why

The lobby currently accepts JSON bodies with browser-simple media types and can spawn an unbounded number of worker processes. Its minimal HTTP parser also treats header names as case-sensitive, accepts duplicate `Content-Length` fields, and ignores `Transfer-Encoding`. We want to close the cross-site creation and process-exhaustion paths while making request framing deterministic, without replacing the deliberately small HTTP server.

## What Changes

- Require `Content-Type: application/json` for `POST /api/sessions`, allowing normal media-type parameters and rejecting missing or other types with `415`.
- Parse recognized HTTP header names case-insensitively; reject malformed or duplicate `Content-Length` and any `Transfer-Encoding` with `400` before routing.
- Add `[backplane] max_sessions` (default 64; zero disables new sessions), counting pending and active sessions but not closed tombstones.
- Enforce the limit atomically in the single-owner registry before allocating an ID or spawning a process. Rejected lobby creation returns `503`; self-registration at capacity is rejected; closing a session frees capacity.
- Preserve open-LAN token defaults while documenting production token guidance, and update the OpenAPI contract and examples.

Out of scope: S2/S3/S5/S6, mandatory authentication, CORS/preflight support, reverse-proxy support, an HTTP framework, client rate limiting, and new dependencies.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `lobby-api`: JSON-only creation, deterministic request framing, and capacity-aware creation responses.
- `session-registry`: bounded live-session admission with capacity released on close.
- `backplane`: self-registration observes the same live-session limit.
- `hub-runtime`: the backplane session limit is strict, documented configuration with a safe default.

## Impact

- `crates/hub/src/lobby/mod.rs`: focused request-read outcome, recognized-header parsing, and media-type/capacity responses.
- `crates/hub/src/{config,registry,backplane_service,spawn}.rs`: configured atomic live-session admission and typed spawn rejection.
- Hub and sample-worker integration tests, `docs/rest_api.yaml`, `README.md`, and `deploy/hub.toml`.
- No dependencies, wire-format changes, database changes, or token-default changes.
