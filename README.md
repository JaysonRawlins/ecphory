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

## Status

Early. M1 (canonical store: episodes, prefix-resolvable ids, version
archive, demote/restore, CLI) is done and tested. Next: tantivy search (M2),
flight recorder (M3), MCP server (M4).

## Lineage

The lessons here were learned operating a fork of
[OscillateLabsLLC/engram](https://github.com/OscillateLabsLLC/engram) (Mike
Gray's memory server — thank you, Mike) as a daily-driver memory system for
Claude Code, instrumented and evaluated over months. ecphory is a clean-room
reimplementation of those published lessons in Rust, under a name from the
same scientist's vocabulary: the engram is the trace; ecphory is the recall.

## License

MIT
