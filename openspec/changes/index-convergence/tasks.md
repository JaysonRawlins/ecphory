## 1. Implementation

- [x] 1.1 `SearchIndex::num_docs` returns the committed document count.
- [x] 1.2 `converge_index(store, index)` compares `store.count()` with
      `index.num_docs()` and rebuilds when they differ, returning the number
      reindexed (None when nothing was needed). INFO on an empty index, WARN on
      a wrong count.
- [x] 1.3 `Ecphory::open_with` calls it in place of the `search("*")` probe.
- [x] 1.4 `Ecphory::indexed_count` exposes the count; `ecphory status` prints
      `indexed:` beside `episodes:`.
- [x] 1.5 CHANGELOG entry under Unreleased.

## 2. Red proof (record what was broken and what it printed)

- [x] 2.1 `reopening_a_healthy_store_does_not_rebuild_the_index` observed
      FAILING against the unchanged implementation (2026-09-14,
      `cargo test --bin ecphory`). It reads tantivy's commit counter straight
      off `meta.json`, which is the issue's own evidence as an assertion:
      `assertion left == right failed: opening a store whose index is already
      converged must not commit  left: 12  right: 8`.
- [x] 2.2 The drift half was proved red the same way, by STAGING the naive fix
      first. With the probe changed to `index.num_docs()? == 0` and nothing
      else — the one-line fix the issue warns against — the two drift tests
      failed:
      `a_store_write_that_missed_the_index_is_repaired_at_open` ->
      `assertion left == right failed: an episode missing from the index must
      be reindexed at open  left: 0  right: 1`;
      `a_purge_that_missed_the_index_is_repaired_at_open` ->
      `the dangling index document must be swept at open`.
      That is the issue's "why it is not a one-line fix", observed rather than
      assumed.
- [x] 2.3 All three green under the count comparison, along with
      `converge_index_reports_whether_it_rebuilt`,
      `a_wildcard_query_is_not_a_probe_for_emptiness` (which pins WHY `search`
      cannot be a probe: `*` is a term, not syntax) and
      `num_docs_counts_committed_documents_only`.

## 3. Verify

- [x] 3.1 `cargo fmt --check` clean, `cargo clippy --all-targets` 0 warnings,
      `cargo test` 110 passed (105 unit + 5 CLI integration), up from 104.
- [x] 3.2 Live, release binaries, against a 900-episode throwaway store built
      through `import` (never the production store, which a daemon holds).
      `main` was built from `origin/main` into a separate target dir so both
      ran against the SAME store file:
      - `main`: 5 `ecphory search` runs in 1.95s (390 ms each), index opstamp
        1803 -> 6318 — five full rebuilds triggered by a read-only command.
      - this change: 5 runs in 0.27s (54 ms each), opstamp 6318 -> 6318.
- [x] 3.3 Live convergence cases on the same store:
      - index directory deleted -> `INFO index is empty over 900 episodes;
        building it`, then `episodes: 900 / indexed: 900`.
      - two episodes added, then the pre-add index directory restored over the
        new one -> `WARN index holds 900 documents for 902 stored episodes;
        rebuilding`, and a search for one of the two unindexed episodes returns
        it (`count: 1`).
      - reopening the repaired store logs no convergence message at all.
