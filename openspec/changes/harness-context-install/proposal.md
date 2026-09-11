## Why

ecphory's value depends on the agent knowing the store exists and searching it
before acting. Today that knowledge arrives by hand-curated per-project index
files that only one harness (Claude Code) loads, which has two costs: other
harnesses get a materially worse memory experience, and the operator cannot tell
whether recall comes from the store or from the curated index.

The harder problem is that context injection **fails silently**. Measured on
2026-09-10 across four harnesses, five distinct misconfigurations each produce a
hook or pointer that installs cleanly, exits 0, and injects nothing:

1. copilot hook emitting plain text instead of JSON
2. codex hook placed in `config.toml` rather than `hooks.json`
3. codex hook without persisted hook trust
4. claude `@import` using an absolute or `~` path (only relative resolves)
5. copilot `AGENTS.md` placed globally (project-level only)

For a user who does not know how agent memory works, every one of these reads as
"ecphory isn't very good." The store is fine; the wiring is dead. Nothing in the
current product can tell those two apart.

## What Changes

- **One rendered context artifact**, already the shape `triggers` and
  `seed-principles-pack` assume: a `rendered-artifact` episode regenerated on
  write. This change consumes it; it does not redefine it.
- **`ecphory install`**: detect installed harnesses and wire each to that one
  artifact using its own proven adapter. Idempotent, reversible, and it never
  silently edits a config it cannot restore byte-identically.
- **`ecphory doctor`**: verify *delivery*, not configuration. Reports per harness
  as DELIVERED / UNPROVEN / MISCONFIGURED. It MUST NOT report OK from static
  inspection, because a correct-looking config and text actually reaching the
  model are different facts — that gap is the entire reason this change exists.

## Adapters (all measured, not assumed)

  harness   adapter                                 reads/copies  proven by
  claude    SessionStart hook                       read          in production
  codex     managed block in ~/.codex/AGENTS.md     copy          TOPAZ-1102
  opencode  config "instructions":[absolute path]   read          CITRINE-8890
  copilot   sessionStart hook + JSON additionalCtx  read          MARBLE-7788

codex is the one copy: its hook rail requires interactive persisted hook trust
that an installer cannot grant, so the managed block is the zero-friction path.
The hook remains an opt-in upgrade. Whether codex's hook stdout injects is
UNVERIFIED — the hook never executed, so it is recorded as untested, not as
broken.

## Non-goals

- Replacing per-project curation. Whether the curated index is load-bearing is a
  separate open question (see `agent-index-spike`); this change makes the
  A/B possible by giving every harness the same footing.
- Shipping content. What goes *in* the artifact is `seed-principles-pack`.
