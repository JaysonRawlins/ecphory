# harness-install Specification

## ADDED Requirements

### Requirement: Static inspection never reports delivery
`ecphory doctor` SHALL classify each detected harness as exactly one of
`DELIVERED`, `UNPROVEN`, or `MISCONFIGURED`, and SHALL NOT emit `DELIVERED` on
the basis of static inspection alone. A harness whose configuration passes every
static check but has not been live-verified in this run SHALL report `UNPROVEN`.
Four of the five known silent-no-op modes pass static inspection, so a config
that "looks right" is not evidence that text reaches the model.

#### Scenario: Config present, no live run
- **WHEN** a harness adapter is correctly installed and `doctor` runs without the live tier
- **THEN** that harness reports `UNPROVEN`, never `DELIVERED` or `OK`

#### Scenario: Adapter absent
- **WHEN** no ecphory adapter is found for a detected harness
- **THEN** that harness reports `MISCONFIGURED` naming the missing adapter

### Requirement: The live tier proves delivery with a canary
`ecphory doctor --live` SHALL, for each detected harness, render a single-use
canary token into that harness's adapter, invoke the harness non-interactively
with an unrelated prompt, and report `DELIVERED` only if the canary appears in
the reply. The canary SHALL be freshly random per run so a stale artifact cannot
produce a false positive, and the adapter SHALL be restored afterwards whether
or not the check succeeded.

#### Scenario: Canary echoed
- **WHEN** the live tier runs against a correctly wired harness
- **THEN** that harness reports `DELIVERED` and its adapter is left byte-identical to its pre-check state

#### Scenario: Harness invocation fails
- **WHEN** the harness binary is missing, unauthenticated, or times out
- **THEN** that harness reports `UNPROVEN` with the reason, and is never reported `DELIVERED`

### Requirement: Each known silent no-op is detected
`doctor` SHALL detect and name each of the five measured failure modes rather
than reporting a generic failure: a copilot hook emitting non-JSON; a codex hook
in `config.toml` instead of `hooks.json`; a codex hook lacking persisted hook
trust; a claude `@import` whose path is absolute or `~`-rooted; and a copilot
`AGENTS.md` installed outside a project directory. Each of these installs
cleanly and injects nothing.

#### Scenario: copilot hook emits plain text
- **WHEN** the copilot `sessionStart` hook command writes plain text to stdout
- **THEN** doctor reports `MISCONFIGURED` naming the JSON requirement, not a generic hook error

#### Scenario: claude import path is not relative
- **WHEN** a `CLAUDE.md` managed by ecphory contains an `@import` with an absolute or `~` path
- **THEN** doctor reports `MISCONFIGURED` naming the relative-path constraint

### Requirement: Install wires detected harnesses to one artifact
`ecphory install` SHALL detect which of the supported harnesses are present and
wire each to the single rendered context artifact using that harness's adapter:
a pointer where the harness reads an absolute path, a managed block delimited by
ecphory begin/end markers where it does not, and a hook where no file rail
reaches global scope. Re-running `install` SHALL be idempotent.

#### Scenario: Second install run
- **WHEN** `install` runs twice against the same machine with no intervening change
- **THEN** the second run makes no modification and reports each harness already wired

#### Scenario: Harness absent
- **WHEN** a supported harness is not installed
- **THEN** it is skipped and named as skipped, and no config file is created for it

### Requirement: Install is reversible byte-identical
`ecphory uninstall` SHALL restore every configuration file it modified to its
exact pre-install bytes. `install` SHALL refuse to modify a file it cannot first
back up.

#### Scenario: Round trip
- **WHEN** `install` then `uninstall` runs against a machine with pre-existing harness configs
- **THEN** every touched file's sha256 equals its pre-install sha256

#### Scenario: Backup not possible
- **WHEN** a target config cannot be read or backed up
- **THEN** `install` skips that harness with an explicit error and modifies nothing for it
