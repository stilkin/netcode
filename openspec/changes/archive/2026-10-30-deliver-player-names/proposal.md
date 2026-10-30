# Proposal

## Why

Clients already supply a player name when joining, and the SDK exposes a
`player_name` argument to `GameSession::on_attach`, but the hub discards the
name and the SDK always passes `""`. Complete this existing path so game
workers receive the display name their callback contract promises.

## What Changes

- **BREAKING (pre-1.0 backplane protocol):** add `player_name` to the
  hub-to-worker `ClientAttached` frame after `client_id`, using the existing
  length-prefixed UTF-8 string encoding.
- Forward the attaching client's name unchanged through the backplane and
  pass it to `GameSession::on_attach`; empty and non-ASCII names retain their
  existing join semantics.
- Remove the registry's unused `player_name` parameter. The registry tracks
  membership, not a player-name roster.
- Update the wire-format documentation and codec, service, SDK, and
  end-to-end tests.
- Out of scope: client join-frame changes, storing names in session
  metadata, directory/roster APIs, authentication changes, payload handling,
  and protocol-version negotiation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `backplane`: client-attach notifications carry the joining player's name
  in addition to the client id.
- `hub-sdk`: `GameSession::on_attach` receives the name from the attachment
  notification instead of a placeholder empty string.

## Impact

Affected code includes `crates/backplane`, the hub's backplane service and
registry, `crates/hub-sdk`, scripted workers, and integration/conformance
tests. `crates/backplane/docs/backplane-format.md` changes with the frame
layout. Hubs and workers built from different sides of this pre-1.0 frame
change must be updated together; no new dependency is introduced.
