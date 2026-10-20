## REMOVED Requirements

### Requirement: swamp runs under the maintainer's own identity or not at all
**Reason**: The repo no longer carries the identity. `scripts/swamp` and the
committed `.envrc` are removed; the fallback they guarded against no longer
exists, since the employer login left swamp's default config dir.
**Migration**: Start the session under `doppler run -p swamp -c personal --`,
or prefix each `swamp` call with it. The intake identity check below replaces
the wrapper's refusal.

## ADDED Requirements

### Requirement: A run stops at intake unless swamp is the maintainer's account
The intake stage SHALL run `swamp auth whoami --json` before reading the issue,
and SHALL stop and tell the operator to start the session under
`doppler run -p swamp -c personal` unless it is authenticated as
`jaysonjrawlins`. `.swamp.yaml` SHALL enroll no AI tool, so swamp writes no
agent hook that runs swamp under whatever identity a shell has.

#### Scenario: A session started without the personal identity
- **WHEN** a run's intake stage executes in a session not started under
  `doppler run -p swamp -c personal`
- **THEN** `swamp auth whoami` reports no personal login and the stage stops
  before reading the issue

#### Scenario: swamp writes no agent hook
- **WHEN** `swamp repo init` or `swamp repo upgrade` runs in this repo
- **THEN** it writes no `.claude/settings.local.json`, because `.swamp.yaml`
  enrolls no AI tool

### Requirement: No workflow value is parsed by a shell
Every value a workflow passes to a shell command SHALL arrive through the
step's `env` and be used only double-quoted; no `run:` SHALL contain `${{`.
Each OpenSpec gate script SHALL exit 2 on a change slug that is not
`[a-z0-9][a-z0-9-]*`.

#### Scenario: An injected change slug
- **WHEN** openspec-plan-verify, openspec-tasks-verify or
  openspec-archive-verify runs with `changeSlug="x; touch <marker>"`
- **THEN** the gate script exits 2 with "not a change slug", the workflow
  fails, and no marker is created

#### Scenario: An injected commit
- **WHEN** pre-pr-verify runs with a `commit` that is not HEAD's sha
- **THEN** the head step fails and the value is never parsed by the shell
