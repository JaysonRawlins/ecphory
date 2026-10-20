#!/usr/bin/env bash
# Factory gate (openspec-tasks-verify): exit 0 only when every task in the
# named OpenSpec change's tasks.md is checked off. `openspec validate` does not
# check this, because an in-progress change is a valid state. Run from the repo
# root. tests/factory_gates.rs pins it.
set -euo pipefail

slug="${1:-}"
[ -n "$slug" ] || { echo "usage: check-tasks-complete.sh <change-slug>" >&2; exit 2; }

list="$(openspec list --json)"
change="$(jq -c --arg s "$slug" '.changes[] | select(.name == $s)' <<<"$list")"
[ -n "$change" ] || { echo "OpenSpec change \"$slug\" not found (openspec list --json)" >&2; exit 1; }

done_n="$(jq -r '.completedTasks' <<<"$change")"
total_n="$(jq -r '.totalTasks' <<<"$change")"
if [ "$(jq -r '.status' <<<"$change")" != complete ]; then
  echo "OpenSpec change \"$slug\" has incomplete tasks: $done_n/$total_n" >&2
  exit 1
fi
echo "OpenSpec change \"$slug\": all $total_n tasks complete"
