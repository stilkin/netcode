# Development guide

## Repo layout

```
Cargo.toml               workspace manifest (members in crates/)
crates/
  protocol/              wire header codec + golden vectors + format doc
    docs/wire-format.md  the protocol v1 byte layout
  hub/                   the `hub` binary (config, logging, shutdown)
docs/                    project documentation (you are here)
openspec/
  specs/<capability>/    current behavior — source of truth
  changes/               active change proposals + archive of applied ones
.github/workflows/ci.yml CI: fmt + clippy -D warnings + tests, Ubuntu & Windows
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

## Checks (same gates as CI)

```sh
cargo fmt --all -- --check                          # formatting
cargo clippy --workspace --all-targets -- -D warnings  # lints
cargo test --workspace                              # unit + golden + integration
```

CI runs the same three on `ubuntu-latest` and `windows-latest`; a change is
mergeable only when the matrix is green.

## Conventions

- **Pure-Rust dependencies only** (no C/C++ build deps) — this is what keeps
  the two-platform CI matrix honest. `libc` (bindings crate) is allowed as a
  dev-dependency where a test needs a raw signal.
- **Golden vectors** (`crates/protocol/tests/golden.rs`) pin the wire format.
  Never regenerate them casually; changing one is a protocol-evolution decision
  recorded in the change's design doc.
- **Unknown input is rejected at the boundary**: config keys, packet magic,
  protocol versions.
- **Commits**: small, conventional-ish prefixes (`feat`, `fix`, `chore`,
  `docs`), message says which change/tasks it implements.
- **Crate skeletons are not stubbed ahead of need** — a crate appears in the
  workspace when its change requires it.
