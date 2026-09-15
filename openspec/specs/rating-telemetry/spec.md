# rating-telemetry Specification

## Purpose
`rate_search` takes two lists of episode ids that look alike and are not.
`used_episode_ids` records what the caller relied on; `intended_episode_ids`
nominates what should have surfaced, which appends the failed query to those
episodes' `search_phrases`. Only the second is a lever. Feeding the union of the
two to self-correction — as the store did until `f3e7304` — turns consumption
telemetry into a write: a `partial` carrying no intended ids at all enriched an
episode about audience-provenance checks with a query about flight-recorder
internals, and displaced the better answer out of the top k. That is the
inference `self_correct` already refuses to make from access joins, for the
reason kept beside it: enriching a wrongly-guessed target buries the right one
behind it. This capability pins the split — what each field means, that used ids
are never edited by any rating, that they keep their read-only jobs as the
retrieval-quality signal and as the protected set behind `displaced_used`, and
that every surface accepting a rating says which of the two writes.

## Requirements
### Requirement: A rating never edits an episode it only used
`rate_search` SHALL treat `used_episode_ids` as consumption telemetry: it SHALL
record them verbatim on the rating and SHALL NOT pass them to self-correction,
for any rating value. The self-correction targets for a non-`hit` rating SHALL
be exactly the ids given in `intended_episode_ids`, deduplicated in the order
supplied. A `hit` SHALL run no corrections at all, whatever it carries. This
holds identically on every surface — MCP, REST, and any client added later —
because all of them enter through the one `rate_search` on the service.

#### Scenario: A partial carrying used ids only
- **WHEN** a `partial` is rated with `used_episode_ids` and no `intended_episode_ids`
- **THEN** the rating records no corrections and no episode's `search_phrases` changes, including for an episode that ranked outside the correction window and would therefore have been enriched had it been a target

#### Scenario: Both fields in one call
- **WHEN** a non-`hit` rating carries a used id and a different intended id
- **THEN** exactly one correction is applied, against the intended id, and the used episode is left untouched

#### Scenario: A hit
- **WHEN** a search is rated `hit`
- **THEN** no correction runs, regardless of which ids the call carries

### Requirement: Used ids keep their two read-only jobs
Recording used ids SHALL continue to serve the two purposes that make them worth
collecting, neither of which mutates an episode: they are the retrieval-quality
signal read back off the rating log, and they are the protected set for the
collateral-damage check, so that a heal displacing a previously used episode out
of the top k is flagged on the correction as `displaced_used`. Ids MAY be unique
prefixes rather than full UUIDs, and SHALL be matched by prefix wherever they
are read, the same as everywhere else ids travel.

#### Scenario: The signal survives the round trip
- **WHEN** a rating carrying used ids is read back from the rating log
- **THEN** the ids come back exactly as supplied, prefixes included

#### Scenario: A heal displaces something previously used
- **WHEN** enriching an intended target pushes an episode that a prior rating marked used out of the top k
- **THEN** the correction names it in `displaced_used` — a warning, never a rollback

### Requirement: The client-facing contract names the field that mutates
Every surface that accepts a rating SHALL state, where the fields are described,
that used ids are never edited and that `intended_episode_ids` is the only field
that changes an episode. For MCP clients this is not documentation about the API
but the whole of the API as an agent will ever read it: the tool description and
the `used_episode_ids` schema field are the specification that reaches the
caller, so a description that says only *"include the results you relied on"*
SHALL be treated as incomplete.

#### Scenario: An agent reads the tool list
- **WHEN** an MCP client inspects `rate_search` before its first rating
- **THEN** the description and field docs tell it which of the two id fields is inert and which one writes

#### Scenario: A reader follows the risk to its reason
- **WHEN** the caller wants to know why passing a doubtful id as intended is discouraged
- **THEN** the surfaces give the reason rather than only the rule: a wrongly guessed target, once enriched, buries the right one behind it

