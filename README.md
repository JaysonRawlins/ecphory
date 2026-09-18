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

## Provenance: who wrote this

Three fields ride along with every episode, and they answer three different
questions:

| field | holds | examples |
| --- | --- | --- |
| `source` | the writing **system** — agent, harness or tool | `claude-code`, `codex`, `teachme` |
| `source_model` | the **model**, alone | `claude-opus-5`, `claude-opus-5[1m]`, `gpt-5` |
| `source_description` | free text: the session, task or run | `"issue #41 repair, 2026-09-17"` |

One name in `source`, never a `system/model` compound: folding the model in is
what makes a later `GROUP BY source_model` quietly undercount. Never blank
either — when the writing system genuinely isn't known, the honest value is the
literal `unknown`, because a query can count `unknown` and cannot count `""`.
Record the model as your harness reports it, context-window marker included:
`claude-opus-5[1m]` was true at write time, and nothing downstream can recover
that afterwards.

None of this is enforced on write. Axiom 3 — the server stores what the edge
gives it — applies to provenance as much as to content, so the convention is
written where an agent actually reads it (the MCP tool schema) rather than
checked in a validator. The counterweight is that all three fields stay
correctable for the life of the episode, archived and reversible like every
other field.

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

winget packaging is submitted and awaiting review
([winget-pkgs#436333](https://github.com/microsoft/winget-pkgs/pull/436333));
until it merges, the portable exe works today from the Release zip. See
[docs/packaging/winget](docs/packaging/winget/).

**cargo-binstall / from source:**

```sh
cargo binstall ecphory   # prebuilt, no compile
cargo install ecphory --locked   # compiles from source
```

Manual download on macOS: clear quarantine before first run
(`xattr -c ecphory`) — Homebrew and the installer script handle this for
you. Binaries are ad-hoc signed; notarization is future work.

See [docs/RELEASING.md](docs/RELEASING.md) for how releases are cut.

## Connect an agent

Installing the binary gets you nothing until an agent can reach it. There are
two transports, and one rule decides which you want: **redb's lock is
process-exclusive, so exactly one process may hold the store.**

- `ecphory serve` — MCP over streamable HTTP at `http://127.0.0.1:3491/mcp`.
  One daemon owns the store and every client dials the URL. This is what you
  want the moment there is more than one agent, or one agent and the CLI.
- `ecphory mcp` — MCP over stdio. The client spawns the binary and *that*
  process opens the store, so it is single-client by construction.

Mixing them fails loudly, which is the merciful case: start a stdio client
while the daemon is up and it exits with `Error: storage error: Database
already open. Cannot acquire lock.` Run the daemon and point everything at
the URL, or run exactly one stdio client and no daemon.

### 1. Start the daemon

```sh
ecphory serve                           # 127.0.0.1:3491, foreground
curl -s http://127.0.0.1:3491/health    # {"status":"healthy"}
```

The store is `$ECPHORY_DB`, else `~/.local/share/ecphory/ecphory.redb` —
never relative to the working directory, because a server started from the
wrong place would otherwise silently create a second empty store. The daemon
binds loopback only.

### 2. Wire the client

**Claude Code** — one command:

```sh
claude mcp add --transport http --scope user ecphory http://127.0.0.1:3491/mcp
```

or, equivalently, by hand in `~/.claude.json`:

```json
{
  "mcpServers": {
    "ecphory": { "type": "http", "url": "http://127.0.0.1:3491/mcp" }
  }
}
```

**Codex** — `~/.codex/config.toml`:

```toml
[mcp_servers.ecphory]
url = "http://127.0.0.1:3491/mcp"
```

**Claude Desktop** — `claude_desktop_config.json` (macOS:
`~/Library/Application Support/Claude/`) launches a command rather than
dialing a URL, so its entry is the stdio one — which means it must be the
only process holding the store, so stop the daemon first. Give it an
absolute path: a GUI app does not inherit your shell's `PATH`.

```json
{
  "mcpServers": {
    "ecphory": {
      "command": "/Users/you/.local/bin/ecphory",
      "args": ["mcp"],
      "env": { "ECPHORY_DB": "/Users/you/.local/share/ecphory/ecphory.redb" }
    }
  }
}
```

Any other client follows the same split: if it accepts a URL, hand it
`http://127.0.0.1:3491/mcp`; if it only spawns a command, hand it
`ecphory mcp` and let nothing else hold the store.

### 3. Confirm the connection

The daemon answers `tools/list` without a session handshake, so a single
curl proves the whole path end to end:

```sh
curl -s -X POST http://127.0.0.1:3491/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

A healthy daemon returns 11 tools: `add_memory`, `search`, `rate_search`,
`get_episode`, `get_episodes`, `get_status`, `update_episode`,
`delete_episode`, `restore_episode`, `get_episode_versions`,
`restore_episode_version`.

Client-side, Claude Code's `/mcp` lists ecphory as connected. The real
end-to-end check is to ask the agent to store something and then search it
back — and then, because the store's whole thesis is measured recall, to
rate that search.

One CLI surprise worth knowing before it bites: while the daemon holds the
lock, most `ecphory` subcommands cannot open the store and fail with that
same `Database already open`. The recorder views (`search-log`, `access-log`,
`ratings`, `heals`), `render-index` and `eval` read through the daemon over
HTTP instead, and anything else against a live store goes through REST —
`curl http://127.0.0.1:3491/api/v1/status` is the running daemon's answer to
`ecphory status`.

Connecting makes the store *reachable*. Making it *findable* is the subject
index above: `ecphory render-index` puts the recall keys in front of the
agent, so it knows what to ask for.

### 4. Keep it running

macOS, as a LaunchAgent at
`~/Library/LaunchAgents/com.ecphory.server.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.ecphory.server</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/you/.local/bin/ecphory</string>
    <string>serve</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>ECPHORY_DB</key>
    <string>/Users/you/.local/share/ecphory/ecphory.redb</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardErrorPath</key>
  <string>/Users/you/.local/share/ecphory/serve.err</string>
</dict>
</plist>
```

Load it with `launchctl bootstrap gui/$(id -u) <plist>`, restart it after an
upgrade with `launchctl kickstart -k gui/$(id -u)/com.ecphory.server`.

Linux, as a systemd user unit
(`~/.config/systemd/user/ecphory.service`):

```ini
[Service]
ExecStart=%h/.local/bin/ecphory serve
Restart=always

[Install]
WantedBy=default.target
```

Then `systemctl --user enable --now ecphory`.

### Configuration

Everything is environment variables, read by the daemon at startup:

| Variable | Default | Effect |
| --- | --- | --- |
| `ECPHORY_DB` | `~/.local/share/ecphory/ecphory.redb` | Store path. Never resolved against the cwd. |
| `ECPHORY_PORT` | `3491` | Port for `serve`. Loopback only. |
| `ECPHORY_URL` | `http://127.0.0.1:3491` | Daemon the CLI's HTTP-backed commands dial. |
| `ECPHORY_AUTH_TOKEN` | unset (open) | Bearer token for the data plane, REST and MCP alike; `/health` stays open for probes. Loopback binding is the wall, this is depth for when the port gets forwarded. See [Security](#security). |
| `ECPHORY_HIDDEN_GROUPS` | unset | Comma-separated groups an unscoped search skips; naming one in `group_id` opts back in. |
| `ECPHORY_EXPORT_DIR` | unset (no mirror) | Git mirror directory. Set it and the daemon exports on a schedule. See [docs/backups.md](docs/backups.md). |
| `ECPHORY_EXPORT_INTERVAL` | `24h` | How often that scheduled export runs. |
| `ECPHORY_EXPORT_PUSH` | on | `false`/`0`/`off` to commit the mirror without pushing it. |
| `ECPHORY_TRIGGERS_FILE` | unset | Post-write trigger definitions. See [docs/triggers.md](docs/triggers.md). |
| `ECPHORY_SEARCH_LOG` | on | `off`/`false`/`0`/`disabled` turns the flight recorder off — which turns off every measurement this project exists for. |
| `ECPHORY_SEARCH_LOG_RETENTION` | `90` (days) | Recorder prune horizon. Heal resolutions are exempt. |
## Security

The daemon binds `127.0.0.1`, and only `127.0.0.1`. That is hard-coded at the
listener rather than offered as a flag or an environment variable — `--port`
and `$ECPHORY_PORT` move the port, nothing moves the address — so there is no
configuration of ecphory that serves your memories to a network. The loopback
binding is the boundary, and it is what is actually protecting the store.

Inside that boundary the data plane is open by default: anything that can reach
the port can read, write and delete episodes. That is deliberate for a
single-user local daemon. A process running as you can already open the store
file directly, so a gate on the port would not be keeping it out of anything.

Set `ECPHORY_AUTH_TOKEN` to require a bearer token on top of that:

```sh
ECPHORY_AUTH_TOKEN=$(openssl rand -hex 32) ecphory serve
```

It covers both surfaces, REST and MCP. `/health` is the single exception, left
open so a supervisor can probe a daemon it holds no token for. The CLI reads
the same variable, so the commands that go through the daemon — `search-log`,
`access-log`, `ratings`, `heals`, `eval`, `render-index` — keep working with no
extra flag, and an MCP client sends `Authorization: Bearer <token>` like any
other client.

Where the client gets that token from is worth a thought, because a token
pasted into a config file is a plaintext secret sitting next to the thing it
protects. If your MCP client can generate headers by running a command, use
that instead: Claude Code's `headersHelper` runs on every connection and merges
its output into the request headers, so the value can come from your secret
manager at connect time and never be written to a file at all.

Be honest with yourself about whether you want it at all. On a single-user
machine whose port is never forwarded, the token protects against nobody — a
process running as you can open the store file directly, token or no token. The
case it exists for is the port ceasing to be loopback-only: an SSH tunnel, a
container port map, a VM forward. It is what stands between a reachable port
and your memories in that situation, which is different from being a reason to
put it in one.

At rest the store is an ordinary unencrypted file protected by filesystem
permissions, and turning on the git mirror (`ECPHORY_EXPORT_DIR`) writes your
episodes to that repository in plaintext, where they travel with it.
[SECURITY.md](SECURITY.md) has the full threat model and how to report a
vulnerability privately.

## Status

v0.3.7. Canonical store, tantivy BM25 search with write-time phrase boosting,
flight recorder, MCP daemon (stdio + streamable HTTP), REST mirror, eval
harness (gold-set, used-signal, and `--from-log` against the real workload),
git mirror export/import. Since v0.3.3 the recorder closes the loop: explicit
search ratings drive the self-correction cycle described above, validated
heals are kept as permanent regression tests (`eval --heals`), and
tape entries carry an `origin` tag so eval and backfill sweeps stay out of the
organic workload statistics. Two-phase deletion (agent demote, operator purge)
landed in v0.3.4. v0.3.7 added the subject index, made provenance
correctable after the fact, and converged the search index on a document
count instead of a wildcard probe that read every index as empty — which
took `ecphory search` on a 900-episode store from 390 ms to 54 ms
([#37](https://github.com/JaysonRawlins/ecphory/pull/37)). See
[CHANGELOG.md](CHANGELOG.md).

Running in production as the author's daily-driver agent memory since
2026-07-12.

## Lineage

The lessons here were learned operating a fork of
[OscillateLabsLLC/engram](https://github.com/OscillateLabsLLC/engram) (Mike
Gray's memory server — thank you, Mike) as a daily-driver memory system for
Claude Code, instrumented and evaluated over months. ecphory is a clean-room
reimplementation of those published lessons in Rust, under a name from the
same scientist's vocabulary: the engram is the trace; ecphory is the recall.

## License

MIT
