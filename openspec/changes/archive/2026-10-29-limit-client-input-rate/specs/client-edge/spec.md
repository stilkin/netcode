# Spec Delta

## ADDED Requirements

### Requirement: Inbound packet budget

Each accepted client connection MUST have one inbound packet budget shared by state datagrams and complete event-stream packets. The budget MUST permit an initial 512-packet burst and refill at 4,096 packets/sec. Under-budget packets MUST be forwarded without throttling delay; a packet with no available budget MUST close and detach only that client. Handshake and outbound packets MUST NOT consume the budget.

#### Scenario: Flood disconnects only the offender

- **WHEN** an accepted client exceeds its available inbound packet budget
- **THEN** that connection closes and detaches while another client and the worker continue normally

#### Scenario: Transports share one allowance

- **WHEN** a client sends datagrams and complete stream packets whose combined rate exceeds the budget
- **THEN** the combined traffic exhausts the same allowance and closes that connection

#### Scenario: Normal traffic and refill are not throttled

- **WHEN** a client sends below the sustained rate or its budget refills after earlier use
- **THEN** packets continue to be forwarded without waiting for a rate-limit timer
