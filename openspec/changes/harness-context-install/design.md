## Evidence

Every adapter below was verified with a behavioral canary: a BUILD CODE plus
"begin every reply with it" planted in the candidate surface, then an unrelated
question ("what is 2+2"). The instrument was proven able to fire before any
negative was trusted — Claude obeyed an instructions-borne rule, so silence from
another harness means something. Self-reports were not accepted as evidence:
codex claimed it had no ecphory tools while the server log showed it attached,
and a forced call printed `mcp: Ecphory/get_status started`.

## Why not one uniform mechanism

Attempted and falsified, in order:

1. **MCP tool `description`** — not delivered at startup by any harness tested.
   Claude defers MCP tool schemas (name only until fetched on demand), which
   makes a description-borne nudge circular: it exists to trigger a search but
   is only read after the agent decided to search. Held under
   `--strict-mcp-config` with a single server, so not tool-count overflow.
2. **MCP server `instructions`** — delivered by Claude, not by codex, and
   opt-in-only for copilot (`--allow-all-mcp-server-instructions`, off by
   default). Three harnesses, three behaviours. Dead as a primary rail.
3. **One file + N thin pointers** — only opencode accepts a pointer to a shared
   absolute path. Claude's `@import` resolves relative paths inside the project
   tree only; absolute, `~/...` and `~/.claude/...` all silently resolved to
   nothing.

What survived is one artifact plus per-harness adapters of three kinds: pointer,
managed block, hook. Prefer a *read* over a *copy* wherever the harness allows
it — a copy goes stale, a read cannot.

## doctor: the tiering, and why static cannot be enough

Static inspection (config present, path exists, JSON well-formed) cannot
distinguish any of the five silent no-ops from a working install. Four of the
five pass every static check. So:

- **Static tier** — cheap, always runs, can only ever output MISCONFIGURED or
  UNPROVEN. It is not permitted to emit DELIVERED.
- **Live tier** — spawns the harness with a one-shot canary and checks the reply.
  This is the only thing that yields DELIVERED. It costs a model call per
  harness, which is the honest price of the claim.

A doctor whose happy path is reachable without the live tier would be the same
hollow gate this change exists to eliminate.

## Red-proof obligation

`doctor` is a verification gate, so it owes its own red before anyone cites it:
for each harness, break that harness's adapter deliberately, confirm doctor
reports it, restore. Recorded in tasks.md with the exact output. A doctor never
observed failing is indistinguishable from a doctor that cannot fail.

## Open questions

- codex hook stdout injection: untested (trust prompt is interactive; the bypass
  flag is correctly refused by local policy). If it injects, codex's copy
  becomes a read and the design has zero copies.
- copilot has no proven global instruction file. Its hook is global, so the hook
  is its only single-source rail — making copilot the harness most dependent on
  the JSON-not-plain-text gotcha being handled correctly.
