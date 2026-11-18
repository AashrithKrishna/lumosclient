# visuality_horizon: Lumos port

Started from the registry release **visuality_horizon 1.0.2** (game 26.3), `visuality_horizon-1.0.2.hnmod`,
SHA-256 `b764f15a062e44b27c55f478b2d02e163bfb01c3d8e222fcd6d83bcc66f19a0d`. It was written for Horizon `c9d474dd9116e84c243f8992d75c11314df56f06` with horizon-mixins `a2af5c0dd4117d51ecd70066ba7a00b4a2c10dab`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: BenjiSyS. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- Injection `removed`: a server's RemoveEntities now goes through `remote::removal::Game::remove_entities(&[i32])` (`remove_entity` calls it too). The hook moved there with a new helper `entities_removed.rs` that forgets each removed id, as `entity_changed.rs` did per id.
