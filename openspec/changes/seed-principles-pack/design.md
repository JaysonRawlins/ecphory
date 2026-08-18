# Design notes (draft — decisions open)

## Pack format: JSONL episodes embedded at build time

Same shape `export`/`import` already speak, so seeding is just `import` with a
bundled reader. Fixed UUIDs in the pack make re-seeding idempotent (import
skips existing ids) and give docs stable references. Episodes carry a
`seed-pack` tag so a user can list, demote, or purge the entire pack with one
tag filter.

## Ownership transfers at seed time

Seeded episodes are ordinary episodes: versioned, editable, demotable. No
special-casing anywhere in the store. "The user hates it" is handled by the
existing deletion story, and "the user improved it" is an ordinary update —
which matters, because their edits survive re-seeding (idempotent import
never overwrites).

## The rendered-artifact loop is the product, principles are the payload

The pack's canonical principles episode ships tagged `rendered-artifact` with
a triggers config template, so the seeded store demonstrates the full loop on
day one: edit the episode from any client → trigger fires → the always-loaded
file re-renders. Render templates per harness (Claude Code SessionStart hook;
Codex AGENTS.md include). Harness detection stays manual v1 — a docs choice,
not code.

## Open questions

- `ecphory seed` explicit command vs. first-run prompt in `serve` vs. both.
- Whether the genericized principles live in this repo (versioned with the
  binary) or a separate pack repo (versioned independently, multiple packs).
- Pack curation bar: which of the reference principles generalize, which are
  the owner's personal workflow. First pass: User Interaction, Problem
  Framing, Verification, Two-Witness, Outcome Stamping, Secrets — drop
  Comments/Git Commits (workflow-specific).
