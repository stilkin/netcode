# Tasks

Ordered so the workspace compiles and tests stay green after every task.
Each section names the requirements it implements.

## 1. xtask scaffold *(enables the requirements below)*
- [x] 1.1 `crates/xtask` workspace member: `cargo run -p xtask -- <cmd>`; commands `preflight`, `release` (skeleton with ordered phases and `--publish`/`--skip-*` flags), shared plumbing (workspace root discovery, command runner that streams output).
- [x] 1.2 Preflight: clean-tree check naming changed files; workspace-version consistency; tool probes (`musl-gcc`, `x86_64-w64-mingw32-gcc`, `docker`, `gh`, `cargo-audit`) with per-missing-tool install hints and skip semantics for optional tools.

## 2. Cross-builds and packaging *(release-artifacts: Cross-build matrix, Packaging and checksums)*
- [x] 2.1 Build phase: `cargo build --release --target x86_64-unknown-linux-musl` and `--target x86_64-pc-windows-gnu` for the three binaries; static-ness check for musl (`ldd`).
- [x] 2.2 Package phase: per-target `.tar.gz`/`.zip` with binaries + example `hub.toml`; `SHA256SUMS`; artifacts under `target/release-artifacts/<version>/`.
- [x] 2.3 Unit tests for the packager (archive contents, checksums correctness) using a scratch directory.

## 3. Docker image *(release-artifacts: Docker image)*
- [x] 3.1 `Dockerfile` (multi-stage, builder → `FROM scratch`, example config with in-image catalog, `EXPOSE 4433/udp 8080/tcp`, hub entrypoint).
- [x] 3.2 Image phase in xtask: `docker build` + version tag (+ `:latest` when version-tagged); skip flag for docker-less machines.
- [x] 3.3 Test: image build from a clean checkout context and a container boot check (startup log or port respond) — gated on docker availability, skipped loudly otherwise.

## 4. Self-validation *(release-artifacts: Artifact self-validation)*
- [x] 4.1 Validate phase: run the musl `hub` with a temp config (ephemeral ports, catalog → the musl `sample-worker`), wait for startup logs, run the musl `netcode-bots` with the smoke scenario pointed at it; non-zero bot exit fails the release; teardown kills the hub.
- [x] 4.2 Test of the validator itself against a deliberately broken "hub" (a stub that exits) proving the phase fails the release.

## 5. Push gates *(build-pipeline: Local commit gates — pre-push)*
- [x] 5.1 `.githooks/pre-push`: the same three gates as pre-commit; `--no-verify` still honored by git for deliberate pushes.
- [x] 5.2 Documentation of activation (already one command per clone) and the bypass contract in development docs.

## 6. Supply-chain check and publishing *(release-artifacts: Release command)*
- [x] 6.1 `cargo audit` phase: run when installed (vulnerabilities fail the release), print-and-continue when absent.
- [x] 6.2 `--publish` phase: git tag `v<version>`; `gh release create` uploading artifacts + SHA256SUMS; `docker push` of the version tag; each step runs only after validation and prints exactly what it will do before doing it.
- [x] 6.3 `--version <x.y.z>` flag rewriting the workspace version for release prep commits.

## 7. Documentation
- [x] 7.1 README: artifact story first (docker one-liner, static binaries, release page), source-build prerequisites per platform (rustup + C toolchains; the documented exception in practice), deployment examples (docker run, binary + hub.toml).
- [x] 7.2 `docs/development.md`: release runbook (preflight → build → validate → publish), pre-push activation/contract, coverage/layout notes; `docs/roadmap.md`: record the post-roadmap change; AGENTS.md: status, gates summary (commit + push), release pointer.

## 8. Wrap-up
- [x] 8.1 `openspec validate add-release-artifacts --strict` green; full workspace gate green (fmt, clippy `-D warnings`, tests).
- [ ] 8.2 Archive the change, commit; a real local release rehearsal (no `--publish`) proving the pipeline end to end on this machine.
