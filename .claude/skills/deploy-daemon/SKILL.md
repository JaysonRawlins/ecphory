---
name: deploy-daemon
description: Deploy the local ecphory daemon from source and verify it BEHAVIORALLY. Use whenever a change should go live on the running daemon (after a merge, "restart the daemon", "put this live", "bounce ecphory"), or when diagnosing whether the running daemon actually contains a change. The core rule — version strings lie, behavioral probes don't — exists because a fresh install once reported the right-looking version while running fix-less code.
metadata:
  version: 1.1.0
disable-model-invocation: false
---

# deploy-daemon — build, install, restart, verify behaviorally

The daemon is a LaunchAgent (`com.ecphory.server`) running `~/.local/bin/ecphory serve`
on port 3491, db at `~/.local/share/ecphory/ecphory.redb`.

## Steps

1. **Confirm what you're building.** `git log --oneline -3` and verify the change
   you want is an ancestor of HEAD (`git merge-base --is-ancestor <sha> HEAD`).
   Building the wrong branch is how a fix-less daemon shipped on 2026-07-13:
   the local checkout was on a pre-fix feature branch, and the resulting binary
   *reported* a plausible version. Release TAGS can also predate a fix —
   release-plz cuts the release PR from where main was when it opened.
2. **Build + test:** `cargo test --quiet && cargo build --release`
3. **Install with a fresh inode** (never overwrite in place — macOS vnode
   signature cache SIGKILLs on the next spawn for signed binaries; rm-then-cp
   is the safe habit regardless):
   `rm ~/.local/bin/ecphory && cp target/release/ecphory ~/.local/bin/ecphory`
4. **Restart:** `launchctl kickstart -k gui/$(id -u)/com.ecphory.server`
   then confirm a NEW pid: `ps aux | grep "ecphory serve" | grep -v grep`
5. **Verify behaviorally — this is the load-bearing step:**
   - `ecphory --version` (necessary, not sufficient)
   - **The floor gate (machine).** Exits non-zero below the floor:
     `ecphory eval --gold ~/.local/share/ecphory/gold.jsonl --min-mrr 0.85`
     It answers exactly one question — *is the right code live?* — and it is
     loose on purpose so it cannot fire on corpus growth.
   - **The drift read (human).** Compare that run against the LAST ROW of
     [`docs/eval-trail.md`](../../../docs/eval-trail.md), then append your
     reading as a new row, in the same commit as the deploy.
     Note what is deliberately absent here: a baseline number. One used to
     live in this step — "as of v0.3.3: 3 misses, MRR 0.928, hit@5 98.9%" —
     and by 2026-09 it reported a regression on every deploy, because the gold
     set is fixed at 275 pairs while the corpus grew ~480 episodes and misses
     accumulate from crowding alone. **Do not re-add a number here.** The trail
     file is the baseline precisely because it gets appended to and this one
     does not.
   - **Index freshness — the eval does NOT cover this.** Since #37 the index
     converges on a document-count comparison instead of rebuilding on every
     open, so a genuinely drifted index can persist where the old accidental
     rebuild would have repaired it. The cheap probe: search for an episode
     written TODAY and confirm it ranks.
   - `ecphory eval --heals` — record held/regressed against the trail's second
     table. A non-zero exit here is NOT a deploy blocker; a heal can regress
     because a better sibling was written later. The trail says which.
   - If the change touched a specific behavior, probe THAT PATH over HTTP
     (port 3491) — e.g. the self-correction loop has a zero-pollution smoke:
     rate a rank-1 search as miss with its own id in intended_episode_ids and
     expect `already_ranks` with phrases unchanged.

## Gotchas

- The `ecphory` CLI can't read the db while the daemon holds the lock
  ("Database already open") — use the HTTP API (`/api/v1/memory/...`) for
  anything against the live store.
- MCP clients connected before the restart don't see newly added tools until
  they reconnect.
- Restarting drops every active session's memory connection for a moment —
  fine, they reconnect; just don't bounce mid-write.
- The gold set is NOT in this repo. It lives at
  `~/.local/share/ecphory/gold.jsonl` in a single unversioned copy — see
  "Known gaps" in [`docs/eval-trail.md`](../../../docs/eval-trail.md).
