# Proposal: add-release-artifacts

## Why

The roadmap is complete, but the project ships as source: everyone — a LAN
party host, a prospective contributor, a curious player — must build it.
That was tolerable while the pure-Rust invariant made builds trivial; with
the transport tree's documented `ring` exception, source builds need a C
toolchain, and the "low entry bar for new users" requirement points the
other way: users must receive artifacts they can run, not a build
environment to assemble. This change builds the local release pipeline
(no hosted CI, per standing owner decision): a one-command release that
cross-builds static binaries, packages them, builds a Docker image,
**validates its own output with the conformance bots**, and publishes.

## What Changes

- New `xtask` crate: `cargo run -p xtask -- release` orchestrates the
  whole flow locally — preflight (clean tree, required toolchains, named
  and loud when missing), version handling, cross-builds, packaging,
  checksums, Docker build/tag, artifact self-validation, optional
  publishing (git tag, `gh release` upload, `docker push`). No hosted CI
  is involved anywhere; `gh release`/`ghcr.io` are artifact *hosting*.
- **Static binary releases** built on one Linux machine:
  - `x86_64-unknown-linux-musl` — fully static `hub`, `sample-worker`,
    `netcode-bots`, an example `hub.toml`, per-target `.tar.gz`.
  - `x86_64-pc-windows-gnu` (cross via mingw-w64) — the same set as a
    `.zip`.
  - A `SHA256SUMS` file covering every artifact.
- **Docker image**: a version-controlled multi-stage `Dockerfile` —
  builder stage compiles the static musl binaries inside the container,
  the runtime stage is `FROM scratch` with just the hub, the sample
  worker, and a default config (the catalog points at `/sample-worker`
  in-image). `docker build` alone produces a working image from a clean
  checkout — no host toolchain at all. Ports: `4433/udp`, `8080/tcp`.
- **Artifact self-validation**: before anything is published, the release
  flow starts the freshly built hub binary and runs the bots' smoke
  scenario against it; a failing smoke fails the release. The change-5
  investment pays for itself here.
- **Pre-push hook** (`.githooks/pre-push`): the full gate (fmt, clippy
  `-D warnings`, tests) on every push — closing the documented
  `--no-verify` hole for anything that reaches other people. Still
  bypassable for WIP pushes with `git push --no-verify`.
- **Supply-chain check**: the release flow runs `cargo audit` when
  installed, and says so loudly when it is not.
- Docs: build prerequisites for source builders (rustup + the C
  toolchains the transport tree needs, per platform), the artifact
  story, and deployment instructions (binary + Docker).

## Capabilities

### New Capabilities

- `release-artifacts` — the local release pipeline: cross-build matrix,
  packaging + checksums, Docker image, artifact self-validation by bots,
  the release command's contract, and publishing.

### Modified Capabilities

- `build-pipeline` — gains push gates: the pre-push hook runs the full
  gate on every push, so `--no-verify` commits cannot reach the remote
  unnoticed.

## Out of Scope

- Hosted CI of any kind (standing owner decision).
- Code signing / SmartScreen suppression — unsigned binaries remain a
  documented friction until it hurts.
- macOS artifacts, and multi-arch (arm64) Docker images — natural
  follow-ups; the Dockerfile and xtask targets are structured so both
  are additive.
- Auto-triggered releases (tag pushes doing work) — a human runs the
  command; that is the point of local CI.
- Binary transparency/reproducible-build attestations beyond checksums.
