# horizonmatica: Lumos port

Started from the registry release **horizonmatica 0.3.2** (game 26.3), `horizonmatica-0.3.2.hnmod`,
SHA-256 `c73b7909541fdc8f2c1127323cb8df30f5e186e46f7c5a672fe5c9b8b8dbdfe1`. It was written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: keksi2k. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- `menus_game.rs`: `Game::options_to_game` is gone; replaced by `panel.recipes_settings_return = Some(Screen::Playing)` (Horizon's one-shot return screen for Options), so closing the settings page still returns to the game.
