# harness-install Specification

## ADDED Requirements

### Requirement: Static inspection never reports delivery
`ecphory doctor` SHALL NOT emit `DELIVERED` on the basis of static inspection.
A harness that has not been live-verified in this run SHALL report `UNPROVEN`.
Four of the five known silent-no-op modes pass every static check, so a config
that parses is not evidence that text reached the model.

#### Scenario: Config present, no live run
- **WHEN** a harness adapter is correctly installed and `doctor` runs without the live tier
- **THEN** that harness reports `UNPROVEN`, never `DELIVERED` or `OK`

### Requirement: Absence of a known adapter is not a defect
Doctor verifies the OUTCOME, not the mechanism. When no ecphory-managed adapter
is found for a detected harness, doctor SHALL report `UNPROVEN` and SHALL NOT
report `MISCONFIGURED`, because delivery may be carried by a rail ecphory does
not own — a host daemon that injects into every harness, or an operator's own
render script. `MISCONFIGURED` SHALL be reserved for an ecphory-managed adapter
that is present and in a shape measured to inject nothing.

Where doctor recognises a plausible foreign rail — a file carrying the ecphory
generated-artifact header, or a configured instructions path that exists — it
SHALL name that rail in the `UNPROVEN` detail so the operator knows what the
live tier will be testing.

#### Scenario: No adapter, no other signal
- **WHEN** a detected harness has no ecphory-managed adapter and no recognisable artifact
- **THEN** it reports `UNPROVEN` noting that no adapter was found and that delivery is undetermined

#### Scenario: A foreign rail is already delivering
- **WHEN** a harness reads a file carrying the ecphory generated-artifact header, written by a script ecphory does not own
- **THEN** it reports `UNPROVEN` naming that artifact, and is never reported `MISCONFIGURED` for lacking an ecphory-installed adapter

### Requirement: The live tier proves delivery regardless of mechanism
`ecphory doctor --live` SHALL, for each detected harness, render a single-use
random canary into whatever rail currently reaches that harness, invoke the
harness non-interactively with an unrelated prompt, and report `DELIVERED` only
if the canary appears in the reply. If the tier runs and the canary does not
appear, it SHALL report `NOT_DELIVERED`, which is distinct from `UNPROVEN`. The
canary SHALL be freshly random per run so a stale artifact cannot produce a
false positive, and the rail SHALL be restored afterwards on both the success
and failure paths.

#### Scenario: Canary echoed
- **WHEN** the live tier runs against a harness whose context is reaching the model
- **THEN** it reports `DELIVERED` and the rail is left byte-identical to its pre-check state

#### Scenario: Canary absent
- **WHEN** the live tier runs to completion and the canary does not appear in the reply
- **THEN** it reports `NOT_DELIVERED`, not `UNPROVEN`

#### Scenario: Harness invocation fails
- **WHEN** the harness binary is missing, unauthenticated, or times out
- **THEN** it reports `UNPROVEN` with the reason, and is never reported `DELIVERED` or `NOT_DELIVERED`

### Requirement: Each known silent no-op is named
For an ecphory-managed adapter, doctor SHALL name the specific defect rather
than failing generically: a copilot hook emitting non-JSON; a codex hook in
`config.toml` instead of `hooks.json`; a claude `@import` whose path is absolute
or `~`-rooted; and an opencode instructions pointer to a path that does not
exist. Each of these installs cleanly and injects nothing.

#### Scenario: claude import path is not relative
- **WHEN** an ecphory-managed block in `CLAUDE.md` contains an `@import` with an absolute or `~` path
- **THEN** doctor reports `MISCONFIGURED` naming the relative-path constraint

#### Scenario: Unrelated mention of ecphory
- **WHEN** a harness config mentions ecphory outside any ecphory-managed adapter, such as a project trust entry naming a path
- **THEN** doctor SHALL NOT report a misconfigured adapter

### Requirement: Install defers to existing delivery
`ecphory install` SHALL wire an adapter only where delivery is not already
happening, and SHALL leave a harness untouched when the live tier reports
`DELIVERED`. Install is a convenience, not the authority; doctor is the
authority. Re-running `install` SHALL be idempotent.

#### Scenario: Another rail already delivers
- **WHEN** `install` runs against a harness that a foreign rail already delivers to
- **THEN** no config is modified for that harness and it is reported as already delivering

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
