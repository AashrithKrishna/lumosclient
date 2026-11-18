# xtoxray: Lumos port

Started from the registry release **xtoxray 2026.29.0** (game 26.3), `xtoxray-2026.29.0.hnmod`,
SHA-256 `3446a4a70a2b87ef6e9de00ce8a255aa83c2e44ccd5acc9f14887662ed55f27c`. It was written for Horizon `942e3b2dedf2f3a84964010dde4e5fc3b6ffdc4a` with horizon-mixins `5140cdf81915c25f10a7745eb532e1c09f275068`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `Apache-2.0` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: 1_st_Cat. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- `filter.rs`: `MeshTables` gained `solid`, `full_faces`, `animated_layers`, `colormaps` and `water_colormap`. The filtered tables derive `solid` and `full_faces` from the filtered `info` exactly as `MeshTables::new` does (so hidden blocks occlude like air) and copy the other three.
