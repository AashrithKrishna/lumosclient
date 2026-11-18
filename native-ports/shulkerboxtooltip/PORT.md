# Shulker Box Tooltip (shulkerboxtooltip) — Lumos port

- Source: registry release `shulkerboxtooltip` 1.0.1 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/shulkerboxtooltip-1.0.1/shulkerboxtooltip-1.0.1.hnmod`)
- SHA-256 of the `.hnmod`: `005244f696d9c5214980f554251f6442523768f040a78f505ef8d36090484bba` (checked against the registry index)
- Written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: DeportedToHell. Licence: MIT (per the registry; the licence file shipped in the mod is kept here). Upstream: https://github.com/DeportedToHell/MCRE-Mods/tree/main/shulkerboxtooltip

## Changes

- Target moved: `Game::slot_at` to `game::inventory_slots` (target path only).
