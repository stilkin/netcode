# Tasks

## 1. Backplane wire and hub delivery

- [x] 1.1 Extend `ClientAttached` with `player_name`, update `backplane-format.md`, and verify exact layout, empty/non-ASCII round trips, and truncated-name rejection with `cargo test -p backplane` (backplane: Client attach and detach notification)
- [x] 1.2 Forward the attach command's name through the hub, remove the registry's unused parameter, update hub frame matchers, and verify named attach/detach delivery—including a client-edge non-ASCII name—with `cargo test -p hub` (backplane: Client attach and detach notification)

## 2. SDK delivery

- [x] 2.1 Pass decoded attachment names to `GameSession::on_attach`, record names in the SDK runtime regression, update scripted-worker matchers, and verify the callback contract with `cargo test -p hub-sdk -p conformance` (hub-sdk: Game session runtime)

## 3. Verification

- [x] 3.1 Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `openspec validate deliver-player-names --strict`; all must pass (backplane: Client attach and detach notification; hub-sdk: Game session runtime)

## Workflow follow-up

- Archive the change and validate all merged specs.
- Remove P2 from the untracked findings document after verification.
