# firstpersonmodel: Lumos port

Started from the registry release **firstpersonmodel 1.0.0** (game 26.3), `firstpersonmodel-1.0.0.hnmod`,
SHA-256 `32f5def8886cc6d815efc8295208b37d9791bb17e77874502fd8d51729a03af9`. It was written for Horizon `2ebf00a0281c592f79191e76fce0e7d522c6f425` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: DeportedToHell. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- `glue.rs`: `Game::studio_replay` is gone (Horizon Studio is `game.studio` now). The body is skipped while Studio's free camera flies (`studio.flying()`), the same gate Horizon's own `hand_geometry` uses.
