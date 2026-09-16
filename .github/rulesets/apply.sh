#!/usr/bin/env bash
# Apply the checked-in rulesets in this directory to the GitHub repo.
#
# Idempotent: a ruleset is matched by name and updated in place, or created
# if absent. Run --check first (or on its own) — it refuses to apply a
# ruleset whose required status checks are not checks this repo actually
# reports, which is the failure mode that deadlocks every future PR.
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: .github/rulesets/apply.sh [--check] [--repo OWNER/NAME] [--against SHA]

  --check        validate only; make no writes
  --repo         target repo (default: the origin remote)
  --against SHA  commit whose check runs are used to validate required
                 contexts (default: the head of the latest merged PR)
USAGE
}

check_only=0
repo=""
against=""
while [ $# -gt 0 ]; do
  case "$1" in
    --check) check_only=1; shift ;;
    --repo) repo="$2"; shift 2 ;;
    --against) against="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

cd "$(dirname "$0")"

if [ -z "$repo" ]; then
  repo=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
fi
echo "repo: $repo"

# --- preflight: is the rulesets API even available on this repo? ----------
# Rulesets need a public repo, or GitHub Pro/Team/Enterprise on a private
# one. On a private repo under a free plan every rulesets endpoint 403s.
if ! probe=$(gh api "repos/$repo/rulesets" 2>&1); then
  if printf '%s' "$probe" | grep -q 'Upgrade to GitHub Pro'; then
    cat >&2 <<EOF

BLOCKED: $repo is private on a plan without rulesets.

  GitHub: "Upgrade to GitHub Pro or make this repository public to enable
  this feature." Rulesets and branch protection are free on public repos,
  and need Pro/Team/Enterprise on private ones.

Unblock by either flipping the repo public (docs/RELEASING.md, public-flip-day
checklist) or upgrading the plan, then re-run this script.
EOF
    exit 3
  fi
  echo "$probe" >&2
  exit 1
fi

# --- validate required contexts against checks that really ran -----------
# A required context that nothing ever reports leaves PRs pending forever,
# and a job that is skipped on pull_request never reports success. Both are
# caught here rather than on the first PR after the ruleset lands.
if [ -z "$against" ]; then
  against=$(gh pr list --repo "$repo" --state merged --limit 1 --json headRefOid --jq '.[0].headRefOid')
fi
echo "validating required contexts against ${against:0:12}"

runs=$(gh api "repos/$repo/commits/$against/check-runs" --paginate \
  --jq '.check_runs[] | [.name, (.app.id|tostring), .conclusion] | @tsv')

fail=0
while IFS=$'\t' read -r want_ctx want_app; do
  [ -n "$want_ctx" ] || continue
  line=$(printf '%s\n' "$runs" | awk -F'\t' -v c="$want_ctx" '$1==c {print; exit}')
  if [ -z "$line" ]; then
    echo "  MISSING  $want_ctx — nothing reported this context on ${against:0:12}" >&2
    fail=1
    continue
  fi
  got_app=$(printf '%s' "$line" | cut -f2)
  got_concl=$(printf '%s' "$line" | cut -f3)
  if [ "$got_app" != "$want_app" ]; then
    echo "  APP      $want_ctx — reported by app $got_app, ruleset pins $want_app" >&2
    fail=1
    continue
  fi
  if [ "$got_concl" = "skipped" ]; then
    echo "  SKIPPED  $want_ctx — skipped on pull_request; requiring it would stall PRs" >&2
    fail=1
    continue
  fi
  echo "  ok       $want_ctx ($got_concl)"
done < <(jq -r '
  .rules[] | select(.type=="required_status_checks")
  | .parameters.required_status_checks[]
  | [.context, (.integration_id|tostring)] | @tsv' main.json)

if [ "$fail" -ne 0 ]; then
  echo "refusing to apply: required status checks do not match this repo" >&2
  exit 4
fi

if [ "$check_only" -eq 1 ]; then
  echo "check only; no changes made"
  exit 0
fi

# --- apply ----------------------------------------------------------------
existing=$(gh api "repos/$repo/rulesets" --jq '.[] | [.name, (.id|tostring)] | @tsv')
for f in main.json release-tags.json; do
  name=$(jq -r .name "$f")
  id=$(printf '%s\n' "$existing" | awk -F'\t' -v n="$name" '$1==n {print $2; exit}')
  if [ -n "$id" ]; then
    echo "updating ruleset '$name' (id $id)"
    gh api --method PUT "repos/$repo/rulesets/$id" --input "$f" --jq '"  -> \(.name) \(.enforcement)"'
  else
    echo "creating ruleset '$name'"
    gh api --method POST "repos/$repo/rulesets" --input "$f" --jq '"  -> \(.name) \(.enforcement) (id \(.id))"'
  fi
done
