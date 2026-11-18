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
| `lumos_map` | Minimap and world map at a block a texel with crisp pixels: each block's top texture averaged once, biome tints blended across biomes (no seams), gentle relief shading, water showing the seabed and darkening with depth. The terrain is kept as cells in the mod image slot and turned, moved and zoomed on the GPU, redrawn only where chunks change. Minimap: square or round with a rim and shadow, sizes, fractional zoom (= and -), any corner with offsets, opacity, turning or north-up, compass, info lines in a chosen order (chunk, angles, weather, real time and the Overworld/Nether place too), show/hide (H), and, when turned on, a rim colour, auto size by GUI scale, terrain depth, darker nights, biome colours in the palette and stained glass seen through, Z enlarges it, overlays for the chunk grid, slime chunks and dark floors (light overlay). Cave view (K: Auto, Manual, Off): Auto turns on under a solid roof over a 1x1, 3x3 or 5x5 square (not leaves or glass), after about a second; it shows the slice under your head down to a set depth, rock flat and dark with lit edges along passages, floors darker with depth (less with "Legible caves"), drops darker still; Manual keeps a start Y of your choice. Entities are coloured dots; holding the player list key (Tab) shows their faces (cut from the game's own textures and models, players' skins included) as glass badges, players with their names (mob names are an option). Waypoints are on-screen markers (a diamond in the waypoint's colour with its first letter or a symbol, distance under it, the name when looked at), kept at the screen's edge with an arrow when off screen; beams are an option. B opens a small "New waypoint" box (name, colour, X / Y / Z prefilled with the block you stand in, set; Enter creates, Escape cancels), or adds one at once with "B adds instantly". The waypoint manager (U) has search, multi-select, sets, death points and a full editor (typed coordinates, custom colours, temporary, all-worlds, per-map visibility, teleport where allowed, copy, share). World map (M): explored chunks saved per world or server (sub-worlds by spawn, by hand, or one map), kept in memory only near you and the view; cave maps per dimension (Off, Layered under a top Y, Full under the roof) with every cave slice saved per 16-block band; a right-click menu (waypoint here, teleport where allowed, edit/hide/delete a waypoint), other players, your footsteps, another dimension's saved map, chunk grid, slime chunks, export to PNG. Entities fade and get up/down marks by height; 2D and vanishing waypoints; death points go when reached; a fair-play switch. A paged settings screen (General, Minimap, Caves, World map, Entities, Waypoints, Extras, Keys) in the map and on its Options > Mods page. |
| `lumos_fullbright_key` | A key (G by default) that turns Horizon's own Fullbright (Video Settings > Quality) on and off, with a notice over the hotbar. On other people's servers, where Horizon holds Fullbright off, it says so instead. |
| `lumos_controls` | Every enabled native mod's keys (Lumos's and registry mods') in Options > Controls > Key Binds, after vanilla's categories, under each mod's name as Java shows mod keys. Click a key to rebind it, Escape unbinds, Reset goes back to the mod's default; a key another binding (vanilla's, Horizon's or a mod's) also uses shows in red. It writes the same `key.<id>.<name>` lines as the mod's own page, which the mods read at once. |

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
`lumos_controls` is the exception: Key Binds is drawn by `horizon-client-render`, so its `runtime.rs`
and `glue.rs` go into `crates/horizon-client-render/src/options_ui.rs` (`lumos_controls` is `pub`, for its
Options > Mods entry).

## On, off and settings

`lumos_hud` and `lumos_discover` also have `keys.rs`, a module (`<id>_keys`) with their hotkeys.

Every hook checks `game.native_on("<id>")` first. (Lumos's builds also start every hook of every mod, these and registry mods alike, with the mod's switch: `tools/lumos_native/gate.py`.) Each mod also adds itself to Options > Mods
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

With `lumos_controls` on, every one of these keys is also in Options > Controls > Key Binds, a
section per mod, rebound there through the same `key.<id>.<name>` lines.

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
