## 1. Implementation

- [x] 1.1 `src/subject_index.rs`: `workspace_root` (git-common-dir, worktree-aware), `workspace_slug` (Claude's `sanitizePath`, including the >200 truncate-and-hash branch), `workspace_tag`, `claude_memory_path`.
- [x] 1.2 Rows and rendering: `row` (phrases → query, name → description, skip when neither), `render_block` (deterministic), `splice` (marked region, half-open is an error), `write_if_changed` (temp file + rename).
- [x] 1.3 `ListOptions.tags` filters in `store.list` before `limit`; `/memory/episodes?tags=` exposes it with the comma-separated convention `/memory/search` already uses (`csv_tags` now shared by both). `ListOptions` loses `Copy`.
- [x] 1.4 `ecphory render-index` and `ecphory workspace-key`, dispatched in main's pre-store block beside `eval` so they never open redb; `fetch_scoped` reads `/memory/episodes` over HTTP and refuses to truncate at the listing cap.
- [x] 1.5 `docs/subject-index.md`, README section, CHANGELOG entry under Unreleased.

## 2. Red proof (record what was broken and what it printed)

- [x] 2.1 One row per workspace. Filter removed from `rows_for` (`.filter(|ep| !ep.is_deleted())`):
      `cargo test subject_index` → FAILED, `assertion left == right failed: expected one row, got:
      ["| unrelated | ws:-work-y | OTHER_BODY_TEXT | 01a079e3-… |", "| why does the CLI hang… |"]`,
      left = 2, right = 1. Restoring the tag filter is the only change between that run and green.
- [x] 2.2 No episode body. With `description: cell(&ep.content)`:
      FAILED, `episode body leaked into the rendered index:` followed by the whole block, whose
      Description column read `SENTINEL_BODY_TEXT redb takes an exclusive file lock`.
- [x] 2.3 Byte-identical re-render. With `out.push_str(&format!("\nRendered {}\n", Utc::now()…))`
      in `render_block`: FAILED, `second render reported a change`. Removing that one line is the
      only difference from the shipped renderer.
      (2.1–2.3 were taken one at a time: each defect was removed in turn so the next assertion
      could be seen failing on its own rather than short-circuiting behind the first.)
- [x] 2.4 Tag-scoped listing. Before the filter existed:
      `http::tests::list_episodes_filters_by_tag` FAILED, `tag filter did not scope the listing:
      {"count":2,"episodes":[…"ws:-work-y"…,…"ws:-work-x"…]}`, left = 2, right = 1.
      Note: the first run of this test hung, not failed — `#[tokio::test]` defaults to a
      current-thread runtime and the blocking `ureq` call deadlocks the spawned server. The other
      HTTP tests use `flavor = "multi_thread"`; this one now does too. Worth recording because the
      hang was indistinguishable from a slow compile.
- [x] 2.5 Half-open marked region: `splice` with only a BEGIN marker errors with `half open`, and
      with END before BEGIN errors with `out of order` (`half_open_region_is_an_error`).
- [x] 2.6 Symlinked target. Found by reading the diff against the spec, not by a failing test:
      the reference deployment symlinks each harness's global instruction file, and temp-file +
      rename replaces a symlink with a regular file. With `let target = path.to_path_buf()`:
      `a_symlinked_target_is_followed_not_replaced` FAILED, `the render replaced a symlinked
      target with a regular file`. Green once the path is canonicalized before the rename.
- [x] 2.7 `--check` goes red. Inside `subject_index_end_to_end`: `--check` exits 0 immediately
      after a render, and exits 1 with `STALE <path>` after one new episode is POSTed to the
      running daemon. Both directions asserted, so the flag is not stuck either way.
- [x] 2.8 `$ECPHORY_URL` reaches the renderer. Rebasing onto main picked up #27's global
      `--url` (`--url`, then `$ECPHORY_URL`, then loopback), and `render-index` still carried a
      `--url` of its own. clap does not reject the collision — it shadows, and the local default
      wins — so the environment was silently ignored:
      `the_daemon_url_can_come_from_the_environment` FAILED, `the render did not reach the daemon
      named by $ECPHORY_URL`, having rendered an empty index from the developer's live loopback
      daemon instead of the test's. Green once the subcommand drops its flag and reads
      `daemon_url(cli.url)` / `auth_token()` like `eval` and the log views do. The test harness
      now scrubs `ECPHORY_URL` too, for the same reason it scrubs the triggers file.

## 3. Verify (live, against real processes)

- [x] 3.1 HTTP is load-bearing, with a control. Daemon serving a temp store on 39117:
      `ecphory --db <db> list --limit 1` → rc=1, `Error: storage error: Database already open.
      Cannot acquire lock.` The identical conditions with `render-index --url http://127.0.0.1:39117`
      → rc=0, `wrote …/OUT.md (0 keys for ws:-…)`. This is why the renderer is HTTP-only.
- [x] 3.2 Worktree and main checkout share one index, on a real linked worktree (this branch's):
      `git rev-parse --show-toplevel` → `/Users/jjrawlins/agent-worktrees/ecphory/issue-24-…`
      (the naive rule, which would have split the index) while `--git-common-dir` →
      `/Users/jjrawlins/code/GitHub/JaysonRawlins/ecphory/.git`. The binary prints
      `ws:-Users-jjrawlins-code-GitHub-JaysonRawlins-ecphory` from both directories — and that slug
      is the directory Claude Code actually injected this session's `MEMORY.md` from.
- [x] 3.3 Slug cross-checked against Claude's own implementation. `sanitizePath` transcribed from
      the 2.1.263 bundle, run under node over 7 vectors (plain paths, two >200-character paths
      exercising the hash branch, a non-BMP path); `diff js.out rust.out` → identical. The first
      run caught a real error — in my hand-written test expectation (158 `a`s where the rule gives
      157), not in the implementation.
- [x] 3.4 `cargo test` 95 passed / 0 failed; `cargo clippy --all-targets -- -D warnings` clean
      (CI's exact invocation).

## 4. Not done here

- [ ] 4.1 Reference-deployment rollout: install the release binary, add a per-workspace trigger to
      `~/.local/share/ecphory/triggers.json`, tag this repo's episodes with
      `ws:-Users-jjrawlins-code-GitHub-JaysonRawlins-ecphory`, and confirm a codex session on the
      repo reads the keys. Deliberately left to the operator — it writes to the live store and to
      `AGENTS.md` in a tracked repo.
