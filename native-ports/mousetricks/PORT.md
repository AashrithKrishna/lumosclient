# Mouse Tricks (mousetricks) — Lumos port

- Source: registry release `mousetricks` 0.2.0 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/mousetricks-0.2.0/mousetricks-0.2.0.hnmod`)
- SHA-256 of the `.hnmod`: `48c60955371963f8e320a9efbe8285bb276d255c29178a8a021e7e2b47535cbe` (checked against the registry index)
- Written for Horizon `142388e1c136e622652ec7505af15d8ad195e041` with horizon-mixins `0d6b103c70633d8fd9443d8d0acd3aadaacedb06`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: d3grando. Licence: MIT (per the registry; the release ships no licence file in its mixins folder). Upstream: https://github.com/d3grando/mousetricks

## Changes

- Targets moved: `tweaks::sort_mouse_press` to `extras::sorting`, `tweaks::tweak_scroll` to `extras::mouse_tweaks`, `inventory_drag_move`/`inventory_mouse_release` to `game::inventory_input`.
- `native.rs`: `stations::hover_name` takes the pack's `&Names` instead of a lookup closure.
