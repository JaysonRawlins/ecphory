# Post-write triggers

Run a local command when a matching episode changes. The store is the one
layer every client shares — MCP, REST, CLI — so an artifact *derived* from an
episode (a rendered context file, an exported doc) gets regenerated on the
write itself, instead of relying on each client remembering a second manual
step.

The motivating incident: a canonical episode was edited, the render step was
skipped, and the rendered file served three days stale while its own metadata
claimed otherwise. Per-client hooks can't close that class of drift; a new
client (or a rollback path) always ships without one. The store can.

## Configuration

Set `ECPHORY_TRIGGERS_FILE` to the path of a JSON file:

```json
{
  "triggers": [
    {
      "name": "render-session-context",
      "events": ["update", "restore", "restore_version"],
      "match": { "tags_any": ["rendered-artifact"] },
      "run": ["/Users/me/.claude/scripts/render-session-context.sh"],
      "timeout_seconds": 60
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `name` | Label used in logs. |
| `events` | Which write events fire it: `insert`, `update`, `demote`, `restore`, `restore_version`. Omit for the default set (`update`, `restore`, `restore_version`) — the three that change an existing episode's content. |
| `match.tags_any` | Fires when the episode carries any of these tags. |
| `match.id_prefix` | Fires when the episode id starts with this prefix. |
| `run` | argv — absolute program path plus args. |
| `timeout_seconds` | Kill the command after this long (default 60). |

A trigger must match on *something*: an empty `match` is a config error, not
match-all — a command running on every write in the store is never what anyone
meant. Unknown event names and relative program paths are also rejected at
load.

The command receives `ECPHORY_TRIGGER_NAME`, `ECPHORY_TRIGGER_EVENT`, and
`ECPHORY_TRIGGER_EPISODE_ID` in its environment.

## Semantics

- **Fire-and-forget.** A trigger can never fail, slow down, or roll back the
  write that fired it. Failures are logged (`tracing` at warn) and that's all.
- **Serialized per trigger.** Overlapping fires of the same trigger queue
  rather than race — renders are idempotent, but two concurrent writers to one
  output file are not. Different triggers run in parallel.
- **Loaded at open.** Every entry point fires triggers — the HTTP/MCP server
  and the CLI alike — because the engine is attached where the store is
  opened, not wired per caller.
- **Fail-open on bad config, loudly.** A malformed triggers file logs an ERROR
  and disables the feature rather than refusing to serve: memory availability
  outranks a derived artifact, and a crash-looping service takes every agent
  down with it. Watch the serve log after editing the config.

## Security posture

Commands come only from the local config file the operator owns. Episode
content, tags, and metadata never influence *what* runs — only *whether* a
configured command runs. Don't point `run` at anything that interprets episode
content as instructions.
