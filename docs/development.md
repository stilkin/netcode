# Development guide

## Repo layout

```
Cargo.toml               workspace manifest (members in crates/)
crates/
  protocol/              client-packet header codec + golden vectors
    docs/wire-format.md  the protocol v1 byte layout
  backplane/             hub-to-worker frame codec
    docs/backplane-format.md  the backplane v1 byte layout
  hub/                   the `hub` binary + library (config, registry,
                         backplane service, worker spawn/watchdog,
                         client_edge: WebTransport listener,
                         lobby: HTTP API + test page ([[games]] catalog))
  hub-sdk/               the game-side contract: GameSession trait + runtime
  sample-worker/         echo arena — reference game + test fixture
  conformance/           netcode-bots: bot client, scenario runner,
                         fixtures, load profiles (scenarios/smoke.toml)
docs/                    project documentation (you are here)
openspec/
  specs/<capability>/    current behavior — source of truth
  changes/               active change proposals + archive of applied ones
```

## Toolchain

- **Rust**: stable via [rustup](https://rustup.rs) (edition 2024 ⇒ ≥ 1.85).
- **OpenSpec CLI** (for the spec workflow): `npm i -g @fission-ai/openspec`.
  If node lives in nvm, export its bin dir first:
  `export PATH="$HOME/.nvm/versions/node/*/bin:$PATH"`.

## The spec-driven workflow

Behavior changes go through [OpenSpec](https://openspec.dev); the flow is
**proposal → delta specs → design → tasks → implement → archive**:

1. Draft a change: `openspec new change "add-<capability>"` (or ask an agent to
   run the propose flow). Artifacts land in `openspec/changes/<name>/`.
2. Validate: `openspec validate --changes` (use `--strict` before archiving).
3. Implement by working through `tasks.md`, ticking checkboxes as you go.
4. Archive when done: `openspec archive <name> --yes` — deltas merge into
   `openspec/specs/`, which is the source of truth for current behavior.

Conventions enforced by `openspec/config.yaml`:

- one reviewable unit per change; state out-of-scope explicitly;
- requirements are observable behavior with ≥ 1 WHEN/THEN scenario each;
- every task traces to a requirement;
- design decisions record the alternatives considered.

## Checks

The same three gates run in the pre-commit hook and by hand:

```sh
cargo fmt --all -- --check                          # formatting
cargo clippy --workspace --all-targets -- -D warnings  # lints
cargo test --workspace                              # unit + golden + integration
```

There is no hosted CI by owner decision — the hook is the enforcement point.
Windows is not exercised automatically: if you touch platform-specific code,
build on a Windows machine before merging (`cfg(unix)` code is confined to
signal handling and gated tests, which keeps the surface small).

## Git hooks

Two version-controlled hooks run the same gates (fmt, clippy, tests):

- `.githooks/pre-commit` — before every commit.
- `.githooks/pre-push` — before every push, so a `git commit --no-verify`
  bypass cannot reach other people unnoticed.

Both activate once per clone:

```sh
git config core.hooksPath .githooks
```

Work-in-progress escape hatches remain: `git commit --no-verify` and
`git push --no-verify` — the contract is "breakage does not reach others
unnoticed", not "you cannot rush locally".

## Releasing

The release pipeline is fully local (no hosted CI, by owner decision):

```sh
cargo run -p xtask -- preflight                # what is missing on this machine?
cargo run -p xtask -- set-version 0.2.0        # bump + commit
cargo run -p xtask -- release                  # build, package, image, self-validate
cargo run -p xtask -- release --publish        # + git tag, gh release, docker push
```

Flow: preflight (clean tree, toolchain probes with install hints) →
cross-build (musl + mingw; `--skip-linux`/`--skip-windows` on partial
machines) → package (archives + SHA256SUMS under
`target/release-artifacts/<version>/`) → Docker image (`--skip-docker`
without docker) → **self-validation**: the built hub boots and the
conformance bots' smoke scenario must pass against it — a failing smoke
fails the release → optional `cargo audit` (skipped loudly when not
installed) → publish, only with `--publish`. `gh release` and ghcr.io are
artifact hosting, not CI.

On a machine without cross toolchains, the docker path still rehearses the
whole pipeline: `cargo run -p xtask -- release --skip-linux --skip-windows`
builds and validates the image (the image compiles musl inside the
container).

## Coverage

Coverage is measured with `cargo-llvm-cov`:

```sh
cargo install cargo-llvm-cov            # once; also: rustup component add llvm-tools-preview
cargo llvm-cov --workspace --summary-only   # add --html for a browsable report
```

Current state (post-change-5): ~90% line coverage overall; `crates/protocol`
is at ~97% (the header codec itself at 100% lines), `registry` ~99%,
`client_edge` ~91%, `lobby` ~92%, `conformance` ~82% (bot join-error and
timeout branches are deliberately unchased; the `netcode-bots` binary is
thin glue). The uncovered remainder is dominated
by branches deliberately not chased with contrived tests: OS-specific signal
fallbacks, the impatient second-signal exit (130), fatal-exit mapping, SDK
connection-failure error paths, and join-frame decode error arms. Re-measure
with `cargo llvm-cov --workspace --summary-only`.

## Conventions

- **Pure-Rust dependencies only** (no C/C++ build deps) — cross-platform
  builds stay trivial. One documented exception (change 3): the WebTransport
  transport tree (`wtransport` → quinn → rustls → `ring`), confined to the
  client edge; users receive prebuilt artifacts and never compile it.
  `libc` (bindings crate) is allowed as a dev-dependency where a test needs
  a raw signal.
- **Golden vectors** (`crates/protocol/tests/golden.rs`) pin the wire format.
  Never regenerate them casually; changing one is a protocol-evolution decision
  recorded in the change's design doc.
- **Unknown input is rejected at the boundary**: config keys, packet magic,
  protocol versions.
- **Commits**: small, conventional-ish prefixes (`feat`, `fix`, `chore`,
  `docs`), message says which change/tasks it implements.
- **Crate skeletons are not stubbed ahead of need** — a crate appears in the
  workspace when its change requires it.
