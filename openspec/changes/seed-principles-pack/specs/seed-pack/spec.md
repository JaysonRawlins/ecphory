# seed-pack Specification

## ADDED Requirements

### Requirement: Seeding is idempotent and never overwrites user state
The system SHALL install the bundled principle pack through the idempotent import path, skipping any episode id that already exists, so that re-seeding after an upgrade adds only new pack episodes and never reverts a user's edits or restores a demoted episode.

#### Scenario: Re-seed after user edits
- **WHEN** a user has edited or demoted seeded episodes and runs seed again
- **THEN** their edited content and demotions are untouched, and only pack episodes absent from the store are added

### Requirement: Seeded episodes are ordinary episodes
Seeded episodes SHALL carry no special status: they are versioned, editable, demotable, and purgeable exactly like user-created episodes, and SHALL be identifiable as a group only by their `seed-pack` tag.

#### Scenario: User removes the whole pack
- **WHEN** a user demotes every episode carrying the `seed-pack` tag
- **THEN** the store behaves as if never seeded, and no feature degrades

### Requirement: The pack demonstrates the rendered-artifact loop
The pack SHALL include one canonical principles episode tagged `rendered-artifact`, a triggers config template, and a render template, such that after setup an edit to that episode from any client regenerates the rendered file without a manual step.

#### Scenario: Edit from any client re-renders
- **WHEN** the seeded principles episode is updated via MCP, REST, or CLI
- **THEN** the post-write trigger runs the render and the rendered file's content reflects the edit
