## Why

Three fields carry an episode's provenance, and only one of them has ever been
described to the agent that fills them in:

```rust
/// Originating system, e.g. "claude-code".
#[serde(default)]
pub source: String,
#[serde(default)]
pub source_model: String,       // no description
#[serde(default)]
pub source_description: String, // no description
```

For an agent client the MCP input schema is not documentation *about* the API,
it is the only spec of it that will ever be read — the same argument #18 made
about `rate_search`. Two of the three fields say nothing, so agents guessed,
and the guesses disagree. A scan of the reference deployment (issue #41) finds
48 episodes written to a convention other than the majority's:

| count | shape | example |
|---|---|---|
| 12 | model folded into `source` with a slash | `source: "claude-code/opus-4-7"` |
| 11 | empty `source` | `source: ""` |
| 28 | hybrid model spelling | `source_model: "opus-4-7"` |

(51 hits over 48 distinct episodes; 3 of the slash rows also carry the hybrid
spelling in `source_model`, which is the tell — those authors wrote the model
in *both* places, so the compound was never a considered choice.)

Retrieval is unaffected: `source` is not in the tantivy schema, exactly as in
the #39 repair. What breaks is the one question these fields exist to answer.
`GROUP BY source_model` today undercounts by 23 and reports `opus-4-7` and
`opus-4.7` as two different models.

Worth separating two findings, because they have different futures. **The
shape errors are closed.** All 23 slash-and-empty rows are UUIDv4 — every one
is engram-migration residue, and no native ecphory write has ever produced
either shape. **The spelling drift is not closed**: `claude-opus-5` (101),
`claude-opus-5-1m` (20) and `claude-opus-5[1m]` (14) are all UUIDv7, all
written since ecphory existed, all still arriving. Documentation is the part
of this change that has a future.

## What Changes

- **The convention is stated where it is read.** `source`, `source_model` and
  `source_description` each get a description on the MCP `add_memory` and
  `update_episode` schemas, on the REST `AddMemoryBody`, and on the `ecphory
  add` / `ecphory update` flags. The README gains a "Provenance: who wrote
  this" section for the human reader.
- **`source` names the writing system alone** — `claude-code`, `codex`,
  `teachme` — never a `system/model` compound, and never empty. When the
  writing system genuinely isn't known, the value is the literal `unknown`,
  because a query can count `unknown` and cannot count `""`.
- **`source_model` names the model alone**, as the harness reports it,
  context-window marker included. `claude-opus-5[1m]` is a correct value: it
  was true at write time and nothing downstream can recover it afterwards.
- **A one-off repair of the 48 affected episodes** in the reference
  deployment, recorded in tasks.md. It ships nothing; the store is data.

No production code changes. Nothing is validated on write, and that is the
point rather than an omission — see below.

## Non-goals

- **Write-time validation of provenance.** Axiom 3 says the server stores what
  the edge gives it, and #39 already decided this for `source` specifically.
  A validator would also have to be wrong about the future: today's illegal
  value is tomorrow's new harness. The counterweight is #40 — all three fields
  stay correctable for the life of the episode — not a gate at the door.
- **A canonical registry of model ids.** The corpus holds `opus-4.8` (282)
  next to `claude-opus-4-8` (44) next to `claude-opus-4-8-1m` (35). Folding
  those would have to decide that the context-window marker is not part of the
  model's identity, and that decision destroys information that cannot be
  recovered from anywhere else. The repair folds exactly one spelling —
  `opus-4-7` → `opus-4.7` — because `opus-4-7` is neither the API id
  (`claude-opus-4-7`, which also exists in the corpus, 20 rows, and is left
  alone) nor the human short form. It is nobody's convention, so folding it
  chooses between two live spellings rather than one.
- **Validating on import.** `src/import.rs` takes the mirror's `source`
  verbatim, `""` included, which is how these rows entered. It stays that way:
  import is a faithful restore path, and a restore that silently improves its
  input cannot be used to verify a backup or to roll one back.
- **Normalizing `source` values that are tools rather than harnesses.**
  `teachme` (28), `todo` (31), `contact` (5) name the skill that captured the
  episode. That is a writing system under this convention and they pass it.
- **Asserting the doc strings in a test.** Following #18: prose pinned in a
  test fails on every rewording and catches nothing a reader would not. The
  served schema is verified live instead, in tasks.md.
