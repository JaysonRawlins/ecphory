## 1. Identity

- [x] 1.1 `scripts/swamp` wrapper, `.envrc` committed (no secret in it; pinned by a test).
- [x] 1.2 Red proof: against a bare `exec swamp "$@"` stub, all six
      `factory_swamp_wrapper` tests failed. Each guard removed alone failed
      exactly its own test. The hop-marker mutation first HUNG the test, so the
      fake direnv now gives up after 3 hops: "expected exactly one direnv hop,
      saw 3".

## 2. Gates

- [x] 2.1 `scripts/factory/check-tasks-complete.sh`, `check-change-archived.sh`,
      `cargo-test-counts.sh`.
- [x] 2.2 Red proof: against `exit 0` stubs, 6 of 7 `factory_gates` tests failed.
      The 7th is the committed-archive pass case. Each guard removed alone
      failed one test. The change-folder and uncommitted checks are layered: the
      gate still refuses without them, so the red comes from the assertion on
      which check fired.
- [x] 2.3 Red proof: against bare `cargo test --all-targets`, the zero-test case
      passed (cargo exits 0 on an empty run). Removing the zero-test check or
      the cargo exit-code check failed its own test. A failure-count check was
      cut because cargo's exit code already covers it, so no mutation could fail
      it.

## 3. Factory

- [x] 3.1 Four workflows, five `command/shell` models, `ecphory-factory.yaml`.
- [x] 3.2 Red proof: all 20 `factory_definition` tests failed before the
      definition existed. Nine one-edit mutations each failed only the pinned
      test (listed in the test's header).
- [x] 3.3 Offline validation with the extension's own zod schema and
      `validateGraph` (2026.09.24.2, under swamp's deno): `valid: 17 stages`.
      The control: exman's definition with a gate `id` renamed to `gateId`, and
      with a transition retargeted at a missing stage, were both rejected.
- [x] 3.4 CI's test job installs `@fission-ai/openspec@1.6.0` so `factory_gates`
      runs there.

## 4. With the personal swamp key

- [ ] 4.1 `scripts/swamp auth whoami` shows the personal account, and the hashes
      of `~/.config/swamp/auth.json` and `auth_verified.json` are the same
      before and after every step below.
- [ ] 4.2 `scripts/swamp repo init`, `extension pull @swamp/software-factory`,
      then `model method run ecphory-factory validate`, which must fail on a
      mutated gate first.
- [ ] 4.3 `workflow validate` for all four workflows. Run each live, red then
      green: plan-verify on a broken and a valid change; tasks-verify with a
      task unticked and ticked; archive-verify before and after the archive
      commit; pre-pr-verify on a dirty tree and on this branch's head.

## Live verification

- the scratch-instance recipe from the live-verify prompt, run against this
  branch's build: an episode id and a search_id came back, the port closed
  afterwards, and the daemon's episode count was the same before and after
