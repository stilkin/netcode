# Proposal

## Why

An accepted client currently has no inbound packet-rate bound. Datagrams and stream packets can flow through unbounded channels into worker input buffers faster than the worker drains them, allowing one client to consume excessive hub and worker memory. The outbound stall watermark does not cover this direction.

## What Changes

- Add a per-connection token budget allowing a 512-packet burst and refilling at 4,096 packets/sec, shared by datagrams and complete control-stream packets.
- Forward under-budget packets immediately; disconnect and detach only the offending client when no token is available.
- Consolidate the cancel-safe control-stream read into the existing relay so one budget covers both ingress paths and the stream reader's unbounded channel disappears.
- Preserve the 256 KiB incomplete-stream cap, handshake behavior, outbound traffic, payload blindness, and existing detach semantics.
- Add deterministic budget boundary tests plus real WebTransport flood, mixed-transport, fragmentation, and healthy-client isolation regressions.

Out of scope: byte/bandwidth limits, global/per-session rates, configuration knobs, throttling delays, payload inspection, new dependencies or metrics systems, and unrelated security/simplification findings.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `client-edge`: accepted connections gain a shared, non-throttling inbound packet budget with per-client flood isolation.

## Impact

- `crates/hub/src/client_edge/mod.rs`: one synchronous budget and one relay-owned stream reader.
- `crates/hub/tests/client_edge.rs`: flood, shared-accounting, fragmentation, and isolation coverage.
- `docs/architecture.md` and the local review work document.
- No dependencies, configuration keys, public APIs, wire formats, or worker SDK changes.
