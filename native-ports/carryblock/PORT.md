# Carry Block (carryblock) — Lumos port

- Source: registry release `carryblock` 0.1.2 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/carryblock-0.1.2/carryblock-0.1.2.hnmod`)
- SHA-256 of the `.hnmod`: `9a6ebd26f640a477815862eb36846fe3289046d0f4c06ef3a815f463ca9a3701` (checked against the registry index)
- Written for Horizon `142388e1c136e622652ec7505af15d8ad195e041` with horizon-mixins `0d6b103c70633d8fd9443d8d0acd3aadaacedb06`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: d3grando. Licence: MIT (per the registry; the release ships no licence file in its mixins folder). Upstream: https://github.com/d3grando/carryblock

## Changes

- Target moved: `Game::use_item` to `game::item_actions`.
- `native.rs`: `Dim::min_y`/`max_y` are gone; the height check reads `world.shape`.
- `native.rs`: `lan::menu::Slots` has a `Native` variant (no block halves, like `Ender`).
- `native.rs`: `world.block_names` holds the `CustomName` tag (a component) instead of a string: the name is put back into the block entity tag as is, and a placed block's name is loaded with the game's own `load_block_name`.
- `native.rs`: `GuestUse::Entity` carries the interact target as a second field; `mob_hit` returns a third value (ignored); a block state's properties are a `BlockProperties` and are copied into the mod's `BTreeMap`.
