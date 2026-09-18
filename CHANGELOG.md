# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.8](https://github.com/JaysonRawlins/ecphory/compare/v0.3.7...v0.3.8) - 2026-09-18

### Fixed

- *(dist)* ship unix artifacts as .tar.gz so curl | sh works on debian and ubuntu ([#51](https://github.com/JaysonRawlins/ecphory/pull/51))

### Other

- *(readme)* wire an agent to the daemon, and correct the stale facts ([#52](https://github.com/JaysonRawlins/ecphory/pull/52))
- *(openspec)* archive provenance-conventions ([#49](https://github.com/JaysonRawlins/ecphory/pull/49))

## [0.3.7](https://github.com/JaysonRawlins/ecphory/compare/v0.3.6...v0.3.7) - 2026-09-17

### Added

- subject index — render a workspace's recall keys into AGENTS.md and MEMORY.md ([#34](https://github.com/JaysonRawlins/ecphory/pull/34))

### Fixed

- *(update)* make source, source_model and source_description updatable ([#40](https://github.com/JaysonRawlins/ecphory/pull/40))
- *(index)* converge the index by document count, not a wildcard search ([#37](https://github.com/JaysonRawlins/ecphory/pull/37))

### Other

- *(provenance)* say what belongs in source and source_model, where it is read ([#48](https://github.com/JaysonRawlins/ecphory/pull/48))
- *(packaging)* add the winget package ([#46](https://github.com/JaysonRawlins/ecphory/pull/46))
- contribution policy and branch-protection rulesets for the public repo ([#45](https://github.com/JaysonRawlins/ecphory/pull/45))
- *(openspec)* archive rating-telemetry ([#44](https://github.com/JaysonRawlins/ecphory/pull/44))
- *(ratings)* name the field that mutates, and pin it at the wire ([#43](https://github.com/JaysonRawlins/ecphory/pull/43))
- *(openspec)* archive updatable-provenance ([#42](https://github.com/JaysonRawlins/ecphory/pull/42))
- *(openspec)* archive index-convergence ([#38](https://github.com/JaysonRawlins/ecphory/pull/38))
- *(openspec)* archive subject-index ([#36](https://github.com/JaysonRawlins/ecphory/pull/36))

### Added

- subject index: `ecphory render-index` writes a workspace's recall keys into `AGENTS.md` (the file codex, agy and OpenCode read) and Claude Code's per-project `MEMORY.md`, inside an idempotent marked region; `ecphory workspace-key` prints the `ws:<slug>` tag that scopes an episode to a workspace; `GET /memory/episodes?tags=` filters a listing (#24)

### Fixed

- *(update)* `source`, `source_model` and `source_description` are updatable. They were settable at write time and unreachable afterwards, so an agent that wrote a mangled value — 26 episodes carry a serialization artifact baked into `source` — left it permanent short of a delete-and-re-add that mints a new id and breaks every reference into the episode. They now travel the same path as every other field on the MCP `update_episode` tool, `PUT /memory/episodes/{id}` and `ecphory update` (`--source`, `--source-model`, `--source-description`), archived into an `EpisodeVersion` and reversible. Empty still means leave unchanged (#39)
- *(index)* stop reindexing the whole store on every open. The cold-start check asked `search("*")`, which tokenizes to nothing and so read every index as empty; it now compares the index's document count against the store's, which keeps repairing real drift (a write or purge that reached only one of the two) while leaving a healthy index untouched. On a 900-episode store `ecphory search` goes from 390 ms to 54 ms, and read-only commands no longer commit to the index. `status` reports the index document count (#30)

### Other

- *(provenance)* write the `source` / `source_model` / `source_description` convention down where an agent reads it. `source_model` and `source_description` carried no schema description at all, so the only guidance a writing agent ever got was `source`'s "Originating system" — and the store drifted into two conventions for the same fact: 12 episodes fold the model into `source` with a slash (`claude-code/opus-4-7`), 11 leave `source` empty, and `opus-4-7` is a spelling of `opus-4.7` that is neither the API id nor the human short form. All three fields now say what belongs in them on the MCP `add_memory` and `update_episode` schemas, the REST body and the CLI, and the README states the convention for humans. Nothing is validated on write — axiom 3 holds for provenance too — so this is documentation plus a one-off repair of the 48 affected episodes in the reference deployment, not a behaviour change (#41)
- *(ratings)* say plainly, on every rating surface, that `used_episode_ids` is telemetry and `intended_episode_ids` is the only field that edits an episode, and pin that promise at the wire with a regression test — a `partial` carrying used ids only must leave every episode it names untouched. The behaviour shipped in 0.3.6; only the contract was unwritten (#18)

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
