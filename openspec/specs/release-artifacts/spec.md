# release-artifacts Specification

## Purpose
The local release pipeline: how one Linux machine turns a clean checkout
into validated, checksummed, publishable artifacts — static binaries, a
Docker image — without any hosted CI, and how those artifacts prove they
work before anyone receives them.

## Requirements

### Requirement: Cross-build matrix

The release flow MUST build, from one Linux machine, static release
binaries for `x86_64-unknown-linux-musl` and `x86_64-pc-windows-gnu`:
`hub`, `sample-worker`, and `netcode-bots` for each target. Missing
cross-toolchains (musl-gcc, mingw-w64) MUST fail the release with an
error naming the missing tool and how to install it — never a confusing
compiler error.

#### Scenario: Missing mingw toolchain

- **WHEN** a release build for the Windows target runs on a machine
  without `x86_64-w64-mingw32-gcc`
- **THEN** the release fails before compiling, naming the missing tool
  and its package name

#### Scenario: Static Linux binary

- **WHEN** the musl target builds successfully
- **THEN** the `hub` binary has no dynamic dependencies (`ldd` reports
  "not a dynamic executable" or equivalent)

### Requirement: Packaging and checksums

Each release MUST package per-target archives (`.tar.gz` for Linux,
`.zip` for Windows) named with the workspace version and target triple,
containing the three binaries plus an example `hub.toml` with a
commented `[[games]]` entry. A `SHA256SUMS` file covering every produced
artifact MUST sit beside them in the artifacts directory.

#### Scenario: Archive contents

- **WHEN** packaging completes
- **THEN** each archive contains `hub`, `sample-worker`, `netcode-bots`
  (suffixed `.exe` on Windows), and `hub.toml`, and the checksums file
  lists every archive with a correct SHA-256

### Requirement: Docker image

A version-controlled multi-stage `Dockerfile` MUST build the static musl
binaries in a builder stage and produce a runtime image `FROM scratch`
containing only the hub, the sample worker, and a default configuration
whose catalog points at the in-image worker. Building the image MUST
require nothing but `docker build` on a clean checkout. The image MUST
declare UDP 4433 and TCP 8080, and the hub MUST be the entrypoint.

#### Scenario: Clean-checkout build

- **WHEN** `docker build` runs on a machine with docker and no Rust
  toolchain
- **THEN** the image builds and a container from it boots the hub with
  the startup log lines on their ports

### Requirement: Artifact self-validation

Before publishing anything, the release flow MUST start the freshly
built hub binary with a temporary configuration and run the conformance
bots' smoke scenario against it end to end. A failing or crashing smoke
MUST fail the release; no artifact is published unvalidated. The
Docker-image validation path MUST build the host `netcode-bots` from the
current checkout before using it.

#### Scenario: Bots validate the artifact

- **WHEN** the release flow reaches validation
- **THEN** the built `hub` and `netcode-bots` run the smoke scenario
  together, and the scenario's exit code gates the release

#### Scenario: A broken binary stops the release

- **WHEN** the built hub binary fails to boot or the smoke scenario fails
- **THEN** the release aborts with the failure named, and no publishing
  step runs

#### Scenario: Docker validation builds its judge

- **WHEN** image validation runs without `target/debug/netcode-bots`
- **THEN** Cargo builds the current host binary before the smoke scenario starts

### Requirement: Release command

`cargo run -p xtask -- release` MUST orchestrate the flow in order —
preflight, cross-build, package, image, self-validate — and MUST refuse
to start on a dirty working tree. Publishing steps (git tag,
`gh release create` with the artifacts, `docker push`) MUST be opt-in
flags, MUST run only after self-validation passes, and MUST never
involve hosted CI. When `cargo audit` is installed it MUST run as part
of the release; when it is not, the release MUST say so and continue.

#### Scenario: Dirty tree refuses

- **WHEN** the working tree has uncommitted changes
- **THEN** the release refuses to start and names the changed files

#### Scenario: Publish is explicit

- **WHEN** a release runs without publish flags
- **THEN** artifacts and the image are built and validated locally, and
  nothing leaves the machine

#### Scenario: Supply-chain check

- **WHEN** `cargo audit` is installed
- **THEN** its findings are part of the release output, and
  vulnerabilities fail the release; when absent, the output says the
  check was skipped
