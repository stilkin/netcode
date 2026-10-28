# Proposal

## Why

The SDK worker runtime reads length-prefixed backplane frames directly inside a `select!` arm. Because frame reads span multiple cancel-unsafe `read_exact` calls, a tick or heartbeat can discard partially consumed bytes, desynchronize the stream, and kill an active session. This must be fixed before real LAN-party load makes the intermittent failure user-visible.

## What Changes

- Give the SDK's inbound backplane socket one dedicated reader task and dispatch frames to the existing game loop through a small bounded channel.
- Preserve frame order, input batching, heartbeats, ticks, output helpers, and clean exit on hub loss.
- Add a regression test that fragments a frame across worker ticks and verifies subsequent frames remain synchronized.
- Correct the review record and client-edge comment that incorrectly called `wtransport::RecvStream::read` cancel-unsafe; the conformance bot needs no code change.

Out of scope: conformance bot refactoring, hub-side changes, wire or configuration changes, public API changes, and unrelated registry or listener work.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `hub-sdk`: the game session runtime must preserve fragmented backplane frames and their ordering while ticks and heartbeats run.

## Impact

- `crates/hub-sdk/src/lib.rs`: inbound reader ownership and main-loop dispatch.
- `crates/hub-sdk/tests/worker_runtime.rs`: fragmented-frame regression coverage.
- `crates/hub/src/client_edge/mod.rs`: documentation-only correction.
- `openspec/specs/hub-sdk/spec.md`: one new runtime scenario.
- No dependencies, wire formats, configuration keys, or public signatures change.
