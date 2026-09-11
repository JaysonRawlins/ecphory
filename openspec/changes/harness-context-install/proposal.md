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
- **`ecphory doctor`**: verify the OUTCOME, not the mechanism. Doctor asks one
  question — did ecphory's context reach this harness's model? — and does not
  care which rail carried it. An ecphory-installed adapter, a host security
  daemon that already injects on every harness, a hand-rolled render script:
  all are equally valid answers. Doctor is the authority.
- **`ecphory install`**: best-effort convenience that wires an adapter where
  nothing already delivers. It is NOT the authority and MUST no-op where
  delivery already happens. Idempotent, reversible, never edits a config it
  cannot restore byte-identically.

The outcome-not-mechanism rule was forced by observation, not taste. The first
doctor build reported `opencode UNPROVEN` on the reference machine by checking
for its own pointer — while that machine's opencode was already reading a live
ecphory-generated artifact at `~/.config/opencode/AGENTS.md`, written by an
existing render script. Doctor looked for its own mechanism and missed a working
delivery path in plain sight. A verification tool that only recognises its own
handiwork is a tool that reports on itself.

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
