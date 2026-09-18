# provenance-conventions Specification

## Purpose
Say what belongs in `source`, `source_model` and `source_description`, and say
it where the agent filling them in will read it. Two of the three carried no
schema description at all, so the only guidance ever given was `source`'s
"Originating system" — and the store drifted into two conventions for one fact:
12 episodes folded the model into `source` with a slash, 11 left `source`
empty, and 28 spelled a model `opus-4-7`, which is neither the API id nor the
human short form. Retrieval never noticed, because `source` is not in the
tantivy schema; what broke is the one question these fields exist to answer.

The store cannot enforce any of this — axiom 3, it takes what the edge gives
it — so the convention's only real enforcement is that it is legible at the
moment of the write, and its counterweight is that all three fields stay
correctable afterwards.

## Requirements

### Requirement: Each provenance field describes itself at the write surface
`source`, `source_model` and `source_description` SHALL each carry a
description on every schema an agent reads before writing — the MCP
`add_memory` and `update_episode` input schemas, the REST add body, and the
`ecphory add` / `ecphory update` flags. An undescribed field is not a field
with an obvious meaning; it is a field each caller will define differently.
For an agent client this text is not documentation *about* the API, it is the
only spec of it that will ever be read.

#### Scenario: The MCP schema carries all three
- **WHEN** a client lists tools
- **THEN** `add_memory`'s input schema carries a description for `source`, `source_model` and `source_description`, and `update_episode`'s does too

#### Scenario: The operator surfaces agree with the agent surface
- **WHEN** `ecphory add --help` or `ecphory update --help` is read
- **THEN** the provenance flags state the same convention as the MCP schema, so a repair driven from a terminal cannot reintroduce the shape a repair exists to fix

### Requirement: `source` names the writing system alone
`source` SHALL hold the agent, harness or tool that captured the episode, as a
single name. It SHALL NOT be a `system/model` compound, because folding the
model in makes a grouping over either field undercount. It SHALL NOT be empty:
where the writing system is genuinely unknown the value SHALL be the literal
`unknown`, which a query can count and group where `""` is silently skipped.

#### Scenario: The model does not belong in `source`
- **WHEN** an episode is written by claude-code running Opus 4.7
- **THEN** `source` is `claude-code` and the model is in `source_model`, so a grouping over `source` counts one system rather than one system-model pair

#### Scenario: Unknown is a value, not a blank
- **WHEN** provenance is repaired on an episode whose writing system cannot be established from anything on the record
- **THEN** `source` becomes `unknown` rather than staying `""`, and the episode is counted as unattributed rather than dropped from the count

#### Scenario: A tool is a writing system
- **WHEN** a skill such as `teachme` captures an episode under its own name
- **THEN** that is a conforming `source`, because the convention names the system that did the writing and does not require it to be the outermost harness

### Requirement: `source_model` names the model alone, as reported
`source_model` SHALL hold the model identifier and nothing else, recorded as
the writing harness reports it, including a context-window marker where the
harness emits one. A marker such as `[1m]` is a fact about the write that no
later reader can recover from anywhere else, so two spellings that differ by
one SHALL NOT be treated as the same value needing reconciliation.

#### Scenario: A context-window marker survives
- **WHEN** a harness reports its model as `claude-opus-5[1m]`
- **THEN** that is the stored `source_model`, and its coexistence with `claude-opus-5` in the same corpus is not a violation of this spec

### Requirement: Provenance is not validated on write
Every write surface SHALL accept the provenance an agent sends, conforming or
not, and store it verbatim. This is axiom 3 applied to provenance: the
counterweight to an unguarded write is that all three fields stay correctable
for the life of the episode, archived and reversible, not a gate at the door
that has to be right about every harness that has not shipped yet.

#### Scenario: A non-conforming write is still accepted
- **WHEN** an episode is written with `source` set to a `system/model` compound
- **THEN** the write succeeds and stores that value verbatim, and the episode is repairable afterwards through the ordinary update path

#### Scenario: Import restores faithfully rather than correcting
- **WHEN** a mirror file carrying an empty `source` is imported
- **THEN** the empty `source` is restored as written, because a restore that quietly improves its input cannot be used to verify a backup or to roll one back

