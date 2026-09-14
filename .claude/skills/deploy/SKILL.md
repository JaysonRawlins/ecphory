---
name: deploy
description: Deploy a locally built ecphory binary to the live launchd daemon on macOS. Use whenever a code change needs to reach the running service — after cargo build, when the user says "deploy", "redeploy", "ship it to the daemon", "restart ecphory with the new build", or when a live gate needs the new binary. Encodes the AMFI fresh-inode + re-sign sequence and the launchd gotchas that SIGKILL naive cp deploys.
metadata:
  version: 1.0.0
disable-model-invocation: false
---
# Deploy ecphory to the live daemon

The binary at `~/.local/bin/ecphory` is served by the LaunchAgent
`com.ecphory.server` (port 3491). macOS AMFI caches code-signing verdicts
per-inode and launchd is the strictest enforcement tier: a plain `cp` over
the old binary gets SIGKILLed (exit 137) even when shell execution works,
and after a crash-looped load even shell exec dies. Every step below exists
because skipping it has already failed once.

## Sequence

1. **Build + verify quality gates:**
   ```
   cargo test -q && cargo clippy --all-targets -q && cargo build --release
   ```
2. **Fresh-inode install (never cp-over-in-place):**
   ```
   rm ~/.local/bin/ecphory
   cp target/release/ecphory ~/.local/bin/ecphory
   xattr -c ~/.local/bin/ecphory
   codesign --force --sign - ~/.local/bin/ecphory
   ```
3. **Restart the daemon — the USER runs this** (Gavel blocks launchctl for
   the agent; hand them the line, single-line only):
   ```
   ! launchctl kickstart -k gui/$(id -u)/com.ecphory.server
   ```
4. **Verify from the client path** (never trust the restart silently):
   ```
   curl -s http://127.0.0.1:3491/health
   curl -s http://127.0.0.1:3491/api/v1/status
   claude mcp list | grep Ecphory   # expect ✔ Connected
   ```

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
- **Restarts are cheap for clients**: streamable HTTP MCP is stateless
  per-request; Claude Code sessions reconnect without a restart (unlike
  engram's stdio proxy).
- Full history: ecphory episodes `f7b2c73d` (runbook) and `1dc68bc4`
  (launchd lessons).
