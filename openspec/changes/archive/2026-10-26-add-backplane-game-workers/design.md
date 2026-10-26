# Design

## Context

The hub is a lib-less binary shell with a proven wire header (protocol v1,
golden-vector pinned) but no networking. Prior decisions bind this design:
backplane transport is **plain TCP on 127.0.0.1** (identical code path on
Windows and Linux); sessions are **one process per session**; sessions **end
when their worker ends** (no host migration); a future direct-connect mode
must remain possible without breaking the client-protocol header. Payloads
are opaque to the hub everywhere.

## Goals / Non-Goals

**Goals:**

- A worker can be written against one trait (`hub-sdk`) with zero transport code.
- The hub can host sessions from spawned workers and from self-registering
  external workers, side by side.
- Full in-process test of the loop: register → attach → route → broadcast →
  detach → close-on-kill, with a real child process.

**Non-Goals:**

- Browser clients / WebTransport (next change) — this change's "clients" are
  attached through the same seam the browser edge will use.
- HTTP/admin API, worker auto-restart, direct-connect mode, non-loopback
  binds, persistence. Each is recorded as out of scope in the roadmap.

## Decisions

### Framing: length-prefixed frames, not the client header
Backplane frames are `[u32 len][u16 opcode][body]`, little-endian, `len`
inclusive of the opcode, capped at 256 KiB. Client-facing packets (24-byte
header + payload) ride as opaque *bodies* of `ToClient`/`ToMany`/`Broadcast`/
`FromClient` frames — the hub parses only their header for routing. Rationale:
control-plane needs (opcode, claimed ids, id lists) have no home in the client
header, and overloading `flags` would poison the forward-compatibility story.

### Opcode registry (v1)

| Dir | Opcode | Body |
|---|---|---|
| w→h | `Hello` | token bytes |
| w→h | `Register` | claimed_id u32, game_type str, display_name str, max_players u16, payload_schema_version u16 |
| h→w | `Registered` | session_id u32 |
| both | `Heartbeat` | empty |
| h→w | `ClientAttached` / `ClientDetached` | client_id u32 |
| w→h | `ToClient` / `Broadcast` | client packet bytes (header + payload) |
| w→h | `ToMany` | count u16, client ids, client packet bytes |
| h→w | `FromClient` | client packet bytes |

Strings are u16-length-prefixed UTF-8. Unknown opcode ⇒ close the connection
(control plane is trusted localhost code; strict beats silent).

### Body codecs: hand-rolled, like the protocol crate
Consistent with the wire-philosophy decision in `add-hub-foundation`: explicit
layouts, documented in `crates/backplane/docs/backplane-format.md`, no serde
on the wire. serde stays config-plane (see below).

### Two registration modes, one frame
`Register.claimed_id == 0` ⇒ hub assigns (self-registration, for externally
supervised workers). `claimed_id != 0` ⇒ must name a **pending** session the
hub pre-created when it spawned the worker; the pending entry times out if the
worker never claims it. One code path, two deployment styles.

### Watchdog = supervise, don't resurrect
The hub tracks spawned children; process exit ⇒ session closed, clients
detached. **No auto-restart**: a crashed worker lost its state, and restarting
into an empty session is a per-game product decision — consistent with the
recorded decision that sessions end when their worker ends.

### `hub` becomes lib + bin
`src/lib.rs` exposes config, registry, backplane service, and spawn APIs;
`main.rs` shrinks to arg parsing + `hub::run()`. This is what lets the
end-to-end test drive the hub in-process, and what the browser edge and admin
API will call next. Exit-code semantics are unchanged (existing specs hold).

### Client seam: registry-owned outbound queues
`registry.attach(session, name)` returns a `ClientHandle { id, rx }` backed by
a per-client mpsc queue. The e2e test drains `rx` like a client; change 3's
WebTransport edge will drain it into datagrams instead. One seam, tested now,
reused then.

### State ownership: one hub core task, no locks
All registry + worker-connection state lives in a single tokio task addressed
by mpsc; client queues are the only shared state. Avoids lock discipline bugs
for the price of a few channel sends, and keeps fan-out ordering trivial.

### Spawned workers get config via JSON argv
`worker --backplane 127.0.0.1:PORT --token T --session-config '{...}'`.
Config-plane serialization (serde_json) is fine; the *hot path* stays
hand-rolled. `SessionConfig { tick_hz: u32 }` in v1.

### Constants over knobs (v1)
heartbeat 5 s, liveness timeout 15 s, registration timeout 10 s, frame cap
256 KiB. Defaults, not config — fewer ways to misconfigure a LAN box; promote
to config when a real need appears.

## Risks / Trade-offs

- **Backplane format drift** between `crates/backplane` and its doc — mitigated
  by round-trip tests per opcode, mirroring the golden-vector discipline.
- **Windows process semantics** differ (no POSIX signals) — the watchdog uses
  child-exit polling only, no signals, so behavior matches on both platforms.
- **Single hub core task** serializes control-plane work — fine at the target
  scale (dozens of sessions; packet bytes never pass through it twice: fan-out
  queues receive the same buffer).
- **`sample-worker` as e2e host** is unusual placement — driven by cargo's
  `CARGO_BIN_EXE_*` locality (a test can only reference its own package's
  bins); documented so nobody "fixes" it into hub/tests.
