## Why

`Ecphory::open_with` derives the tantivy index directory from the database's
PARENT DIRECTORY, not from the database itself:

```rust
let index_dir = db_path.parent().unwrap_or(Path::new(".")).join("index");
```

So every `.redb` file in one directory shares one index. With a daemon serving
`a.redb`, opening an unrelated `b.redb` beside it fails (issue #29):

```
Error: storage error: creating index writer: Failed to acquire Lockfile: LockBusy.
```

The error names a tantivy lock, so it reads as an index problem rather than as
"these two stores are secretly the same index" — which is what it is. The
binding between a store and its index is positional, and nothing records it.

It has not bitten in production because the deployment is one store per
directory and the tests use a fresh `tempfile::tempdir()` each. Every existing
caller is accidentally correct.

## What Changes

- The index directory is keyed on the database file: `<db_dir>/<db_stem>.index`
  (`ecphory.redb` → `ecphory.index`). Two stores in one directory no longer
  collide.
- The index directory records the id of the store that built it
  (`.ecphory-index.json`). The store gains a durable `store_id`, minted on
  first open and kept in a new `meta` table.
- An index directory stamped by a DIFFERENT store is wiped and rebuilt rather
  than reused — the case where a store file was deleted and recreated under the
  same name. The index is derived data (the doc comment on `SearchIndex` says
  so), and the store is the source of truth, so self-healing beats refusing to
  start; the existing schema-mismatch path already makes that call. The wipe
  is announced at WARN.
- The legacy `<db_dir>/index` directory is adopted (renamed) on first open of
  the new binary, but ONLY when exactly one `.redb` file sits in that
  directory. With two, the legacy index cannot be attributed to either store,
  and guessing is the bug this change removes; it is left on disk and its
  owner logs that it is now unused.

## Non-goals

- Refusing to open on a stamp mismatch (the issue's second fix direction, as
  written). It turns a self-healable condition into a start-up failure for the
  daemon, and the index holds nothing the store cannot rebuild.
- Sharing one index across stores on purpose. There is no such use.
- Fixing the cold-start emptiness probe. `index.search("*", 1, true)` tokenizes
  to zero terms, so it matches nothing and the probe reads EVERY index as
  empty: every open of a non-empty store currently reindexes it wholesale.
  Verified 2026-09-13 (a scratch test asserting hits on a one-doc index printed
  0). That is a separate bug with its own risk — the accidental rebuild is
  currently repairing any index/store drift on every open, so fixing the probe
  without a drift check would change more than it looks like. Filed separately;
  this change is written so it does not depend on which way that goes.
