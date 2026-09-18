## 1. Implementation

- [x] 1.1 MCP `AddMemoryRequest`: all three provenance fields describe
      themselves. `source` says one name, never a `system/model` compound,
      and `"unknown"` rather than blank; `source_model` says the model alone,
      as the harness reports it, context-window marker included;
      `source_description` says free text naming the session or run.
- [x] 1.2 MCP `UpdateEpisodeRequest`: the same convention on the repair path,
      keeping the existing "empty means leave unchanged" sentence. A repair
      surface that does not state the convention is how a repair reintroduces
      what it is fixing.
- [x] 1.3 REST `AddMemoryBody`: the same three, and `source` records that it
      defaults to `"http"` when omitted — the REST path already cannot store
      an empty `source`, which is worth saying next to the field rather than
      leaving in the handler.
- [x] 1.4 CLI: `ecphory add --source` and `ecphory update --source /
      --source-model` carry the convention in their `--help` text.
- [x] 1.5 README: a "Provenance: who wrote this" section — the three fields as
      a table, the two rules, and why none of it is validated on write.
- [x] 1.6 CHANGELOG entry under Unreleased → Other, saying plainly that this
      is documentation plus a one-off repair of the reference deployment, not
      a behaviour change.
- [x] 1.7 No production code changed. Every edit is a doc comment or a
      `#[arg]` help string; `cargo test` is unchanged at 112 passing.

## 2. Red proof (record what was broken and what it printed)

- [x] 2.1 The convention was written as an executable audit *before* anything
      was changed, and run against the live store — three predicates, one per
      shape: `source == ""`, `source` contains `/`, `source_model ==
      "opus-4-7"`. Red, 2026-09-17:

          scanned            1260 episodes (include_deleted)
            A empty source   11
            B slash compound 12
            C hybrid model   28
          NON-CONFORMING     48 (distinct episodes)
          => RED (exit 1)

      51 hits over 48 distinct episodes: 3 of the slash rows also carry the
      hybrid spelling in `source_model`.
- [x] 2.2 The REST listing cannot reach expired episodes (1260 of 1273), so
      that blind spot was closed before trusting the number rather than after.
      The same three predicates over the git mirror — which exports every
      episode including expired and demoted — returned **the same 48**, and
      the two id sets were diffed rather than the two counts compared:
      `mirror: 48  rest: 48` / `IDENTICAL SETS`. No expired episode is
      affected.
- [x] 2.3 Both non-goals were proved on a throwaway store (port 3497, debug
      binary), not assumed. A REST write with `source:
      "claude-code/opus-4-7"` returned 201 and stored that value verbatim —
      provenance is genuinely unvalidated on write — and a subsequent `PUT`
      naming only the two fields repaired it to `claude-code` / `opus-4.7`.
      A mirror file carrying `source: ""` imported as `source: ""`: import
      restores faithfully and does not quietly improve its input.
- [x] 2.4 The descriptions are *served*, not merely typed. `tools/list`
      against the real MCP endpoint returns a description on all three
      provenance properties of `add_memory` and all three of
      `update_episode`. This is the surface the proposal names as the only
      spec an agent will ever read, so it is checked where a client reads it
      rather than where it is written. Following #18, the prose itself is not
      string-matched in a test.

## 3. The repair — 48 episodes

- [x] 3.1 Rollback net established first. The scheduled export had already
      caught the mirror up to all 1273 episodes, so `d97955a` is a committed,
      clean, **already-pushed** snapshot of every pre-repair value; a forced
      export confirmed it with `unchanged: 1273, written: 0`. Second layer:
      every write archives an `EpisodeVersion`, individually reversible via
      `restore_episode_version`.
- [x] 3.2 Checked that the repair fires no side effects: none of the 48
      carries the `rendered-artifact` tag, which is the only matcher in the
      deployment's `triggers.json`. 48 update events, 0 trigger runs.
- [x] 3.3 Recovery rules, one per shape, dry-run before apply:
      - **split compound (12)** — `source` takes the part before the slash,
        `source_model` takes the existing value or the part after. All 12
        land on `claude-code` / `opus-4.7`. The one row spelled
        `claude-code/opus-4.7` already had the right `source_model`, so only
        its `source` changed.
      - **infer from source_model (3)** — an empty `source` on a row that
        carries a `source_model` *and* a `source_description` naming the
        session (`"claude-gavel session 'telegram-prompt' ..."`,
        `"Plan-mode /propose session ..."`) becomes `claude-code`. The
        evidence is on the record; nothing is read out of the content.
      - **no evidence -> unknown (8)** — an empty `source` with no
        `source_model`, no `source_description` and no tags becomes
        `unknown`. Deliberately not inferred from content, which is where
        invention would start.
      - **fold opus-4-7 -> opus-4.7 (25)** — the standalone hybrid spelling.
        `claude-opus-4-7` (20 rows, the API id) is left alone.
- [x] 3.4 Applied over REST against the running daemon: `repaired 48,
      mismatched 0`, each write verified against the episode the store
      returned rather than against the request that was sent.

## 4. Verify

- [x] 4.1 `cargo fmt --check` clean, `cargo clippy --all-targets` exit 0 with
      zero output, `cargo test` 112 passed (107 unit + 5 integration) —
      unchanged from main, as it should be for a change that adds no
      production code.
- [x] 4.2 `openspec validate provenance-conventions --strict` passes.
- [x] 4.3 The audit from 2.1, re-run unchanged, is green:
      `A 0 / B 0 / C 0 / NON-CONFORMING 0 => GREEN (exit 0)`.
- [x] 4.4 A fresh provenance census confirms the arithmetic rather than
      trusting the exit code: `claude-code`/`opus-4.7` 105 -> 142 (+37 = 25
      folds + 12 splits), `claude-code`/`opus-4.8` 117 -> 120 (+3 inferred),
      `unknown` 0 -> 8, and `""`, `claude-code/*` and `opus-4-7` are gone.
      `claude-opus-4-7` still sits at 20 and `claude-opus-5[1m]` at 14,
      untouched — the spec says those are correct, so a repair that had
      "tidied" them would have been the bug.
- [x] 4.5 Third confirmation from a code path that shares nothing with the
      repair script: the mirror export reports `written: 48, unchanged:
      1225`, and the mirror re-scan finds 0 non-conforming files across all
      1273. Post-repair mirror commit `15fefb8`, child of the rollback point.
- [x] 4.6 Spot-checked the per-episode net: `f952ebf9`, `e8ea96a6`,
      `d8cd4646` and `f7faca85` each carry an archived version whose
      `operation` is `update` and whose snapshot holds the pre-repair
      provenance (`""`, `"claude-code/opus-4-7"`/`"opus-4-7"`, `""`/
      `"opus-4.8"`, `"opus-4-7"`).
- [x] 4.7 Store healthy after 48 updates and 48 index upserts: `status` still
      reports 1273 episodes, search still returns results.
