## Why

Work on ecphory goes from issue to merged PR by hand, and the checks that make a
change trustworthy (a plan someone else tried to break, a suite that really ran,
a live probe, specs reconciled and archived) happen when someone remembers
them. The swamp software factory makes them stages of a state machine: a stage
cannot advance until its gate is satisfied, and the run's record is data rather
than a transcript.

exman runs this factory already. ecphory takes its shape and maps each stage to
ecphory's own tooling, instead of copying exman's pnpm, CDK and attestation
machinery.

## What Changes

- **`ecphory-factory`** (`models/@swamp/software-factory/ecphory-factory.yaml`):
  intake from a GitHub issue -> OpenSpec planning -> plan verify -> plan refute
  (at most twice) -> human implementation approval -> implementing -> tasks
  verify -> pre-PR verify -> adversarial review -> live verify -> reconcile ->
  archive -> archive verify -> final pre-PR verify -> open PR -> in review ->
  merged.
- **Four workflows** under `workflows/` and the `command/shell` models they run.
  pre-pr-verify runs the suite with its totals, clippy with warnings denied and
  strict spec validation, against a clean checkout of exactly the named commit.
- **Gate scripts** under `scripts/factory/`: tasks complete, change archived and
  committed, and cargo test totals that fail on zero tests.
- **`scripts/swamp`** and `.envrc.example`: swamp runs under the maintainer's own
  swamp-club identity, with the key read from Doppler through direnv, or not at
  all.
- **Tests**: `factory_definition`, `factory_gates`, `factory_test_counts` and
  `factory_swamp_wrapper` in `cargo test`. CI installs the openspec CLI.

## Impact

- No change to the binary, its store format or its MCP, REST or CLI surfaces.
- New dev-dependency `serde_yaml_ng` (tests only).
- CI's test job installs `@fission-ai/openspec`.
