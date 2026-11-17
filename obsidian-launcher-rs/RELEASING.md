# Releasing Lumos Client

LumosClient updates itself from the latest GitHub release of
[AashrithKrishna/lumosclient](https://github.com/AashrithKrishna/lumosclient/releases) (`src/updater.rs`).
It only installs a release automatically when that release publishes a SHA-256 for the package; without one
it opens the release page instead. So every release needs the checksums below.

## Steps

1. Set the new version in `obsidian-launcher-rs/Cargo.toml` (`version = "X.Y.Z"`).
2. Build the Mac app: `./obsidian-launcher-rs/build_app.sh`
   → `dist/Lumos.dmg`, `dist/Lumos.dmg.sha256`, `dist/SHA256SUMS`.
3. Package Windows from the `lumos windows` workflow's programs:
   `./obsidian-launcher-rs/package_windows.sh <folder with the three .exe files>`
   → `dist/Lumos-Windows-x64.zip`, `dist/Lumos-Windows-x64.zip.sha256`, and `dist/SHA256SUMS` rewritten to list
   both packages.
4. Check: `(cd dist && shasum -a 256 -c SHA256SUMS)` prints `OK` for both files.
5. Publish (not as a draft or pre-release; the updater reads `releases/latest`):

   ```sh
   gh release create vX.Y.Z dist/Lumos.dmg dist/Lumos-Windows-x64.zip dist/SHA256SUMS \
     --repo AashrithKrishna/lumosclient --title "Lumos Client X.Y.Z" --notes-file notes.md
   ```

   The release notes appear in the launcher's "What's new" panel (plain text).

## What the updater relies on

- The tag is `vX.Y.Z` (semver; `-beta.1` style suffixes sort before the release) and is newer than the
  running build's `Cargo.toml` version.
- Asset names stay exactly `Lumos.dmg` and `Lumos-Windows-x64.zip`.
- `SHA256SUMS` lines are `shasum -a 256` output (`<hex>  <file name>`). A per-file `<asset>.sha256` asset
  works too if `SHA256SUMS` is missing.
- The DMG has `Lumos.app` at its root, and its `Info.plist` `CFBundleShortVersionString` equals the tag's
  version (the updater refuses an app with another version).
- The Windows zip has a `Lumos/` folder with `Lumos.exe` and `resources/`.

Releases made before the updater existed (v0.3.0 and older) can't update themselves: players install the
first updater build by hand once. Checksums prove the download is the file published on the release; they
don't prove who published it, so keep the GitHub account secure.

## Testing a build without the startup check

Set `LUMOS_NO_UPDATE_CHECK=1` to skip the check at startup (Settings › About › Check for updates still works).
