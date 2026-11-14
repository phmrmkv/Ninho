# Scoop manifest for Ninho

`ninho.json` installs the portable exe as `ninho.exe` (the `#/ninho.exe` at the end of each URL renames the
download), puts the `ninho` command on the PATH and adds a Start menu shortcut. Scoop picks the 64-bit or the 32-bit
build by itself.

## Put it in a bucket

1. Create a GitHub repository named `scoop-ninho` (any name starting with `scoop-` is the convention), or use an
   existing bucket.
2. Copy `ninho.json` to its `bucket/` folder (or the root of the repository).
3. Fill the hashes of the release in `ninho.json`. They are in the release's `SHA256SUMS.txt`
   (the "Packaging hashes" workflow also prints them). Replace both `REPLACE_WITH_SHA256`.
4. Users install it with:

   ```powershell
   scoop bucket add ninho https://github.com/paulohmaciel/scoop-ninho
   scoop install ninho
   ```

## Test locally

```powershell
scoop install .\packaging\scoop\ninho.json
ninho status
```

## Keep it up to date

`checkver` reads the latest GitHub release and `autoupdate` rebuilds the URLs and takes the hashes from
`SHA256SUMS.txt`. In a bucket made from the Scoop bucket template, a scheduled GitHub Action runs `checkver -u`
and commits the new version. By hand: `.\bin\checkver.ps1 -App ninho -Update` in a clone of the Scoop installation.
