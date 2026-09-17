# winget packaging

`winget install JaysonRawlins.ecphory` — not live yet, but no longer blocked.
Submission required the repo to be public, because winget's validation
pipeline downloads `InstallerUrl` anonymously; that landed 2026-09-16 and the
release assets now resolve:

```console
$ curl -sS -o /dev/null -w '%{http_code}\n' -L \
    https://github.com/JaysonRawlins/ecphory/releases/download/v0.3.6/ecphory-x86_64-pc-windows-msvc.zip
200
```

Everything else is ready and checked in. What remains is the first submission.

## Why winget and not Chocolatey

Chocolatey is the only channel with per-version *human* moderation — days to
weeks per submission until a package earns trusted status. winget
auto-approves updates once the first submission is merged, and ships
preinstalled on Windows 10 and 11, so it reaches users without asking them to
install a package manager first. A personal Scoop bucket is a cheap optional
third channel if anyone asks for one.

## What is here

| File | Purpose |
| --- | --- |
| `manifests/*.yaml` | The three-file manifest set, with `__VERSION__` / `__SHA256__` / `__RELEASE_DATE__` placeholders. |
| `render-manifests.sh` | Fills those placeholders from a published release and lays the files out the way winget-pkgs expects. |

winget requires a **multi-file** manifest set — singleton manifests are not
accepted in the community repository — so there are three files: `version`,
`installer`, and `defaultLocale`. They target schema **1.12.0**, the version
winget-pkgs currently recommends.

Rendering works today, private repo and all; it is only submission that is
gated:

```console
$ docs/packaging/winget/render-manifests.sh 0.3.6
==> version=0.3.6 sha256=7636EEF9...B0A5 date=2026-09-14
==> rendered into /tmp/.../manifests/j/JaysonRawlins/ecphory/0.3.6
```

The digest is read from the release's own `.sha256` asset rather than
recomputed, so a mismatch between what dist published and what the manifest
claims shows up here instead of in the winget PR.

## Installer shape

dist publishes a plain zip, not an installer, so the manifest is
`InstallerType: zip` + `NestedInstallerType: portable` — winget unpacks it and
puts `ecphory` on PATH through the portable shim.

The zip is **flat**:

```console
$ unzip -l ecphory-x86_64-pc-windows-msvc.zip
     4069  CHANGELOG.md
 15911936  ecphory.exe
     1094  LICENSE
     9038  README.md
```

so `RelativeFilePath` is `ecphory.exe` with no directory prefix. Worth
re-checking after any dist upgrade: a wrong path here passes `winget validate`
(the manifest is still well-formed) and fails later during install
validation, in the winget-pkgs PR, where the feedback loop is much slower.

## First submission — manual, once

winget-releaser can only *update* a package, so the first version has to be
submitted by hand. **No Windows machine is required for this**; the one
Windows-only step is an optional test, below.

1. Confirm the release's installer URL returns **200** anonymously. It does
   as of the 2026-09-16 flip; re-check per release, since this is what
   winget's validation pipeline will do.
2. Submit with [Komac](https://github.com/russellbanks/Komac), which is
   cross-platform (`brew install komac`) and is the same tool winget-releaser
   runs under the hood:
   ```sh
   komac new JaysonRawlins.ecphory --submit
   ```
   It prompts for the package metadata and opens the PR against a fork of
   winget-pkgs. It needs a **classic** GitHub token with `public_repo` scope —
   the same one `WINGET_TOKEN` will hold later. Answer its prompts from
   [`manifests/`](manifests/): those files exist so the metadata is decided
   and reviewed here rather than improvised at a prompt.

   To hand-assemble instead, `render-manifests.sh <version> ./out` produces the
   tree to copy into a winget-pkgs fork. Keep the PR to **manifest files only,
   one package version** — the two rules that fail first submissions most often.

### Testing the install (optional, needs Windows)

Worth doing once, because it catches the one failure the schema cannot:
a wrong `RelativeFilePath` passes `winget validate` and only breaks at install
time — in the winget-pkgs PR, if you skip this.

A Parallels/UTM VM works, and beats Windows Sandbox here since you can
snapshot before installing and revert between attempts:

```powershell
winget validate --manifest .\out\manifests\j\JaysonRawlins\ecphory\<version>
winget install  --manifest .\out\manifests\j\JaysonRawlins\ecphory\<version>
ecphory --version   # from a FRESH shell: proves the portable alias and PATH shim
```

No VM handy? The `windows-2025` GitHub runner image ships winget preinstalled,
so a throwaway `workflow_dispatch` job does the same thing. And skipping it
entirely is survivable — winget-pkgs validates every submission automatically;
you just get the feedback a round later.

## After that — automated

[`.github/workflows/winget.yml`](../../../.github/workflows/winget.yml) runs
[winget-releaser](https://github.com/vedantmgoyal9/winget-releaser) (Komac
under the hood) to bump the version on subsequent releases. It is inert until
the repo variable `WINGET_ENABLED` is set to `true`, and it needs a classic
PAT with `public_repo` scope in the `WINGET_TOKEN` secret plus a fork of
winget-pkgs on the same account. The action can only *update* a package, which
is why the first submission above has to happen by hand.

It is `workflow_dispatch` rather than `on: release` deliberately — dist creates
the Release with the default `GITHUB_TOKEN`, and GitHub does not start
workflows from events raised by that token. Making it zero-touch means wiring
it in as a dist custom publish job via `dist-workspace.toml` + `dist generate`;
until then it is one click after a release.
