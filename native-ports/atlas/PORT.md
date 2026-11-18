# Atlas (atlas) — Lumos port

- Source: registry release `atlas` 0.2.0 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/atlas-0.2.0/atlas-0.2.0.hnmod`)
- SHA-256 of the `.hnmod`: `07c3651728523e349858ef7f575b6336508a8e616a99aef2ef19e9723d1694c3` (checked against the frozen registry index, `tools/lumos_native/native-v1-index.json`)
- Written for Horizon `f319701e8c9ab657ff562250b3ba07b2d654e3bc` with horizon-mixins `e8cd0aa38cffef49cb2e088ef716baa107148c02`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: MrCloudy2 (built on Waypoints by Nirokx). Licence: MIT, the release's own `LICENSE` file, kept unchanged. Upstream: https://github.com/MrCloudy2/horizon-atlas

## Changes

- `bridge.rs`, `slot_for`: at 9f42064c `maps.slots.claim` returns `Option<(slot, evicted)>`. It returns `None` when every map texture slot is already showing something this frame (E0308 before). `slot_for` now returns `Option` too. When no slot is free, a world map tile is skipped for this frame, the same as when its upload budget runs out: its slot entry is dropped and it is tried again next frame. The mob face sheet is also skipped for this frame, and `face_slot` is cleared so no face is drawn from a slot that now holds another image. Nothing else changed.
