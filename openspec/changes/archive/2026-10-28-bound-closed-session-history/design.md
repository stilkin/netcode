# Design

## Context

`Registry::close` marks an entry `Closed` and returns its attached clients, but never removes metadata. Closed entries currently make `get` return metadata and `attach` return `NotJoinable`; unknown IDs return `None` and `NotFound`. The registry is owned by one hub-core task, so eviction needs no locking or additional synchronization. IDs are allocated from a separate monotonic counter.

## Goals / Non-Goals

**Goals:**

- Put a deterministic hard bound on closed-session metadata.
- Preserve behavior for live and recently closed sessions.
- Keep close idempotent and ID allocation unchanged.

**Non-Goals:**

- No time-based expiry, configuration setting, persistence, or active-session cap.
- No changes to the live-session listing or public function signatures.

## Decisions

1. **Cap by count, not time.** A fixed 1,024-entry history provides a hard memory bound independent of churn and avoids clock-dependent behavior. Alternative: TTL eviction, rejected because entries can still accumulate without bound within the interval and tests become time-dependent.
2. **Use one `VecDeque<SessionId>` beside the existing map.** `close` appends only on an open-to-closed transition, then pops and removes the oldest ID while the queue exceeds the cap. This is constant time and does not require scanning the map. Alternative: timestamps plus an oldest-entry scan, rejected as more state and work for the same policy.
3. **Evict synchronously inside `Registry::close`.** The existing single-owner model already serializes closes; no sweep task is needed. Repeated close returns before touching the queue, so it cannot evict another entry.
4. **Keep the cap private.** A generous 1,024 tombstones preserves recent behavior without adding a configuration surface. Live entries are never in the queue; the separate ID counter continues to prevent reuse after eviction.
5. **Test at the actual boundary.** Unit tests close 1,025 sessions, assert the oldest is unknown and the newest remains closed/not joinable, and verify repeated close does not append history. The small constant-size loop avoids test-only capacity knobs.

## Risks / Trade-offs

- [Evicted closed sessions change from `NotJoinable` to `NotFound`] → make this the explicit spec contract; recent tombstones retain the old behavior.
- [Queue and map could disagree if close transitions are mishandled] → append only after confirming an open-to-closed transition and cover idempotence in tests.
- [1,024 entries still retain some memory] → this is the deliberate bounded-history trade-off, not unbounded growth.

## Migration Plan

No data or configuration migration is needed. Existing live sessions are unaffected. After deployment, only closed IDs beyond the retained history acquire unknown-ID behavior; IDs remain unique for the process lifetime.
