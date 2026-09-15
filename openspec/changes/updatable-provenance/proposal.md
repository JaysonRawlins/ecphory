## Why

`source`, `source_model` and `source_description` were settable at write time
and unreachable afterwards. Every other episode field could be corrected; these
three could not, so a bad value was permanent for the life of the episode.

The value that made it concrete: episode `60fc62be` stores this as its `source`.

```
claude-code</source>\n<parameter name="source_model">opus-4.7
```

An Opus 4.6/4.7-era agent emitted the closing tag and the following parameter
block inside the string value, and the store took it verbatim — as designed,
since the store never rewrites what an agent writes. The intended values are
plainly recoverable from the text: `source: "claude-code"`,
`source_model: "opus-4.7"`. 26 of 1209 mirrored episodes carry the same shape,
all from one bounded era.

No route existed to fix it:

- `UpdateParams` carried `content`, `name`, `search_phrases`, `tags`,
  `expired_at`, `metadata`. The MCP tool, `PUT /memory/episodes/{id}` and
  `ecphory update` all route through it, so none could reach the source fields.
  The REST body did not even reject the attempt — serde ignores unknown fields,
  so a `PUT` naming `source` returned 200 and changed nothing.
- `/admin/import` is insert-only (`if self.store.get(..).is_ok() { continue }`),
  so repairing the mirror markdown and re-importing reports `already_present`.
- Delete and re-add mints a fresh UUIDv7, breaking every `[[wikilink]]` and id
  reference into the episode and discarding its version history and
  access/rating-log linkage. For an episode referenced by id from other
  episodes and from a MEMORY.md index, that cure is worse than the disease.

Scope of the harm, stated honestly: retrieval is unaffected. The tantivy schema
indexes `id`, `name`, `content`, `phrases`, `deleted` — `source` is not a field
in it. This is provenance metadata, and it matters for "which model wrote this"
analysis, not for recall.

The reason to do it is the general one rather than the 26 rows: a field that
can be written but never corrected is a sharp edge, and the next serialization
quirk will land on it too.

## What Changes

- `UpdateParams` gains `source`, `source_model` and `source_description`, and
  `Store::update` applies them, so they are archived into an `EpisodeVersion`
  and roll back like every other field.
- All three call surfaces expose them under the existing "empty means leave
  unchanged" rule: the REST `UpdateBody` (`none_if_empty`), the MCP
  `update_episode` tool schema (`opt_str`), and `ecphory update`
  (`--source`, `--source-model`, `--source-description`).
- The MCP tool description says provenance is updatable, so an agent holding a
  wrong value knows there is a way back.

## Non-goals

- Clearing a provenance field. Empty means "leave unchanged" across the whole
  update surface — that is how `name`, `tags` and `content` already behave, and
  a clear-vs-leave distinction is a separate change to all of them, not one
  field's exception.
- Validating or sanitising `source` at write time. The store takes content
  verbatim by design; the fix for a bad write is that it stays correctable, not
  that the store starts second-guessing its callers.
- Making `/admin/import` upsert. Insert-only is deliberate (it is what makes
  re-importing a mirror safe); the repair path is update, not import.
- Indexing `source`. It is not a retrieval field and this change does not make
  it one.
