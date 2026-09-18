## Why

The repo went public on 2026-09-16 (#45). Two things a stranger looks for on
first read were missing, and the pre-flip audit flagged both.

There is no `SECURITY.md`. GitHub keys its "Report a vulnerability" path off
that file plus private vulnerability reporting, and with neither, a finder's
only channel into a memory store holding one person's private data is a public
issue. That is the worst way to receive one, and the cost of preventing it is a
file.

`ECPHORY_AUTH_TOKEN` is undocumented. It appears in `src/main.rs`, `src/mcp.rs`
and `src/http.rs`, in one archived `tasks.md`, and nowhere a user reads. What a
reader finds instead, on a repo that describes itself as a memory store, is a
data plane that answers `curl` with no credential. The design is defensible —
loopback binding is the boundary, and a process running as you can open the
redb file anyway — but an undocumented open port reads as a hole, and "expect
this to be the first issue a stranger files" is the right prediction.

Writing it down turned up a third thing, which is the reason this is a spec and
not two paragraphs of docs. **The token is narrower than its own doc comment
claims.** `src/http.rs` says "Data plane is gated by the opt-in bearer token";
`require_auth` is layered on `/api/v1` inside `build_router`, and `serve_http`
in `src/mcp.rs` nests `/mcp` onto the returned router, outside that layer. With
the token set, on a 0.3.6 build:

```
/health                 -> 200
/api/v1/status no token -> 401
/api/v1/status w/ token -> 200
/mcp           no token -> 200, full initialize handshake
```

No `Mcp-Session-Id` is issued or required, `tools/list` returns all 11 tools,
and an unauthenticated `add_memory` over `/mcp` lands an episode on a store
whose `/api/v1` had just refused the same caller. Filed as #47.

The first instinct was to document the carve-out and defer the fix as breaking.
That was wrong, and worth writing down because the reasoning generalises:
`require_auth` returns early when no token is configured, so moving the layer
is **inert for anyone without a token set**. It changes behaviour only for
someone who has a token and does not send the header, and since the variable
was undocumented everywhere a user reads, that set was empty. Deferring would
have meant publishing the variable's first documentation with the wart in it,
which is the moment a wart becomes a compatibility obligation. The cheapest
time to fix an undocumented behaviour is before you document it.

And the gate had no test. Every harness in `src/http.rs` builds the router with
`token: None`, which is the one configuration where `require_auth` returns
before deciding anything, so the entire authenticated path was unexercised.
A second wall nobody has watched fail is not a wall.

## What Changes

- **`SECURITY.md`**, pointing at GitHub private vulnerability reporting (now
  enabled on the repo) rather than an email address, so no address is published
  and reports land in the native advisory flow. It states the threat model,
  a one-week first-response expectation from a single maintainer, and an
  explicit in-scope / out-of-scope list. Out of scope names the two things that
  would otherwise be filed first: the open-by-default loopback data plane, and
  #47.
- **`/mcp` moved inside the gate.** `build_router` now takes the MCP
  transport as a parameter and nests it beside the REST plane, under one auth
  layer, instead of `serve_http` nesting it onto the finished router. The shape
  is the fix: a surface is added by nesting it inside the guarded router, and
  the old way of bolting one on afterwards is no longer the path of least
  resistance. Closes #47.
- **A README `Security` section** saying the binding is the boundary, that the
  address is hard-coded and not configurable, that the data plane is open by
  default and why that is deliberate, and here is the variable. It says plainly
  that on a machine whose port is never forwarded the token protects nobody,
  and points at a dynamic header command (Claude Code's `headersHelper`) so the
  token need not be pasted into a client config as plaintext.
- **A wire-level regression test**,
  `bearer_auth_gates_the_data_plane_and_leaves_the_probe_open`, driving the real
  middleware over a real socket: unauthenticated read, unauthenticated write,
  an equal-length wrong token, a token with no `Bearer` scheme, the correct
  token, and `/health` open throughout. It asserts the refused calls also
  refused to write, because a 401 that mutates anyway is the failure worth
  catching. Staged red twice, below.
- **Comments at both ends of the fix**, `build_router` and the `serve_http`
  call site, naming #47 so the next person to add a surface is told where it
  goes before they reach for `.nest_service`.

## Non-goals

- **Turning the token on anywhere.** The daemon this was developed against runs
  with no `ECPHORY_AUTH_TOKEN` and should keep doing so: single user, loopback,
  no forwarding, so the token would guard nothing that the filesystem does not
  already. Verified inert for that configuration, below.
- **Client-side secret plumbing.** The README points at dynamic header commands
  and stops there. How a given MCP client reaches a secret manager is that
  client's business, and naming one vendor's mechanism as *the* answer would
  date badly.
- **Making the bind address configurable.** Being hard-coded is the strongest
  claim this project can make about its own exposure, and a `--host` flag would
  trade that for a footgun. If it is ever wanted, the token stops being defense
  in depth and becomes the only wall.
- **Encryption at rest.** The store is a file behind filesystem permissions and
  `SECURITY.md` says so plainly rather than implying more.
- **Specifying the MCP transport's own surface.** This spec claims the HTTP
  boundary it can prove and says where its own edge is.
