# fair_freecam: Lumos port

Started from the registry release **fair_freecam 1.2.0** (game 26.3), `fair_freecam-1.2.0.hnmod`,
SHA-256 `4fdd0b3067a00d23fe30ee4d594293ef0705325bb5e6e5b292f90c1e8b8a5f8a`. It was written for Horizon `aa27eca326db26e999d4e59d7899576016764b1e` with horizon-mixins `4342a0bfbe71d4d14185528d64e496d6022f16a1`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: Lutziano888. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- Targets `hand_geometry`, `key_pressed` and `play_mouse` moved to `game::frame_geometry`, `game::play_keys` and `game::play_input`.
- `hand.rs`: `hand_geometry` now returns `(Vec<Vertex>, Vec<(u32, i32)>)`; the helper returns two empty lists while the camera is free.
