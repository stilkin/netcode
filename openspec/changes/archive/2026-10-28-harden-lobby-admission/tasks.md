# Tasks

## 1. HTTP request admission

- [x] 1.1 Add raw-request regressions for JSON media types, case-insensitive auth/body headers, duplicate or malformed `Content-Length`, and `Transfer-Encoding`; confirm they fail on the current parser before implementing the focused read outcome and header validation (lobby-api: Session creation, HTTP request framing)
- [x] 1.2 Implement `415` JSON admission and `400` framing rejection, then run `cargo test -p hub --test lobby` with all existing token, timeout, and route tests green (lobby-api: Session creation, HTTP request framing)

## 2. Atomic live-session capacity

- [x] 2.1 Add `[backplane] max_sessions` configuration (default 64, zero denies creation) and registry/service regressions for pending/active limits, no ID consumption, close releasing capacity, and self-registration at the limit (hub-runtime: Backplane session limit; session-registry: Live session capacity; backplane: Worker registration and session claiming)
- [x] 2.2 Enforce the limit in registry creation before ID allocation, propagate a typed spawn rejection to lobby `503`, preserve pending claims and spawn-I/O `500`s, and add a real-worker create/reject/close/create end-to-end regression (lobby-api: Session creation; session-registry: Live session capacity)

## 3. Documentation and integration

- [x] 3.1 Update `docs/rest_api.yaml`, README configuration/API guidance, and the deployment example with the new statuses, limit, and production token guidance; remove S1/S4 from the local findings document without committing it
- [x] 3.2 Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `openspec validate harden-lobby-admission --strict`; all must pass

## Workflow follow-up

- Archive the change and verify all capability specs.
- Commit and push the implementation and OpenSpec files; keep the review work document and `.pi/` untracked.
