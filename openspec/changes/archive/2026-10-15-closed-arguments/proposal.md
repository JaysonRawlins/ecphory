## Why

On 2026-10-15 an agent stamping an OUTCOMES line onto episode `01a0841d` (a
~5,500-word placet record) called `update_episode` with `content` and
`mode: "append"`. There was no `mode` argument. serde drops fields a struct
does not name, so the guess vanished, `content` did what it always does, and
the whole body was replaced by two sentences. The call reported success and
echoed the truncated episode. It was recovered from `get_episode_versions`
(ecphory `01a14120`).

This is the second time the same class has bitten. `updatable-provenance`
recorded that a `PUT` naming `source` "returned 200 and changed nothing" for
the same reason, and fixed it by adding the fields, which closed that instance
and left the class open.

Two things were wrong, and they are fixed separately:

- **An unknown argument is dropped, not refused.** That is the class. Every MCP
  request struct and every REST body/query now denies unknown fields, so a
  guess fails loudly, with an error naming the field and listing the valid
  ones. The schema advertises `additionalProperties: false`.
- **There was no append.** Stamping an outcome onto a long episode is routine
  (the close-out ritual does it to the longest, oldest episodes), and the only
  safe route was for the agent to retype the whole body, which is itself a
  truncation risk. `update_episode` gains `mode: "replace" | "append"`, the
  shape the agent reached for; REST takes the same `mode`; the CLI gains
  `--append`.

## What changes for callers

A caller sending a field the tool does not define now gets an error where it
used to get silent success. Known in the wild: the `resume-tailor` skill passed
engram's `search_mode`/`search_alpha` to `search`, fixed alongside this.
