#!/usr/bin/env bash
# Factory gate (openspec-archive-verify): exit 0 only when the named OpenSpec
# change has been archived AND the archive is committed, and the merged specs
# pass strict validation. The archive stage records evidence saying it did
# this; this gate checks the tree instead of trusting that evidence. Run from
# the repo root. tests/factory_gates.rs pins it.
set -euo pipefail

slug="${1:-}"
[ -n "$slug" ] || { echo "usage: check-change-archived.sh <change-slug>" >&2; exit 2; }
# Not from a trusted hand: see validate-change.sh.
[[ $slug =~ ^[a-z0-9][a-z0-9-]*$ ]] || { echo "not a change slug: '$slug'" >&2; exit 2; }

fail() { echo "OpenSpec change \"$slug\": $1" >&2; exit 1; }

[ ! -e "openspec/changes/$slug" ] || fail "openspec/changes/$slug still exists (not archived)"

shopt -s nullglob
archived=(openspec/changes/archive/????-??-??-"$slug")
shopt -u nullglob
[ "${#archived[@]}" -eq 1 ] || fail "expected one openspec/changes/archive/<date>-$slug, found ${#archived[@]}"

dirty="$(git status --porcelain -- openspec/)"
[ -z "$dirty" ] || fail "uncommitted changes under openspec/: $(tr '\n' ' ' <<<"$dirty")"
[ -n "$(git ls-files -- "${archived[0]}")" ] || fail "${archived[0]} is not tracked by git"

# `openspec archive` writes this placeholder into a new capability's spec, and
# it is long enough to pass --strict.
placeholder="$(grep -rl '^TBD - created by archiving' openspec/specs/ || true)"
[ -z "$placeholder" ] || fail "replace the placeholder Purpose in: $(tr '\n' ' ' <<<"$placeholder")"

if ! report="$(openspec validate --specs --strict 2>&1)"; then
  fail "merged specs fail openspec validate --specs --strict: $report"
fi
echo "OpenSpec change \"$slug\": archived at ${archived[0]}, committed, specs strictly valid"
