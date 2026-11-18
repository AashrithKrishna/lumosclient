# viahorizon: Lumos port

Started from the registry release **viahorizon 0.8.8-dev** (game 26.3), `viahorizon-0.8.8-dev.hnmod`,
SHA-256 `ee28b2131a066870acdd853dcf31096bc556cb574a9599f00cd296c1d563a2ba`. It was written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `GPL-3.0-or-later` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: hhhhsffd. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- Target `hand_geometry` moved to `game::frame_geometry`.
- `legacy_mesh_region.rs`: `Streaming::region` returns `Option<Arc<Region>>` now; the helper wraps the legacy region in an `Arc`.
