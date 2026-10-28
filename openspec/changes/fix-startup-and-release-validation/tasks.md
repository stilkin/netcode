# Tasks

## 1. Tick rate validation

- [x] 1.1 Add zero and unrepresentable-rate regressions, then validate the computed tick period at SDK startup with a dedicated error; confirm the tests fail with the old connection-first behavior and pass after validation, and `cargo test -p hub-sdk` is green

## 2. Docker validation tooling

- [x] 2.1 Build the current host conformance bot at the start of Docker validation, correct the `--skip-linux` message and development runbook, and verify with `cargo test -p xtask`

## 3. Integration gate

- [x] 3.1 Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `openspec validate fix-startup-and-release-validation --strict`; all must pass before the initial implementation commit

## 4. Fresh-checkout release rehearsal

- [ ] 4.1 After the initial commit, run `cargo run -p xtask -- release --skip-linux --skip-windows` in a detached worktree with no host debug bot; verify Cargo builds the bot, image self-validation passes, no publish occurs, and the worktree is removed; then remove B6/B7 from the local findings document

## Workflow follow-up

- Archive the change and verify all capability specs.
- Make the archive/spec-merge commit and push both commits; keep the review work document and `.pi/` untracked.
