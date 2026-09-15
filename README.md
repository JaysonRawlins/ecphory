# ecphory

> **ecphory** (n.) — the retrieval of a memory: reactivation of a stored
> trace by a cue. Coined by Richard Semon in the same 1904 work that gave us
> *engram*; revived by Endel Tulving. Pronounced "EK-fuh-ree."

**Recall, measured.** A single-binary, lexical-first memory store for AI
agents, written in Rust.

No model dependencies. No sidecar daemons. No vector database. Every
architectural choice here traces to a measured retrieval experiment on a
production agent-memory workload — most centrally: at personal-corpus scale,
BM25 over well-curated, verbatim episodes carries recall, and embeddings were
not load-bearing. So the lexical path is the foundation, and anything else
must earn its way in through the flight recorder.

## Design axioms

1. **Lexical-first.** Full-text (BM25) search over content, names, and
   write-time `search_phrases`. Embeddings are an optional future add-on,
   admitted only if this system's own query log proves a miss class that
   lexical can't cover.
2. **The flight recorder is core.** Every search and every by-id fetch is
   logged from the first migration. `eval --from-log` scores retrieval
   against the *real* workload, not a hand-built gold set.
3. **Verbatim storage; intelligence at the edge.** The server never rewrites
   content. The capturing agent — which holds full context — supplies
   paraphrase search cues at write time.
4. **Curation over accumulation.** Agents demote; they never destroy. Every
   mutation archives the prior state. Operator purge tiers content into git
   before it leaves the hot store.
5. **One static binary.** Pure-Rust stack (redb + tantivy + axum). No
   extension downloads at startup, no embedding daemon to silently fail, no
   cwd-relative database paths.

## Self-correction: search → rate → heal

![The search, rate, heal cycle: a query misses, the miss is rated with the
episode that should have surfaced, the failed query becomes that episode's next
search phrase, and the same query then ranks.](docs/assets/heal-cycle.gif)

Retrieval failures are the most valuable signal the store produces, so they are
captured rather than discarded. An agent rates a search by its `search_id`
(`hit` / `partial` / `miss`); on a miss it passes `intended_episode_ids` — the
episode it *should* have found, discovered by some other route.

That triggers self-correction. The mechanism is deliberately dumb: **the failed
query is appended verbatim to the target episode's `search_phrases`.** No model
call, no paraphrasing, exactly one phrase per heal — the premise being that the
query *is* how that episode will be asked for again. The `phrases` field carries
a 2.0× BM25 boost, so a single appended phrase is a large lexical signal for
exactly the wording that failed. The search then re-runs to validate, and
whether it worked is recorded rather than assumed:
`Enriched` (target now in the top 5), `AlreadyRanks` (it was already there — a
stale rating), `EnrichedStillLow` (the gap is crowding, not vocabulary),
`DuplicatePhrase`, or `PhraseCapReached`.

Two guardrails keep this from degrading into lexical spam. `search_phrases` is
capped at 8 per episode — unbounded miss-driven growth would turn a much-missed
episode into lexical mass that crowds out its siblings. And a collateral check
warns when a heal displaces an episode that prior ratings marked as *used*.

**Used is signal, intended is the lever.** `used_episode_ids` travels in the
same call and is the opposite kind of thing: it is recorded, read back as the
retrieval-quality signal, used as the protected set above — and never edited,
on any rating. Only an id passed as *intended* can change an episode. Guessing
a target from what merely got used is the same inference the store already
refuses to make from access joins, for the same reason: a wrongly guessed
target, once enriched, buries the right one behind it.

**Ratings are never mutated.** Healing an old miss mints a *new* rating rather
than flipping the original: first-contact failure rate is an acceptance metric,
and rewriting a miss into a hit would be cooking it. Each validated heal instead
writes a `resolution_log` row, which is **exempt from the 90-day recorder
prune** — every healed miss becomes a permanent regression test. `ecphory eval
--heals` replays them all and exits non-zero if any heal has regressed.

## Deletion story

Deletion is two-phase, and the phases have different owners:

1. **Demote** — agent-safe, recoverable. `delete_episode` (MCP), `DELETE`
   (REST), and `ecphory demote` soft-delete: the episode is hidden from
   search and listings but still resolvable by id, and `restore` undoes it.
   The prior state is archived first, so nothing an agent can do destroys
   content.
2. **Purge** — operator-only, destructive. `ecphory purge <id>...` is a CLI
   command with no MCP or REST equivalent: agents may demote; only a human
   at a terminal destroys. It refuses episodes that are not already demoted
   (there is no `--force`), and it is a dry run by default — it prints a
   manifest of everything that would be destroyed and executes only with
   `--yes`. On execute it removes the episode record, its entire archived
   version history, its search-index entries, and its file in the git
   mirror (when `ECPHORY_EXPORT_DIR` is set); the mirror removal is
   committed when the mirror is already a git repository, matching how the
   daemon's scheduled export commits. The daemon holds the store lock, so
   stop it before purging.

Version rollback is separate from deletion restore. `restore_episode` and
`ecphory restore` only reverse demotion. To roll back an update, list snapshots
with `get_episode_versions` / `ecphory versions`, then restore one with
`restore_episode_version` / `ecphory rollback <id> <version-id>`. Rollback
restores the snapshot exactly and archives the displaced current state first,
so the rollback is itself reversible.

**What purge cannot do** — the honest limits, for the leak-response case:
flight-recorder rows that reference a purged id survive (they hold ids,
ranks, and scores only — no content — and age out with recorder retention),
and the git mirror's *history* still contains the episode content even
after the file's removal is committed. If a purge is a response to leaked
secrets, rewrite the mirror history with
[git filter-repo](https://github.com/newren/git-filter-repo)
(`git filter-repo --invert-paths --path <group_id>/<episode_id>.md`),
force-push any remotes that carried it, and rotate the leaked credentials
anyway — assume anything that ever reached a remote was read.

## Subject index

Reachable is not the same as findable. Claude Code injects a per-project
`MEMORY.md` of search keys, and that index is what makes recall happen — an
agent holding it knows *what to ask for*. Codex, agy and OpenCode read
`AGENTS.md`, get the same MCP access and none of the index, so on the same
repo they start behind.

`ecphory render-index` renders a workspace's keys — query phrase, tags,
description, episode id, never episode bodies — into both files, inside a
marked region that leaves hand-written content alone:

```sh
ecphory workspace-key          # ws:-Users-me-code-myrepo — tag episodes with this
ecphory render-index           # writes AGENTS.md + Claude's MEMORY.md
ecphory render-index --check   # exit 1 if a target is stale
```

The workspace key is the git toplevel, resolved through `--git-common-dir` so
a linked worktree and its main checkout share one index. Rendering is
deterministic and reads the daemon over HTTP, so it is safe to hang off a
[post-write trigger](docs/triggers.md). See
[docs/subject-index.md](docs/subject-index.md).

## Install

Prebuilt binaries for macOS (arm64, x86_64), Linux (static musl — arm64,
x86_64, runs on any distro back to ~2014), and Windows (x86_64) ship with
every [GitHub Release](https://github.com/JaysonRawlins/ecphory/releases).

**Homebrew (macOS/Linux):**

```sh
brew install jaysonrawlins/tap/ecphory
```

**Shell one-liner (macOS/Linux):**

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/JaysonRawlins/ecphory/releases/latest/download/ecphory-installer.sh | sh
```

**Windows (PowerShell):**

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/JaysonRawlins/ecphory/releases/latest/download/ecphory-installer.ps1 | iex"
```

winget packaging is planned (portable exe works today from the Release zip).

**cargo-binstall / from source:**

```sh
cargo binstall ecphory   # prebuilt, no compile
cargo install ecphory --locked   # compiles from source
```

Manual download on macOS: clear quarantine before first run
(`xattr -c ecphory`) — Homebrew and the installer script handle this for
you. Binaries are ad-hoc signed; notarization is future work.

See [docs/RELEASING.md](docs/RELEASING.md) for how releases are cut.

## Status

v0.3.5. Canonical store, tantivy BM25 search with write-time phrase boosting,
flight recorder, MCP daemon (stdio + streamable HTTP), REST mirror, eval
harness (gold-set, used-signal, and `--from-log` against the real workload),
git mirror export/import. Since v0.3.3 the recorder closes the loop: explicit
search ratings drive the self-correction cycle described above, validated
heals are kept as permanent regression tests (`eval --heals`), and
tape entries carry an `origin` tag so eval and backfill sweeps stay out of the
organic workload statistics. Two-phase deletion (agent demote, operator purge)
landed in v0.3.4.

Running in production as the author's daily-driver agent memory since
2026-07-12.

Known gaps: every open of a non-empty store reindexes it wholesale, because the
cold-start emptiness probe tokenizes to nothing and so reads every index as
empty ([#30](https://github.com/JaysonRawlins/ecphory/issues/30)). It is
invisible at the current corpus size, and that accidental rebuild is what
repairs index drift today, which makes it a two-part fix rather than a
one-liner.

## Lineage

The lessons here were learned operating a fork of
[OscillateLabsLLC/engram](https://github.com/OscillateLabsLLC/engram) (Mike
Gray's memory server — thank you, Mike) as a daily-driver memory system for
Claude Code, instrumented and evaluated over months. ecphory is a clean-room
reimplementation of those published lessons in Rust, under a name from the
same scientist's vocabulary: the engram is the trace; ecphory is the recall.

## License

MIT
