# appleskin_horizon: Lumos port

Started from the registry release **appleskin_horizon 1.0.0** (game 26.3), `appleskin_horizon-1.0.0.hnmod`,
SHA-256 `38a729f1aad00f6148f29578a2725cb5e7f9aea79a870fcf3cee76f39838c5ff`. It was written for Horizon `ba6344a59cb9e755b730dcf02285fc0d33b97d3c` with horizon-mixins `3f99a5c60dfa311cc365524c44ae2fd799bfa1de`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: CayMac. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- Injection `hud`: `titles::Game::draw_titles` is now `draw_title` (same signature, still drawn after the health and food rows); target updated.
