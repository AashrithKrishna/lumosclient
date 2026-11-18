# orbital: Lumos port

Started from the registry release **orbital 0.5.10** (game 26.3), `orbital-0.5.10.hnmod`,
SHA-256 `f860b61f2c54cc4efd6e6373054858768b0fd829afc0b59c60893a556ef4da63`. It was written for Horizon `e57b1d34bb0dcf6bf081c19fe6d2e7f69b60cdf4` with horizon-mixins `a2af5c0dd4117d51ecd70066ba7a00b4a2c10dab`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: Stink-O. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- `glue.rs`: `NativeBlockEdit` gained `neighbour_updates` and `block_entities`; the literal fills them with `..Default::default()` (no neighbour updates, no block entity data), which is what the edit did before.
- Only the bundle's `mixins/orbital/` folder is here; the release's `assets/`, `data/` and `horizon-mod.toml` (outside that folder) are not part of a native port.
