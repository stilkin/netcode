# build-pipeline Specification

## Purpose
Guarantees that the workspace builds and passes its checks identically on
Ubuntu and Windows, so the pure-Rust dependency rule and cross-platform promise
are enforced continuously rather than aspirationally.

## Requirements

### Requirement: Local commit gates

Every commit MUST pass `cargo fmt --check`, `cargo clippy --all-targets` with
warnings denied, and `cargo test --workspace` through the version-controlled
pre-commit hook at `.githooks/pre-commit`, activated per clone with
`git config core.hooksPath .githooks`. The hook MUST remain bypassable with
`git commit --no-verify` for work-in-progress commits.

#### Scenario: Hook blocks a failing commit

- **WHEN** a commit is attempted with changes that fail any gate
- **THEN** the pre-commit hook rejects the commit and names the failing gate

#### Scenario: Work-in-progress bypass

- **WHEN** a commit is made with `--no-verify`
- **THEN** the commit succeeds without running the gates
