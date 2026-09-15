## Why

`rate_search` takes two lists of episode ids, and they are not the same kind of
thing. `used_episode_ids` says what the caller relied on — telemetry.
`intended_episode_ids` says what should have surfaced — a lever that appends the
failed query to those episodes' `search_phrases`. For a while the union of the
two was fed to `self_correct`, so any non-hit rating quietly enriched whatever
the caller had merely used (issue #18).

It cost a real episode. Rating `019f808f` on search `019f808e` (2026-07-20) was
a `partial` with no intended ids at all, three used ids passed purely to record
what the answer came from:

```json
{"action":"enriched","after_rank":1,"episode_id":"711f734a...","displaced_used":["019f63c5-f0e0..."]}
```

`711f734a` is about audience-provenance checks before outward communications. It
gained the phrase `"flight recorder tape contains private names before
publishing public artifact"` — a question about recorder internals — and in
doing so displaced `019f63c5-f0e0`, which was the right answer and which prior
ratings had already marked used. Reverted by hand with `update_episode`.

This is the inference `self_correct` already refuses to make from access joins,
for the reason written at src/recorder.rs: *enriching a wrongly-guessed target
would bury the right one behind it*. The union made it at the API boundary
instead.

**The behaviour is already correct.** `f3e7304` ("separate rating telemetry and
add version rollback", shipped in 0.3.6) narrowed the target list to
`intended_episode_ids` and added
`used_episode_ids_never_trigger_self_correction`. What did not land is the rest
of what the issue asked for: the contract was never written down. The MCP tool
description still said only *"include used_episode_ids for the results you
actually relied on"*, which is the sentence that made the union look reasonable
in the first place, and the guarantee had no coverage at the wire — where a
client sends both fields in one body, and where the live incident came through.

An unwritten guarantee is one refactor from being re-litigated.

## What Changes

- **The contract is stated at each place it is read.** The MCP `rate_search`
  description and its `used_episode_ids` schema field, the MCP server
  instructions, the REST `RateSearchBody` fields, and the README's
  self-correction section now all say the same two things: used ids are never
  edited, and `intended_episode_ids` is the only field that changes an episode.
  The MCP text matters most — for an agent client it is not documentation
  *about* the API, it is the only spec of it that will ever be read.
- **A wire-level regression test**,
  `used_ids_are_telemetry_over_the_wire_even_on_a_partial`, drives the real REST
  endpoint with the incident's own shape: a `partial` carrying used ids only,
  then a `partial` carrying both. Its rig puts the used episode at rank 6 —
  outside the correction window (k = 5), inside the returned page (limit = 10) —
  which is the one position where a union does not shrug `already_ranks` but
  actually appends a phrase. Staged red, it reproduces the incident's log line.
- **The behaviour becomes a spec**, so the next person to read
  `used_episode_ids` in a signature finds out what it is for without excavating
  a commit from August.

No production code changes. The guard being specified already exists at
src/service.rs and is proved load-bearing below.

## Non-goals

- **Resolutions for partial-driven heals.** A `partial` with real intended ids
  enriches but logs no resolution, so it is invisible to `ecphory heals` and
  never replayed by `eval --heals`. The issue raised it and then withdrew it:
  resolutions track the miss lifecycle, and a partial was never *outstanding*,
  so there is nothing to mark healed
  (`partial_heal_runs_corrections_but_records_no_resolution`). Deliberate, and
  left alone.
- **Specifying the whole self-correction capability.** The enrich → redo →
  validate loop, the phrase cap, `CorrectionAction`, and the heal replay pass
  all deserve a backfilled spec; that is a bigger excavation than this issue,
  and guessing at it from the outside is how specs start lying. This spec claims
  only the rating contract it can prove.
- **Asserting the doc strings in a test.** The served MCP description is
  reviewed, not string-matched: a test that pins prose fails on every rewording
  and catches nothing a reader would not.
