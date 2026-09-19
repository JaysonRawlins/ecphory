#!/usr/bin/env bash
#
# Fail if a release tag has no published GitHub Release behind it.
#
# v0.3.7 is why this exists. release-plz pushed the tag on 2026-09-17; in run
# 35296550696 one dist matrix leg (x86_64-apple-darwin) died in Upload
# artifacts with "Failed to CreateArtifact: Unable to make request: ENOTFOUND",
# so dist's host, build-global-artifacts and announce jobs skipped and no
# Release was ever created. The failed run is still there to read -- nothing
# hid it. It simply is not on any path anyone walks after a release, so the
# tag sat orphaned for two days until someone checked for an unrelated reason.
#
# A tag with no Release is a version that looks shipped and that nobody can
# install. Note what it does NOT look like: /releases/tag/<tag> still returns
# 200, because GitHub renders a plain tag page for a tag with no Release behind
# it. Only the asset URLs give it away -- /releases/download/<tag>/
# ecphory-installer.sh is the 404. Eyeballing the tag page is not a check,
# which is why this reads the API instead.
#
# Only stable vX.Y.Z tags are audited. Rehearsal -rc.N tags exist precisely as
# a tag-without-a-finished-release for the length of the rehearsal and are
# deleted afterwards, so auditing them would report a gap for exactly as long
# as they are supposed to be there.
#
# Usage:
#   docs/audit-tag-releases.sh [--repo OWNER/NAME] [--ignore TAG]...
#
#   --repo    default: whatever `gh repo view` resolves in the cwd
#   --ignore  exempt one tag; repeatable. Used to prove a red came from the
#             tag it names rather than from a bug in the comparison.
#
# Exit 0 = every stable tag has a published Release. Exit 1 = at least one does
# not, and each is listed.
#
# RED PROOF (2026-09-19, against the live repo while v0.3.7 was still orphaned):
#
#   $ ./docs/audit-tag-releases.sh                       # rc=1
#   FAIL: tag with no published GitHub Release on JaysonRawlins/ecphory
#     v0.3.7
#       .../releases/download/v0.3.7/ecphory-installer.sh -> 404
#
#   $ ./docs/audit-tag-releases.sh --ignore v0.3.7       # rc=0
#   ok: all 9 stable tags on JaysonRawlins/ecphory have a published Release
#
# The control is the half that matters: the same comparison over the same nine
# tags goes green when the one known-bad tag is exempted, so the red came from
# v0.3.7 and not from the comparison being broken.

set -euo pipefail

repo=""
ignored=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)   repo="${2:?--repo needs OWNER/NAME}"; shift 2 ;;
    --ignore) ignored+=("${2:?--ignore needs a tag}"); shift 2 ;;
    -h|--help) sed -n '3,36p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [[ -z "$repo" ]]; then
  repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
fi

# Both fetches run on their own so a gh failure aborts here under `set -e`.
# Folding them into a filtering pipeline would turn "the API call failed" into
# "no tags found", and the audit would report ok on no evidence at all.
tags_raw="$(gh api --paginate "repos/$repo/tags" --jq '.[].name')"
# A draft Release is deliberately not a Release: its assets are unreachable to
# anyone who is not a maintainer, which is the same 404 the gap produces.
releases_raw="$(gh api --paginate "repos/$repo/releases" \
                --jq '.[] | select(.draft | not) | .tag_name')"

tags="$(printf '%s\n' "$tags_raw" | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -u || true)"
releases="$(printf '%s\n' "$releases_raw" | sort -u)"

if [[ -z "$tags" ]]; then
  echo "no stable vX.Y.Z tags on $repo -- nothing to audit"
  exit 0
fi

orphans="$(comm -23 <(printf '%s\n' "$tags") <(printf '%s\n' "$releases"))"

for tag in "${ignored[@]+"${ignored[@]}"}"; do
  orphans="$(printf '%s\n' "$orphans" | grep -v -x -F "$tag" || true)"
done

tag_count="$(printf '%s\n' "$tags" | grep -c . || true)"

if [[ -z "$orphans" ]]; then
  echo "ok: all $tag_count stable tags on $repo have a published Release"
  exit 0
fi

echo "FAIL: tag with no published GitHub Release on $repo" >&2
while read -r tag; do
  echo "  $tag" >&2
  echo "    https://github.com/$repo/releases/download/$tag/ecphory-installer.sh -> 404" >&2
done <<< "$orphans"
cat >&2 <<'HINT'

A tag without a Release has no installable artifacts. Either re-cut the
Release for that tag, or delete the tag. docs/RELEASING.md covers both.
HINT
exit 1
