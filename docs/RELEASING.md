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
| `RELEASE_PLZ_TOKEN` | release-plz jobs | Fine-grained PAT (this repo, contents:write + pull-requests:write). **Required for push-button flow**: tags pushed by the default `GITHUB_TOKEN` cannot trigger `release.yml`. Without it the release PR still appears, but the tag won't kick off builds. |
| `HOMEBREW_TAP_TOKEN` | homebrew publish job | PAT with push to `JaysonRawlins/homebrew-tap`. Only needed once the publish job is enabled (public flip). |
| `CARGO_REGISTRY_TOKEN` | crates.io publish | Only at/after public flip, and only for the first publish if Trusted Publishing is set up afterwards. |

## Rehearsal (works on the private repo)

Push an rc tag from a green commit: `git tag v0.3.0-rc.N && git push origin
v0.3.0-rc.N`. dist treats it as a prerelease: full builds, a prerelease
GitHub Release, no Homebrew/crates publish. Delete the tag + release after.
PRs also run dist's `plan` step as a cheap config check.

Private-repo limits (both fixed by flipping public): `ecphory-installer.sh`
can't fetch assets anonymously — override the download base with
`ECPHORY_INSTALLER_DOWNLOAD_URL` to test; and the public tap must not
reference private URLs, so the homebrew *publish* job stays disabled.

## Public-flip-day checklist

1. Flip repo visibility to public.
2. `dist-workspace.toml`: add `publish-jobs = ["homebrew"]` and
   `github-attestations = true`; run `dist generate`; add
   `HOMEBREW_TAP_TOKEN` secret. Formula lands in
   `JaysonRawlins/homebrew-tap/Formula/ecphory.rb` on the next release →
   `brew install jaysonrawlins/tap/ecphory`.
3. crates.io (`cargo install ecphory` channel): in `release-plz.toml` set
   `publish = true`, drop `git_only`; add `CARGO_REGISTRY_TOKEN`; the name
   `ecphory` was free as of 2026-07-12. Don't publish before the flip — the
   .crate file exposes the source.
4. Windows/winget: first submission is manual once assets are public —
   `wingetcreate new` against the release zip (portable type), then wire
   [winget-releaser](https://github.com/vedantmgoyal9/winget-releaser) into
   the release workflow for zero-touch version bumps. **Decision record:
   winget over Chocolatey** — choco is the only channel with per-version
   human moderation (days–weeks until "trusted"); winget auto-approves
   updates after the first merge and ships preinstalled on Win10/11. A
   personal Scoop bucket is a cheap optional third channel.
5. Announce; `cargo binstall ecphory` works with no extra config (dist's
   artifact naming is auto-detected).

## macOS note (manual installs only)

Homebrew strips the quarantine xattr itself. For a manually downloaded
binary Gatekeeper/AMFI may block or SIGKILL it: `xattr -c ecphory &&
codesign --force --sign - ecphory`. Real Apple codesigning/notarization is
future work; binaries are ad-hoc signed.
