# daemon-first-log-views Specification

## Purpose
Make the read-only recorder views (`heals`, `ratings`, `search-log`,
`access-log`) usable while `ecphory serve` holds the store. redb's lock is
process-exclusive, so a direct store open fails for as long as the daemon runs;
these views read through the daemon when one answers and through the store when
none does.

## Requirements
### Requirement: Read-only views prefer a reachable daemon
`heals`, `ratings`, `search-log` and `access-log` SHALL resolve a base URL from
`--url`, then `$ECPHORY_URL`, then `http://127.0.0.1:3491`, probe it with
`GET /api/v1/status`, and read their rows over HTTP when it answers. They SHALL
NOT open the store in that case.

#### Scenario: Daemon running
- **WHEN** `ecphory serve` holds the store and `ecphory heals` runs
- **THEN** the resolution rows print and the command exits 0, where before it
  exited 1 with "Database already open. Cannot acquire lock."

#### Scenario: Every view, not just heals
- **WHEN** the daemon is up and `ratings`, `search-log` or `access-log` runs
- **THEN** each prints its rows and exits 0

### Requirement: The rows are the same either way
The HTTP path SHALL decode the canonical recorder entries — `ResolutionLogEntry`,
`RatingLogEntry`, `SearchLogEntry`, `AccessLogEntry` — not a narrower projection,
so rendered output does not depend on which source answered.

#### Scenario: Byte-identical output
- **WHEN** the same store is read once through a running daemon and once
  directly with that daemon stopped
- **THEN** all four commands produce identical output

#### Scenario: Replay outcome survives the wire
- **WHEN** a resolution carries a `last_replay` outcome and `heals` reads it
  over HTTP
- **THEN** the replay verdict is present in the decoded row, because a flatter
  report shape (`HealEntry`) would drop it silently

### Requirement: Reads fall back to the store when nothing answers
Unlike `eval`, these views SHALL NOT be HTTP-only: when the probe fails they
SHALL open the store as before.

#### Scenario: No daemon
- **WHEN** nothing listens on the resolved base URL
- **THEN** the rows are read from the store and the command exits 0

### Requirement: An explicit --db pins the source
When `--db` is given the view SHALL read that store and SHALL NOT consult a
daemon, so a reachable daemon can never answer for a store the operator did not
ask about.

#### Scenario: Daemon serving a different store
- **WHEN** a daemon serving store A is running and `ecphory --db B search-log`
  runs, with `--url` pointing at that daemon
- **THEN** the rows come from B, and A's rows do not appear

### Requirement: The source is disclosed
Each read SHALL report its source on stderr — `(reading from daemon <url>)` or
`(reading from store <path>)` — leaving stdout to the rows alone.

#### Scenario: Piping is unaffected
- **WHEN** a view's stdout is redirected
- **THEN** only the rows are captured, and the source line goes to stderr

### Requirement: --url is global and backward compatible
`--url` SHALL be accepted for any subcommand, before or after it. `eval`'s
existing invocation form SHALL keep working, and `eval` SHALL remain HTTP-only.

#### Scenario: Pre-change eval invocation
- **WHEN** `ecphory eval --gold <file> --url <base>` runs
- **THEN** it behaves as it did before this change

#### Scenario: eval never falls back
- **WHEN** `eval` runs with nothing listening on the base URL
- **THEN** it fails with a connection error and does not open the store

