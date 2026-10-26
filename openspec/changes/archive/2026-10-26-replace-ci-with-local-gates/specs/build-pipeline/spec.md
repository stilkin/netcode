# Spec Delta

## Purpose

Replaces hosted CI enforcement with the committed pre-commit hook: the same
formatting, lint, and test gates run locally before every commit, with zero
GitHub Actions usage.

## REMOVED Requirements

### Requirement: Cross-platform CI gate

~~Every push and pull request MUST run, on both Ubuntu and Windows runners:
`cargo fmt --check`, `cargo clippy --all-targets` with warnings denied, and
`cargo test`. A change SHALL be mergeable only when the matrix is green on
both platforms.~~ Removed: hosted runners are out of the project's budget by
owner decision. The gates move to the local pre-commit hook (below), which
runs the identical checks on the contributor's machine.

## ADDED Requirements

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
