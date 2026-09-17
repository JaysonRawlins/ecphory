#!/usr/bin/env bash
# Render a submission-ready winget manifest set from a published release.
#
# Only the FIRST submission to microsoft/winget-pkgs is done this way; after
# that winget-releaser (.github/workflows/winget.yml) bumps the version on
# every release. This script stays because the first submission needs it, it
# is the fallback if that action breaks, and it is the only way to check the
# manifest shape without cutting a release.
#
# The sha256 is read from the release's own .sha256 asset rather than
# recomputed locally: that is the file winget's validation pipeline is
# effectively checked against, so reading it catches a mismatch between what
# dist published and what we claim.
#
# Usage:
#   docs/packaging/winget/render-manifests.sh [VERSION] [OUT_DIR]
#
#   VERSION   defaults to the version in Cargo.toml (no leading "v")
#   OUT_DIR   defaults to a temp dir; prints the path when done
#
# Requires: gh (authenticated). Works against a private repo — rendering is
# not what is blocked by visibility; submission is.

set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
tpl_dir="$repo_root/docs/packaging/winget/manifests"
asset="ecphory-x86_64-pc-windows-msvc.zip"

version=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -1)}
version=${version#v}
tag="v${version}"

out_dir=${2:-$(mktemp -d)}
# winget-pkgs expects manifests/<first-letter-lowercased>/<Publisher>/<Package>/<Version>/
dest="$out_dir/manifests/j/JaysonRawlins/ecphory/$version"
mkdir -p "$dest"

echo "==> release $tag"
if ! gh release view "$tag" >/dev/null 2>&1; then
  echo "error: release $tag not found. Cut the release before rendering manifests." >&2
  exit 1
fi

echo "==> reading $asset.sha256 from the release"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
gh release download "$tag" -p "$asset.sha256" -D "$tmp" --clobber
sha256=$(awk '{print $1}' "$tmp/$asset.sha256")

if [[ ! $sha256 =~ ^[A-Fa-f0-9]{64}$ ]]; then
  echo "error: could not read a sha256 from $asset.sha256 (got: '$sha256')" >&2
  exit 1
fi

# winget wants the digest uppercase.
sha256=$(printf '%s' "$sha256" | tr '[:lower:]' '[:upper:]')

release_date=$(gh release view "$tag" --json publishedAt --jq '.publishedAt[0:10]')

echo "==> version=$version sha256=$sha256 date=$release_date"

# Everything except the schema header is a note to ourselves; the submitted
# manifest carries the header and the body only.
for tpl in "$tpl_dir"/*.yaml; do
  name=$(basename "$tpl")
  sed -e "s/__VERSION__/$version/g" \
      -e "s/__SHA256__/$sha256/g" \
      -e "s/__RELEASE_DATE__/$release_date/g" \
      "$tpl" \
  | awk '
      /^# yaml-language-server:/ { print; next }
      /^#/                       { next }
      NF == 0 && !seen           { next }
      { seen = 1; print }
    ' \
  > "$dest/$name"
done

if grep -rn '__[A-Z_]*__' "$dest"; then
  echo "error: unfilled placeholders remain (above)" >&2
  exit 1
fi

# Validate against the published winget schemas when we can. `winget validate`
# is Windows-only, so without this the first feedback on a malformed manifest
# is the winget-pkgs PR. It earns its keep: it caught an unquoted ReleaseDate
# that YAML parses as a date object where the schema wants a string.
if python3 -c 'import yaml, jsonschema' 2>/dev/null; then
  echo "==> validating against winget 1.12.0 schemas"
  schema_dir="$tmp/schemas"
  mkdir -p "$schema_dir"
  ok=1
  for t in version installer defaultLocale; do
    curl -sS -L --max-time 30 -o "$schema_dir/$t.json" \
      "https://aka.ms/winget-manifest.$t.1.12.0.schema.json" || ok=0
  done
  if [ "$ok" = 1 ]; then
    python3 - "$dest" "$schema_dir" <<'PYEOF'
import json, sys, pathlib, yaml, jsonschema

dest, schema_dir = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
pairs = [
    ("JaysonRawlins.ecphory.yaml", "version"),
    ("JaysonRawlins.ecphory.installer.yaml", "installer"),
    ("JaysonRawlins.ecphory.locale.en-US.yaml", "defaultLocale"),
]
failed = False
for fname, sname in pairs:
    doc = yaml.safe_load((dest / fname).read_text())
    schema = json.loads((schema_dir / f"{sname}.json").read_text())
    errors = sorted(
        jsonschema.Draft7Validator(schema).iter_errors(doc),
        key=lambda e: list(e.path),
    )
    if errors:
        failed = True
        print(f"    FAIL {fname}")
        for e in errors:
            where = "/".join(map(str, e.path)) or "<root>"
            print(f"      - {where}: {e.message}")
    else:
        print(f"    ok   {fname}")
sys.exit(1 if failed else 0)
PYEOF
  else
    echo "    (skipped: could not fetch schemas)" >&2
  fi
else
  echo "==> skipping schema validation (need python3 with pyyaml + jsonschema)"
fi

echo "==> rendered into $dest"
ls -1 "$dest"
cat <<EOF

Next:
  1. Validate on Windows:   winget validate --manifest "$dest"
  2. Test the install:      winget install --manifest "$dest"
     (Windows Sandbox: winget-pkgs/Tools/SandboxTest.ps1 "$dest")
  3. Submit: copy the manifests/ tree into a fork of microsoft/winget-pkgs
     and open a PR containing manifest files only.

Submission requires the installer URL to be anonymously downloadable —
verify before submitting:
  curl -sSI -o /dev/null -w '%{http_code}\\n' -L \\
    https://github.com/JaysonRawlins/ecphory/releases/download/$tag/$asset
  # must be 200; 404 means the repo is still private
EOF
