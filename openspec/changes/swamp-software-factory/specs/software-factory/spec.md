## ADDED Requirements

### Requirement: swamp runs under the maintainer's own identity or not at all
`scripts/swamp` SHALL run swamp only when `SWAMP_CONFIG_DIR` is
`~/.config/swamp-personal` and `SWAMP_API_KEY` is non-empty. It SHALL remove
`SWAMP_API_KEY_FILE` and `SWAMP_SIGNIN_TOKEN`, and SHALL refuse
`--club-api-key-file`. Any of these could otherwise put another account's
credential ahead of the personal key.

#### Scenario: No identity loaded
- **WHEN** the wrapper runs with neither variable set and direnv loads nothing
- **THEN** it exits non-zero after at most one direnv hop and swamp is never
  invoked

#### Scenario: A key with the default config dir
- **WHEN** `SWAMP_API_KEY` is set but `SWAMP_CONFIG_DIR` is unset or
  `~/.config/swamp`
- **THEN** the wrapper refuses, so swamp never writes the personal identity into
  the other account's auth.json

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
