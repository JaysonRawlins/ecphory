## Context

The reference is exman's factory with the reconcile and archive stages from its
PR #329. Four things in it are specific to exman and are not carried over: the
attestation manifest and its CI check, the DSQL/CDK/migration checks, deploying
an AWS stack for live verification, and the swamp GitHub extensions that need a
vault token.

## Decisions

**Work item = GitHub issue, read with `gh`.** Issues are already how work comes
into this repo, and the PR closes one. `gh` uses the maintainer's existing
login, so no swamp GitHub extension or vault token is involved.

**Live verify = a scratch instance, never the daemon.** The LaunchAgent daemon is
the maintainer's real memory store and serves every agent session. Deploying an
unmerged build there would put unreviewed code under all of them. live-verify
builds the commit and runs `ecphory serve --db <temp> --port <free>`, with
`ECPHORY_EXPORT_DIR`, `ECPHORY_TRIGGERS_FILE` and `ECPHORY_DB` unset. A probe
must produce an identifier (a search_id or an episode id). Criteria a test
already proves resolve as `static`.

**The run ends at merged.** Putting a merged build on the daemon stays the
`deploy-daemon` skill's job, with its eval floor and trail.

**pre-pr-verify refuses a dirty tree.** exman's verifies whatever the working
tree holds. Here the first step requires HEAD to equal the commit being verified
and the tree to be clean, so an uncommitted fix cannot pass verification and
then be missing from the PR.

**Identity isolation is structural.** The machine that runs this may hold
another swamp-club login in `~/.config/swamp/auth.json`. swamp falls back to it
silently when `SWAMP_API_KEY` is unset or empty. With the default config dir it
also writes the personal identity into that file. `scripts/swamp` refuses unless
`SWAMP_CONFIG_DIR` is `~/.config/swamp-personal` and the key is non-empty. It
drops `SWAMP_API_KEY_FILE` and `SWAMP_SIGNIN_TOKEN` and refuses
`--club-api-key-file`. An agent shell never runs the direnv hook, so the wrapper
loads `.envrc` once through `direnv exec`. All of this comes from swamp's source
(`api_key_source.ts`, `paths.ts`, `auth_repository.ts`).

**swamp enrolls no AI tool (`tools: []`).** With `tools: [claude]`, swamp writes
a Claude Code hook that runs bare `swamp` after every Bash call, which is the
leak the wrapper exists to stop, and a CLAUDE.md telling agents to run bare
`swamp`. Its skills are loaded from user-level and vendored copies instead.

## Risks

- swamp's extension helper reads credentials from `XDG_CONFIG_HOME` or `HOME`,
  ignoring `SWAMP_CONFIG_DIR`, and falls back to that auth.json when
  `SWAMP_API_KEY` is empty. The wrapper's non-empty-key rule covers it. A bare
  `swamp` call that bypasses the wrapper does not. `PATH_add scripts` routes
  bare `swamp` to the wrapper in shells that loaded direnv. An agent shell
  launched without direnv still finds the real swamp first.
- swamp's `validate` checks the schema and that every stage is reachable,
  nothing more. The stage graph and prompt rules are only as safe as
  `tests/factory_definition.rs`, and no factory run has yet driven a real work
  item through the definition end to end.
