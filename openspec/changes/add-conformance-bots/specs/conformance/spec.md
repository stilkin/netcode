# Spec Delta

## Purpose

The conformance tooling: protocol-verified headless clients (bots), the
scenario runner that drives them, the deterministic fixtures that pin
session-level behavior, the load smoke profiles that answer operational
questions, and the report that records the verdict.

## ADDED Requirements

### Requirement: Bot client

A bot MUST connect as an external client exactly as a browser does: fetch
the certificate hash from the hub's lobby API, establish WebTransport with
that hash pinned, perform the join handshake, and exchange state packets
as datagrams and event packets on the control stream. Every received
packet MUST be verified against the client-protocol contract — header
decode, stream framing, channel mapping — and any violation MUST be
reported as a conformance failure.

#### Scenario: Bot joins and exchanges packets

- **WHEN** a bot is pointed at a live hub with a joinable session
- **THEN** it completes the handshake, delivers its scripted inputs, and
  receives the session's greetings, echoes, and broadcasts

#### Scenario: Protocol violation is a failure

- **WHEN** a hub sends a malformed stream packet to a connected bot
- **THEN** the bot reports a conformance failure naming the violation
  instead of discarding the packet silently

### Requirement: Scenario runner

`netcode-bots run <scenario>` MUST read a TOML scenario declaring the
hub's endpoints, session sourcing (lobby-created or joined by id), bot
count and session spread, per-bot input scripts, a bounded duration, and
pass thresholds (join success rate, maximum datagram loss ratio, ordering
violations, latency bounds). The runner MUST exit 0 only when every
threshold is met, and MUST exit non-zero with a JSON report on any
failure or unreachable hub.

#### Scenario: Passing scenario

- **WHEN** a healthy hub is driven by a scenario whose thresholds it meets
- **THEN** the runner exits 0 and writes a report showing the measured
  values against each threshold

#### Scenario: Threshold exceeded

- **WHEN** a measured value crosses a declared threshold (e.g. loss
  exceeds the maximum)
- **THEN** the runner exits non-zero and the report names the threshold
  and the measured value

### Requirement: Deterministic fixtures

A scripted run against the scripted test worker MUST produce a fixture
recording, per bot: event payloads in exact order, unicast payloads for a
loss-bounded set comparison, and broadcast counters for a monotonicity
check — never wall-clock timing. Replaying the fixture MUST succeed
against an unchanged hub and MUST fail when any recorded event payload,
event ordering, or unicast payload is altered, or when loss exceeds the
scenario's declared bound.

#### Scenario: Fixture reproduction

- **WHEN** the recorded fixture runs against the scripted worker
- **THEN** every event and unicast payload matches exactly and in order,
  and broadcast counters strictly increase

#### Scenario: Mutated fixture fails

- **WHEN** a recorded payload in a fixture is altered
- **THEN** the comparison fails and names the bot, channel, and position
  of the divergence

### Requirement: Load smoke profile

The runner MUST support a declared load profile: many bots across many
sessions for a bounded duration with join/leave churn. The profile MUST
assert the declared thresholds, and a deliberately stalled bot MUST NOT
degrade any other bot's measured traffic (delivery, ordering, or latency)
beyond the declared bounds.

#### Scenario: Declared profile holds

- **WHEN** a profile of dozens of bots across several sessions runs to
  its duration against a healthy hub
- **THEN** all declared thresholds hold and the run completes

#### Scenario: Stalled bot is isolated

- **WHEN** one bot in a load profile stops reading its event stream for
  the rest of the run
- **THEN** every other bot still meets its thresholds

### Requirement: Run report

Every run MUST produce a JSON report: aggregate measurements (joins,
packets sent/received/lost, ordering violations, latency percentiles)
against each declared threshold, and per-bot detail sufficient to locate a
failure (session, client id, first divergence).

#### Scenario: Report pinpoints a failure

- **WHEN** a run fails because one bot's inputs stopped being echoed
- **THEN** the report identifies that bot by session and client id, and
  its first missing echo
