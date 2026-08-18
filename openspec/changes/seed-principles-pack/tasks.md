## 1. Pack

- [ ] 1.1 Genericize the reference principles into pack episodes (no names, no org tooling); fixed UUIDs; `seed-pack` tag.
- [ ] 1.2 Bundle as JSONL embedded at build time.

## 2. Mechanism

- [ ] 2.1 `ecphory seed` command over the existing idempotent import.
- [ ] 2.2 Rendered-artifact episode + triggers config template + per-harness render templates.
- [ ] 2.3 First-run offer in `serve` (decision open — see design).

## 3. Docs + E2E

- [ ] 3.1 Docs page: philosophy (start tight, prune by evidence), customize, remove.
- [ ] 3.2 E2E: fresh store → seed → edit principles episode from MCP → rendered file regenerates; re-seed → user edits untouched.
