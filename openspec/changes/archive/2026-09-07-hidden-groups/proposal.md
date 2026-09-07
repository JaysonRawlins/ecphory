## Why

Default search is global on purpose: the operator guidance says not to
filter by group, so nothing is missed. That guarantee breaks the moment a
group is used as a structured store. Todo episodes are short, imperative and
built from common nouns ("Contact", "Review", "Update calendar"), so BM25
ranks them well against queries that have nothing to do with them. Observed
2026-09-03 on the reference deployment: `todo: Contact Nick the electrician`
ranked #2 for "Bear notes contacts export migration to Obsidian". Putting a
live todo list back into the store (issue #22) would make every session's
recall noisier.

## What Changes

- **Hidden groups**: `ECPHORY_HIDDEN_GROUPS=todos,other` (comma-separated),
  read once at open. Unscoped searches (no `group_id`) skip episodes whose
  group is hidden. A search that names a `group_id` behaves exactly as today,
  including when it names a hidden group. Hidden means "opt in by naming",
  never "invisible".
- Applies uniformly because it lives in `search_impl`, which MCP, REST and
  CLI all call. `get_episode` by id is unaffected.
- `/status` reports `hidden_groups` so a deployment can be checked from
  outside without reading its plist.
- Tool and endpoint docs for `group_id` say so.

## Non-goals

- Hiding from `get_episodes` / listing. Listing is a browse, not retrieval,
  and the operator guidance already says not to rely on it for ordering.
- Per-episode visibility flags. Group is the right grain: the store that
  needs hiding is a whole group by construction.
- Any change to ranking. Hidden episodes are removed after scoring, in the
  same post-filter pass as group/source/tag filters.
