# Releasing ecphory

The release pipeline is push-button: conventional commits on `main` drive
everything. Two tools split the work:

- **[release-plz](https://release-plz.dev)** (`release-plz.toml`,
  `.github/workflows/release-plz.yml`) — on every push to `main` it
  maintains a **release PR** carrying the semver bump (derived from
  conventional commits) and the `CHANGELOG.md` update. Merging that PR makes
  release-plz push the `vX.Y.Z` tag. It does **not** create the GitHub
  Release (`git_release_enable = false`) and does **not** publish to
  crates.io while the repo is private (`publish = false`, `git_only = true`).
- **[dist / cargo-dist](https://github.com/axodotdev/cargo-dist)**
  (`dist-workspace.toml`, `.github/workflows/release.yml`, generated — edit
  the toml and run `dist generate`, never the yml) — triggers on the tag
  push, cross-builds every target, and creates the GitHub Release with:
  - `ecphory-aarch64-apple-darwin.tar.xz`, `ecphory-x86_64-apple-darwin.tar.xz`
  - `ecphory-aarch64-unknown-linux-musl.tar.xz`, `ecphory-x86_64-unknown-linux-musl.tar.xz`
    (static binaries — no glibc floor, run on any Linux back to ~2014)
  - `ecphory-x86_64-pc-windows-msvc.zip`
  - `ecphory-installer.sh`, `ecphory-installer.ps1`, `ecphory.rb` (Homebrew
    formula), `sha256.sum` + per-artifact `.sha256`

## Normal release

1. Land conventional commits on `main` (`feat:` → minor, `fix:` → patch,
   `feat!:`/`BREAKING CHANGE` → major while >1.0; pre-1.0 majors bump minor).
2. Review the open **release PR** (version + changelog) and merge it.
3. release-plz tags; dist builds and publishes the GitHub Release. Done.

## Secrets (names only — values live in GitHub repo secrets)

| Secret | Needed for | Notes |
| --- | --- | --- |
| `RELEASE_APP_ID` + `RELEASE_APP_PRIVATE_KEY` | release-plz jobs | The `ecphory-release-bot` GitHub App (must be installed on this repo). **Required for push-button flow**: tags pushed by the default `GITHUB_TOKEN` cannot trigger `release.yml`; app-minted tokens can. Same pattern as claude-gavel's `gavel-release-bot`. |
| `HOMEBREW_TAP_TOKEN` | homebrew publish job | PAT with push to `JaysonRawlins/homebrew-tap` — or install `ecphory-release-bot` on the tap and mint a token in a pre-step (needs `allow-dirty = ["ci"]` to customize dist's release.yml; PAT is less invasive). Only needed once the publish job is enabled (public flip). |
| `CARGO_REGISTRY_TOKEN` | crates.io publish | Only at/after public flip, and only for the first publish if Trusted Publishing is set up afterwards. |
| `WINGET_TOKEN` | winget publish job | **Classic** PAT with `public_repo` scope (winget-releaser rejects fine-grained tokens), plus a fork of `microsoft/winget-pkgs` on the same account. Paired with the `WINGET_ENABLED` repo *variable*, which keeps `winget.yml` inert until both exist. Only after the first manual submission merges. |

## Rehearsal

Push an rc tag from a green commit: `git tag v0.3.0-rc.N && git push origin
v0.3.0-rc.N`. dist treats it as a prerelease: full builds, a prerelease
GitHub Release, no Homebrew/crates publish. Delete the tag + release after.
PRs also run dist's `plan` step as a cheap config check.

Both former private-repo limits are gone as of the 2026-09-16 flip:
`ecphory-installer.sh` can now fetch release assets anonymously (it was
never broken — the repo being private was the only thing stopping it), and
the Homebrew publish job is no longer blocked by a tap that must not
reference private URLs. Enabling that job is step 3 below.

## Public-flip-day checklist

Steps 1 and 2 are the flip itself and are **done** (2026-09-16); 3–6
remain. Smoke-test a real `curl | sh` install from a clean machine before
working through them — it is the one thing the private repo could never
prove.

1. ~~Flip repo visibility to public.~~ Done 2026-09-16.
2. ~~Decide the contribution policy.~~ Done: issues yes, external PRs
   auto-closed — see [CONTRIBUTING.md](../CONTRIBUTING.md). Branch
   protection is written but **not applied**; the payloads and the
   reasoning are in [.github/rulesets/](../.github/rulesets/README.md).
3. `dist-workspace.toml`: add `publish-jobs = ["homebrew"]` and
   `github-attestations = true`; run `dist generate`; add
   `HOMEBREW_TAP_TOKEN` secret. Formula lands in
   `JaysonRawlins/homebrew-tap/Formula/ecphory.rb` on the next release →
   `brew install jaysonrawlins/tap/ecphory`.
4. crates.io (`cargo install ecphory` channel): in `release-plz.toml` set
   `publish = true`, drop `git_only`; add `CARGO_REGISTRY_TOKEN`; the name
   `ecphory` was free as of 2026-07-12. Don't publish before the flip — the
   .crate file exposes the source.
5. Windows/winget: the manifests, the renderer and the publish workflow are
   already in the repo — see [packaging/winget](packaging/winget/) for the
   first submission (manual, once) and the automation that follows it. Set
   the `WINGET_ENABLED` repo variable and the `WINGET_TOKEN` secret only
   after that first submission merges. **Decision record: winget over
   Chocolatey** — choco is the only channel with per-version human
   moderation (days–weeks until "trusted"); winget auto-approves updates
   after the first merge and ships preinstalled on Win10/11. A personal
   Scoop bucket is a cheap optional third channel.
6. Announce; `cargo binstall ecphory` works with no extra config (dist's
   artifact naming is auto-detected).

## macOS note (manual installs only)

Homebrew strips the quarantine xattr itself. For a manually downloaded
binary Gatekeeper/AMFI may block or SIGKILL it: `xattr -c ecphory &&
codesign --force --sign - ecphory`. Real Apple codesigning/notarization is
future work; binaries are ad-hoc signed.
