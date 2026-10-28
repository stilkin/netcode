# Spec Delta

## Purpose

The client edge becomes WebTransport-only: the two-route bootstrap stopgap
is absorbed by the lobby API (change 4). Transport behavior is unchanged.

## REMOVED Requirements

### Requirement: Cert hash bootstrap endpoint

**Reason:** absorbed by `lobby-api` (cert hash and test page hosting). The
`client_edge.bootstrap_bind` configuration key is retired in favor of
`[lobby] bind`; serving the hash and the test page is now a lobby-service
concern, reading the live certificate hash from the client edge.
