# Proposal

## Why

The session registry currently retains every closed session's metadata for the lifetime of the hub. Long-lived, high-churn deployments therefore grow memory without bound. We want a deterministic hard bound while preserving the useful behavior of recent closed-session tombstones.

## What Changes

- Retain only the 1,024 most recently closed session entries, evicting the oldest closed entry when a new close exceeds the cap.
- Track close order inside the existing registry with one `VecDeque`; eviction happens synchronously during `close`, with no timer or background task.
- Preserve recent closed-session metadata and `NotJoinable` behavior, idempotent close, live-session behavior, and never-reused IDs. Evicted closed IDs behave like unknown IDs.
- Add boundary and repeated-close regression coverage and remove B8 from the local findings document.

Out of scope: time-based expiry, configurable history size, persistence, active-session limits, and changes to the lobby's live-session listing.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `session-registry`: closed-session metadata retention is bounded to a deterministic oldest-first history.

## Impact

- `crates/hub/src/registry.rs`: one close-order queue, constant-time eviction, and unit tests.
- `openspec/specs/session-registry/spec.md`: closed-history retention and eviction behavior.
- No dependencies, configuration keys, wire formats, or public function signatures change.
