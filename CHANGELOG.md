# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- subject index: `ecphory render-index` writes a workspace's recall keys into `AGENTS.md` (the file codex, agy and OpenCode read) and Claude Code's per-project `MEMORY.md`, inside an idempotent marked region; `ecphory workspace-key` prints the `ws:<slug>` tag that scopes an episode to a workspace; `GET /memory/episodes?tags=` filters a listing (#24)

### Fixed

- *(index)* stop reindexing the whole store on every open. The cold-start check asked `search("*")`, which tokenizes to nothing and so read every index as empty; it now compares the index's document count against the store's, which keeps repairing real drift (a write or purge that reached only one of the two) while leaving a healthy index untouched. On a 900-episode store `ecphory search` goes from 390 ms to 54 ms, and read-only commands no longer commit to the index. `status` reports the index document count (#30)

## [0.3.6](https://github.com/JaysonRawlins/ecphory/compare/v0.3.5...v0.3.6) - 2026-09-14

### Added

- *(search)* hidden groups are skipped by unscoped search ([#23](https://github.com/JaysonRawlins/ecphory/pull/23))
- offsite mirror — auto-push to a git remote and an export trigger event ([#21](https://github.com/JaysonRawlins/ecphory/pull/21))
- post-write triggers — run a local command when a matching episode changes ([#20](https://github.com/JaysonRawlins/ecphory/pull/20))

### Fixed

- *(index)* key the search index on the database file, not its directory ([#31](https://github.com/JaysonRawlins/ecphory/pull/31))
- *(cli)* read recorder log views through the daemon instead of the store ([#27](https://github.com/JaysonRawlins/ecphory/pull/27))
- separate rating telemetry and add version rollback

### Other

- *(openspec)* archive per-store-index ([#33](https://github.com/JaysonRawlins/ecphory/pull/33))
- self-correction walkthrough, heal-cycle gif, and the deploy skills ([#32](https://github.com/JaysonRawlins/ecphory/pull/32))
- *(openspec)* archive daemon-first-log-views ([#28](https://github.com/JaysonRawlins/ecphory/pull/28))
- *(openspec)* archive the two shipped changes, backfill the triggers spec ([#26](https://github.com/JaysonRawlins/ecphory/pull/26))
- offsite-mirror verify task complete — reference deployment pushing
- openspec draft — seed-principles-pack distribution change

### Added

- hidden groups: `ECPHORY_HIDDEN_GROUPS` lists groups an unscoped search skips; naming the group in `group_id` opts back in; `/status` reports the set (#22)
- atomic archived-version rollback through MCP, REST, and CLI

### Fixed

- the search index is now keyed on the database file (`<db>.index`) instead of its parent directory, so two stores in one directory no longer share one index — which failed with "Failed to acquire Lockfile: LockBusy" on the second open. The index directory records the id of the store that built it and is rebuilt if it names another. An existing `index/` directory is adopted automatically when it sits beside exactly one store; beside two it is left alone and can be deleted once each store has rebuilt ([#29](https://github.com/JaysonRawlins/ecphory/issues/29))
- `heals`, `ratings`, `search-log` and `access-log` read through the daemon when one is running instead of dying on redb's process-exclusive lock; `--url` is now a global flag and each read reports its source on stderr ([#15](https://github.com/JaysonRawlins/ecphory/issues/15))
- keep `used_episode_ids` as telemetry instead of self-correction targets

## [0.3.5](https://github.com/JaysonRawlins/ecphory/compare/v0.3.4...v0.3.5) - 2026-07-18

### Fixed

- count healed old misses out of rated_miss_outstanding ([#17](https://github.com/JaysonRawlins/ecphory/pull/17))

## [0.3.4](https://github.com/JaysonRawlins/ecphory/compare/v0.3.3...v0.3.4) - 2026-07-15

### Added

- heal lifecycle — resolution state, replay regression pass, collateral check, source-tagged tape ([#11](https://github.com/JaysonRawlins/ecphory/pull/11))

## [0.3.3](https://github.com/JaysonRawlins/ecphory/compare/v0.3.2...v0.3.3) - 2026-07-14

### Added

- search ratings + self-correction loop — misses enrich, redo, and validate ([#7](https://github.com/JaysonRawlins/ecphory/pull/7))

## [0.3.2](https://github.com/JaysonRawlins/ecphory/compare/v0.3.1...v0.3.2) - 2026-07-13

### Other

- add syft sbom + grype vulnerability scan ([#5](https://github.com/JaysonRawlins/ecphory/pull/5))

## [0.3.1](https://github.com/JaysonRawlins/ecphory/compare/v0.3.0...v0.3.1) - 2026-07-13

### Fixed

- *(index)* treat search queries as plain text, not tantivy syntax ([#3](https://github.com/JaysonRawlins/ecphory/pull/3))
