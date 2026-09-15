## 1. Implementation

- [x] 1.1 `UpdateParams` gains `source`, `source_model`, `source_description`.
- [x] 1.2 `Store::update` applies them inside the existing `mutate` closure, so
      they are archived into an `EpisodeVersion` before the write like every
      other field.
- [x] 1.3 REST: `UpdateBody` gains the three fields, threaded through with
      `none_if_empty`.
- [x] 1.4 MCP: `UpdateEpisodeRequest` gains them with `opt_str`, and the tool
      description says provenance is updatable.
- [x] 1.5 CLI: `ecphory update --source/--source-model/--source-description`.
- [x] 1.6 CHANGELOG entry under Unreleased.

## 2. Red proof (record what was broken and what it printed)

- [x] 2.1 `update_repairs_a_mangled_source_without_minting_a_new_id` observed
      FAILING against the unchanged implementation (2026-09-15,
      `cargo test --bin ecphory`). It drives a real axum server over the real
      `PUT` with the issue's own corruption. The PUT returned 200 and changed
      nothing, which is the bug exactly:
      `assertion left == right failed: source must be correctable: {...}
      left: "claude-code</source>\n<parameter name=\"source_model\">opus-4.7"
      right: "claude-code"`.
- [x] 2.2 The converse assertion — a content-only update must not blank
      provenance — was red-proved on its own by a CONTROL: `source` alone was
      changed from `none_if_empty(body.source)` to `Some(body.source)` and the
      test failed on that assertion and no other
      (`a content-only update wiped provenance: ... left: "" right:
      "claude-code"`, with `source_model` and `source_description` still
      correct, proving the control and not something else caused it). Reverted.
- [x] 2.3 `restore_version_restores_exact_snapshot_and_archives_displaced_state`
      extended to set all three fields, so the rollback half is covered by an
      existing exhaustive-equality test rather than asserted.

## 3. Verify

- [x] 3.1 `cargo fmt --check` clean, `cargo clippy --all-targets` 0 warnings,
      `cargo test` 111 passed (106 unit + 5 CLI integration), up from 110.
- [x] 3.2 Live, release binary, throwaway store and port 34910 — never the
      production store, which the daemon holds. All three surfaces repaired the
      issue's exact corruption on a real episode:
      - CLI (`ecphory update --source ... --source-model ...`, store opened
        directly, no daemon): `source: 'claude-code'`, `model: 'opus-4.7'`,
        id unchanged, content intact.
      - MCP over streamable HTTP (`tools/call update_episode` against
        `ecphory serve`): schema listed all three fields; source went from
        `'claude-code</source>\n<parameter name="source_model">opus-4.7'` to
        `'claude-code'`; a follow-up content-only call left provenance intact;
        `get_episode_versions` returned the mangled value still archived.
      - REST (`curl -X PUT` against the same running daemon): corrected
        provenance in the response.
- [x] 3.3 `POST /admin/export` on that store wrote the corrected `source`,
      `source_model` and `source_description` into the mirror frontmatter, so a
      repair propagates offsite. An episode left unrepaired in the same export
      still showed the mangled value — the control proving the export reflects
      stored state rather than rewriting it.
