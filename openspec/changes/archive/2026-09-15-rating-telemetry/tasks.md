## 1. Implementation

- [x] 1.1 MCP: `rate_search`'s tool description says used ids are read-only
      telemetry never edited by any rating, and names
      `intended_episode_ids` as the only field that changes an episode; the
      `used_episode_ids` schema field says the same where the field is
      declared.
- [x] 1.2 MCP server instructions (the text that lands in an agent's system
      prompt) carry the short form: "signal only, never edited".
- [x] 1.3 REST: `RateSearchBody`'s two id fields document the same split.
- [x] 1.4 README, self-correction section: a "Used is signal, intended is the
      lever" paragraph, placed where `used` first appears (the collateral
      check), giving the reason and not just the rule.
- [x] 1.5 CHANGELOG entry under Unreleased, saying plainly that the behaviour
      shipped in 0.3.6 and only the contract was unwritten.
- [x] 1.6 No production code changed. `src/service.rs` already loops over
      `intended_episode_ids` alone (`f3e7304`), and its doc comment already
      says so; this change specifies and pins that, it does not alter it.

## 2. Red proof (record what was broken and what it printed)

- [x] 2.1 The existing service-level guard is load-bearing, not decorative.
      Staging the union back — `intended_episode_ids.iter().chain(used_episode_ids.iter())`,
      one line, exactly the pre-`f3e7304` behaviour — turned
      `used_episode_ids_never_trigger_self_correction` red (2026-09-15,
      `cargo test --bin ecphory`):
      `panicked at src/service.rs:1327: assertion failed: entry.corrections.is_empty()`.
- [x] 2.2 The new wire test was written against that same staged union and
      observed red BEFORE the union was reverted. First fixture put the used
      episode at rank 1, which proved too weak — the union produced
      `already_ranks`, a spurious log row but no mutation:
      `left: Array [Object {"action": String("already_ranks"), "after_rank": Number(1), ...}]`.
      The rig was rebuilt to land the used episode at **rank 6** — outside the
      correction window (k = 5), inside the returned page (limit = 10) — and
      then reproduced the live incident's own log line:
      `a used id is not a heal target, got [{"action":"enriched","after_rank":1,"episode_id":"01a0a5cd-..."}]`.
      That rank is asserted in the test, so the fixture cannot silently decay
      back into the weak version.
- [x] 2.3 The assertion was then REWORDED (an empty `corrections` is
      `skip_serializing_if`-ed, so absent and `[]` are the same answer on the
      wire) and the union re-staged a third time to prove the FINAL form of
      the check still fails. Both guards red together, 2 failed / 2 passed
      across `cargo test --bin ecphory used_`.
- [x] 2.4 Union reverted (`git checkout src/service.rs`); working tree carries
      no production-code diff.

## 3. Verify

- [x] 3.1 `cargo fmt --check` clean, `cargo clippy --all-targets` 0 warnings,
      `cargo test` 112 passed (107 unit + 5 CLI integration), up from 111 —
      re-run after rebasing onto main's #40, which had landed meanwhile and
      touches the same two files.
- [x] 3.2 Live, against a throwaway store on port 3497 — never the production
      store, which a daemon holds. Seeded the incident's shape: five padding
      episodes, the used episode at rank 6 (confirmed in the search response),
      and a better answer that genuinely misses.
      - `partial` with used ids only: `corrections` absent, and the used
        episode's `search_phrases` still absent. The incident's exact call,
        now inert.
      - `partial` with both fields: exactly one correction,
        `{"action":"enriched","after_rank":1,"episode_id":"<better>"}`; the
        better answer gained `["quartz crystal resonance"]`; the used
        episode's phrases still absent.
- [x] 3.3 The corrected contract is actually SERVED, not just written: over
      the real MCP endpoint, `initialize` returns instructions containing
      "signal only, never edited", and `tools/list` returns the `rate_search`
      description carrying "read-only telemetry, never edited by any rating"
      plus the `used_episode_ids` schema field's "never edited by any rating.
      The field that changes an episode is `intended_episode_ids`". This is
      the surface the issue named as the thing that invited the mistake, so
      it is checked where a client reads it rather than where it is typed.
