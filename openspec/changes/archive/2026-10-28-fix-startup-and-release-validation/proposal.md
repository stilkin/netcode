# Proposal

## Why

A zero or unrepresentable worker tick rate currently reaches a Tokio interval panic instead of a useful configuration error. Separately, Docker release validation can execute a missing or stale host `netcode-bots`, undermining the guarantee that the current conformance tooling judges each artifact.

## What Changes

- Validate `tick_hz` and its computed tick duration at SDK startup, before opening a backplane connection, and return an error naming the field.
- Build the current host `netcode-bots` with Cargo before every Docker-image validation, making the step a no-op when already current.
- Correct the `--skip-linux` output so it no longer claims Docker self-validation is skipped.
- Add SDK regression coverage and rehearse Docker validation from a fresh worktree with no prebuilt host bots.

Out of scope: closed-session retention, new tick-rate configuration, mtime heuristics, release-flag or wire changes, process-runner abstraction, and unrelated security or simplification findings.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `hub-sdk`: invalid tick rates must produce configuration errors before any connection attempt.
- `release-artifacts`: Docker-image validation must build its host conformance bot from the current checkout.

## Impact

- `crates/hub-sdk/src/lib.rs`: one startup error variant and early tick-period validation.
- `crates/hub-sdk/tests/worker_runtime.rs`: zero and unrepresentable-rate coverage.
- `crates/xtask/src/{validate,release}.rs`: deterministic host-bot build and accurate skip output.
- `docs/development.md`: Docker rehearsal behavior clarified.
- `openspec/specs/{hub-sdk,release-artifacts}/spec.md`: validation requirements.
- No production dependencies or configuration keys change.
