# Proposal: replace-ci-with-local-gates

## Why

The project owner does not want to use hosted GitHub runners. The repository
is public (Actions would be free), but the decision stands on simplicity: the
same gates already run locally through the committed pre-commit hook, so the
hosted matrix duplicates enforcement this machine can do itself.

## What Changes

- Remove `.github/workflows/ci.yml` — no GitHub Actions usage at all.
- The gates move entirely to the version-controlled pre-commit hook
  (`.githooks/pre-commit`): `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test`. Nothing new to build; the hook already exists and runs.
- Documentation stops referencing a CI matrix and describes the local gates
  (and their `--no-verify` bypass) as the single enforcement point.
- **Trade-off accepted:** Windows compile/test verification is no longer
  automatic. It becomes best-effort on contributor machines (the pure-Rust
  dependency policy keeps it cheap). Revisit hosted CI if the project gains
  Windows contributors or a release cadence that needs it.

## Capabilities

### Modified Capabilities

- `build-pipeline` — the Cross-platform CI gate requirement is removed and
  replaced by a Local commit gates requirement.

### New Capabilities

- None.
