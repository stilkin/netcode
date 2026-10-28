# Tasks

Ordered so the workspace compiles and tests stay green after every task.
Each section names the requirements it implements.

## 1. Foundations *(enables the requirements below)*
- [x] 1.1 Add `wtransport` (with client feature for tests) and `rcgen` to `hub`'s dependencies; verify the workspace still builds on the current stable toolchain and the pure-Rust dependency policy holds (no new system deps).
- [x] 1.2 `[client_edge]` config section in `hub::config`: `bind` (default `0.0.0.0:4433`), `bootstrap_bind` (default `0.0.0.0:8080`), `token` (default empty), nested `deny_unknown_fields`, defaults without the section. *(hub-runtime: Client edge startup — config keys, unknown-key rejection)*
- [x] 1.3 Unit tests: defaults, valid section, unknown nested key rejected naming the key.

## 2. `protocol`: join frames *(wire-protocol: Join frame codecs; client-protocol: Join frames)*
- [x] 2.1 `Connect`/`Accept`/`Reject` codecs (little-endian, version byte = 1, reject other versions, truncated input is an error, std-only).
- [x] 2.2 Golden vectors for all three frames in `crates/protocol/tests/golden.rs`; byte tables and rules in `crates/protocol/docs/wire-format.md`.
- [x] 2.3 Unit tests: round-trips, unknown version, truncation, empty token/name edge cases.

## 3. `hub::client_edge`: endpoint + certificate *(client-edge: WebTransport endpoint)*
- [x] 3.1 Certificate generation at boot (rcgen, ECDSA P-256), SHA-256(DER) computation, log the listen address and hash on startup; bind failure is a fatal error.
- [x] 3.2 `Service::bind`/`start`/`shutdown`/`drain` mirroring `backplane_service`'s shape; accept loop spawning one task per connection.
- [x] 3.3 Test helper: hash-pinned wtransport client verifier (the browser trust path), reused by all edge tests.

## 4. `hub::client_edge`: handshake and join *(client-edge: Connect handshake, Token gate, Session join rules)*
- [x] 4.1 On session establishment: require one bidi stream, read `Connect`, validate token (empty-config = open), attach via `ServiceHandle::attach`, reply `Accept`; on any failure reply `Reject` (reason string) and close; non-`Connect` or malformed first frame ⇒ close, no state change.
- [x] 4.2 After `Accept`, the stream is the events channel (no reopen).
- [x] 4.3 Integration tests against a real backplane service: accept+join; wrong token; unknown session; full session; garbage first frame; player counts only move on accept.

## 5. `hub::client_edge`: relay *(client-edge: State relay over datagrams, Event relay over the control stream)*
- [x] 5.1 Inbound pump: WT datagrams and control-stream bytes → incrementally framed by header length → `feed_packet` (identity validation stays in the backplane service); invalid header on the stream ⇒ close connection.
- [x] 5.2 Outbound pump per client: drain `ClientHandle::recv`, route by channel byte — state → non-blocking datagram send (drop + debug log on refusal; oversized ⇒ drop + warn), events → awaited stream write (QUIC flow control).
- [x] 5.3 Stall guard: pending outbound backlog watermark (1024 packets) ⇒ close the connection.
- [x] 5.4 Integration tests: input round trip via datagrams; two events in order on the stream; oversized unicast dropped for its client only; slow reader stalls only itself; datagram sends never block other clients.

## 6. `hub::client_edge`: lifecycle *(client-edge: Connection lifecycle; hub-runtime: Client edge lifecycle — shutdown)*
- [x] 6.1 Connection close/break ⇒ drop the `ClientHandle` (existing detach semantics: worker notified, count decremented). *(client-edge: Connection lifecycle)*
- [x] 6.2 Wire `client_edge::Service` into `shutdown_loop`: bind at boot, log lines, stop accepting + close client sessions within `GRACE_PERIOD` on signal; SIGINT e2e (`cli.rs`) still exits 0. *(hub-runtime: Client edge startup, Client edge shutdown)*
- [x] 6.3 Test: abrupt disconnect detaches (worker observes, count returns); shutdown with attached clients exits cleanly.

## 7. Bootstrap endpoint *(client-edge: Cert hash bootstrap endpoint)*
- [x] 7.1 Minimal HTTP/1.1 TCP responder on `bootstrap_bind`: `GET /cert-hash` → `{"sha256": "<hex>"}` with `Access-Control-Allow-Origin: *`; `GET /` → embedded test page (connect by hash, join a session, watch broadcasts, send inputs); else 404. Handshake of the future lobby API, not a framework.
- [x] 7.2 Tests: hash matches the logged/generated certificate; unknown path 404; test page served.

## 8. Flagship end-to-end *(client-edge + client-protocol: full loop)*
- [x] 8.1 `crates/sample-worker/tests/client_edge_end_to_end.rs` *(moved from `crates/hub/tests/`: `CARGO_BIN_EXE_sample-worker` is only set for a package's own integration tests)*: in-process hub (backplane + client edge) + spawned `sample-worker` + hash-pinned wtransport client: connect → join → greeting event on stream → ordered datagram inputs echoed → streamed event input echoed → broadcast ticks observed → disconnect ⇒ detach, count decrements.
- [x] 8.1a *Bonus fix surfaced by 5.4's volume test:* the backplane link pump had non-cancel-safe `conn::read_frame` inside a `select!` arm — an outbound write winning mid-body desynchronized the worker TCP stream. Reader task + channel now; the edge relay got the same treatment for its stream reads.
- [x] 8.2 Manual verification pass: run the hub, open the test page in a desktop browser, play echo arena.

## 9. Documentation
- [x] 9.1 `README.md`: status note, technologies row (wtransport/rcgen), `[client_edge]` config keys, run instructions incl. the test page; `docs/architecture.md`: move client edge from planned to implemented; `docs/roadmap.md`: mark change 3 implemented.
- [x] 9.2 `docs/development.md`: repo layout gains `client_edge` module note; coverage expectations re-measured.

## 10. Wrap-up
- [x] 10.1 `openspec validate add-webtransport-client-edge --strict` green; full workspace gate (fmt, clippy `-D warnings`, tests) green on a clean build.
- [ ] 10.2 Archive the change (`openspec archive add-webtransport-client-edge --yes`), mark roadmap status, commit.
