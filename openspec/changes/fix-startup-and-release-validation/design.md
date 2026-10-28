# Design

## Context

`hub_sdk::run` computes `1.0 / tick_hz` only after connecting and registering. Zero produces an infinite duration panic; sufficiently large rates can round to a zero duration, which Tokio's interval also rejects. `SdkError` already distinguishes connection and registration failures but has no configuration variant.

`validate_docker_image` owns the host debug-bot path but does not build it. The release command correctly refuses dirty trees, and the local review document and `.pi/` are intentionally untracked. A temporary worktree therefore provides the cleanest fresh-checkout rehearsal without moving or committing those files.

## Goals / Non-Goals

**Goals:**

- Fail invalid tick rates clearly before any network side effect.
- Guarantee current host bots for every Docker-image validation.
- Preserve existing valid startup and release flows with minimal code.

**Non-Goals:**

- No new rate setting, mtime logic, process-runner abstraction, or production dependency.
- No closed-session retention or unrelated release-pipeline redesign.

## Decisions

1. **Validate once at `run` entry.** Reject zero, compute the period, reject a zero duration, and only then connect. Reuse the computed period for the interval. Alternative: custom `Deserialize`, rejected because manually constructed `SessionConfig` values would bypass it; validating only zero was rejected because the same interval requires a non-zero duration for extreme rates too.
2. **Add a precise `SdkError` variant.** `InvalidTickRate` names the field and non-zero-period rule. Reusing `Registration` would misclassify an operator error; panicking or `expect` preserves the bug. The additive enum variant is the only API surface change; no function signatures change.
3. **Always `cargo build -p conformance` before Docker validation.** Cargo rebuilds missing or stale outputs and is otherwise a no-op, so this is more reliable than timestamps. Place the build inside `validate_docker_image`, before starting a container, so every caller inherits it and build failure leaves no container behind.
4. **Keep release glue free of mocks.** The meaningful B7 verification is the real release rehearsal. Correct the contradictory `--skip-linux` message and the development runbook while touching that path.
5. **Rehearse from a temporary worktree.** After an initial implementation commit, add a detached worktree at that commit. Its empty target directory proves the no-host-bot case and its clean tree satisfies preflight without altering the main checkout's untracked work documents. Run without `--publish`, then remove the worktree.

## Risks / Trade-offs

- [A new public error variant can affect exhaustive downstream matches] → use one explicit variant with stable semantics rather than overloading unrelated errors; valid callers are unaffected.
- [Building host bots adds work to Docker validation] → Cargo's fresh-check makes repeated runs cheap, and correctness requires the current judge.
- [A worktree rehearsal builds more than an in-place rehearsal] → the extra compile is intentional fresh-checkout evidence and leaves the main working tree untouched.

## Migration Plan

No migration is required. Valid tick rates and existing release commands behave as before. Invalid rates now return an error instead of panicking; Docker validation gains a deterministic build step.
