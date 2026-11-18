# Inventory Profiles (inventory_profiles) — Lumos port

- Source: registry release `inventory_profiles` 1.0.0 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/inventory_profiles-1.0.0/inventory_profiles-1.0.0.hnmod`)
- SHA-256 of the `.hnmod`: `1e5d1c75d05230172820b5bc016d7d3ac792adf473de7dbfb84394d8b795a633` (checked against the registry index)
- Written for Horizon `ba6344a59cb9e755b730dcf02285fc0d33b97d3c` with horizon-mixins `3f99a5c60dfa311cc365524c44ae2fd799bfa1de`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: CayMac. Licence: MIT (per the registry; the release ships no licence file in its mixins folder). Upstream: https://github.com/CayMac97/inventory-profiles

## Changes

- Target moved: `tweaks::sort_slots` to `extras::sorting`.
- `glue.rs`: `stations::hover_name` takes the pack's `&Names` instead of a lookup closure.
