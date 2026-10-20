## 1. Identity

- [x] 1.1 Intake runs `swamp auth whoami --json` first and stops unless it is
      jaysonjrawlins. RED-PROOF: `intake_checks_the_swamp_identity_before_reading_the_issue`
      failed on a6f8195; dropping the username printed `lacks "jaysonjrawlins"`;
      whoami moved after `gh issue view` printed "does not check the swamp
      identity before reading the issue".
- [x] 1.2 Remove `.envrc`, `scripts/swamp`, `tests/factory_swamp_wrapper.rs`;
      `scripts/swamp data query` -> `swamp data query` in open-pr.
      RED-PROOF: `nothing_routes_swamp_through_a_wrapper` failed on a6f8195
      naming ecphory-factory.yaml. The `tools: []` pin moved to
      factory_definition and was re-observed red with `tools: [claude]`.
- [x] 1.3 Driver skill: already byte-identical to upstream (diffed against
      placet's verbatim copy); nothing to restore.

## 2. Injection

- [x] 2.1 Live RED: openspec-plan-verify with `changeSlug="x; touch <m>"` ran
      the touch, also created `--type`, `change` and `--strict` in the
      worktree, and reported SUCCEEDED (run 3bc10ea4).
- [x] 2.2 Values into `env`, quoted; `validate-change.sh` for plan-verify.
      RED-PROOF: `no_value_is_interpolated_into_a_shell_command` failed on each
      of the four a6f8195 workflows restored alone, and on `$CHANGE_SLUG`
      unquoted in tasks-verify.
- [x] 2.3 Slug check in all three gate scripts. RED-PROOF:
      `every_gate_refuses_a_malformed_slug` failed for each script with its
      check deleted (exit 1: "has issues", "not found", "found 0").
      `change_gate_fails_a_change_that_is_not_strictly_valid` failed with
      `openspec validate` replaced by `exit 0`.

## Live verification

- the injected plan-verify run again: FAILED at assert-valid, exit 2 "not a
  change slug", no marker, no stray files (run f73fbad4)
- tasks-verify (95a1029e) and archive-verify (f30ef835) with the same slug:
  FAILED, "not a change slug", no marker
- pre-pr-verify with `commit="x; touch <m>"`: FAILED at head, the value printed
  as data (8644078d)
- archive-verify on `swamp-software-factory`: SUCCEEDED, so the env value
  reaches the script intact
- plan-verify and tasks-verify on this change: SUCCEEDED ("is valid", "all 6
  tasks complete")
