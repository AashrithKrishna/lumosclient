# Lumos native mods

Horizon native mods written for Lumos Client. Each folder is one horizon-mixins bundle: what a
registry `.hnmod` keeps under `mixins/<id>/` (`horizon-mixin.toml`, the `.rs` files it names).
They are original Rust. Some do the same jobs as Java Fabric mods but share no code or assets
with them.

| Folder | What it does |
|---|---|
| `lumos_durability_tooltip` | Adds "Durability: 123 / 250" to the tooltip of every damageable item, coloured from green to red by how much is left. |
| `lumos_totem_counter` | Shows a HUD counter (totem icon and number) of the Totems of Undying you carry. It flashes when the count drops, turns gold when you are low, and shows a notice over the hotbar when one of your totems pops. |
| `lumos_particle_rain` | Turns rain into falling drop particles with splashes on the ground and snow into drifting flakes. Heavy weather can add a mist. It replaces vanilla's rain sheets and limits its own particle count. |
| `lumos_paginated_advancements` | Splits the Advancements screen's tabs into pages. Use the `<` `>` buttons, Page Up/Page Down or the mouse wheel over the tabs to reach every tab. |
| `lumos_hud` | Lunar-style HUD modules (FPS, CPS, coordinates, keystrokes, armour, potion effects, ping, clock, server, day, speed, reach, combo, item counter, memory, sprint, direction, biome) in the Lunar or Minimal style. Right Shift opens the editor: drag modules, right-click to hide, switch them on and off and pick the style. The layout is saved to `horizon-mods/.horizon-mod-data/lumos_hud/layout`. Its hotkeys (editor, HUD toggle) are rebindable on its Options > Mods page; `keys.rs` is the only file that reads them. |
| `lumos_freelook` | Hold a key (Left Alt by default) to look around freely while the player keeps facing, aiming and moving the way it did; letting go turns the camera back (eased or at once). From first person it shows third person while you look. Hold or toggle, invert, and first/third person on its page. |
| `lumos_map` | Minimap and world map in the game's own colours: each block's top texture averaged once and tinted with its biome's grass, foliage and water colours, slopes shaded and water darkened with depth (or the flat map palette). Minimap: square or round, size, opacity, zoom keys (= and -), turning or north-up with N/E/S/W, cave view (K) under your head underground and in the Nether, show/hide (H), arrows for players (with names) and hostile/passive mobs, and coordinate, biome, light and time lines. Waypoints: B adds one, sets with their own visibility, hidden-for-now, sorting by name/distance/newest, sharing to chat, coordinates picked up from chat, the last N deaths, initials, fading with distance and Overworld/Nether conversion. World map (M): explored chunks saved per world or server (sub-worlds told apart by spawn, or by hand), unloaded ground dimmed, Y-layer view, chunk grid, slime chunks where the seed is known, follow, a waypoint filter, WASD/arrow panning and export to PNG (`<instance>/lumos-map/exports/`). |
| `lumos_fullbright_key` | A key (G by default) that turns Horizon's own Fullbright (Video Settings > Quality) on and off, with a notice over the hotbar. On other people's servers, where Horizon holds Fullbright off, it says so instead. |

## Pin

All of them are planned and built against Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0`
| `lumos_discover` | Discover inside the game: Horizon's registry (mods, resource packs, shaders, with its categories and search) and Lumos's own mods, with what this instance has. Right Control or the pause menu's Discover button opens it. Packs and shaders install and apply at once; content mods with only assets and data switch live, others on restart; native mods are added or taken out by Lumos when the game closes (`lumos-requests.json`), and Restart to apply closes the game for Lumos to start it again. Downloads are checked against the index (size, SHA-256, the author's signature, the file's manifest). |
(Nightly 2026-11-13) with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`. The pin is set by
`[compatibility] horizon_revision` in each manifest. For another Horizon commit, re-plan the mods and
rebuild them. The hooks they use are listed in each manifest.

## Layout and files

```text
lumos-mods/<id>/
  horizon-mixin.toml   # modules + injections
  runtime.rs           # the rules: std only, unit tests inside
  glue.rs              # reads Horizon's Game, calls runtime
  keys.rs              # a mod with keys: every key it reads (id, label, default), and only here
  hooks/*.rs           # one small helper per injection
```

`runtime.rs` and `glue.rs` become modules of `crates/horizon-client/src/game.rs`, named `<id>` and
`<id>_glue`.

## On, off and settings

`lumos_hud` and `lumos_discover` also have `keys.rs`, a module (`<id>_keys`) with their hotkeys.

Every hook checks `game.native_on("<id>")` first. Each mod also adds itself to Options > Mods
(`native_mods`), so it can be switched off there. That writes `mod.<id>:false` to `horizon-mods.txt`
and takes effect at runtime with no rebuild. Each mod has its own settings page, saved with
`native_set_setting`.

## Keys

Horizon 9f42064c has no Controls-screen API for a native mod's keys. A mod with keys lists them in
`NativeMod::keys` (name, label, default such as `key.keyboard.g`): its page in Options > Mods shows
them as Key Binds rows and rebinds them there, kept as `key.<id>.<name>` in `horizon-mods.txt`. All
key handling goes through the mod's `keys.rs` (`listed`, `bound`, `pressed`, `held`), so moving to
`Registrar::register_key_mapping` later changes that one file. A key pressed with F3 held is left
to the debug keys.

## Tests

`runtime.rs` holds the pure logic and its tests, and it builds on its own:

```sh
for m in lumos_*; do rustc --edition 2024 --test "$m/runtime.rs" -o "/tmp/$m-test" && "/tmp/$m-test"; done
```

## Adding them to a Lumos build

`tools/lumos_native/apply.py` applies `work/mods/<id>/` (the contents of a bundle's `mixins/<id>/`) to
`work/src`. To add these mods to the 9f42064c group, copy them in after `stage.py`:

```sh
python3 tools/lumos_native/stage.py --plan plan.json --commit 9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0 --work work/
cp -R lumos-mods/lumos_* work/mods/
python3 tools/lumos_native/apply.py --tool <horizon-mixins e59da909> --work work/
```

Lumos's own mods and registry mods meet in the same `work/mods/`. `apply.py` tries each mod alone,
then all of them together, and reports any mod that conflicts.

To check them on their own:

```sh
horizon-mixins apply --workspace work/src --mods lumos-mods --out work/stage
cd work/stage && cargo build --locked --profile fast -p horizon-client --bin horizon-client
```
