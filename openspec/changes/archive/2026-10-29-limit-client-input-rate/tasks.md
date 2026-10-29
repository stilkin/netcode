# Tasks

## 1. Deterministic packet budget

- [x] 1.1 Add explicit-time tests for the 512-packet burst, continuous 4,096/sec refill, and idle-refill cap, then implement the small synchronous `InputBudget` and run `cargo test -p hub --lib client_edge` (client-edge: Inbound packet budget)

## 2. Relay enforcement and isolation

- [x] 2.1 Add real WebTransport regressions for fragmented stream delivery across competing relay traffic, datagram flood isolation, and combined datagram/stream budget exhaustion; confirm flood tests fail on the unlimited relay (client-edge: Inbound packet budget, Event relay over the control stream)
- [x] 2.2 Consolidate the cancel-safe stream read into the relay, enforce the shared budget before `feed_packet`, preserve the 256 KiB incomplete-stream cap and detach path, and run `cargo test -p hub --test client_edge` plus the conformance load tests (client-edge: Inbound packet budget)

## 3. Documentation and integration

- [x] 3.1 Update the architecture's client-edge flow-control description and remove S5 plus the stale post-S5 note from the local findings document without committing it
- [x] 3.2 Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `openspec validate limit-client-input-rate --strict`; all must pass

## Workflow follow-up

- Archive the change and verify all capability specs.
- Commit and push the implementation and OpenSpec files; keep the review work document and `.pi/` untracked.
