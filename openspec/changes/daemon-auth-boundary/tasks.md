## 1. Implementation

- [x] 1.1 `SECURITY.md` at the repo root: private reporting via GitHub's
      advisory flow, one-week first response, latest-release-only support,
      threat model, and an in-scope / out-of-scope list. Out of scope names the
      open loopback data plane and #47 by name, so the two most likely first
      reports are answered before they are filed.
- [x] 1.2 Private vulnerability reporting enabled on the repo
      (`PUT /repos/JaysonRawlins/ecphory/private-vulnerability-reporting`),
      confirmed `{"enabled":false}` -> `{"enabled":true}`. Without this the
      "Report a vulnerability" button `SECURITY.md` points at does not exist.
- [x] 1.3 README `Security` section between Install and Status: hard-coded
      loopback bind as the boundary, open-by-default data plane and why,
      `ECPHORY_AUTH_TOKEN` with a generating one-liner, which CLI commands pick
      it up, what it does not cover (`/health`, `/mcp` per #47), store
      unencrypted at rest, mirror travels in plaintext.
- [x] 1.4 Comment at the `/mcp` nest site in `src/mcp.rs` recording that the
      nest places it outside the bearer gate, with the verified evidence and
      the issue number, at the line that causes it.
- [x] 1.5 `spawn_server_with_token(Option<&str>)` test harness;
      `spawn_server_with_state` delegates to it so existing tests are
      unchanged and the auth-on path uses the same `build_router`.
- [x] 1.6 `bearer_auth_gates_the_data_plane_and_leaves_the_probe_open`.
- [x] 1.7 CHANGELOG entry under Unreleased.
- [x] 1.8 No production code changed. Confirmed by diff: all three hunks in
      `src/http.rs` fall inside `mod tests` (starts line 610; hunks at 636,
      649, 1191), and `src/mcp.rs` gains comment lines only.

## 2. Red proof (record what was broken and what it printed)

- [x] 2.1 Gate disabled. An unconditional `return next.run(req).await` inserted
      at the top of `require_auth`, which is what a refactor that drops the
      middleware would look like:

      ```
      assertion `left == right` failed: no Authorization header must not reach the data plane
        left: 200
       right: 401
      ```

- [x] 2.2 Comparison weakened, which is the subtler failure and the one a
      coarse test would miss. `constant_time_eq` replaced with
      `a.first() == b.first()`, a prefix check that still refuses obviously
      wrong tokens:

      ```
      assertion `left == right` failed: an equal-length wrong token must still be refused
        left: 200
       right: 401
      ```

      This is why the test carries an equal-length near miss rather than only
      a wrong token: 2.1 alone would have passed against a broken compare.

- [x] 2.3 Restored, green: `test http::tests::bearer_auth_gates_the_data_plane_and_leaves_the_probe_open ... ok`,
      and `grep -c "DELIBERATE BREAK" src/http.rs` -> 0.

## 3. Live verification (against a real daemon, never the live store)

- [x] 3.1 `ECPHORY_DB` pointed at a temp store, `ECPHORY_AUTH_TOKEN` set,
      `serve --port 34917`:

      ```
      /health                 -> 200
      /api/v1/status no token -> 401
      /api/v1/status w/ token -> 200
      /mcp           no token -> 200, full initialize handshake
      ```

- [x] 3.2 The `/mcp` gap is a whole open surface, not just a reachable
      endpoint. No `Mcp-Session-Id` is issued or required; `tools/list` returns
      all 11 tools including `add_memory`, `update_episode` and
      `delete_episode`; an unauthenticated `add_memory` returned
      `{"success":true}` and the write was confirmed by asking the gated REST
      plane with the token (`{"episodes":1}`) on the same store that had just
      answered that caller `401`.
- [x] 3.3 Filed as #47 with the evidence and the two decisions it needs
      (whether to move the layer; whether `/health` stays open).

## 4. Follow-ups, not in this change

- [ ] 4.1 Decide #47. Moving the auth layer over `/mcp` is breaking for every
      MCP client config that does not send the header, so it wants a release
      note and a version bump, not a ride-along in a docs change.
- [ ] 4.2 `src/http.rs`'s own module doc says "Data plane is gated by the
      opt-in bearer token", which reads as covering more than it does. Left
      alone here rather than reworded in passing: it is accurate about
      `/api/v1`, and rewriting it is part of closing #47 either way.
