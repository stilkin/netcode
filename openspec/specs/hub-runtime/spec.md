# hub-runtime Specification

## Purpose
Defines the observable runtime behavior of the hub binary itself — how it is
configured, how it reports what it is doing, and how it starts and stops — so
later changes can hang networking features on a stable, testable shell.

## Requirements

### Requirement: Configuration loading

The hub MUST read an optional TOML configuration file whose path is given by
`--config` (default `hub.toml` in the working directory). A missing file SHALL
mean all defaults. An unknown configuration key MUST be rejected with an error
naming that key, and invalid configuration MUST exit with code 1.

#### Scenario: Boots without config

- **WHEN** the hub starts and no config file exists
- **THEN** it starts with default settings and logs the resolved configuration

#### Scenario: Typo in config

- **WHEN** the config file contains a key the hub does not know
- **THEN** startup fails with an error naming the key and exit code 1

### Requirement: Structured logging

The hub MUST log to stdout via `tracing`. The log level MUST default to `info`,
MUST be settable in the config file, and MUST be overridable by the `RUST_LOG`
environment variable. The first startup log line MUST include the hub version
and the resolved configuration.

#### Scenario: Default verbosity

- **WHEN** the hub starts with no log level configured
- **THEN** it logs at info level, including a startup line with version and resolved config

#### Scenario: Environment override

- **WHEN** `RUST_LOG=debug` is set at startup
- **THEN** debug-level logs are emitted regardless of the configured level

### Requirement: Graceful shutdown

On the first SIGINT/SIGTERM (or Ctrl-C) the hub MUST begin graceful shutdown
with a bounded grace period and MUST exit with code 0 when finished. A second
signal MUST exit immediately. Fatal runtime errors MUST exit with code 2.

#### Scenario: Clean stop

- **WHEN** a running hub receives one SIGINT
- **THEN** it stops accepting work, finishes in-flight work within the grace period, and exits 0

#### Scenario: Impatient operator

- **WHEN** a second signal arrives during graceful shutdown
- **THEN** the hub exits immediately

### Requirement: Version reporting

`hub --version` MUST print the binary name and semantic version and MUST exit
with code 0.

#### Scenario: Version flag

- **WHEN** the hub is invoked with `--version`
- **THEN** it prints `hub <semver>` and exits 0

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
