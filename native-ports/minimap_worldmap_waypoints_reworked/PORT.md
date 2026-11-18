# minimap_worldmap_waypoints_reworked: Lumos port

Started from the registry release **minimap_worldmap_waypoints_reworked 0.4.1** (game 26.3), `minimap_worldmap_waypoints_reworked-0.4.1.hnmod`,
SHA-256 `93b92a321377227840d5adbb21a21b968b0d4d124e8f64de3bffeccf9eada6f8`. It was written for Horizon `2ebf00a0281c592f79191e76fce0e7d522c6f425` with horizon-mixins `d9c83d7096b7ea5b83678da2efdfd6161f74997e`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `GPL-3.0-only` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: daniliammo. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- Injection `settings-flush`: `ApplicationHandler::exiting` for `Game` moved to `game::window_handler`.
- Injection `binding-rows`: `OptionsUi::native_rows` moved to the `options_ui::pages` module.
- Target `forget_atlas_slots` moved to `game::render_settings`.
