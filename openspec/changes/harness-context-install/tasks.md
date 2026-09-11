## 1. Detection

- [x] 1.1 Detect installed harnesses by CONFIG DIRECTORY presence.
      DEVIATION from the written task, stated rather than quietly substituted: the task
      said detect by resolved binary. Binary detection is untestable under a temp `$HOME`
      and all four CLIs are shell functions anyway, so the config dir — which is where the
      adapter gets written regardless — is both more robust and testable.
- [x] 1.2 Config targets resolved. `~/.config/github-copilot/` is the EDITOR's dir and is
      not a CLI rail (measured: a canary there was not delivered); `~/.copilot/` is the CLI's.

## 1b. PIVOT — doctor verifies the OUTCOME, not the mechanism

- [x] 1b.1 Forced by observation: the first doctor build reported `opencode UNPROVEN`
      by looking for its own pointer, while that machine's opencode was already reading a
      live ecphory-generated artifact written by an external render script. A verification
      tool that recognises only its own handiwork reports on itself.
- [x] 1b.2 `MISCONFIGURED` narrowed to "an ecphory-managed adapter exists and is broken".
      Absence of a known adapter is now `UNPROVEN` — delivery may ride a rail ecphory does
      not own. Test `absent_adapter_reports_misconfigured` asserted the OLD rule and was
      changed deliberately; the requirement moved, so the test had to.
- [x] 1b.3 Artifact recognition via the `GENERATED from ecphory episode` header. On the
      reference machine this immediately surfaced a rail the manual survey had MISSED:
      `~/.codex/AGENTS.md` also carries it.
- [x] 1b.4 `install` demoted from authority to convenience; it must no-op where delivery
      already happens. A host security daemon on that machine already has trusted hooks on
      all four harnesses — install must not fight or duplicate that.
- [x] 1b.5 `NOT_DELIVERED` added: the live tier running and finding no canary is a real
      negative, distinct from `UNPROVEN` ("not checked").

## 2. doctor (build FIRST — install without it ships all five silent no-ops at once)

- [x] 2.1 Static tier: classifies MISCONFIGURED / UNPROVEN. The guarantee is TYPE-LEVEL, not
      merely asserted: `StaticStatus` has no `Delivered` variant, so "static claims delivery"
      is unrepresentable. `Status::Delivered` is currently unconstructed anywhere in the
      crate, and rustc's dead-code warning is the standing proof of that.
      RED-PROOF: mutated `From<StaticStatus>` to map `Unproven -> Delivered`.
      `static_tier_never_reports_delivered` FAILED (rc=101) while the other two tests stayed
      green — so the assertion is specific, not incidentally satisfied. Restored, rc=0.
- [ ] 2.2 Live tier: fresh random canary per run, per harness, restore adapter afterwards on both success and failure paths.
- [~] 2.3 Named detection implemented for the statically-visible modes: claude non-relative
      `@import`, codex hook misplaced in `config.toml`, copilot missing `sessionStart` hook,
      opencode pointer absent or dangling. The copilot plain-text-vs-JSON mode is currently
      surfaced as a warning in the UNPROVEN detail; proving it needs the live tier.
      REAL-PATH BUG FOUND HERE, not by the tests: the first codex check ANDed "file contains
      `[[hooks.`" with "file contains ecphory" and reported a misplaced hook on a machine
      whose only lowercase match was a `[projects."/…/ecphory"]` trust entry — a confident
      wrong diagnosis telling the operator to move a file that did not exist. Fixed with a
      scoped section scan; regression test
      `codex_unrelated_ecphory_mention_is_not_a_hook_misconfiguration` observed red first.
- [ ] 2.4 RED-PROOF, one per harness — break that harness's adapter deliberately, confirm doctor reports it, restore. Record the exact output here. A doctor never observed failing cannot be cited.
      - [ ] 2.4.1 claude: rewrite the managed `@import` to an absolute path -> expect MISCONFIGURED (relative-path constraint)
      - [ ] 2.4.2 codex: move the hook from `hooks.json` into `config.toml` -> expect MISCONFIGURED
      - [ ] 2.4.3 opencode: point `instructions` at a non-existent path -> expect MISCONFIGURED
      - [ ] 2.4.4 copilot: make the hook emit plain text instead of JSON -> expect MISCONFIGURED naming the JSON requirement
      - [ ] 2.4.5 CONTROL: with all four correctly wired, `doctor --live` reports DELIVERED 4/4
- [ ] 2.5 Verify 2.4.5 is not vacuous: confirm each DELIVERED corresponds to a canary that was actually absent from the adapter before the run (fresh token, no stale artifact).

## 3. install / uninstall

- [ ] 3.1 Adapters: claude hook, codex managed block, opencode pointer, copilot hook (JSON).
- [ ] 3.2 Managed blocks delimited with ecphory begin/end markers; never rewrite outside the markers.
- [ ] 3.3 Backup-before-write; refuse to modify what cannot be backed up.
- [ ] 3.4 Idempotence test: second run is a no-op.
- [ ] 3.5 Round-trip test: install -> uninstall -> sha256 equal on every touched file. The reference machine's `~/.codex/` carries ~10 `config.toml` backup siblings, which is evidence this class of edit has drawn blood before.

## 4. Docs

- [ ] 4.1 A "does it actually work" page leading with `doctor --live`, written for someone who does not know how agent memory works: the failure they will actually experience is not an error message, it is unremarkable answers.
- [ ] 4.2 Document the codex hook-trust upgrade path (opt-in, turns codex's copy into a read).

## 5. E2E

- [ ] 5.1 Fresh machine simulation (isolated HOME per harness): `install` -> `doctor --live` reports DELIVERED 4/4.
- [ ] 5.2 Mutation check: for each harness, break the adapter -> `doctor --live` flips that harness off DELIVERED -> repair -> back to DELIVERED. This is the only test that proves doctor tracks reality rather than printing a constant.
- [ ] 5.3 Artifact regeneration: edit the source episode -> trigger rewrites the artifact -> a NEW session in each harness sees the updated content (proves the read-vs-copy distinction holds end to end, and that codex's copy is actually refreshed).
- [ ] 5.4 `uninstall` -> `doctor` reports MISCONFIGURED 4/4 and all configs are byte-identical to pre-install.
