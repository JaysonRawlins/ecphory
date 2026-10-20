# software-factory Specification

## Purpose
Take a change from GitHub issue to merged PR through gates that check the tree
rather than the agent's account of it: a plan someone else tried to break, a
suite that really ran, a live probe that never touches the daemon, and specs
reconciled and archived before the PR opens. A run belongs to the maintainer's
own swamp-club identity, which the session carries, and no workflow value is
ever parsed by a shell.
## Requirements
### Requirement: The factory refutes and approves a plan before implementation
The factory SHALL verify the OpenSpec change strictly, then have one reviewer
who did not write it record findings, at most twice per run. A human SHALL
approve `implementation-approval` on that cycle's findings before
implementing starts.

#### Scenario: Nothing skips the refute
- **WHEN** openspec-plan-verify passes
- **THEN** the only way forward is plan-refute, and plan-refute reaches
  implementing only with fresh findings and the human approval

### Requirement: Verification checks the tree, not the agent's account
Each verification stage SHALL be a workflow whose pass depends on a script's
exit code. Tasks verify requires every task ticked. Archive verify requires the
change folder gone, one archived copy, nothing uncommitted under `openspec/`,
and specs valid under `--strict`. pre-PR verify requires HEAD to equal the
named commit with a clean tree, a suite that ran at least one test with none
failing, clippy with warnings denied, and strict specs.

#### Scenario: A suite that ran nothing
- **WHEN** `cargo test` exits 0 having run zero tests
- **THEN** the test step exits non-zero and pre-pr-verify fails

#### Scenario: An archive that was never committed
- **WHEN** `openspec archive` ran but its result is uncommitted
- **THEN** openspec-archive-verify fails and the run returns to archive

### Requirement: Live verification never touches the live daemon
live-verify SHALL resolve every acceptance criterion as `static`, with the test
that proves it, or as `live`, with an identifier a probe produced. That probe
SHALL run against a scratch `ecphory serve` on a temp store and a port other
than 3491, with the export, triggers and db environment variables unset.

#### Scenario: A live criterion
- **WHEN** a criterion needs a running binary
- **THEN** the evidence names a search_id or episode id from the scratch
  instance, and the daemon's episode count is unchanged

### Requirement: The PR opens only after reconcile, archive and a final verify
Only final-pre-pr-verify SHALL lead to open-pr. It runs on the archive commit,
after reconcile has found no open blocking finding. The PR body SHALL end with
the Co-Authored-By and Reviewed-By trailers and SHALL carry no AI attribution
footer. The run SHALL end when the PR merges through the merge-pr skill.

#### Scenario: Live verify cannot open a PR
- **WHEN** live-verify passes
- **THEN** it leads to reconcile, never to open-pr

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

