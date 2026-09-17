# daemon-auth-boundary Specification

## ADDED Requirements

### Requirement: The daemon is reachable only over loopback
`ecphory serve` SHALL bind `127.0.0.1` and no other address. The bind address
SHALL NOT be configurable by flag, environment variable, or config file, so
that no configuration of ecphory exposes a store to a network. The port SHALL
remain configurable via `--port` and `$ECPHORY_PORT`. This binding, not the
bearer token, is the security boundary, and the project's documentation SHALL
say so rather than presenting the token as the protection.

#### Scenario: A user goes looking for a host setting
- **WHEN** someone reads the CLI help, the README, or `SECURITY.md` for a way to serve the daemon on a LAN address
- **THEN** they find a statement that the address is hard-coded and that no such setting exists, rather than an absence they have to interpret

#### Scenario: The port moves
- **WHEN** the daemon is started with `--port` or `$ECPHORY_PORT`
- **THEN** it listens on that port on `127.0.0.1`, and the address is unaffected

### Requirement: The data plane is open by default and documented as such
With no `ECPHORY_AUTH_TOKEN` set, the REST data plane SHALL serve reads and
writes to any caller that reaches the port. This is intended for a single-user
local daemon, because a process running as the user can open the store file
directly and a gate on the port would not exclude it. Because an undocumented
open port is indistinguishable from an oversight, the README SHALL state the
default, the reason for it, and the variable that changes it; and `SECURITY.md`
SHALL list the open-by-default data plane as out of scope for vulnerability
reports.

#### Scenario: A stranger curls the port
- **WHEN** someone finds that `curl http://127.0.0.1:3491/api/v1/status` answers without a credential
- **THEN** the README and `SECURITY.md` have already said it does, said why, and said what to set instead, so it is a documented design rather than a finding

### Requirement: The bearer token gates the REST data plane
When `ECPHORY_AUTH_TOKEN` is set to a non-empty value, every route under
`/api/v1` SHALL require an `Authorization: Bearer <token>` header whose value
matches, and SHALL answer `401` otherwise. The comparison SHALL be
constant-time. A refused request SHALL have no effect on the store. `/health`
SHALL remain outside the gate so a supervisor can probe a daemon it holds no
token for. The CLI commands that read through the daemon SHALL pick the same
variable up with no extra flag.

#### Scenario: No credential
- **WHEN** a request reaches `/api/v1` with no `Authorization` header while a token is configured
- **THEN** it is refused with `401`, and if it was a write, nothing was written

#### Scenario: A wrong token of the right length
- **WHEN** a request presents a token equal in length to the configured one but differing in a byte
- **THEN** it is refused, exercising the comparison rather than the length shortcut

#### Scenario: The scheme is missing
- **WHEN** a request presents the correct token as a bare `Authorization` value with no `Bearer ` prefix
- **THEN** it is refused

#### Scenario: The probe stays reachable
- **WHEN** `/health` is requested with no credential while a token is configured
- **THEN** it answers `200`

### Requirement: The token's coverage is stated, including where it stops
The documentation SHALL name the surfaces the token does not cover, so that
coverage is never inferred from the variable's existence. Specifically it SHALL
state that `/health` is deliberately open, and that the MCP transport at `/mcp`
is nested outside the auth layer and is therefore reachable without a
credential even when the token is set (#47). The token SHALL be described as
defense in depth for a forwarded port, never as a reason it is safe to forward
one. A test that pins the REST half SHALL say in the source that it asserts
nothing about `/mcp`, and the nest site in `src/mcp.rs` SHALL carry the same
note.

#### Scenario: An operator decides whether to forward the port
- **WHEN** someone with the token set considers exposing the port through a tunnel or a container port map
- **THEN** the README tells them the MCP surface is not covered, so the decision is made with the gap visible rather than after discovering it

#### Scenario: Someone moves the auth layer later
- **WHEN** a change makes the token cover `/mcp`
- **THEN** the comments at the test and at the nest site are the two places that name the old carve-out, and both point at #47

### Requirement: A disclosure channel exists and is private
The repository SHALL carry a `SECURITY.md` naming a private reporting channel,
and private vulnerability reporting SHALL be enabled so GitHub's "Report a
vulnerability" path resolves. The file SHALL set a first-response expectation,
SHALL state that only the latest release receives fixes, and SHALL give a
fallback for a report that goes unanswered which does not require disclosing
details publicly. It SHALL NOT direct vulnerability reports to the public issue
tracker, which remains the channel for everything else.

#### Scenario: A finder looks for where to report
- **WHEN** someone with a vulnerability opens the repository's Security tab
- **THEN** "Report a vulnerability" is available and opens a private thread, and `SECURITY.md` tells them not to use a public issue

#### Scenario: A report gets no answer
- **WHEN** a week passes with no response to a private report
- **THEN** `SECURITY.md` has told the finder to open a public issue saying only that they are waiting, with no details in it
