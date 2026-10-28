# Design

## Context

`hub-sdk::run` currently selects among `conn::read_frame`, tick, and heartbeat futures. `read_frame` performs separate length and body reads with Tokio's cancel-unsafe `read_exact`. The hub-side pump and conformance test rig already demonstrate the dedicated-reader pattern; no shared helper exists because their error handling differs.

The proposed conformance finding was checked against the locked transport stack: wtransport 0.7.2 delegates `RecvStream::read` to quinn 0.11.12, whose `read` is documented cancel-safe. Only `read_exact` is unsafe. The bot therefore remains unchanged.

## Goals / Non-Goals

**Goals:**

- Preserve inbound frame bytes and ordering regardless of tick/heartbeat scheduling.
- Keep backpressure and clean hub-loss behavior.
- Make the smallest change behind the existing SDK API.

**Non-Goals:**

- No generic reader abstraction, new dependency, or wire/API change.
- No conformance, hub listener, or unrelated runtime refactoring.

## Decisions

1. **One dedicated socket reader.** Move `rd` into a task that loops over `read_frame` and sends decoded frames to the game loop. The loop selects on `mpsc::Receiver::recv`, which is cancel-safe. Alternative: retain direct reads or introduce a codec framework; the first preserves the bug, the second adds machinery without addressing a broader need.
2. **Bounded channel of capacity one.** This permits minimal read-ahead without unbounded buffering while game code runs; excess frames remain behind TCP backpressure. Alternative: copy the hub's unbounded channel, rejected because a slow game callback could then drain TCP into unlimited memory.
3. **Channel closure means hub loss.** The reader drops its sender on any read/codec error; buffered frames are handled before `None` ends the game loop, matching the existing clean-stop semantics. Errors need no new channel payload because the current loop discards them.
4. **Explicit task cleanup.** Retain and abort the reader handle on runtime exit beside the existing writer-pump abort. This prevents a detached reader if a future exit path is added.
5. **Regression before implementation.** Fragment a real encoded `FromClient` frame on the stub hub, observe a tick before completing it, then require two inputs to arrive intact and ordered. Existing runtime and end-to-end tests guard unchanged behavior.

## Risks / Trade-offs

- [A timing-sensitive regression test could pass by accident on the old code] → synchronize fragmentation with an observed game tick and bound every wait.
- [Capacity one adds scheduler handoffs under heavy traffic] → acceptable for LAN workloads; it preserves backpressure and avoids a new tuning surface.
- [A detached task could outlive the runtime] → abort its handle during the existing cleanup path.

## Migration Plan

No deployment migration or rollback steps are required. The change is wire-compatible and can be reverted as one commit without configuration changes.
