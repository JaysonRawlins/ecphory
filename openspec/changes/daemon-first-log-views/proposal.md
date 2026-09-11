## Why

redb's lock is process-exclusive. `heals`, `ratings`, `search-log` and
`access-log` open the store directly, so on any machine where `ecphory serve`
stays up — which is the reference deployment, and the intended way to run it —
all four are permanently broken:

```
$ ecphory heals
Error: storage error: Database already open. Cannot acquire lock.
```

Hit live 2026-07-14, minutes after the v0.3.4 deploy, and still reproducing on
2026-09-10 against 0.3.5 (issue #15). These are the instruments for the heal
lifecycle: the one moment you want to read the resolution log is while the
daemon is up and healing. `eval` already solved this by being HTTP-only.

## What Changes

- The four read-only recorder views ask the daemon first and fall back to
  opening the store only when nothing answers. Unlike `eval` they are NOT
  HTTP-only: reading a store with no daemon running is legitimate.
- `--url` is promoted to a global flag (was `eval`-only), defaulting to
  `$ECPHORY_URL` then `http://127.0.0.1:3491`. `ecphory eval --url X` keeps
  working; the flag is now also accepted before the subcommand.
- An explicit `--db` pins the read to that store and skips HTTP entirely.
  Silently reading a *different* store because some daemon answered is a worse
  failure than the lock error this change exists to remove.
- Every read announces its source on stderr — `(reading from daemon <url>)` or
  `(reading from store <path>)` — so the answer is never ambiguous. stderr, so
  piping the rows is unaffected.
- `EvalClient`'s log readers become generic in the row type, and gain
  `resolution_log`. `eval` keeps its narrow projections; the CLI decodes the
  canonical recorder entries. One fetch, two views.

## Non-goals

- Making the views HTTP-only like `eval`. Offline reads are a real use.
- Writing through the daemon. These four only read; `purge` and friends keep
  their direct-store, CLI-only design.
- Detecting that a reachable daemon serves a *different* store than
  `$ECPHORY_DB` names. `/status` does not report a db path, and adding one
  would couple the CLI to a daemon version. Disclosure on stderr is the
  guarantee here, not verification.
