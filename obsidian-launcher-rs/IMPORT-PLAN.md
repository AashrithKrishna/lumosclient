# Importing instances from other launchers (plan)

Goal: a player coming from another launcher finds their instances listed in Lumos (name,
Minecraft version, loader, mod / world counts), ticks the ones they want, and gets a new Lumos
instance for each, with a **copy** of their worlds, packs and settings. Nothing in the other
launcher's folder is changed, moved or deleted.

Code: `src/import.rs` (finding and copying), `src/sqlite_ro.rs` (read-only SQLite reader for the
Modrinth App), IPC in `src/main.rs` (`import_scan`, `import_run`, `import_cancel`), UI in
`src/ui.html` (Library › "Import from launcher", ⌘K, the getting-started guide).

## How Lumos stores an instance (so an imported one is a normal instance)

- Launcher folder: `~/LumosClient` (`AppState::base_dir`), settings in `launcher.json`
  (`LauncherConfig`, written atomically by `AppState::save_config`).
- `config.profiles: Vec<Profile>`; a profile is `{ id, name, icon, version, … }`. `id` is
  `prof-<epoch ms>` (with `x` appended until unique), exactly as `add_profile` / the modpack import
  make it. Its game folder is `instances/<id>/` (`AppState::target_instance`).
- `version` is a Lumos version spec, parsed by `installer::parse_spec`:
  - vanilla: the plain Mojang id (`1.21.11`, `1.8.9`);
  - `Fabric <mc>` (Fabric supports 1.14+ and every year-numbered version, `installer::fabric_supports`);
  - `Forge <mc>` (1.12.2, 1.13+, 26+; `forge::spec_supported`);
  - `NeoForge <mc>` (1.20.2+, 1.21+, 26+);
  - `Horizon 26.3 (Rust)` (`HORIZON_ID`), the native 26.3 game.
  Lumos installs the loader version it picks itself (latest stable Fabric, recommended Forge,
  latest NeoForge), not the exact one the other launcher used.
- A Java instance's folder is a normal `.minecraft`-style game dir: `mods/`, `saves/`,
  `resourcepacks/`, `shaderpacks/`, `config/`, `options.txt`, `servers.dat`, `screenshots/`.
- A Horizon instance's folder uses the same `saves/`, `resourcepacks/`, `shaderpacks/` (`.zip`
  classic packs and `.hshader`), `options.txt`, `servers.dat`, `screenshots/`; its mods are Horizon
  mods in `horizon-mods/` (`.hmod`) and native mods, so Java `.jar` mods never apply.
- The import adds the profile only after the copy finished, makes it the active instance, saves
  `launcher.json`, and pushes the state, as the modpack import does. A failed or cancelled copy
  removes the (brand-new) folder it was writing.

## Launchers

`data` below is `dirs::data_dir()`: `~/Library/Application Support` on macOS, `%APPDATA%`
(Roaming) on Windows. Only macOS and Windows are handled.

### Official Minecraft launcher

- Folder: macOS `data/minecraft`, Windows `%APPDATA%\.minecraft` (the same as
  `installer::official_minecraft_dir`).
- Metadata: `launcher_profiles.json` → `profiles` (object of id → profile). Fields read:
  `name` (empty for the built-in ones), `type` (`custom`, `latest-release`, `latest-snapshot`),
  `lastVersionId`, `gameDir` (absent = the launcher folder itself).
- Version: `lastVersionId` is an id of `versions/<id>/<id>.json`. If that JSON has
  `inheritsFrom`, that is the Minecraft version and the loader comes from the id:
  `fabric-loader-<v>-<mc>` → Fabric, `quilt-loader-<v>-<mc>` → Quilt, `neoforge-<v>` → NeoForge,
  `<mc>-forge-<v>` → Forge, an id containing `OptiFine` → OptiFine; also library names
  (`net.fabricmc:fabric-loader`, `org.quiltmc:quilt-loader`, `net.neoforged`,
  `net.minecraftforge`, `optifine:OptiFine`) when the id says nothing. Without `inheritsFrom`
  the id is a vanilla version.
  `latest-release` → the newest release in Lumos's version list (known once the manifest has
  loaded); `latest-snapshot` → not imported (Lumos doesn't offer snapshots and a snapshot world
  must not be opened in an older release).
- Shared folder: profiles without `gameDir` share the launcher folder, which also holds
  `versions/`, `libraries/`, `assets/`. Only the listed game items are ever copied, so those are
  never touched.
- Edge cases seen on this Mac: TLauncher-created ids like `Fabric 1.21.11` / `OptiFine 1.21.11`
  in `versions/` (handled by the JSON `inheritsFrom` + library check); the two built-in profiles
  have empty names (shown as "Latest release" / "Latest snapshot").

### Prism Launcher / MultiMC

- Folder: Prism macOS `data/PrismLauncher`, Windows `%APPDATA%\PrismLauncher`. MultiMC on macOS
  keeps its data inside the app (`MultiMC.app/Data`, from MultiMC's `Application.cpp`:
  `applicationDirPath()/../../Data`), so `/Applications/MultiMC.app/Data` and
  `~/Applications/MultiMC.app/Data` are checked. MultiMC on Windows is a portable folder with no
  fixed place: not found automatically.
- Instances folder: `InstanceDir` in `prismlauncher.cfg` / `multimc.cfg` (default `instances`,
  relative to the data folder or absolute).
- Per instance `instances/<x>/`: `instance.cfg` (INI; `name=`), `mmc-pack.json` →
  `components[]` with `uid`, `version` (or `cachedVersion`), `disabled`. Uids:
  `net.minecraft` (game), `net.fabricmc.fabric-loader`, `org.quiltmc.quilt-loader`,
  `net.minecraftforge`, `net.neoforged`, `com.mumfrey.liteloader` (Prism's
  `Component::KNOWN_MODLOADERS`). Disabled components are ignored.
- Game folder: `minecraft/`, or `.minecraft/` when only that exists (Prism's
  `MinecraftInstance::gameRoot`).

### Modrinth App

- Folder: macOS `data/ModrinthApp`, Windows `%APPDATA%\ModrinthApp` (Tauri identifier
  `ModrinthApp`); before 0.8 `data/com.modrinth.theseus`.
- Instances live in `<config_dir>/profiles/<path>/`; `config_dir` is the app folder unless
  `settings.custom_dir` is set (app-lib `state/dirs.rs`). The game folder is that profile folder.
- Metadata in `app.db` (SQLite, WAL mode). Two schemas exist (app-lib `migrations/`):
  - since 2026-06 (`instances-content-foundation`, `finish-instance-profile-migration`, which
    drops `profiles`): `instances(id, path, name, applied_content_set_id, …)` and
    `instance_content_sets(id, game_version, loader, loader_version, …)`, joined on
    `instances.applied_content_set_id = instance_content_sets.id`;
  - before: `profiles(path, name, game_version, mod_loader, mod_loader_version, …)`.
  Loader values: `vanilla`, `fabric`, `forge`, `neoforge`, `quilt`.
- Very old versions kept `profiles/<x>/profile.json` (`metadata.name`, `metadata.game_version`,
  `metadata.loader`): read when there is no database row.
- Reading SQLite: Cargo.toml has no SQLite crate. Adding `rusqlite` (bundled C SQLite) would also
  need a write-capable open to see un-checkpointed WAL pages (it creates/updates `app.db-shm`
  in the Modrinth folder). Instead `src/sqlite_ro.rs` is a small read-only reader of the file
  format (sqlite.org/fileformat2.html): table b-trees, overflow pages, records, the schema
  table, and committed `app.db-wal` frames (salt + checksum checked). It never opens anything
  for writing. Unreadable database → instances are listed by folder with "unknown version" and
  not importable.
- Newer versions may store options.txt and content as symlinks into the app's own store
  (`synced-options/`, `store/`): links are followed only when they resolve inside the Modrinth
  folder.

### CurseForge app

- Folder: macOS `~/Documents/curseforge/minecraft/Instances`, Windows
  `%USERPROFILE%\curseforge\minecraft\Instances` (the CurseForge support articles; the user can move
  it in the app's settings, which isn't followed).
- Per instance `Instances/<x>/minecraftinstance.json` (may start with a BOM): `name`,
  `gameVersion`, `baseModLoader.name` (`fabric-0.16.14-1.21.5`, `forge-47.2.0`,
  `neoforge-21.1.77`, `quilt-…`; null for vanilla). Fallback `manifest.json`
  (`minecraft.version`, `minecraft.modLoaders[].id`, primary first).
- Game folder: the instance folder itself.

### SKLauncher

- Folder: macOS `data/sklauncher` (this Mac), Windows `%APPDATA%\sklauncher` (Setup build);
  other builds keep `sklauncher/` inside the official `.minecraft`, also checked.
- Metadata (read from the real file here): `instances.json` →
  `instances[]` with `id`, `name`, `type` (`latest-release`, `latest-snapshot`, `custom`),
  `gameType` (`vanilla`, `fabric`, `forge`, also `quilt`/`neoforge` mapped by name),
  `minecraftVersion` (`26.3`, `1.21.11`, `1.8.9`, or `latest-release`/`latest-snapshot`),
  `loaderVersion`, `versionId`, `directory` (absolute game folder).
- Game folder: `directory` when it is an existing folder, else `instances/<id>/`.
- Real data on this Mac: `26-3` (Fabric 26.3, 109 mods, 4 worlds ≈1 GB, 11 resource packs,
  6 shader packs incl. one unpacked folder), `pvp` and `1-21-11-survival` (Fabric 1.21.11),
  `forge-1-8-9` (Forge 1.8.9), and the two empty `latest-*` instances.
- `settings.json` and `accounts.json` hold keys/accounts: never read.

### ATLauncher (cheap, so included)

- Folder: `data/ATLauncher/instances/<x>/` (macOS and the Windows installer default).
- `instance.json` at the instance root: `id` (Minecraft version), `launcher.name`,
  `launcher.loaderVersion.type` (`Fabric`, `Forge`, `NeoForge`, `Quilt`). The instance folder is
  the game folder.

### GDLauncher: not included

Its current version (Carbon) keeps instance metadata in its own database layout that isn't
documented; not cheap to do right.

## Mapping to a Lumos version

| Source | Lumos version | Mods |
|---|---|---|
| Minecraft 26.3 (any loader) | Horizon 26.3 | Skipped: Java mods don't run in Horizon |
| Vanilla `<mc>` | `<mc>` | – |
| Fabric `<mc>`, Fabric supports it | `Fabric <mc>` | Copied |
| Forge `<mc>`, Lumos installs it | `Forge <mc>` | Copied |
| NeoForge `<mc>`, Lumos installs it | `NeoForge <mc>` | Copied |
| Forge / NeoForge on a version Lumos can't install (e.g. Forge 1.8.9) | `<mc>` (vanilla) | Skipped, said so |
| Quilt `<mc>` | `Fabric <mc>` (else `<mc>`) | Skipped: Lumos can't install Quilt |
| OptiFine / LiteLoader / unknown loader | `<mc>` | Skipped, said so |
| Unknown version, latest snapshot | not importable | – |

The version is never changed to an older one (opening a world in an older version can damage it):
every fallback keeps the same Minecraft version. The UI shows the target and every note before
the player imports, and an instance with a fallback is unticked by default (the player ticks it to
import worlds/packs only, or leaves it to skip).

## What is copied

From the game folder into the new `instances/<id>/`:

- Always: `saves/` (whole worlds), `resourcepacks/`, `shaderpacks/` (files and unpacked
  folders), `options.txt`, `servers.dat`, `screenshots/`.
- Java instances only: `schematics/`; `config/` only when the mods are copied (it is the mods'
  configuration).
- Mods: only when the loader matches (table above): the top-level `*.jar` and `*.jar.disabled`
  files of `mods/` (Lumos and Prism both switch a mod off as `.disabled`).
- Never: logs, crash reports, caches, the other launcher's own files, anything else.

Counts shown in the list: mods = `mods/*.jar(.disabled)`, worlds = folders in `saves/` with a
`level.dat`, plus resource / shader pack counts.

## Safety

- Read-only on the source: files are opened for reading only; nothing is created, renamed or
  deleted there (SQLite read without `-shm`).
- Symlinks: walked with `symlink_metadata`; a link is followed only when its canonical target is
  inside the source launcher's folder or the instance's own game folder; otherwise skipped (and
  counted in the summary). Loops are stopped by remembering visited canonical folders.
- Destination: a brand-new `instances/<new id>/` that must not exist yet; every written path is
  that folder joined with plain names read from the source; files are created with
  `create_new` (never overwrite). On error/cancel the new folder is removed and no profile is
  added.
- The UI never sends paths: it sends the keys of the last scan; the backend keeps the scan.

## Progress, threads

- `import_scan` and `import_run` run on their own threads. Scanning only lists folders and reads
  small metadata files (no size walk).
- `import_run` marks the launcher busy (`AppState::task`, as modpack imports do: no launch while
  copying), first lists the files to copy (sizes), then copies with byte progress through
  `TaskReporter` (the task strip: "Copying <name> · 42%"), several instances one after another.
  `import_cancel` stops it between files (and inside a large file, per 1 MB chunk).
- At the end `onImportDone` lists, per instance, what was copied, what was skipped and why.

## Tests (`cargo test`)

Temp-folder fixtures for each format: official (custom + latest profiles, loader version JSON,
shared root), Prism (`.minecraft` vs `minecraft`, disabled component, `InstanceDir`),
Modrinth (new schema, old schema, WAL-only rows, `profile.json`), CurseForge (BOM,
`manifest.json` fallback), SKLauncher (the real file's shape), ATLauncher; mapping table;
the copy (mods only for matching loader, Horizon skips jars/config, symlink outside skipped,
symlink inside followed, never overwrites, cancel cleans up). SQLite fixtures are made with the
system `python3`'s sqlite3 module when it exists (skipped otherwise).
