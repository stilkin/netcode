# Design

## Context

See `proposal.md` — Why. The client edge already passes `Connect.name` into
the backplane's attach command. The core currently uses that value only as
an unused registry argument, while `ClientAttached` carries just an id and
the SDK substitutes an empty name. Backplane frames already provide ordered
worker delivery and a bounded, length-prefixed UTF-8 string codec.

## Goals / Non-Goals

**Goals:**

- Deliver attachment identity and display name as one ordered notification.
- Reuse existing framing, validation, and worker-callback paths.
- Keep registry membership independent of display-name storage.

**Non-Goals:**

- Add a player directory or mutable profile state.
- Change client-facing packets or the client join-frame layout.
- Introduce protocol negotiation or a compatibility adapter.

## Decisions

### Extend `ClientAttached` rather than add another notification

Encode `client_id: u32` followed by `player_name: str` in the existing
opcode-5 frame, where `str` retains its u16 byte length plus UTF-8 encoding.
This keeps identity and name atomic and preserves the existing attach order.
A separate name frame or callback would add ordering and partial-delivery
cases without providing needed functionality.

### Forward the name without storing it in the registry

The backplane core already owns the attach command's `String`. After a
successful registry attach, move that string into `ClientAttached`. Remove
the registry's unused name parameter rather than adding redundant roster
state. Rejected attaches produce no notification, as before.

Alternative: store names in `SessionEntry`. Rejected because no directory or
post-attach lookup requirement exists, and it would add memory and lifecycle
invariants unrelated to worker notification.

### Reuse codec failures and the existing SDK callback

`BodyWriter::write_str` and `BodyReader::read_str` handle length limits,
truncation, and UTF-8 errors. No new limit or error type is needed. The SDK
keeps its public `GameSession::on_attach` signature and replaces only the
placeholder `""` with the decoded name.

### Treat this as a coordinated pre-1.0 update

The Rust `Frame::ClientAttached` shape changes, and new workers require the
name field on the wire. Update the bundled hub, SDK, sample worker, and
scripted workers together. Do not add version negotiation solely for this
pre-release frame addition.

## Risks / Trade-offs

- [A new worker connected to an old hub rejects the short attach body] →
  Document the frame change and validate truncation; coordinate upgrades.
- [A malformed or non-UTF-8 name could affect the worker link] → Reuse the
  existing codec's `MalformedBody` and `InvalidUtf8` connection errors.
- [Names could be mistaken for authenticated identities] → Keep them as
  unverified display metadata; no authorization or directory semantics are
  added.

## Migration Plan

Update hub and worker artifacts together, then deploy normally. Rolling back
requires rolling both sides back to the same prior artifact set. Client
packets and saved session state are unaffected; there is no persistent data
migration.
