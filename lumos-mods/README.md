# Lumos native mods

Five Horizon native mods written for Lumos Client. Each folder is one horizon-mixins bundle: what a
registry `.hnmod` keeps under `mixins/<id>/` (`horizon-mixin.toml`, the `.rs` files it names).
They are original Rust. The first four do the same jobs as four Java Fabric mods but share no code or
assets with them; `lumos_hud` is the native form of Lumos's own Fabric LumosHUD.

| Folder | What it does |
|---|---|
| `lumos_durability_tooltip` | Adds "Durability: 123 / 250" to the tooltip of every damageable item, coloured from green to red by how much is left. |
| `lumos_totem_counter` | Shows a HUD counter (totem icon and number) of the Totems of Undying you carry. It flashes when the count drops, turns gold when you are low, and shows a notice over the hotbar when one of your totems pops. |
| `lumos_particle_rain` | Turns rain into falling drop particles with splashes on the ground and snow into drifting flakes. Heavy weather can add a mist. It replaces vanilla's rain sheets and limits its own particle count. |
| `lumos_paginated_advancements` | Splits the Advancements screen's tabs into pages. Use the `<` `>` buttons, Page Up/Page Down or the mouse wheel over the tabs to reach every tab. |
| `lumos_hud` | Lunar-style HUD modules (FPS, CPS, coordinates, keystrokes, armour, potion effects, ping, clock, server, day, speed, reach, combo, item counter, memory, sprint, direction, biome) in the Lunar or Minimal style. Right Shift opens the editor: drag modules, right-click to hide, switch them on and off and pick the style. The layout is saved to `horizon-mods/.horizon-mod-data/lumos_hud/layout`. Its hotkeys (editor, HUD toggle) are rebindable on its Options > Mods page; `keys.rs` is the only file that reads them. |

## Pin

All five are planned and built against Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0`
(Nightly 2026-11-13) with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`. The pin is set by
`[compatibility] horizon_revision` in each manifest. For another Horizon commit, re-plan the mods and
rebuild them. The hooks they use are listed in each manifest.

## Layout and files

```text
lumos-mods/<id>/
  horizon-mixin.toml   # modules + injections
  runtime.rs           # the rules: std only, unit tests inside
  glue.rs              # reads Horizon's Game, calls runtime
  hooks/*.rs           # one small helper per injection
```

`runtime.rs` and `glue.rs` become modules of `crates/horizon-client/src/game.rs`, named `<id>` and
`<id>_glue`.

## On, off and settings

`lumos_hud` also has `keys.rs`, a module (`lumos_hud_keys`) with its hotkeys.

Every hook checks `game.native_on("<id>")` first. Each mod also adds itself to Options > Mods
(`native_mods`), so it can be switched off there. That writes `mod.<id>:false` to `horizon-mods.txt`
and takes effect at runtime with no rebuild. Each mod has its own settings page, saved with
`native_set_setting`.

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
