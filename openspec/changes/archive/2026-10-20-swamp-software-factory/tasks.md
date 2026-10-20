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

- [x] 4.1 With the work login in the default dir, bare `swamp` answered as
      the work account (collectives include the employer's) and
      `scripts/swamp` from an agent shell as the personal one. The work
      login's auth.json and auth_verified.json hashed the same before and
      after. With the hop suppressed and no identity, the wrapper refused
      (rc=78) without reaching swamp.
- [x] 4.2 `repo init`, `extension pull @swamp/software-factory` (2026.10.13.1),
      then real `validate`: rc=1 on a gate renamed to `gateId` ("Unrecognized
      key"), rc=0 on the definition. `typeVersion` corrected to the type's own
      2026.06.24.1. `workflow validate` passes all four and fails a step
      depending on a missing step.
- [x] 4.2a `repo init` with `tools: [claude]` wrote `.claude/settings.local.json`
      with a hook running bare `swamp audit record` after every Bash call.
      Agent shells never load direnv, so it ran 5 times as the default dir's
      (work) login before it was removed. Now `tools: []`, pinned, with
      telemetry off, and no swamp CLAUDE.md. `PATH_add scripts` in `.envrc`
      sends a bare `swamp` in any direnv shell to the wrapper.
- [x] 4.3 Live workflow runs, each red then green:
      - plan-verify: rc=1 on a change that does not exist, rc=0 on this one.
      - tasks-verify: rc=1 with "incomplete tasks: 9/12" while 4.x were open.
      - archive-verify: rc=1 before the archive.
      - pre-pr-verify: rc=1 on a dirty tree ("working tree not clean") and on
        the previous sha ("HEAD is 985a29e…, not eaddc71…"). rc=0 on 985a29e:
        8/8 steps, `{"tests":164,"failures":0,"ignored":0}`. The open-pr
        stage's runId-scoped totals query and its report query both returned
        that run's data.
      The greens for tasks-verify, archive-verify and the final pre-pr-verify
      run on the archive commit, after this task is ticked.

## Live verification

- the scratch-instance recipe from the live-verify prompt, run against this
  branch's build: an episode id and a search_id came back, the port closed
  afterwards, and the daemon's episode count was the same before and after
