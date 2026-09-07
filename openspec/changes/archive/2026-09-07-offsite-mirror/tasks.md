## 1. Implementation

- [x] 1.1 `export::git_push` (first remote, HEAD, no-remote = clean no-op) + `ECPHORY_EXPORT_PUSH` gate.
- [x] 1.2 Store-wide `export` trigger event: matcher not required, `ECPHORY_TRIGGER_EXPORT_DIR` env, fired on committed exports (scheduled + CLI).
- [x] 1.3 Tests: push lifecycle against a bare remote; export-event validation and firing.

## 2. Docs

- [x] 2.1 docs/backups.md — tiers, recipes (git remote, S3/restic via trigger), restore drill.
- [x] 2.2 docs/triggers.md — store-event class documented.

## 3. Verify (reference deployment)

- [x] 3.1 Private mirror remote configured; scheduled export pushes (serve log `pushed=true`; remote shows the commit). Verified 2026-08-18: boot export pushed 9b74ef3 to JaysonRawlins/ecphory-mirror (private).
