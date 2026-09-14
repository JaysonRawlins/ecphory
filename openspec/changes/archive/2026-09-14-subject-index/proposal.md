## Why

ecphory is the shared store, but only one agent finds its way in unaided.

Claude Code injects a per-project `MEMORY.md` as a first-class instruction
source (attachment type `AutoMem`). That file is a slim index of search keys,
and it is what makes recall work: an agent holding the index knows *what to
search for*. Without it ecphory is still reachable and still returns good
results, but the agent never asks the questions.

Codex, agy and OpenCode read `AGENTS.md`. They get the same MCP access and none
of the index, so on the same repo they start materially behind. Closing that is
this change; Claude is not the gap.

Two measurements shaped the design, both correcting an earlier assumption:

- **Claude's memory path is git-root-keyed, not cwd-keyed.** In 2.1.263,
  `getAutoMemPath().defaultPath()` slugs `findCanonicalGitRoot(projectRoot) ??
  projectRoot`, and `findCanonicalGitRoot` has an explicit linked-worktree case
  (parse the `.git` file's `gitdir:`, read `commondir`, verify the
  `worktrees/` layout and the `gitdir` back-pointer, return the main repo
  root). Confirmed live: a session whose cwd is
  `~/agent-worktrees/ecphory/issue-24-…` was injected
  `~/.claude/projects/-Users-jjrawlins-code-GitHub-JaysonRawlins-ecphory/memory/MEMORY.md`.
- **A non-git working directory resolves to itself** and gets its own index.

## What Changes

- **`ws:<slug>` tag** scopes an episode to a workspace. The slug is Claude's
  own `sanitizePath` rule applied to the git toplevel, so the two agree on
  what "this project" means rather than drifting. Tags, not `group_id`: an
  episode can belong to two workspaces, and tagging is additive — existing
  episodes join by having a tag added, with no migration.
- **`ecphory workspace-key [--workspace <dir>]`** prints that tag, so a
  capturing agent knows what to tag without reimplementing the rule.
- **`ecphory render-index`** renders the workspace's keys into
  `<workspace>/AGENTS.md` and Claude's per-project `MEMORY.md` (both
  overridable with repeatable `--out`), inside a marked region.
  `--check` exits 1 on a stale target and writes nothing.
- **Keys only** — query phrase, tags, one-line description, episode id. Never
  episode bodies. The file is a pointer; the store holds the content.
- **`GET /memory/episodes?tags=a,b`** filters a listing, applied before
  `limit` so a scoped listing is not truncated by episodes it would never
  return. Same comma-separated convention `/memory/search` already uses.
- Reads the daemon over **HTTP**, never the database, for the same reason
  `eval` does: the render is fired by a post-write trigger, which runs while
  the daemon holds redb's process-exclusive lock.

## Decisions

**`AGENTS.md` is a tracked file, and this writes into it.** Chosen anyway: a
separate gitignored include only helps if the harness follows includes, and
codex, agy and OpenCode do not guarantee that — the include would be read by
nobody, which defeats the purpose. Churn is bounded by keys-only content and
write-if-changed (an unchanged index is not a diff), the marked region keeps
hand-written content intact, and `--out` lets a shared repo put the file
somewhere untracked. The convention is a default, not a requirement.

**`CLAUDE_MEMORY_STORES` is not the seam and cannot replace the `MEMORY.md`
target.** Spiked in 2.1.263. Its `path` is an HTTP path on Anthropic's memory
service, not a filesystem path (`listBase = path + "/memories"`, host
`"memory"`, server errors like `content must be at most 102400 bytes`), so
ecphory cannot be mounted through it. Worse, setting it *at all* flips
`isMemoryRecallEnabled` — `PZe` returns true for any non-empty value, without
parsing — and `ZIe`/`tNe` then filter every `AutoMem` attachment out of
context, MEMORY.md included. It is a switch that would turn this target off,
not an alternative to it.

## Non-goals

- Deciding *which* episodes deserve a key. The tag is written by whoever
  stores the episode; this change renders what is tagged.
- A global (cross-workspace) index. `~/.codex/AGENTS.md` and the OpenCode
  equivalent are already symlinks to one ecphory-rendered file on the
  reference deployment; one file cannot hold per-workspace keys for
  concurrent sessions in different repos.
- New trigger machinery. The index is a render target on the existing
  post-write triggers — one trigger per workspace, matching its `ws:` tag.
- Reversing the slug. It is lossy by construction (`/a/b-c` and `/a/b/c` slug
  the same); the workspace path is always carried explicitly.
