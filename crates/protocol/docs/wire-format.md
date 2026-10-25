# Netcode hub wire format (protocol v1)

One packet header is shared by every link in the system — browser clients to
the hub, hub to game workers — so any implementation (Rust, TypeScript, Unity,
…) can interoperate by writing these bytes alone. Payloads are opaque to the
hub; they are a contract between a game worker and its clients, versioned by
`payload_schema_version`.

All integers are **little-endian**.

## Packet layout

A packet is a header followed by exactly `payload_len` payload bytes:

| Offset | Size | Field                   | Notes |
|-------:|-----:|-------------------------|-------|
| 0      | 4    | `magic`                 | ASCII `NHUB` (`0x4E 0x48 0x55 0x42`). Mismatch ⇒ drop. |
| 4      | 2    | `version`               | `1`. Greater than this build supports ⇒ reject before reading further. |
| 6      | 2    | `header_size`           | Bytes in the whole header, **including** the fixed 24. Receivers locate the payload here. |
| 8      | 1    | `channel`               | Routing-class hint. Opaque to the hub; defined per link (e.g. `0` = state, `1` = events). |
| 9      | 1    | `flags`                 | Opaque per-link flag bits. |
| 10     | 4    | `session_id`            | Session the packet belongs to. |
| 14     | 4    | `client_id`             | Client within the session. |
| 18     | 2    | `payload_schema_version`| Payload contract version between game and clients. |
| 20     | 4    | `payload_len`           | Payload size in bytes. |
| 24     | …    | *(unknown in v1)*       | Bytes 24..`header_size` belong to future versions; v1 receivers MUST ignore them. |
| `header_size` | `payload_len` | payload    | Opaque bytes. |

Reference size: a v1 header is exactly **24 bytes**.

## Rules

1. **Validate cheap, reject early.** Magic and version are checked before any
   other field; rejected packets never touch receiver state.
2. **Payload offset is `header_size`, not 24.** A larger `header_size` is a
   legitimate packet from the future: parse the 24 known bytes, skip the rest.
3. **`channel`, `flags`, and the payload are opaque** to the hub. The hub routes;
   it never interprets.
4. **Trailing bytes after the payload are ignored**, which lets links pad or
   batch without changing this format.
5. **Sanity bound**: payloads larger than 64 KiB are rejected by the reference
   codec; links that need more must make that an explicit, versioned decision.

## Evolution policy

- Adding a header field: increase `header_size`, put the new field after byte
  24 (or into reserved/unknown space), bump `version` only if semantics change
  incompatibly. Old receivers keep working — this is the designed path.
- The golden vectors in `crates/protocol/tests/golden.rs` pin this format.
  Changing one requires a deliberate protocol-evolution decision; it must never
  be a refactoring side effect.
