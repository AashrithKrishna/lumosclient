# ae2: Lumos port

Started from the registry release **ae2 15.4.18** (game 26.3), `ae2-15.4.18.hnmod`,
SHA-256 `a0f56e3800fa43ad95e6cd62dec8a0bf926e8dde7f03dc530df618d692c99a82`. It was written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `LGPL-3.0-only AND CC-BY-NC-SA-3.0` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: AlgorithmX2, TeamAppliedEnergistics, Joeypc6. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- Target `clicked_outside` moved to `game::inventory_input`.
- Injection `bus-hopper-eject`: `machines::Game::hopper_eject` is now `hopper_eject_with_state` (it also takes the hopper's state and view; the hook binds only `pos`, as before).
- `me.rs`: the two `PrimedTnt` literals fill the new fields (`fall_distance`, `in_water`, `stuck`, `used_portal`) with the defaults Horizon spawns TNT with.
- `me.rs`: `world.block_names` holds text components now; the part's name is stored as `Tag::String`.
- `me_embedded_content.rs` / injection on `mods::content_root`: it takes the disabled mods too now; bound and passed through.
