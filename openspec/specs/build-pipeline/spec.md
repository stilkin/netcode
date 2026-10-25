# build-pipeline Specification

## Purpose
Guarantees that the workspace builds and passes its checks identically on
Ubuntu and Windows, so the pure-Rust dependency rule and cross-platform promise
are enforced continuously rather than aspirationally.

## Requirements

### Requirement: Cross-platform CI gate

Every push and pull request MUST run, on both Ubuntu and Windows runners:
`cargo fmt --check`, `cargo clippy --all-targets` with warnings denied, and
`cargo test`. A change SHALL be mergeable only when the matrix is green on both
platforms.

#### Scenario: Windows-only regression

- **WHEN** a commit compiles and passes tests on Linux but fails on Windows
- **THEN** CI fails on the Windows leg and the commit cannot merge
