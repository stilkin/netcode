# Spec Delta

## MODIFIED Requirements

### Requirement: Artifact self-validation

Before publishing anything, the release flow MUST start the freshly
built hub binary with a temporary configuration and run the conformance
bots' smoke scenario against it end to end. A failing or crashing smoke
MUST fail the release; no artifact is published unvalidated. The
Docker-image validation path MUST build the host `netcode-bots` from the
current checkout before using it.

#### Scenario: Bots validate the artifact

- **WHEN** the release flow reaches validation
- **THEN** the built `hub` and `netcode-bots` run the smoke scenario
  together, and the scenario's exit code gates the release

#### Scenario: A broken binary stops the release

- **WHEN** the built hub binary fails to boot or the smoke scenario fails
- **THEN** the release aborts with the failure named, and no publishing
  step runs

#### Scenario: Docker validation builds its judge

- **WHEN** image validation runs without `target/debug/netcode-bots`
- **THEN** Cargo builds the current host binary before the smoke scenario starts
