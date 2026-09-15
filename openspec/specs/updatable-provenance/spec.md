# updatable-provenance Specification

## Purpose
Keep an episode's provenance correctable for the life of the episode. The store
takes what an agent writes verbatim, which means it will sometimes take
something wrong; the counterweight is that nothing written is beyond repair
without destroying the episode's identity.

## Requirements

### Requirement: Provenance is updatable in place
`source`, `source_model` and `source_description` SHALL be changeable on an
existing episode through the same update path as every other field, without
minting a new id.

#### Scenario: A mangled source is repaired
- **WHEN** an episode whose `source` contains a serialization artifact is
  updated with the corrected `source` and `source_model`
- **THEN** the stored provenance is the corrected value, and the episode keeps
  its id, its content and its creation time

#### Scenario: Provenance survives an unrelated edit
- **WHEN** an episode is updated naming only `content`
- **THEN** its `source`, `source_model` and `source_description` are unchanged,
  because empty means "leave unchanged" here as it does for every other field

### Requirement: A provenance change is archived and reversible
An update that changes provenance SHALL archive the prior episode state as an
`EpisodeVersion`, and restoring that version SHALL restore the prior
provenance exactly.

#### Scenario: The wrong value is still recoverable
- **WHEN** provenance is corrected and the episode's versions are listed
- **THEN** the archived snapshot holds the pre-repair provenance

#### Scenario: A repair is rolled back
- **WHEN** the archived version is restored
- **THEN** the episode's provenance matches the snapshot field for field

### Requirement: Every update surface reaches provenance
The three provenance fields SHALL be settable from the MCP `update_episode`
tool, `PUT /memory/episodes/{id}`, and `ecphory update`, so a repair does not
depend on which client is at hand — notably not on one that can open the store
directly, since a running daemon holds redb's process-exclusive lock.

#### Scenario: The MCP tool advertises them
- **WHEN** a client lists tools
- **THEN** `update_episode`'s input schema carries `source`, `source_model` and
  `source_description`

#### Scenario: Repair over REST against a running daemon
- **WHEN** a `PUT` names only provenance fields
- **THEN** the response carries the corrected provenance, where before the same
  request returned 200 and changed nothing

