# Spec Delta

## MODIFIED Requirements

### Requirement: Session creation

`POST /api/sessions` MUST require `Content-Type: application/json` (parameters allowed) and a body `{ "game_type": "<type>" }`. Missing or other media types MUST return `415`; malformed JSON MUST return `400`. Valid requests spawn the catalog worker with its defaults and return `201` with the new id. Unknown types return `404` with a JSON error naming the type. At the live-session limit, creation MUST return `503` without allocating an id or spawning a worker.

#### Scenario: Create an echo-arena session

- **WHEN** the catalog contains `echo-arena` and a client POSTs `{"game_type": "echo-arena"}` with a JSON content type
- **THEN** the response is `201` with a session id, and the session reaches `active` once the spawned worker claims it

#### Scenario: Browser-simple media type is rejected

- **WHEN** a client POSTs a valid creation body as `text/plain` or without a content type
- **THEN** the response is `415` and no session is created

#### Scenario: Unknown game type

- **WHEN** a client POSTs `{"game_type": "chess"}` with a JSON content type
- **THEN** the response is `404` and the error body names `chess`

#### Scenario: Live-session limit is full

- **WHEN** a valid creation request arrives while pending and active sessions fill the configured limit
- **THEN** the response is `503`, no session id is allocated, and no worker is spawned

## ADDED Requirements

### Requirement: HTTP request framing

Recognized HTTP header names MUST be matched case-insensitively. The lobby MUST reject duplicate or malformed `Content-Length` fields and any `Transfer-Encoding` with `400` before routing the request.

#### Scenario: Lowercase headers carry auth and body

- **WHEN** a request uses lowercase `authorization`, `content-type`, and `content-length` names
- **THEN** authentication and body processing behave exactly as for their canonical spellings

#### Scenario: Ambiguous body framing is rejected

- **WHEN** a request contains two `Content-Length` fields, a nonnumeric length, or any `Transfer-Encoding`
- **THEN** the response is `400` and no session is created
