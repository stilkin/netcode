# Design

## Context

The client-edge accept loop currently awaits each peer's WebTransport negotiation inline, while established connections already live in a shutdown-aware `JoinSet`. Its relay reader accumulates incomplete packets without a cap. The lobby spawns one task per TCP request with no deadline; its accept-loop drain is separately bounded at three seconds.

Wtransport 0.7.2 confirms that `incoming.await` includes QUIC establishment plus WebTransport settings/session negotiation. Existing tests supply joined clients, scripted workers, and lobby helpers. Quinn is already locked transitively, and wtransport exposes its hash-pinning TLS verifier for a raw-QUIC test fixture.

## Goals / Non-Goals

**Goals:**

- Isolate each peer's negotiation and shutdown from every other peer.
- Bound incomplete stream data and total HTTP request duration.
- Reuse existing lifecycle and test patterns with no new production surface.

**Non-Goals:**

- No connection/rate limits, new configuration, or public API changes.
- No lobby task-supervisor rewrite or unrelated listener refactoring.
- No production dependency additions.

## Decisions

1. **Extend the existing client-edge `JoinSet` to negotiation.** Spawn the negotiate-then-relay lifecycle immediately after `server.accept()` returns. Both `incoming.await` and `request.accept()` select against shutdown; established connections continue through `handle_connection` unchanged. Alternative: detached `tokio::spawn`, rejected because it bypasses reaping and orderly drain.
2. **Use a 256 KiB incomplete-packet cap.** The reader drains complete packets first, then signals the existing stream error and returns if the remainder exceeds the cap. The largest valid packet is about 128 KiB, so valid fragmentation has ample headroom. Alternative: parse-time error on bad magic, rejected because it would change behavior for invalid but small streams beyond this finding.
3. **Apply one total 30-second lobby deadline.** Wrap the existing `serve_one` future at its spawn site; expiry drops the request stream and logs a warning. A total deadline cannot be reset by trickled bytes. Alternative: per-read timeouts, rejected because a peer could keep a request alive indefinitely by sending within each interval.
4. **Keep the existing lobby drain.** The request deadline bounds per-request work while running; the established three-second accept-loop drain remains unchanged. Adding a second `JoinSet` solely for symmetry would enlarge this fix without addressing the reported unbounded wait.
5. **Prove B3 with raw QUIC, not a custom verifier.** A dev-only quinn client establishes QUIC with wtransport's ALPN and `ServerHashVerification`, then sends no WebTransport settings. This deterministically stalls the server's negotiation while preserving certificate pinning. The normal wtransport client cannot express this state.
6. **Test B5 with paused Tokio time.** Dev-only `test-util` permits advancing past 30 seconds without real sleeps or production timeout parameters. B3 and B4 use real transports and bounded wall-clock timeouts.

## Risks / Trade-offs

- [Negotiation tasks now need shutdown handling before a `Connection` exists] → select both negotiation stages against the same watch channel used by established connections.
- [A raw-QUIC fixture could accidentally complete WebTransport negotiation] → hold only quinn endpoint/connection objects and create no WebTransport driver or streams.
- [The stream cap could reject a valid maximal packet] → cap at twice the approximate maximum packet size and drain complete packets before checking.
- [Paused time could affect other rig services] → use it only in the lobby timeout test and keep real-time transport tests separate.

## Migration Plan

No migration is needed. Behavior, wire formats, and configuration remain compatible; only stalled or over-limit peers gain bounded handling. The change can be reverted as one commit.
