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
whose `/api/v1` had just refused the same caller. Tracked as #47.

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
- **A README `Security` section** saying the binding is the boundary, that the
  address is hard-coded and not configurable, that the data plane is open by
  default and why that is deliberate, and here is the variable. It names what
  the token does *not* cover, rather than letting a reader infer coverage from
  its existence.
- **A wire-level regression test**,
  `bearer_auth_gates_the_data_plane_and_leaves_the_probe_open`, driving the real
  middleware over a real socket: unauthenticated read, unauthenticated write,
  an equal-length wrong token, a token with no `Bearer` scheme, the correct
  token, and `/health` open throughout. It asserts the refused calls also
  refused to write, because a 401 that mutates anyway is the failure worth
  catching. Staged red twice, below.
- **A comment at the nest site** in `src/mcp.rs`, so the next reader finds the
  carve-out at the line that causes it instead of in a README.

No production code changes. The boundary being specified is the one that exists.

## Non-goals

- **Gating `/mcp`.** It is the obvious fix and it is deliberately not here.
  Moving the layer breaks every MCP client config pointed at this daemon the
  moment the token is set, none of which send the header today, so it needs a
  release note and a version bump rather than a ride-along in a docs change.
  Filed as #47 with the decision written out.
- **Making the bind address configurable.** Being hard-coded is the strongest
  claim this project can make about its own exposure, and a `--host` flag would
  trade that for a footgun. If it is ever wanted, the token stops being defense
  in depth and becomes load-bearing, and #47 has to close first.
- **Encryption at rest.** The store is a file behind filesystem permissions and
  `SECURITY.md` says so plainly rather than implying more.
- **Specifying the MCP transport's own surface.** This spec claims the HTTP
  boundary it can prove and says where its own edge is.
