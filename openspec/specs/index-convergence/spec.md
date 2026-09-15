# index-convergence Specification

## Purpose
Keep the derived search index in agreement with the store it is derived from,
and make opening the store cost nothing when it already is. The store is the
source of truth; the index is rebuilt from it whenever the two disagree about
how many episodes exist.

## Requirements
### Requirement: The index is probed by counting documents, not by searching
The check for whether an index is populated SHALL read the committed document
count. It SHALL NOT be expressed as a search, because queries are plain text
and no query term is guaranteed to match a populated index.

#### Scenario: A wildcard matches nothing on a populated index
- **WHEN** an index holding one document is searched for `*`
- **THEN** the search returns no hits, while the document count returns 1

### Requirement: Opening a converged store does not write to the index
Opening a store whose index document count equals its episode count SHALL NOT
commit to the index.

#### Scenario: Repeated read-only opens
- **WHEN** a store with a healthy index is opened repeatedly, including by
  read-only commands such as `search`
- **THEN** the index's commit counter is unchanged, where before every open
  reindexed the whole store

### Requirement: An index that disagrees with the store is rebuilt at open
When the index document count differs from the store's episode count, the index
SHALL be rebuilt from the store at open, and the rebuild SHALL be logged — at
INFO when the index was empty, at WARN otherwise.

#### Scenario: A store write that never reached the index
- **WHEN** an episode is written to the store without a matching index commit
  and the store is reopened
- **THEN** a search finds that episode

#### Scenario: A purge that never reached the index
- **WHEN** an episode is purged from the store without a matching index removal
  and the store is reopened
- **THEN** the index no longer holds a document for it

#### Scenario: A new or deleted index directory
- **WHEN** a non-empty store is opened with no index directory
- **THEN** the index is built from the store and search answers

### Requirement: Drift that preserves the count is repaired only on demand
Drift that leaves the counts equal — a stale document where a fresh one belongs
— SHALL NOT be detected at open. `reindex` SHALL remain the repair.

#### Scenario: Operator repairs a stale index
- **WHEN** `reindex` is run
- **THEN** the index is rebuilt from the store in full

### Requirement: Status reports the index document count
`status` SHALL print the number of documents in the index alongside the number
of episodes in the store.

#### Scenario: Status on a healthy store
- **WHEN** `ecphory status` is run
- **THEN** it prints both the episode count and the index document count

