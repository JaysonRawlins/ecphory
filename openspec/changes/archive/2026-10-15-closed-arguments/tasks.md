## 1. Implementation

- [x] 1.1 `#[serde(deny_unknown_fields)]` on all 9 MCP request structs and all
      10 REST body/query structs.
- [x] 1.2 `ContentMode { Replace, Append }` on `UpdateParams`; `Store::update`
      appends inside the existing `mutate` closure (one archived version) and
      refuses append with no content.
- [x] 1.3 MCP `update_episode` and REST `PUT` take `mode`; CLI takes `--append`.
- [x] 1.4 `mcp_router` extracted from `serve_http` so tests drive the
      production transport rather than a copy.

## 2. Red proof (record what was broken and what it printed)

- [x] 2.1 The four `mcp::tests` observed FAILING against the unchanged
      implementation (2026-10-15). The append test reproduced the incident
      byte for byte: `left: "OUTCOMES: shipped" right: "the long original
      body\n\nOUTCOMES: shipped"`. The unknown-field test got a success echo
      with `"content": "stamp"`. The schema test: `"add_memory" must advertise
      a closed argument list left: Null right: Bool(false)`.
- [x] 2.2 Each assertion re-proved by its own mutation, each reverted:
      deny removed from `UpdateEpisodeRequest` alone -> red at the update
      assertion; from `SearchRequest` alone -> red at the search assertion;
      append made to replace -> red with the incident's `left`; the
      append-needs-content guard removed -> red at "append without content";
      `ContentMode` parse accepting anything -> red at "an unrecognised mode".
- [x] 2.3 REST test: deny removed from `UpdateBody` -> `an unknown body field
      must be refused, got 200`; from `SearchQuery` -> `an unknown query
      parameter must be refused, got 200`; REST dropping `mode` -> `left: 200
      right: 400` at append-without-content.
- [x] 2.4 CLI test: `--append` ignored -> `left: "OUTCOMES: shipped"`.
      Removing clap's `requires = "content"` stayed GREEN: the store guard
      refuses it further in. Kept for the clearer CLI message; the test comment
      says both layers refuse, so neither alone is pinned.

## 3. Verify

- [x] 3.1 `cargo fmt`, `cargo clippy --all-targets -D warnings` clean,
      `cargo test` all green (113 unit + integration).
- [x] 3.2 Live, release binary, throwaway store, port 34917 (never the
      production store). MCP `merge: true` -> `isError: true`, "unknown field
      `merge`, expected one of `id`, `content`, `mode`, ...". The incident's
      exact call (`mode: "append"`) -> `"LIVE original body, long\n\nOUTCOMES:
      shipped"`, one archived version holding the original. REST unknown body
      field -> 422 naming it; unknown query param (`search_mode`) -> 400 naming
      it; a known-params search -> 200. `tools/list` -> 11 tools,
      `update_episode` schema `additionalProperties: false` with `mode`.
- [x] 3.3 Callers outside the repo checked against the closed structs: the
      `ecphory_recall.sh` hook, `render-session-context.sh`, the `recall` and
      `loops` servers send only defined fields. `resume-tailor` passed engram's
      `search_mode`/`search_alpha` and was corrected in claude-config.
- [ ] 3.4 Post-deploy: the production daemon refuses an unknown field and
      appends on `mode: "append"`, and a restarted Claude Code session still
      loads the tools. Recorded in ecphory, not here, since it runs after merge.
