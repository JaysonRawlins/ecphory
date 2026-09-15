## Why

The cold-start convergence check in `Ecphory::open_with` asked whether the
index was empty by searching for `*`:

```rust
if store.count()? > 0 && index.search("*", 1, true)?.is_empty() {
    index.rebuild(...)
}
```

`SearchIndex::search` has not used tantivy's query parser since #3: queries are
raw user text, so `plain_text_query` tokenizes them and builds one clause per
token. `*` produces zero tokens, the BooleanQuery has no clauses, and it
matches nothing. The probe read EVERY index as empty, including a healthy one,
so every open of a non-empty store reindexed it wholesale — daemon startup, and
every CLI command that opens the store directly, `search` included (issue #30).

Measured on a 900-episode store (the reference deployment's scale), release
build, comparing `main` with this change against the same store file:

| | per `ecphory search` | index opstamp over 5 searches |
| --- | --- | --- |
| `main` | 390 ms | 1803 -> 6318 (five full rebuilds) |
| this change | 54 ms | 6318 -> 6318 (no writes) |

That is the cost the issue left open: ~7x on a read-only command, and an index
rewritten by every process that reads it.

## What Changes

- The emptiness probe becomes a document count. `SearchIndex::num_docs` reads
  tantivy's committed doc count; `search` is never asked a question it cannot
  answer.
- The check is a count COMPARISON, not an emptiness test, and lives in one
  named function, `converge_index`. Every stored episode has exactly one index
  document (every write path goes through `Ecphory`, which upserts and
  commits), so the counts agree in a healthy store and any difference is drift.
  Both sides are O(1) — redb's table length, tantivy's doc count — so the check
  stays free to ask on every open.
- A rebuild is logged: INFO when the index was empty (cold start, the expected
  case), WARN when it held the wrong number of documents. After this change a
  rebuild means something went wrong, so it is worth hearing; before, it was
  the sound of every open and meant nothing.
- `ecphory status` prints the index document count beside the episode count.

## The drift the accidental rebuild was repairing

Rebuilding on every open was silently repairing any index/store drift, for
free. Making the probe honest without deciding this would have traded a
performance bug for a correctness one — so the count comparison is chosen
precisely because it keeps the repair:

- A store write that never reached the index (crash between `store.insert` and
  `index.commit`) leaves an episode nothing can find. Counts differ, so it is
  repaired at open.
- A purge that never reached the index leaves a dangling document. Search
  already tolerates it (the store join drops it), but counts differ, so it is
  repaired at open.
- A new or deleted index directory is the same condition with `indexed == 0`.

What a count cannot see is drift that PRESERVES it: an update whose store write
landed and whose index upsert did not leaves one stale document where one fresh
one belongs. That case is no longer repaired by opening the store. It is
narrower than it sounds — `Ecphory::update` commits the index inside the same
call, so the window is a crash between two adjacent statements — and `reindex`
remains the repair. Accepting it is the deliberate trade: detecting it costs a
full content comparison at every open, which is the rebuild we are removing.

## Non-goals

- Detecting same-count content drift at open. Named above; `reindex` repairs
  it. Anything cheaper than a rebuild would have to hash the corpus, which is
  the same read.
- A `doctor`-style integrity command. Worth having, but it is a feature, not
  this bug fix.
- Incremental convergence (indexing only the missing ids). At 900 episodes a
  full rebuild is ~1.5s and now runs only when something is actually broken;
  computing the id-set diff costs a full index scan anyway.
