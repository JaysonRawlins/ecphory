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
      it up, `/health` as the one exception, store unencrypted at rest, mirror
      travels in plaintext. Plus the two things a reader actually needs to
      decide: that on an unforwarded single-user box the token protects nobody,
      and that a token in a client config is a plaintext secret next to what it
      protects, so prefer a dynamic header command.
- [x] 1.4 `/mcp` moved inside the gate. `build_router` takes the transport as
      a parameter and nests it beside `/api/v1` under one auth layer;
      `serve_http` hands it over instead of nesting it onto the finished
      router. Comments at both ends name #47. Closes #47.
- [x] 1.5 `spawn_server_with_token(Option<&str>)` test harness;
      `spawn_server_with_state` delegates to it so existing tests are
      unchanged and the auth-on path uses the same `build_router`.
- [x] 1.6 `bearer_auth_gates_the_data_plane_and_leaves_the_probe_open`.
- [x] 1.7 CHANGELOG entry under Unreleased.
- [x] 1.8 Production code changed only in router assembly: `build_router`'s
      signature and nesting, and the `serve_http` call site. No handler, no
      middleware body, no storage path touched. `require_auth` itself is
      byte-identical.

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

- [x] 2.3 The #47 regression itself, staged as the third red. Reinstating the
      pre-fix shape (layer `/api/v1`, then `.nest("/mcp", ...)` onto the result)
      turns the new assertion red:

      ```
      assertion `left == right` failed: the MCP surface must be behind the token, not beside it
        left: 200
       right: 401
      ```

      This is the one that matters: it proves the test would have caught the
      original bug, rather than merely passing against the fix.

- [x] 2.4 Restored, green: `test http::tests::bearer_auth_gates_the_data_plane_and_leaves_the_probe_open ... ok`,
      and `grep -c "DELIBERATE BREAK" src/http.rs` -> 0. Full suite 113 tests,
      0 failures; `cargo fmt --check` and `clippy --all-targets -D warnings`
      clean.

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
- [x] 3.3 Filed as #47, then fixed in the same branch once it was clear the
      change is inert without a token set and therefore breaks nobody.
- [x] 3.4 Re-probed the fixed binary with the REAL transport, not the stub.
      Token set: `/health` 200, `/api/v1/status` 401, `/mcp` initialize 401
      with no header and 401 with a wrong one, 200 with the right one.
      `tools/list` and `add_memory` unauthenticated both return
      `{"error":"missing or invalid bearer token"}`, and the store reports
      `{"episodes":0}` afterwards, so the refused write refused to write.
- [x] 3.5 Re-probed with NO token set, which is how the author's daemon runs:
      `/health` 200, `/api/v1/status` 200, `/mcp` initialize 200,
      `tools/list` 11 tools. Unchanged, so the deployed daemon needs no
      coordinated config change and the LaunchAgent is untouched.
- [x] 3.6 `src/http.rs`'s module doc said "Data plane is gated by the opt-in
      bearer token", which was accurate about `/api/v1` and misleading about
      everything else. Now names both surfaces, since it is true.

## 4. Follow-ups, not in this change

- [ ] 4.1 Nothing blocking. If the bind address is ever made configurable, the
      token stops being defense in depth and becomes the only wall, and this
      spec's first requirement is the one to revisit.
