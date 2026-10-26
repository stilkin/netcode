# Backplane wire format (protocol v1)

Frames between the hub and game workers over loopback TCP. This is the
control plane: frames are small, typed messages; client-facing packets (see
`crates/protocol/docs/wire-format.md`) ride inside routing frames as opaque
bodies. All integers are **little-endian**.

## Frame layout

| Offset | Size | Field |
|-------:|-----:|-------|
| 0      | 4    | `len` — bytes that follow (opcode + body), capped at 256 KiB |
| 4      | 2    | `opcode` |
| 6      | …    | `body` |

A frame that declares `len` over the cap, an unknown opcode, or a malformed
body is a protocol violation: the receiver closes the connection.

The **first** frame on a connection MUST be `Hello`. After `Hello`, frames
MUST arrive from the registered direction set; unannounced `ToClient`/…
frames before registration are violations.

## Opcodes

| Opcode | Value | Direction | Body |
|---|---:|---|---|
| `Hello` | 1 | w→h | `token: str` |
| `Register` | 2 | w→h | `claimed_id: u32, game_type: str, display_name: str, max_players: u16, payload_schema_version: u16` |
| `Registered` | 3 | h→w | `session_id: u32` |
| `Heartbeat` | 4 | both | *(empty)* |
| `ClientAttached` | 5 | h→w | `client_id: u32` |
| `ClientDetached` | 6 | h→w | `client_id: u32` |
| `ToClient` | 7 | w→h | `packet: bytes` — client packet; recipient = its header `client_id` |
| `ToMany` | 8 | w→h | `count: u16, client_ids: u32×count, packet: bytes` |
| `Broadcast` | 9 | w→h | `packet: bytes` — to every attached client |
| `FromClient` | 10 | h→w | `packet: bytes` — a client's input packet |

`str` = `len: u16` followed by that many UTF-8 bytes.

## Registration contract

- `Register.claimed_id == 0`: the hub assigns a fresh session id and replies
  `Registered`.
- `Register.claimed_id != 0`: must name a **pending** session the hub created
  when it spawned this worker; the hub activates it and replies `Registered`.
  Anything else closes the connection.

## Liveness

Workers MUST send `Heartbeat` periodically (SDK default: every 5 s). If the
hub receives no frame from an authenticated connection for 15 s, it closes
the connection and closes that worker's sessions.
