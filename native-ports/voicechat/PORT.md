# voicechat: Lumos port

Started from the registry release **voicechat 1.0.0** (game 26.3), `voicechat-1.0.0.hnmod`,
SHA-256 `7a12ef9e691d10128ea081906a313d597c67f22897380618871477f9dd8eabf1`. It was written for Horizon `c9d474dd9116e84c243f8992d75c11314df56f06` with horizon-mixins `ddd9700b21997c95d95ac6ee5ecafe3830cf622a`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: Nirokx. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- `tick.rs`: `Game::remote_entity_position` is gone. The other players' positions are read from `game.mobs` (the server's player entities, matched by entity id), as Horizon's own player-position code does.
