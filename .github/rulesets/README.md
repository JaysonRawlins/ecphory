# Repository rulesets

**Not applied yet.** These payloads are reviewed and ready; turning them on
is a separate, deliberate step.

Until 2026-09-16 they could not be applied at all — GitHub refuses every
rulesets endpoint on a private repo without Pro/Team/Enterprise (*"Upgrade
to GitHub Pro or make this repository public to enable this feature"*, HTTP
403). The repo is public now, so that block is gone and `apply.sh` will
work. What remains is the judgement call about running it, because these
rules bind the maintainer too.

```bash
.github/rulesets/apply.sh --check   # validate, write nothing
.github/rulesets/apply.sh           # create or update, by ruleset name
```

Both files are also importable by hand in Settings → Rules → New ruleset →
Import a ruleset.

## `main.json` — the default branch

`main` has taken 13 direct pushes, the most recent on 2026-08-18. Every
commit since has arrived as a squash merge from a PR, and there is not one
merge commit in the history. The ruleset codifies the practice that is
already there rather than introducing a new one:

| Rule | Why |
| --- | --- |
| `pull_request`, 0 approvals | **0 is load-bearing.** GitHub will not let you approve your own PR, so any non-zero count locks a solo maintainer out of their own default branch — including out of release-plz's release PR. |
| `required_status_checks` | The four below. |
| `required_linear_history` | Matches reality: 0 merge commits in 43. |
| `allowed_merge_methods: [squash]` | Same; also what `/merge-pr` does. |
| `deletion`, `non_fast_forward` | No deleting or force-pushing `main`. |

Required checks, pinned to the GitHub Actions app (`integration_id`
15368) so another app cannot satisfy them by reporting the same name:

- `test + clippy`, `check x86_64-pc-windows-msvc` — `ci.yml`
- `sbom + grype` — `security-scan.yml`
- `plan` — `release.yml`, dist's config check. Cheap, and a broken dist
  config otherwise surfaces at tag-push time, i.e. mid-release.

Deliberately **not** required:

- `announce`, `host`, `build-local-artifacts`, `build-global-artifacts` —
  release-only jobs, `skipped` on every PR. A required check that reports
  `skipped` never goes green, so requiring these stalls all PRs.
- `Socket Security: *` — a third-party app (id 156372), not ours. If it is
  ever uninstalled its checks stop reporting and PRs hang on a gate no one
  can satisfy. It stays advisory.

`strict_required_status_checks_policy` is `false`: with several agents
opening PRs off `main` at once, requiring every branch to be up to date
turns into a rebase treadmill, and auto-merge is off on this repo.

**Watch item:** `release.yml` is generated — edit `dist-workspace.toml` and
run `dist generate`, never the yml. If a future dist renames the `plan` job,
the required context stops reporting and PRs hang. `apply.sh --check`
catches it; run it after a dist upgrade.

## `release-tags.json` — `v*` tags

Blocks deletion and force-updates of release tags. It does **not** block
creation, because release-plz pushes `vX.Y.Z` with its app token and must
keep being able to.

Prereleases are excluded (`refs/tags/v*-*`) on purpose: the release
rehearsal in docs/RELEASING.md creates an `rc` tag and then deletes it. A
blanket `refs/tags/v*` deletion rule would break that documented flow, so
`v0.3.0-rc.1` stays disposable while `v0.3.0` does not.

## Escape hatch

`bypass_actors` is empty in both — nobody has a standing exemption. For a
genuine emergency, an admin disables the ruleset in Settings, does the work,
and turns it back on. That leaves a trace in the audit log; a permanent
bypass entry would not.
