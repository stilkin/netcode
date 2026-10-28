# Design

## Context

The lobby has one request reader/router and returns structured JSON errors. All registry mutations already pass through one backplane core task. Spawned sessions reserve a pending ID before process creation, and failed spawns close that reservation. The registry also retains closed tombstones, so raw map length is not a live-session count. The built-in page and documented clients already send JSON content types.

## Goals / Non-Goals

**Goals:**

- Block browser-simple session creation and bound worker-process creation.
- Make recognized headers and body framing deterministic.
- Enforce capacity atomically before ID allocation or spawning, for both creation modes.
- Preserve the small dependency-free HTTP server and open-LAN token defaults.

**Non-Goals:**

- No HTTP framework, CORS/preflight support, reverse-proxy claim, or general HTTP rewrite.
- No credential-handling, client-rate, container, or other security findings in this change.

## Decisions

1. **Use a three-way internal request-read outcome.** Valid requests route normally, invalid framing returns the existing JSON `400`, and incomplete input retains the current close behavior. Alternative: nested `Result<Option<...>>` or an error DTO inside `Request`, rejected as less direct.
2. **Parse only recognized headers.** Split once on `:`, use ASCII case-insensitive names, and trim values. Reject every duplicate `Content-Length`, malformed lengths, and every `Transfer-Encoding`. Alternative: a general header map/framework, rejected as unnecessary surface and a new dependency.
3. **Validate only the session-creation media type.** Accept `application/json` case-insensitively with optional parameters; missing/other types return `415` before JSON decoding or spawning. Authentication remains before routing. Keep `OPTIONS` unsupported so cross-origin JSON preflight fails. Alternative: removing CORS response headers alone, rejected because simple cross-site requests are still sent.
4. **Put `max_sessions` in `BackplaneConfig` and the registry.** The backplane core owns both creation modes. Default 64, zero denies creation, and only pending/active entries count. Alternative: `[lobby]` plus a separate precheck, rejected because it races concurrent POSTs and leaves self-registration with ambiguous semantics.
5. **Return `Option<SessionId>` from registry/core creation.** Capacity is the sole creation failure, so a new error hierarchy is unnecessary. Check before ID allocation. Add one explicit `SpawnError::SessionLimitReached` for the existing spawn result and map it to lobby `503`; actual spawn I/O remains `500`.
6. **Reuse normal lifecycle cleanup.** Closing active/pending sessions and failed spawn cleanup immediately free capacity. Pending claims activate without another admission check because they do not increase live count. Self-registration with claimed ID zero uses the checked creation path.
7. **Keep token defaults unchanged.** Document strong `lobby.token` use for untrusted/production networks and commented deployment guidance; do not ship a known hardcoded secret or break trusted local defaults.

## Risks / Trade-offs

- [A fixed default could surprise very large existing deployments] → 64 is generous for LAN use and explicitly configurable; closed entries do not count.
- [Rejecting all transfer codings may affect unusual clients] → the server never supported them, so explicit `400` replaces ambiguous behavior; normal JSON clients are unchanged.
- [More API statuses require documentation drift] → update OpenAPI and exercise `415`/`503` plus existing route-class guards.
- [Capacity spread across registry, spawn, and HTTP could diverge] → registry owns admission, spawn has one typed rejection, and HTTP only maps that result.

## Migration Plan

Normal clients need no change if they already send `Content-Type: application/json`, as the built-in page and examples do. Operators relying on more than 64 simultaneous sessions must set `max_sessions` explicitly. Unsupported media types and ambiguous framing now receive deterministic errors instead of being processed or silently misparsed.
