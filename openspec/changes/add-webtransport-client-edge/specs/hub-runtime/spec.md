# Spec Delta

## Purpose

Extends the hub runtime with the client edge lifecycle: the browser-facing
listener is a first-class boot concern, on par with the backplane. Existing
behavior (configuration strictness, logging, exit codes, backplane
lifecycle) is unchanged.

## ADDED Requirements

### Requirement: Client edge startup

At boot the hub MUST bind the WebTransport client listener with a fresh
self-signed certificate and bind the bootstrap HTTP endpoint, log each
listener's address plus the certificate's SHA-256 hash, and treat any bind
failure as a fatal startup error. The `[client_edge]` section follows the
same configuration rules as every other section (strict unknown-key
rejection, all-defaults when missing).

#### Scenario: Boot and log lines

- **WHEN** the hub boots
- **THEN** the log names the client edge address, the bootstrap address, and
  the certificate hash alongside the existing backplane line

#### Scenario: Port already taken

- **WHEN** the configured client edge bind address is already in use
- **THEN** startup fails with exit code 2 and the error names the listener

#### Scenario: Unknown key

- **WHEN** `hub.toml` contains `[client_edge]` with key `protocl`
- **THEN** startup fails with exit code 1 and the error names `protocl`

### Requirement: Client edge shutdown

On shutdown the hub MUST stop accepting client connections and close
active ones within the existing drain grace period, before the process
exits 0.

#### Scenario: Clean stop with clients attached

- **WHEN** SIGTERM arrives while browser clients are mid-session
- **THEN** their connections close within the grace period and the hub exits 0
