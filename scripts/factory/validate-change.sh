#!/usr/bin/env bash
# Factory gate (openspec-plan-verify): exit 0 only when the named OpenSpec
# change passes `openspec validate --type change --strict`. Run from the repo
# root. tests/factory_gates.rs pins it.
set -euo pipefail

slug="${1:-}"
# The slug comes from an artifact the agent wrote while reading someone else's
# issue, and swamp does not enforce the workflow input's pattern.
[[ $slug =~ ^[a-z0-9][a-z0-9-]*$ ]] || { echo "not a change slug: '$slug'" >&2; exit 2; }

openspec validate "$slug" --type change --strict
