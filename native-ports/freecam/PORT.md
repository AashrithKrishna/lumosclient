# freecam: Lumos port

Started from the registry release **freecam 0.1.0** (game 26.3), `freecam-0.1.0.hnmod`,
SHA-256 `6314ed87d3b58ed0244bed388201c64bffb8deab4f5ff7beed2a3791b391a6da`. It was written for Horizon `af55a734865da4f1b11836164a979ff06a1f0449` with horizon-mixins `4342a0bfbe71d4d14185528d64e496d6022f16a1`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: hakergeniusz. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- Targets `mapped_key` and `play_mouse` moved to `game::play_input`.
