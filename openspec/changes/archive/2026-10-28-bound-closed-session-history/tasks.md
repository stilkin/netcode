# Tasks

## 1. Bounded closed-session history

- [x] 1.1 Add overflow and repeated-close regressions, then implement the private 1,024-entry close-order queue with synchronous oldest-first eviction; confirm overflow fails before the fix and both tests pass after it, and `cargo test -p hub registry::tests` is green

## 2. Documentation and integration

- [x] 2.1 Remove B8 from the local findings document, then run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `openspec validate bound-closed-session-history --strict`; all must pass

## Workflow follow-up

- Archive the change and verify all capability specs.
- Commit and push the implementation and OpenSpec files; keep the review work document and `.pi/` untracked.
