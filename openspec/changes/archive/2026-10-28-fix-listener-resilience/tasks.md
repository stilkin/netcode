# Tasks

## 1. Connection admission isolation

- [x] 1.1 Add the raw-QUIC stalled-negotiation regression and dev-only quinn fixture, then move negotiation into the existing shutdown-aware `JoinSet`; confirm the test fails on the old accept loop and passes with the fix, and `cargo test -p hub --test client_edge` is green

## 2. Inbound stream bounds

- [x] 2.1 Add the two-client unframed-flood regression, then enforce the 256 KiB incomplete-buffer cap through the existing relay error path; confirm the test fails before the cap and passes after it, and `cargo test -p hub --test client_edge` is green

## 3. Lobby request deadline

- [x] 3.1 Add the paused-time incomplete-request regression and dev-only Tokio `test-util` feature, then wrap `serve_one` in the total 30-second deadline; confirm the test fails before the deadline and passes after it, and `cargo test -p hub --test lobby` is green

## 4. Integration and documentation

- [x] 4.1 Remove the addressed B3–B5 findings from the local review document, then run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `openspec validate fix-listener-resilience --strict`; all must pass

## Workflow follow-up

- Archive the change and verify all capability specs.
- Commit and push the implementation and OpenSpec files; keep the review work document and `.pi/` untracked.
