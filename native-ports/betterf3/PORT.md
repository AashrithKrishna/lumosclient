# betterf3: Lumos port

Started from the registry release **betterf3 1.0.0** (game 26.3), `betterf3-1.0.0.hnmod`,
SHA-256 `6903e9fd75fc89e1bf780c28944cd4483800e02a258cd69f794a5cbe648a0d9e`. It was written for Horizon `2ebf00a0281c592f79191e76fce0e7d522c6f425` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: DeportedToHell. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- `screens.rs`: `Game::options_to_game` is gone. Opening a mod's options page from the game now sets `panel.recipes_settings_return = Some(Screen::Playing)`, Horizon's one-shot "close Options back to this screen", so closing still returns to the game.
