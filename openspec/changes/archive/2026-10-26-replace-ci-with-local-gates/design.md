# Design

## Context

CI (GitHub Actions, Ubuntu + Windows matrix) was introduced by
`add-hub-foundation` and enforced by the `build-pipeline` capability. The
owner has decided against hosted runners; the pre-commit hook from the
developer-experience work already runs the identical gates locally.

## Goals / Non-Goals

**Goals:**
- Zero GitHub Actions usage.
- One enforcement point, version-controlled: `.githooks/pre-commit`.
- Docs describe reality (no phantom CI references).

**Non-Goals:**
- Replacing hosted CI with a self-hosted runner (unnecessary for one machine).
- Windows cross-compilation checks (mingw install needs interactive sudo;
  revisit if Windows breakage becomes a pattern).

## Decisions

### Delete the workflow, keep the hook as the single enforcement point
The hook already ran fmt + clippy + tests on every commit. Removing the
workflow loses the Windows leg of verification — accepted deliberately by the
owner; the pure-Rust dependency policy keeps Windows builds trivial to verify
on any Windows machine (`cargo test --workspace`).

### Spec transition
`build-pipeline`'s Cross-platform CI gate is REMOVED and replaced by the
Local commit gates requirement, which describes the hook's behavior (gates,
activation, bypass) — enforceable and testable on the actual development
machine, unlike the removed requirement.

## Risks / Trade-offs

- **Windows regressions land unnoticed** until someone builds on Windows.
  Mitigation: the pure-Rust policy (no platform-specific code paths beyond
  `cfg(unix)` signal handling and tests) keeps the surface small; revisit if a
  pattern of Windows-only breakage appears.
- **Contributors can bypass hooks** with `--no-verify`. Accepted for a
  single-developer project; the gates also run in `cargo llvm-cov`-style local
  verification and any future release script.
