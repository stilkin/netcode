# Tasks

## 1. Runtime correctness

- [x] 1.1 Add the fragmented-frame regression and replace the SDK's direct `select!` frame read with a dedicated reader and capacity-one channel; confirm the test fails on the old code, then passes with the fix, and `cargo test -p hub-sdk` is green

## 2. Documentation and integration

- [x] 2.1 Correct the B2 review record and client-edge cancel-safety comment to distinguish `read` from `read_exact`; verify with `cargo test -p conformance` that the unchanged bot remains green
- [x] 2.2 Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `openspec validate fix-worker-frame-reader --strict`; all must pass

## Workflow follow-up

- Archive the change after review requirements are satisfied.
- Verify the archived specs with `openspec validate --specs`.
- Commit only the implementation and OpenSpec files; leave the review work document and `.pi/` untracked.
