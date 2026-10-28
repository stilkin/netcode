# Design: add-release-artifacts

Grounding facts from the codebase (verified):

- `.githooks/pre-commit` runs fmt, clippy `-D warnings`, tests; activated
  per clone via `core.hooksPath`; deliberately no hosted CI (archived
  change `replace-ci-with-local-gates`).
- Workspace version lives once in `[workspace.package]` (0.1.0); every
  crate inherits it.
- Three binaries ship: `hub`, `sample-worker` (from its crate), and
  `netcode-bots` (conformance crate). The hub and workers are colocated
  by architecture — one deployment unit, so one archive.
- `crates/conformance/scenarios/smoke.toml` already health-checks a
  running hub end to end; the bots were built to judge from outside.
- The transport tree compiles `ring` (documented exception) → source
  builds need a C toolchain per platform (musl-gcc, mingw-w64); this is
  exactly what artifacts make users forget.
- `gh` is authenticated; `gh release` and ghcr.io are artifact hosting,
  not CI — consistent with the no-hosted-CI decision.

## D1 — `xtask`: release tooling as a workspace crate, not scripts

A minimal `xtask` crate (the standard cargo pattern) run via
`cargo run -p xtask -- release`. Rust, so it runs identically on the
release machine's OS and is testable like any other code in the tree.

*Alternatives considered:* shell scripts or Make (rejected: the project
targets Windows identically — the release box might be either); a
`just`file (rejected: a new tool dependency to accomplish what a crate
already does); doing it by hand (rejected: a documented-by-doing
pipeline rots; a commanded one is re-run every release).

## D2 — One Linux release machine, two cross targets, static binaries

`x86_64-unknown-linux-musl` (fully static, `ldd`-verifiable) and
`x86_64-pc-windows-gnu` (mingw cross). Prerequisites are *probed*, not
assumed: musl-gcc and mingw-w64 are checked by command before any
compilation starts, with install hints (package names per distro), so a
missing toolchain is a sentence, not a compiler error dump.

*Alternatives considered:* building Windows binaries on Windows
(rejected: one machine produces everything — the whole point);
`cargo-zigbuild` (rejected for v1: an extra tool layer for the same
output; a future flag if it proves valuable).

## D3 — The Dockerfile builds from scratch inside the container

Multi-stage: a builder stage with the Rust toolchain and musl-tools
compiles the static binaries; the runtime stage is `FROM scratch` with
only `hub`, `sample-worker`, and a default `hub.toml` (catalog →
`/sample-worker`). Anyone with docker gets a working image from a clean
checkout — no Rust, no C compiler, no xtask. The image is the
lowest-entry-bar artifact of all, below even the static binaries.

*Alternatives considered:* copying host-built binaries into the image
(rejected: silently couples the image to the host's toolchain; the
in-container build is self-contained); `cargo-chef` layer caching
(rejected for v1: complexity for rebuild speed the release cadence does
not need; additive later).

## D4 — Artifacts prove themselves before anyone receives them

After packaging, the release flow starts the just-built static `hub`
with a temp config (ephemeral ports) and runs the just-built
`netcode-bots` smoke scenario against it. The release proceeds only on
exit 0. This is the release-validation role the bots were designed for —
the loop closes: the tool that judges hubs judges ours first.

*Alternatives considered:* manual post-release checks (rejected: the
first user should not be the smoke test); validating only in CI
(rejected: there is no CI, by decision).

## D5 — Publishing is opt-in flags, hosting is not CI

Default: everything builds, validates, and stops. `--publish` adds: git
tag `v<version>`, `gh release create` with artifacts + SHA256SUMS,
`docker push` of the versioned tag (registry/login is the operator's
concern). `cargo audit` runs when installed (vulnerability findings fail
the release) and its absence is printed, not fatal.

## D6 — Pre-push closes the bypass, keeps the escape hatch

`.githooks/pre-push` runs the same three gates as pre-commit on every
push. `--no-verify` remains available at both levels for deliberate WIP
— the contract is "breakage doesn't reach other people unnoticed", not
"you can't rush locally".

## D7 — Scope guard for v1

Host-arch Docker image only, x86_64 targets only, no signing, no macOS.
The xtask target list and the Dockerfile are structured so arm64 and
multi-arch buildx are additive flags later, not redesigns.
