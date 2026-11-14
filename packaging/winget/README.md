# winget manifest for Ninho

Package identifier: `Phmrmkv.Ninho`. Ninho ships as a single portable exe, so the installer type is `portable`
and winget adds the `ninho` command to the PATH.

Files:

- `Phmrmkv.Ninho.yaml` (version)
- `Phmrmkv.Ninho.locale.en-US.yaml` (name, description, tags, license)
- `Phmrmkv.Ninho.installer.yaml` (x64 and x86 download links and hashes)

## Fill in a release

1. Open the release page and download `SHA256SUMS.txt`. It has one line per file:
   `<hash>  Ninho-x64.exe` and `<hash>  Ninho-x86.exe`. (The "Packaging hashes" workflow also prints them in its
   job summary.)
2. In `Phmrmkv.Ninho.installer.yaml` replace each `REPLACE_WITH_SHA256` with the hash of the matching exe, and
   `REPLACE_WITH_RELEASE_DATE` with the release date (`YYYY-MM-DD`). winget wants the hash in upper case or lower
   case, either works.
3. Set `PackageVersion` to the new version in all three files (and the version in the URLs).
4. Test it:

   ```powershell
   winget validate --manifest packaging\winget
   winget settings --enable LocalManifestFiles
   winget install --manifest packaging\winget
   ninho status
   ```

## Submit to microsoft/winget-pkgs

First submission (the manifests go under `manifests/p/Phmrmkv/Ninho/<version>/`):

```powershell
winget install Microsoft.WingetCreate
wingetcreate submit packaging\winget
```

Later releases are simpler: `wingetcreate update` downloads both exes, computes the hashes and opens the pull request
for you (it needs a GitHub token the first time, `wingetcreate token --store`):

```powershell
wingetcreate update Phmrmkv.Ninho --version 0.6.0 `
  --urls https://github.com/phmrmkv/Ninho/releases/download/v0.6.0/Ninho-x64.exe `
         https://github.com/phmrmkv/Ninho/releases/download/v0.6.0/Ninho-x86.exe `
  --submit
```

The winget-pkgs bot validates the package and a moderator approves it, usually within a few days.
