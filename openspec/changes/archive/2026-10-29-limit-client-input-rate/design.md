# Design

## Context

Each accepted client has one relay. Datagrams currently feed the backplane directly from that relay; a separate stream reader parses complete packets into an unbounded channel consumed by the relay. wtransport/quinn `RecvStream::read` is cancel-safe. Relay exit already drops `ClientHandle`, detaches the client, notifies the worker, and closes the transport. Existing tests provide real WebTransport clients and isolation patterns; current profiles send 50 and 333 packets/sec per client.

## Goals / Non-Goals

**Goals:**

- Stop one client from submitting an unbounded packet flood.
- Preserve normal burstiness and avoid any throttling delay.
- Cover both transports before the first unbounded queue with one policy.
- Keep the relay small and deterministic.

**Non-Goals:**

- No byte limits, global fairness, configuration knobs, metrics, dependencies, or payload inspection.
- No changes to handshake, outbound backpressure, wire protocols, or worker APIs.

## Decisions

1. **Use a synchronous token budget with 4,096/sec refill and 512-token capacity.** This is about 12× the heaviest current per-client profile while bounding initial acceptance. Alternative: fixed one-second windows, rejected because boundary double-bursts and timing behavior are less predictable.
2. **Pass `Instant` into `try_take`.** Production reads monotonic time per packet; tests supply explicit times for exact burst/refill boundaries. No timer task, sleep, allocation, or async wait is involved. Floating-point tokens remain capped at 512, avoiding long-lived precision accumulation.
3. **Make the relay the sole stream reader.** Move the existing buffer/chunk loop into a `select!` branch and delete the separate task/channel. This gives datagram and stream branches one budget without locks and removes the first unbounded queue. Alternative: `Arc<Mutex<InputBudget>>`, rejected as unnecessary shared state and contention for two short ingress paths.
4. **Charge complete packets before `feed_packet`.** Datagrams cost one token on receipt; stream packets cost one after `parse_packet` succeeds, never per read fragment. Incomplete data remains governed by the existing 256 KiB cap. Invalid datagrams consume budget before backplane validation. Handshake and outbound packets are outside the budget.
5. **Disconnect rather than throttle.** Allowed packets call `feed_packet` immediately. A denied packet exits the relay and reuses existing detach/close behavior, with one warning. Other clients and workers are untouched.
6. **Keep limits private.** Existing profiles provide substantial headroom; no operational need for configuration has been demonstrated.
7. **Test arithmetic separately from networking.** Exact counts use explicit instants. Real transport tests assert close/detach/isolation without assuming an exact cutoff because legitimate refill occurs during the test. A fragmented-stream test forces other relay traffic to protect cancel-safe restructuring.

## Risks / Trade-offs

- [A future game may legitimately exceed 4,096 packets/sec] → the limit is deliberately private but easy to revisit from measured requirements; current profiles have large headroom.
- [Moving the read branch changes scheduling] → `read` is cancel-safe, existing relay traffic tests remain required, and fragmentation coverage forces competing branches.
- [Packet rate does not bound bandwidth at the 64 KiB payload cap] → explicitly out of scope; this change addresses queue/packet flooding without payload-byte accounting.
- [Accepted packets before a flood closes still reach the worker] → tests drain until the expected detach instead of assuming detach is the first frame.

## Migration Plan

No operator or client migration is needed. Normal existing profiles remain below the budget. Clients that previously flooded without bound are now disconnected after their available burst/refill is exhausted.
