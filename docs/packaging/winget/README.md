# winget manifest skeleton

Submission is blocked until the repo is public (winget's validation
pipeline must download the installer URL anonymously; private-repo release
assets 404 without auth). Decision record and flip-day steps:
[docs/RELEASING.md](../../RELEASING.md).

First submission (manual, once):

```powershell
wingetcreate new https://github.com/JaysonRawlins/ecphory/releases/download/vX.Y.Z/ecphory-x86_64-pc-windows-msvc.zip
# PackageIdentifier: JaysonRawlins.ecphory
# InstallerType: zip, NestedInstallerType: portable
```

Then automate version bumps with
[winget-releaser](https://github.com/vedantmgoyal9/winget-releaser)
(requires a fork of microsoft/winget-pkgs + classic PAT with public_repo;
override `installers-regex` to match the `.zip` asset).

`ecphory.installer.yaml` sketch — wingetcreate generates the real thing:

```yaml
PackageIdentifier: JaysonRawlins.ecphory
PackageVersion: X.Y.Z
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
  - RelativeFilePath: ecphory-x86_64-pc-windows-msvc\ecphory.exe
    PortableCommandAlias: ecphory
Installers:
  - Architecture: x64
    InstallerUrl: https://github.com/JaysonRawlins/ecphory/releases/download/vX.Y.Z/ecphory-x86_64-pc-windows-msvc.zip
    InstallerSha256: <from sha256.sum on the release>
ManifestType: installer
ManifestVersion: 1.6.0
```
