# TreeFall (treefall) — Lumos port

- Source: registry release `treefall` 0.3.0 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/treefall-0.3.0/treefall-0.3.0.hnmod`)
- SHA-256 of the `.hnmod`: `6f7feea8d0f55291953476b272f00cfc4be42d64972e7efefeb3df47c9dae8ce` (checked against the registry index)
- Written for Horizon `142388e1c136e622652ec7505af15d8ad195e041` with horizon-mixins `0d6b103c70633d8fd9443d8d0acd3aadaacedb06`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: d3grando. Licence: MIT (per the registry; the release ships no licence file in its mixins folder). Upstream: https://github.com/d3grando/treefall

## Changes

- `mining` hook: `Game::break_block` moved to `game::block_breaking` (target path only).
- `drops` hook: `Game::pop_resource` moved to `game::item_entities` (target path only).
- `speed` hook: `hand::Game::destroy_progress` takes `&mut self` at 9f42064c, so the helper binds `&mut Game` and `treefall_destroy_progress` takes `&mut self` (it calls `destroy_progress`).
- `native.rs`: `Dim::min_y`/`max_y` are gone; the height check reads the level's own `world.shape` (`min_y`, exclusive `max_y()`), same range in vanilla dimensions.
