# Mod Menu (mod_menu) — Lumos port

- Source: registry release `mod_menu` 0.3.5 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/mod_menu-0.3.5/mod_menu-0.3.5.hnmod`)
- SHA-256 of the `.hnmod`: `5fef518e6ce7417944ab2621711be013cf7e25fdd6d3b25922876e78f090bfa3` (checked against the registry index)
- Written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `d9c83d7096b7ea5b83678da2efdfd6161f74997e`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: tariksabanuz. Licence: MIT (per the registry; the release ships no licence file in its mixins folder). Upstream: https://github.com/tariksabanuz/ModMenuHorizon

## Changes

- `ui.rs`: the Mods screen's "settings" button kept `Game::options_to_game` across `open_options_at`. That field is gone at 9f42064c and `open_options_at` no longer resets where Options returns to, so only `options_from_title` is carried over now.
