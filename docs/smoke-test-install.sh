#!/usr/bin/env bash
# Smoke-test the published `curl | sh` install the way a stranger runs it.
#
# This exercises the one path the private repo could never prove: an
# anonymous fetch of a real release asset onto a machine that has never seen
# this repo. It does NOT use ECPHORY_DOWNLOAD_URL or any other override —
# the whole point is that the genuine public URL works unassisted.
#
# What each row checks: the installer exits 0, the binary lands, `--version`
# matches the release, the receipt is written, the env script ("shim") is
# written, the PATH advice the installer printed actually resolves the
# binary, and `add` + `search` work on a cold store. A row is PASS only if
# all of that holds.
#
# The Linux rows run in throwaway containers, which is what makes them
# honest: HOME is untouched, nothing is pre-installed, and the image's own
# toolset is what the installer has to cope with. The macOS row runs on this
# host with HOME redirected to a temp dir — everything else (PATH, SHELL,
# proxies) is the real environment, because `env -i` would hide exactly the
# kind of interference this test exists to catch.
#
# THE debian-no-xz ROW IS THE LOAD-BEARING ONE. dist's default archive
# format is .tar.xz, and the generated installer's preflight checks for
# `tar` but never for `xz`, so on any image with tar-but-no-xz the install
# died deep inside tar with "tar (child): xz: Cannot exec". That is why
# dist-workspace.toml pins `unix-archive = ".tar.gz"`. Keep this row: it is
# the regression test for that decision, and it fails against any release
# cut before the fix.
#
# Usage:
#   docs/smoke-test-install.sh [VERSION]
#
#   VERSION   e.g. 0.3.6 — defaults to the version in Cargo.toml (no "v").
#             Pass a published version; this reads the real Release.
#
# Requires: curl. docker for the Linux rows (skipped if absent). The macOS
# row only runs on Darwin. Leaves nothing behind on the host.

set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
version=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -1)}
url="https://github.com/JaysonRawlins/ecphory/releases/download/v${version}/ecphory-installer.sh"

pass=0
fail=0
skip=0

report() { # result, label, detail
    case "$1" in
        PASS) pass=$((pass + 1)); printf '  \033[32mPASS\033[0m  %-22s %s\n' "$2" "${3:-}" ;;
        FAIL) fail=$((fail + 1)); printf '  \033[31mFAIL\033[0m  %-22s %s\n' "$2" "${3:-}" ;;
        SKIP) skip=$((skip + 1)); printf '  SKIP  %-22s %s\n' "$2" "${3:-}" ;;
    esac
}

echo "smoke-testing ecphory ${version}"
echo "  ${url}"
echo

# --- the asset must be fetchable with no credentials at all ----------------
# Unsetting the tokens matters: this box has gh auth set up, and a test that
# quietly rides the maintainer's credentials proves nothing about a stranger.
anon_code=$(env -u GITHUB_TOKEN -u GH_TOKEN \
    curl --proto '=https' --tlsv1.2 -sS -L --no-netrc -o /dev/null \
         -w '%{http_code}' "$url" || echo 000)
if [ "$anon_code" = "200" ]; then
    report PASS "anonymous fetch" "HTTP 200"
else
    report FAIL "anonymous fetch" "HTTP $anon_code — the rest is meaningless"
    echo
    echo "summary: $pass passed, $fail failed, $skip skipped"
    exit 1
fi

# --- the checks every platform has to satisfy ------------------------------
# Written once, run identically on the host and in each container. $HOME is
# whatever the caller set, so this is agnostic about where it is running.
read -r -d '' checks <<'CHECKS' || true
set -u
bin="$HOME/.cargo/bin/ecphory"
[ -x "$bin" ]                                  || { echo "no binary at $bin"; exit 1; }
[ -f "$HOME/.config/ecphory/ecphory-receipt.json" ] || { echo "no receipt"; exit 1; }
[ -f "$HOME/.cargo/env" ]                      || { echo "no env shim"; exit 1; }
got=$("$bin" --version 2>/dev/null) || { echo "binary will not run"; exit 1; }
[ "$got" = "ecphory $EXPECT" ]                 || { echo "version: got '$got' want 'ecphory $EXPECT'"; exit 1; }
# The PATH advice the installer printed has to actually work.
. "$HOME/.cargo/env"
resolved=$(command -v ecphory) || { echo "PATH advice did not put ecphory on PATH"; exit 1; }
[ "$resolved" = "$bin" ]                       || { echo "PATH resolves to $resolved, not $bin"; exit 1; }
# A cold store must accept the very first write and read it back.
ecphory add "install smoke test" --name smoke >/dev/null 2>&1 || { echo "add failed"; exit 1; }
ecphory search "install smoke" 2>/dev/null | grep -q '"count": 1' || { echo "search failed"; exit 1; }
echo OK
CHECKS

# --- macOS host row --------------------------------------------------------
# Redirect HOME so nothing lands in the real one, and unset CARGO_HOME so the
# installer takes its default ($HOME/.cargo) — which is what a stranger hits.
if [ "$(uname -s)" = "Darwin" ]; then
    tmp_home=$(mktemp -d)
    trap 'rm -rf "$tmp_home"' EXIT
    if out=$(env -u CARGO_HOME HOME="$tmp_home" EXPECT="$version" bash -c "
            set -o pipefail
            curl --proto '=https' --tlsv1.2 -LsSf '$url' | sh >/dev/null || exit 1
            $checks" 2>&1); then
        # Gatekeeper: curl does not set the quarantine xattr, so the binary
        # runs. A browser download would be quarantined — that is the case
        # the README's `xattr -c` note is for, not this one.
        if xattr "$tmp_home/.cargo/bin/ecphory" 2>/dev/null | grep -q 'com.apple.quarantine'; then
            report FAIL "macos-$(uname -m)" "binary carries com.apple.quarantine"
        else
            report PASS "macos-$(uname -m)" "no quarantine xattr; adhoc-signed binary runs"
        fi
    else
        report FAIL "macos-$(uname -m)" "${out##*$'\n'}"
    fi
else
    report SKIP "macos" "not running on Darwin"
fi

# --- Linux container rows --------------------------------------------------
# Each entry is "label|image|prep". The prep step installs only what is
# needed to RUN the one-liner (curl), never a decompressor — supplying xz
# would paper over the exact failure this matrix exists to detect.
linux_rows=(
    "debian-no-xz|debian:bookworm-slim|apt-get -qq update >/dev/null 2>&1; apt-get -qq install -y curl ca-certificates >/dev/null 2>&1"
    "ubuntu-no-xz|ubuntu:24.04|apt-get -qq update >/dev/null 2>&1; apt-get -qq install -y curl ca-certificates >/dev/null 2>&1"
    "alpine-busybox|alpine:3.20|true"
    "fedora|fedora:40|true"
)

if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    for row in "${linux_rows[@]}"; do
        IFS='|' read -r label image prep <<<"$row"
        # alpine has no curl, only busybox wget — which exercises the
        # installer's wget fallback, worth keeping as its own row.
        # SC2016 is deliberate below: $URL must reach the container
        # unexpanded and be resolved by the shell inside it.
        # shellcheck disable=SC2016
        fetch='curl --proto "=https" --tlsv1.2 -LsSf "$URL" -o /tmp/i.sh'
        # shellcheck disable=SC2016
        [ "$label" = "alpine-busybox" ] && fetch='wget -q "$URL" -O /tmp/i.sh'
        if out=$(docker run --rm -e URL="$url" -e EXPECT="$version" "$image" sh -c "
                $prep
                $fetch || exit 1
                sh /tmp/i.sh >/dev/null || exit 1
                $checks" 2>&1); then
            report PASS "$label" "$(docker run --rm "$image" sh -c 'command -v xz >/dev/null && echo "has xz" || echo "no xz on the box"' 2>/dev/null)"
        else
            report FAIL "$label" "${out##*$'\n'}"
        fi
    done
else
    for row in "${linux_rows[@]}"; do
        report SKIP "${row%%|*}" "docker unavailable"
    done
fi

echo
echo "summary: $pass passed, $fail failed, $skip skipped"
[ "$fail" -eq 0 ]
