# Spec Delta

## ADDED Requirements

### Requirement: Request deadline

The lobby MUST impose a total 30-second deadline on each HTTP request. An incomplete request exceeding that deadline MUST be closed without affecting other requests.

#### Scenario: Slow request expires

- **WHEN** a client leaves its request incomplete beyond the deadline
- **THEN** its connection closes and subsequent requests are served normally
