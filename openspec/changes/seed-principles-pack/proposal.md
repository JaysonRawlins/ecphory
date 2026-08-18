## Why

ecphory ships as an empty store: correct, but a new user starts from zero and
the product's differentiators (self-correcting retrieval, versioned canonical
episodes, post-write triggers) only show value after weeks of accumulation.
Meanwhile the operator of the reference deployment has a battle-tested pack of
behavioral principles — verify before asserting, two-witness, structure over
discipline, secret-handoff via the secret store — that measurably improve
agent sessions and that most users would never think to write down.

Ship the store seeded. A new user gets tighter standards on day one; the
principles arrive as ordinary episodes, so a user who hates one prunes it the
same way they'd prune anything else (demote, never delete), and the pack
adapts to its owner from the first correction. Starting strict and relaxing
by evidence beats starting empty. Real-world motivation: even with context
injection, models still skip check-before-assert — a seeded, always-rendered
principle file raises the floor for everyone who installs.

## What Changes

- **A bundled starter pack**: principle episodes as JSONL, embedded in the
  binary at build time. Content derived from the reference deployment's
  stable-core behavioral session context, genericized (no personal names,
  no org-specific tooling).
- **`ecphory seed` command** (and a first-run offer in `serve`): imports the
  pack idempotently through the existing import machinery (existing ids are
  skipped, so re-seeding after upgrade only adds new principles and never
  clobbers user edits).
- **The pack includes one canonical `rendered-artifact` episode** — the
  always-loaded principles file — plus a triggers config template and render
  script templates for known harnesses (Claude Code hook, Codex AGENTS.md),
  so the edit→render loop works out of the box.
- **Docs**: a "seeded principles" page explaining the philosophy (start
  tight, prune by evidence) and how to customize or fully remove the pack.

## Non-goals

- No LLM authorship or rewriting of principles at seed time (write path stays
  LLM-free — design axiom).
- No telemetry about which principles users keep or prune.
- No auto-update of seeded content on upgrade beyond idempotent re-seed.

## Capabilities

### New Capabilities

- `seed-pack`: bundle, install, and re-install a starter set of principle
  episodes plus the rendered-artifact machinery, idempotently, with the user
  owning every episode from the moment it lands.

## Impact

- Binary grows by the embedded JSONL (kilobytes).
- `import` path gains a caller; no schema changes (tags/metadata already
  carry everything the pack needs).
- The reference deployment's principles need a genericization pass before
  bundling — they currently name people, clients, and local tools.
