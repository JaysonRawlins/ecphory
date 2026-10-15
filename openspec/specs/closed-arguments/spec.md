# closed-arguments Specification

## Purpose
Make a caller's mistake visible instead of letting it do something else. The
store takes what an agent writes verbatim, so the surfaces that take its
arguments must not quietly reinterpret them.

## Requirements

### Requirement: An argument the surface does not define is refused
Every MCP tool and every REST body and query SHALL refuse a field it does not
define, with an error naming the field, and SHALL change nothing. A dropped
field is a silent change of meaning, which is worse than a failed call because
the caller is told it succeeded.

#### Scenario: A guessed field on a write
- **WHEN** `update_episode` is called with `content` and a field it does not
  define
- **THEN** the call fails naming that field, and the episode's content is
  unchanged

#### Scenario: A stale field on a read
- **WHEN** `search` is called with engram's `search_mode`
- **THEN** the call fails rather than running a different search than the
  caller asked for

#### Scenario: The schema says so
- **WHEN** a client lists tools
- **THEN** every tool that takes arguments advertises
  `additionalProperties: false`

### Requirement: Content can be appended without resending it
`update_episode`, `PUT /memory/episodes/{id}` and `ecphory update` SHALL accept
an append mode that adds `content` after the stored content, separated by a
blank line, archived as one version like any other update. Replace stays the
default.

#### Scenario: An outcome is stamped onto a long episode
- **WHEN** an episode is updated with `content` and `mode: "append"`
- **THEN** its content is the original, a blank line, then the new text, and
  its versions hold the pre-append state

#### Scenario: A mode that cannot mean anything
- **WHEN** `mode` is neither `replace` nor `append`, or is `append` with no
  `content`
- **THEN** the update is refused and the episode is unchanged
