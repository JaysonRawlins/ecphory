## 1. Implementation

- [x] 1.1 `--url` promoted to a global flag with `$ECPHORY_URL` fallback; `eval` reads it instead of its own arg.
- [x] 1.2 `EvalClient::reachable()` probes `/api/v1/status` (the bare `/status` 404s — the router mounts under the versioned prefix).
- [x] 1.3 `EvalClient` log readers made generic in the row type; `resolution_log` added. `eval` keeps its narrow projections, the CLI decodes the canonical recorder entries.
- [x] 1.4 `LogReader` (Remote | Local) chooses the source, honours an explicit `--db`, and announces it on stderr; the four views dispatch before the store open, like `Eval`.
- [x] 1.5 CHANGELOG entry under Unreleased.

## 2. Red proof (record what was broken and what it printed)

- [x] 2.1 All four views observed FAILING against the live daemon before the change.
      DONE 2026-09-10, worktree binary, production daemon pid 23102 up:
      `heals`, `ratings`, `search-log`, `access-log` each → exit 1,
      `Error: storage error: Database already open. Cannot acquire lock.`
- [x] 2.2 Same four green after 1.4. DONE: each exit 0, 3 rows,
      stderr `(reading from daemon http://127.0.0.1:3491)`.
- [x] 2.3 `cli_log_views_match_the_store_over_http` mutation-checked, since a
      passing test proves nothing until seen failing:
      probing `/status` instead of `/api/v1/status` → FAILED "the daemon we
      just started must answer"; `last_replay` marked `skip_serializing` →
      FAILED "replay outcome must survive the wire". Both reverted, green.

## 3. Verify (isolated fixture, never the live store)

- [x] 3.1 Byte-identical output, HTTP vs direct store open: temp store seeded
      with an episode, a fetch, a search and a healed miss; read through a
      daemon on an ephemeral port, then again with it stopped.
      DONE 2026-09-10: heals / ratings / search-log / access-log all IDENTICAL.
- [x] 3.2 Fallback with no daemon: `$ECPHORY_DB` ambient, `--url` pointing at a
      dead port → exit 0, rows printed, stderr `(reading from store …)`, output
      equal to the direct read.
- [x] 3.3 No silent cross-store read: daemon serving store A on :3997,
      `--db B --url http://127.0.0.1:3997 search-log` → 0 rows from B while A
      had 1, stderr `(reading from store …/b/store.redb)`. Same invocation
      without `--db` → 1 row, `(reading from daemon …)`.
- [x] 3.4 `eval` unbroken by the `--url` move: url after the subcommand (the
      pre-change form), url before it, and `$ECPHORY_URL` with no flag all
      score the same gold set; `--heals` and `--from-log` run; with the daemon
      stopped `eval` fails with connection refused rather than opening the
      store.
- [x] 3.5 `cargo fmt --check`, `cargo clippy --all-targets` (0 warnings),
      `cargo test` (85 passed) all green.
