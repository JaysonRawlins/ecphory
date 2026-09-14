## 1. Implementation

- [x] 1.1 `Store` mints a `store_id` (UUIDv7) into a new `meta` table on first open — including the first open of an existing store.
- [x] 1.2 `index_dir_for` derives `<db_dir>/<db_stem>.index`; `open_with` uses it.
- [x] 1.3 `bind_index_dir` writes `.ecphory-index.json` at open; a stamp naming another store wipes the directory at WARN and lets the store rebuild it.
- [x] 1.4 `adopt_legacy_index_dir` renames `<db_dir>/index` into place, guarded to the one-`.redb` case; otherwise it logs that the legacy directory is unused and leaves it.
- [x] 1.5 CHANGELOG entry under Unreleased.

## 2. Red proof (record what was broken and what it printed)

- [x] 2.1 All five new tests observed FAILING before the implementation
      (2026-09-13, `cargo test --bin ecphory`). The two that exercise the bug
      itself printed the issue's error verbatim:
      `two_stores_in_one_directory_get_separate_indexes` and
      `legacy_index_dir_is_left_alone_beside_two_stores` →
      `Storage("creating index writer: Failed to acquire Lockfile: LockBusy. …")`.
      `index_dir_is_named_for_the_store_file` → `assertion failed:
      dir.path().join("alpha.index").is_dir()`;
      `legacy_index_dir_is_adopted_when_it_is_unambiguous` → NotFound renaming
      a directory that was never created; `an_index_built_by_another_store_is_rebuilt`
      → `index dir records its store: … NotFound` (no stamp existed).
      `store_id_is_minted_once_and_survives_reopen` did not compile before 1.1.
- [x] 2.2 All six green after 1.1–1.4.
- [x] 2.3 The `search("*", 1, true)` emptiness probe was measured, not assumed:
      a scratch test over a one-document index printed
      `star-probe hits on a non-empty index: 0`. Recorded because it means the
      stale-index cases in this change would ALSO be papered over by the
      accidental rebuild-on-every-open; the stamp assertions are what make
      those tests load-bearing, not the search results. See the proposal's
      non-goals.

## 3. Verify

- [x] 3.1 `cargo fmt --check` clean, `cargo clippy --all-targets` 0 warnings,
      `cargo test` 91 passed (89 unit + 2 CLI integration).
- [x] 3.2 Live, with the built binary and throwaway stores (never the
      production store, which a daemon holds):
      - the issue's own repro: `serve` on `a.redb`, then `add` to `b.redb` in
        the same directory → exit 0, where the issue recorded LockBusy. Cross
        searches: `b` finds bison/cormorants (1 each) and aardvarks 0; `a`
        finds aardvarks 1 and bison 0. Stamps name two different store ids.
      - legacy adoption: a store with its index at `<dir>/index` reopened →
        `INFO adopted legacy index …/index as …/ecphory.index`, legacy path
        gone, search still returns its episode.
      - ambiguous legacy: two stores plus `<dir>/index` → `WARN … holds 2
        stores and a legacy index dir …`, the directory is still there
        afterwards, and each store answers from its own.
- [x] 3.3 Nothing outside the repo names the index directory (checked
      `~/Library/LaunchAgents` and `~/.claude/scripts`), and
      `~/.local/share/ecphory` holds exactly one `.redb`, so the reference
      deployment takes the adoption path on its next daemon restart.
