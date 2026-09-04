## 1. Implementation

- [x] 1.1 `Ecphory` carries `hidden_groups: Vec<String>` read from `ECPHORY_HIDDEN_GROUPS` in `open_with`; `with_hidden_groups` builder for tests and embedders.
- [x] 1.2 `search_impl` skips an episode whose group is hidden when `opts.group_id` is `None`.
- [x] 1.3 `/status` includes `hidden_groups`; MCP `search` doc for `group_id` and the REST/CLI docs mention the behaviour.
- [x] 1.4 CHANGELOG entry under Unreleased.

## 2. Red proof (record what was broken and what it printed)

- [x] 2.1 `hidden_group_excluded_unless_named` observed FAILING with the builder present and the filter absent.
      DONE 2026-09-03: `cargo test hidden_group_excluded_unless_named` → FAILED,
      `assertion \`left == right\` failed: unscoped search leaked a hidden group`,
      left = both episode ids, right = the visible one. Filter commit is the only
      change between that run and 2.2.
- [x] 2.2 Same test green after 1.2. DONE: `test result: ok. 1 passed`.

## 3. Verify (reference deployment)

- [x] 3.1 Release binary installed, `ECPHORY_HIDDEN_GROUPS=todos` in the launchd plist, service restarted, `/status` shows it.
      DONE 2026-09-03: `/api/v1/status` → `{"episodes":1115,"hidden_groups":["todos"],"status":"operational","version":"0.3.5"}`.
      Note: the first launch after replacing the binary in place exited with OS_REASON_CODESIGNING; KeepAlive relaunched it and run 2 is healthy.
- [x] 3.2 `GET /api/v1/memory/search?query=Bear+notes+contacts+export+migration+to+Obsidian` returns no `todos` episodes; the same query with `group_id=todos` returns them.
      DONE 2026-09-03 (origin=eval): unscoped → 10 results, groups all default/defiance, no todos.
      Scoped `group_id=todos` → 2 results, top = `todo: Contact Nick the electrician` (the episode that ranked #2 unscoped before the change).
