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

v0.2: canonical store, tantivy BM25 search with write-time phrase boosting,
flight recorder, MCP daemon (stdio + streamable HTTP), REST mirror, eval
harness (gold-set + used-signal), git mirror export/import. Running in
production as the author's daily-driver agent memory since 2026-07-12.

## Lineage

The lessons here were learned operating a fork of
[OscillateLabsLLC/engram](https://github.com/OscillateLabsLLC/engram) (Mike
Gray's memory server — thank you, Mike) as a daily-driver memory system for
Claude Code, instrumented and evaluated over months. ecphory is a clean-room
reimplementation of those published lessons in Rust, under a name from the
same scientist's vocabulary: the engram is the trace; ecphory is the recall.

## License

MIT
