#!/usr/bin/env bash
# Factory pre-pr-verify step: run the suite and print its totals as the last
# line of output, {"tests":N,"failures":N,"ignored":N}. Exits non-zero when
# cargo does, which covers any failed test, or when NO test ran: `cargo test`
# exits 0 on an empty run, so its exit code alone cannot show the suite ran.
# Extra arguments go to `cargo test`. tests/factory_test_counts.rs pins it.
set -uo pipefail

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

cargo test --all-targets "$@" 2>&1 | tee "$log"
cargo_rc=${PIPESTATUS[0]}

# One `test result: ok. 3 passed; 0 failed; 1 ignored; ...` line per test binary.
read -r tests failures ignored < <(
  grep -E '^test result: ' "$log" |
    sed -E 's/.* ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored;.*/\1 \2 \3/' |
    awk '{p+=$1; f+=$2; i+=$3} END {print p+0, f+0, i+0}'
)
echo "{\"tests\":$tests,\"failures\":$failures,\"ignored\":$ignored}"

[ "$cargo_rc" -eq 0 ] || { echo "cargo test exited $cargo_rc" >&2; exit 1; }
[ "$tests" -gt 0 ] || { echo "no tests ran" >&2; exit 1; }
