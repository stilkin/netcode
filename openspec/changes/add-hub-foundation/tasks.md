# Tasks

## 1. Workspace scaffolding *(enables all requirements below)*
- [x] 1.1 Create workspace `Cargo.toml` with members `crates/protocol` and `crates/hub`; `[workspace.package]` (edition 2024, version 0.1.0); `[workspace.dependencies]` for clap, serde, toml, tracing, tracing-subscriber, thiserror, anyhow, tokio.
- [x] 1.2 Add `.gitignore` (`/target`, `hub.local.toml`); create the two crate skeletons; `cargo build` and `cargo test` green.

## 2. wire-protocol
- [x] 2.1 `Header` struct + constants in `crates/protocol`; `crates/protocol/docs/wire-format.md` with the byte table and evolution rules. *(Canonical packet header)*
- [x] 2.2 Encoder/decoder with `thiserror` error set (`BadMagic`, `UnsupportedVersion`, `Truncated`, `PayloadTooLarge`); reject bad magic and newer versions before reading payload. *(Magic and version rejection)*
- [x] 2.3 Use `header_size` for the payload offset: accept headers larger than 24 bytes, ignore unknown header bytes. *(Forward-compatible header growth)*
- [x] 2.4 Channel field round-trips unvalidated; payload returned as an opaque byte slice. *(Channel and payload opacity)*
- [x] 2.5 Golden vector tests in `crates/protocol/tests/golden.rs`: committed hex fixtures — valid v1 packet; future `header_size = 32`; bad magic; unsupported version; unknown channel; truncated buffer. *(Golden vector stability)*
- [x] 2.6 Round-trip test sweeping representative values of every field, including payload correctness at non-default `header_size`. *(Canonical packet header)*

## 3. hub-runtime
- [x] 3.1 clap-derive CLI: `--config <path>` (default `hub.toml`), `--version` printing `<name> <semver>` from `CARGO_PKG_VERSION`. *(Version reporting)*
- [x] 3.2 Config structs (serde + toml, `deny_unknown_fields`), all-default when file missing, error names the offending key, exit code 1 on invalid config. *(Configuration loading)*
- [x] 3.3 `tracing-subscriber` env-filter init; config level default `info`, `RUST_LOG` overrides; startup line with version + resolved config. *(Structured logging)*
- [x] 3.4 tokio runtime + signal handling: first SIGINT/SIGTERM/Ctrl-C begins graceful shutdown (5 s cap), second signal exits immediately, clean shutdown exits 0, fatal errors exit 2. *(Graceful shutdown)*
- [x] 3.5 Integration test: boot hub with temp config, capture startup log line, deliver SIGINT, assert exit 0 (`#[cfg(unix)]`-gated). *(Graceful shutdown, Structured logging)*

## 4. build-pipeline
- [ ] 4.1 `.github/workflows/ci.yml`: matrix `ubuntu-latest`/`windows-latest`; checkout, stable toolchain, rust cache; `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. *(Cross-platform CI gate)*

## 5. Wrap-up
- [ ] 5.1 Install openspec CLI if available and run `openspec validate --changes`; fix any artifact complaints.
- [ ] 5.2 Mark change 1 implemented in README roadmap; `openspec archive add-hub-foundation` to merge deltas into `openspec/specs/`.
