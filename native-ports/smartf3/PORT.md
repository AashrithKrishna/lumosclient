# SmartF3 (smartf3) — Lumos port

- Source: registry release `smartf3` 0.1.0 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/smartf3-0.1.0/smartf3-0.1.0.hnmod`)
- SHA-256 of the `.hnmod`: `666fc77a12411c8b525dca3d3d478595e760b73c48b62b936fb6026419547929` (checked against the registry index)
- Written for Horizon `142388e1c136e622652ec7505af15d8ad195e041` with horizon-mixins `0d6b103c70633d8fd9443d8d0acd3aadaacedb06`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: d3grando. Licence: MIT (per the registry; the release ships no licence file in its mixins folder). Upstream: https://github.com/d3grando/smartf3

## Changes

- `native.rs`: `Game::mob_hit` returns `(index, distance, point)` now; the point is ignored.
- `native.rs`: `Game::guest_hit` (the LAN guest under the crosshair) is gone at 9f42064c; its old body is kept as a local `smartf3_guest_hit` using the same helpers (`view_vector`, `ray_box`, `lan_mobs::guest_bounds`).
