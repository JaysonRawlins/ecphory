# daemon-auth-boundary Specification

## Purpose
Define where ecphory's network boundary actually is, and what the opt-in bearer
token does and does not buy on either side of it.

The daemon binds loopback and nothing else, and that binding is the security
model. `ECPHORY_AUTH_TOKEN` is defense in depth for the case the port stops
being loopback-only, so it gates every surface that touches memories, REST and
MCP alike, with `/health` the one deliberate exception. The structural half
matters as much as the behaviour: one router owns the gate, because the `/mcp`
hole existed precisely by mounting a surface outside a layer that had already
been applied.

This capability also covers the project's disclosure posture, since a memory
store's threat model is only useful if a finder has somewhere private to send
what they find.

## Requirements
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

### Requirement: The bearer token gates every surface that touches memories
When `ECPHORY_AUTH_TOKEN` is set to a non-empty value, every route that reads
or writes episodes SHALL require an `Authorization: Bearer <token>` header
whose value matches, and SHALL answer `401` otherwise. This SHALL include the
MCP transport at `/mcp` as well as `/api/v1`, because the MCP tool surface is
the same store by another door and gating one without the other buys nothing. The comparison SHALL be
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

#### Scenario: The MCP surface
- **WHEN** an MCP client attempts `initialize`, `tools/list` or `tools/call` against `/mcp` with no credential or a wrong one, while a token is configured
- **THEN** it is refused with `401`, no tool is listed, and no episode is written

#### Scenario: No token configured
- **WHEN** no `ECPHORY_AUTH_TOKEN` is set
- **THEN** every surface including `/mcp` answers as it did before the gate existed, so enabling the gate is inert for a deployment that never opted in

### Requirement: One router owns the boundary
The auth layer SHALL be applied once, to a single router into which every
guarded surface is nested, rather than per-surface. A new surface SHALL be
added by nesting it inside that router; nesting one onto the router returned by
`build_router` places it outside the gate and SHALL NOT be how a surface is
mounted. This is a structural requirement rather than a stylistic one: the
`/mcp` hole existed because the transport was attached by the caller after the
layer had already been applied, which no reading of either file made obvious.

#### Scenario: A surface is added later
- **WHEN** a contributor mounts a new transport or admin surface on the daemon
- **THEN** the signature of `build_router` and the comments at both ends direct them inside the guarded router, and the auth test fails if they mount it outside

#### Scenario: The regression is reintroduced
- **WHEN** `/mcp` is nested onto the finished router again, as it was before #47
- **THEN** the bearer-auth test fails on the `/mcp` assertion rather than the change reaching a release

### Requirement: The token's limits are stated where they are read
The documentation SHALL describe what the token does and does not buy, so its
value is never inferred from its existence. It SHALL state that `/health` is
deliberately open; that the token is defense in depth for a forwarded port and
not a reason to forward one; and that on a single-user machine whose port is
never forwarded it protects against nobody, because a process running as that
user can open the store file regardless. It SHALL also warn that a token
pasted into a client configuration is a plaintext secret adjacent to what it
protects, and point at dynamic header generation as the better shape.

#### Scenario: A reader decides whether to set it at all
- **WHEN** someone reads the README's Security section on a single-user laptop
- **THEN** they are told plainly that the token would protect nobody in that setup, rather than being nudged into managing a secret for no gain

#### Scenario: A reader decides where the token lives
- **WHEN** someone does need the token and looks for how a client should send it
- **THEN** the README names a dynamic header command as the way to keep the value out of a config file entirely

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

