---
name: deploy-daemon
description: Deploy a locally built ecphory binary to the live launchd daemon on macOS and verify it BEHAVIORALLY. Use whenever a change needs to reach the running service — after a merge or cargo build, when the user says "deploy", "redeploy", "ship it to the daemon", "put this live", "bounce ecphory", "restart ecphory with the new build" — or when diagnosing whether the running daemon actually contains a change. Encodes the AMFI fresh-inode + re-sign sequence, the launchd gotchas that SIGKILL a naive cp deploy, and the core rule: version strings lie, behavioral probes don't.
metadata:
  version: 2.0.0
disable-model-invocation: false
---

# deploy-daemon — build, install, restart, verify behaviorally

The daemon is a LaunchAgent (`com.ecphory.server`) running `~/.local/bin/ecphory serve`
on port 3491, db at `~/.local/share/ecphory/ecphory.redb`.

macOS AMFI caches code-signing verdicts per-inode and launchd is the strictest
enforcement tier: a plain `cp` over the old binary gets SIGKILLed (exit 137)
even when shell execution still works, and after a crash-looped load even shell
exec dies. Every step below exists because skipping it has already failed once.

## Steps

1. **Confirm what you're building.** `git log --oneline -3` and verify the change
   you want is an ancestor of HEAD (`git merge-base --is-ancestor <sha> HEAD`).
   Building the wrong branch is how a fix-less daemon shipped on 2026-07-13:
   the local checkout was on a pre-fix feature branch, and the resulting binary
   *reported* a plausible version. Release TAGS can also predate a fix —
   release-plz cuts the release PR from where main was when it opened.
2. **Build + verify quality gates:**
   ```
   cargo test -q && cargo clippy --all-targets -q && cargo build --release
   ```
3. **Fresh-inode install (never cp-over-in-place):**
   ```
   rm ~/.local/bin/ecphory
   cp target/release/ecphory ~/.local/bin/ecphory
   xattr -c ~/.local/bin/ecphory
   codesign --force --sign - ~/.local/bin/ecphory
   ```
   The `rm` is what gets a new inode, so AMFI re-evaluates instead of serving a
   cached verdict for the old one. `xattr -c` drops quarantine and any stale
   signing xattrs; the ad-hoc re-sign then gives the new inode a verdict launchd
   will accept.
4. **Restart the daemon,** then confirm a NEW pid:
   ```
   launchctl kickstart -k gui/$(id -u)/com.ecphory.server
   ps aux | grep "ecphory serve" | grep -v grep
   ```
   Run this yourself — it is not gated away from the agent. Placet's only
   hardcoded launchctl deny is scoped to `(bootout|unload|remove)` against
   Placet's *own* LaunchAgent, which a kickstart on `com.ecphory.server` cannot
   match. An earlier copy of this runbook claimed otherwise and cost a
   round-trip through a human on every deploy;
   `tests/deploy_skill_singular.rs` now fails if that claim comes back.

   Then check the client path before trusting the restart:
   ```
   curl -s http://127.0.0.1:3491/health
   curl -s http://127.0.0.1:3491/api/v1/status
   claude mcp list | grep Ecphory   # expect ✔ Connected
   ```
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

## Gotchas (each one has bitten)

- **Plist env changes need a full reload, not kickstart**: `kickstart -k`
  reuses the loaded job definition. After editing the plist:
  `bootout` → pause → `bootstrap` (separate single-line commands). Verify
  what the process actually got with `ps eww <pid>`.
- **KeepAlive respawn tests lie after crash loops**: launchd applies
  escalating backoff per label; a `pkill` test that shows no respawn is the
  penalty box, not a config bug. `kickstart` (no `-k`) forces a spawn.
- **The real log is `serve.err`** — tracing writes to stderr; `serve.log`
  (stdout) stays empty.
- **`codesign --verify` passing proves nothing** about the launchd tier.
- **The `ecphory` CLI can't read the db while the daemon holds the lock**
  ("Database already open") — use the HTTP API (`/api/v1/memory/...`) for
  anything against the live store.
- **Restarts are cheap for clients**: streamable HTTP MCP is stateless
  per-request, so sessions reconnect on their own (unlike engram's stdio
  proxy). It does drop every active memory connection for a moment — fine,
  just don't bounce mid-write. MCP clients connected before the restart won't
  see newly added *tools* until they reconnect.
- **The gold set is NOT in this repo.** It lives at
  `~/.local/share/ecphory/gold.jsonl` in a single unversioned copy — see
  "Known gaps" in [`docs/eval-trail.md`](../../../docs/eval-trail.md).
- Full history: ecphory episodes `f7b2c73d` (runbook) and `1dc68bc4`
  (launchd lessons).
