# per-store-index Specification

## Purpose
Bind each store to its own search index, explicitly. The index directory is
derived from the database file rather than its parent directory, and it records
which store built it, so two stores in one directory can never share an index
and a leftover index can never be mistaken for one.

## ADDED Requirements

### Requirement: The index directory is derived from the database file
Opening a store at `<dir>/<name>.redb` SHALL use `<dir>/<name>.index` as its
tantivy index directory.

#### Scenario: Two stores in one directory
- **WHEN** `a.redb` is open and `b.redb` in the same directory is opened
- **THEN** both opens succeed, where before the second failed with
  "Failed to acquire Lockfile: LockBusy"

#### Scenario: Neither store sees the other's episodes
- **WHEN** an episode is written to each of two stores in one directory
- **THEN** a search of either store returns only its own episode

### Requirement: A store has a durable id
A store SHALL mint a UUID on first open and keep it for the life of the store,
so its identity survives being moved or renamed.

#### Scenario: Stable across opens
- **WHEN** a store is opened, closed and opened again
- **THEN** the id is unchanged

### Requirement: The index directory records its store
An index directory SHALL carry the id of the store that built it, written at
open.

#### Scenario: Fresh index
- **WHEN** a store is opened and its index directory created
- **THEN** the directory holds a stamp naming that store's id

### Requirement: An index built by a different store is rebuilt, not reused
When the stamp names a store id other than the one being opened, the index
SHALL be wiped and rebuilt from the store, and the wipe SHALL be logged at WARN.

#### Scenario: Store deleted and recreated under the same name
- **WHEN** the `.redb` file is deleted, a new store is created at the same path,
  and an episode is written to it
- **THEN** searches return only the new store's episodes, and the stamp names
  the new store

### Requirement: The legacy layout is adopted only when it is unambiguous
On opening a store whose `<name>.index` does not exist while a legacy
`<dir>/index` does, the legacy directory SHALL be renamed to `<name>.index`
when exactly one `.redb` file is present in that directory, and SHALL be left
untouched otherwise.

#### Scenario: One store in the directory
- **WHEN** a store whose index sits at the legacy path is opened
- **THEN** the index moves to `<name>.index`, the legacy path is gone, and
  search still answers

#### Scenario: Two stores in the directory
- **WHEN** two `.redb` files share a directory with a legacy `index` directory
- **THEN** the legacy directory is left in place and each store builds its own
