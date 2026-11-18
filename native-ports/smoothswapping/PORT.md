# Smooth Swapping (smoothswapping) — Lumos port

- Source: registry release `smoothswapping` 1.1.2 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/smoothswapping-1.1.2/smoothswapping-1.1.2.hnmod`)
- SHA-256 of the `.hnmod`: `8ce51f852ad29512d9184bf390df9bce56395bf25906679959a223f00d912a61` (checked against the registry index)
- Written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: DeportedToHell. Licence: LGPL-3.0 (per the registry; the licence file shipped in the mod is kept here). Upstream: https://github.com/DeportedToHell/MCRE-Mods/tree/6060e0fba1ee4b720f43e005a4cb12b67ade4eab/smoothswapping

## Changes

- Targets moved: `draw_inventory_slots`/`draw_slot_item` to `game::inventory_slots`, `tweaks::draw_sort_buttons` to `extras::sorting`.
