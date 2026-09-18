# Retrieval eval trail

Every deploy verification needs something to compare a fresh reading against.
A number written into a skill, a README or a commit message is frozen the
moment it is written, while the corpus it was measured on keeps growing — so
the comparison drifts into a false alarm, and a check that cries wolf is a
check that gets ignored. This file is the comparison point instead. It is
appended to, so the newest row is always the current baseline.

**Compare a new reading against the LAST ROW of the table below, and nothing
else.** If a number quoted in a skill, a README, a commit message or a memory
episode disagrees with the last row here, this file wins.

## The trail

| Date | Version | Episodes | Pairs | hit@1 | hit@5 | MRR | Misses | p50 | Recorded in |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| — | v0.3.0 | — | — | — | — | 0.894 | 13 | — | ecphory `019f6dd1` |
| — | v0.3.3 | ~700 | 275 | — | 98.9% | 0.928 | 3 | — | ecphory `019f6dd1` |
| 2026-07-16 | v0.3.5-dev | 834 | 275 | 87.6% | 98.5% | 0.924 | 4 | 1.32ms | ecphory `019f6dd1` |
| 2026-09-17 | v0.3.8 +#53 +#56 | 1302 | 275 | 88.0% | 97.8% | 0.923 | 6 | 2.95ms | ecphory `01a0b29d-a202` |
| 2026-09-18 | v0.3.8 +#53 +#56 | 1317 | 275 | 88.0% | 97.8% | 0.923 | 6 | 2.05ms | this file |

The first two rows are undated because the habit of recording *when* a reading
was taken started at the day-5 checkpoint — which is most of the reason this
file exists. Their blank cells are genuinely unrecorded, not zero. The `Pairs`
value for v0.3.3 is recovered arithmetically rather than quoted: 3 misses at
hit@5 98.9% implies 275 pairs, matching every later row.

`Episodes` is the load-bearing column. The gold set is fixed at 275 pairs while
the corpus grows, so misses accumulate from crowding alone — more episodes
competing for the same five slots. A reading without its corpus size cannot be
compared to anything.

## What counts as a regression

The MRR series is **0.894 → 0.928 → 0.924 → 0.923 → 0.923**: flat across two
months and ~480 added episodes. Drift is on the order of 0.001. So:

- **A drop of ~0.01 or less, with the miss set unchanged, is crowding.** Record
  the row and move on. This is the normal reading, and treating it as a
  regression is the failure this file was written to stop.
- **The miss *identities* matter more than the miss count.** The misses are one
  long-running sibling-crowding cluster, not scattered failures. As of
  2026-09-18 the set is `30e10a15`, `de61fa78`, `0024dcac`, `3a0a2f35`,
  `e7afc614`, `019f5906`. Four of those (`de61fa78`, `3a0a2f35`, `e7afc614`,
  `019f5906`) have been missing since 2026-07-16; `30e10a15` and `0024dcac`
  joined since. A miss id you have not seen before is worth a look. A count
  that ticked up while the ids stayed in this set is not.
- **A drop past the floor below, or a miss set that changes wholesale, is a
  real signal** — most likely the wrong binary is live or the index has drifted.

Diagnosing these misses is not the goal. They are ranking problems caused by
near-identical siblings, and enrichment aimed at one would bury its neighbours,
so the self-correction loop correctly leaves them alone.

### Settling attribution when a reading does look genuinely worse

Roll back to the previous binary and re-run the same eval against the same
store. Same gold set, same corpus, one variable. It costs two extra daemon
bounces — each drops every agent's memory connection for a moment — which is
why it is the escalation and not the routine check.

## The floor gate

`ecphory eval --gold <file> --min-mrr <floor>` exits non-zero when overall MRR
falls below the floor. Use **0.85**.

That floor sits below the lowest reading ever recorded here (0.894, v0.3.0,
before write-time enrichment), so it cannot fire on crowding — at the observed
drift rate it has decades of headroom. It is deliberately not a drift detector.
It answers one question: *is the right code live?* Drift is the human read
against the last row; catastrophe is the machine read against the floor.

Red-proof, recorded here because a gate nobody has watched fail is not
evidence (observed 2026-09-18, 1317 episodes, MRR 0.923):

```
$ ecphory eval --gold ~/.local/share/ecphory/gold.jsonl --min-mrr 0.95
  overall      n=275  hit@1  88.0%  hit@k  97.8%  MRR 0.923
FAIL: MRR 0.923 < min 0.950
rc=1

$ ecphory eval --gold ~/.local/share/ecphory/gold.jsonl --min-mrr 0.85   # control
  overall      n=275  hit@1  88.0%  hit@k  97.8%  MRR 0.923
rc=0
```

The control is the same command with only the floor changed, so the floor and
nothing else produced the red.

## The heal-replay reading

`ecphory eval --heals` re-runs every healed miss against the live index and
exits non-zero if any healed episode no longer ranks. Record it alongside the
gold reading:

| Date | Heals | Held | Regressed |
| --- | --- | --- | --- |
| 2026-07-17 | 2 | 2 | 0 |
| 2026-09-18 | 48 | 47 | 1 |

**Do not treat a non-zero exit here as a deploy gate.** A heal can regress
because a *better* sibling was written later, which is the store working, not
failing. The 2026-09-18 regression is exactly that: the heal targeting
`01a0a278` now ranks nowhere in the top 8 for its query, because `bb835474` was
written since and scores 57.62 against it. Compare against the recorded row
instead: *more* regressions than the last reading is the signal.

## Adding a row

After a deploy, from the repo, against the live daemon:

```
ecphory eval --gold ~/.local/share/ecphory/gold.jsonl --min-mrr 0.85
curl -s http://127.0.0.1:3491/api/v1/status      # the episode count for the row
ecphory eval --heals                              # for the second table
```

Append one row to each table and commit it with the deploy. A row added in the
same commit as the change it verifies is the whole point; a trail updated
"later" is a trail that stops at whenever later stopped happening.

## What this file must not contain

**Aggregates and episode-id prefixes only. Never query text, episode names, or
verbatim eval output.**

The gold set is a private corpus — its queries name real clients, people and
ticket ids, and this repository is public. Episode ids are opaque and safe to
record, which is why every miss and heal above is tracked by id and never by
the text that found it.

## Known gaps

- **`gold.jsonl` is not version-controlled.** It lives at
  `~/.local/share/ecphory/gold.jsonl`, dated 2026-07-12, in exactly one copy on
  one disk. It is not in this repo, not in the mirror, and not covered by any
  of the three tiers in [backups.md](backups.md). Every number in the table
  above is measured against a fixture that a disk failure would end. Keeping it
  out of *this* repo is deliberate — see the section above — but "not here" and
  "nowhere" are different, and it is currently nowhere.
