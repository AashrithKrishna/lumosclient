# MouseTweaks (mousetweaks) — Lumos port

- Source: registry release `mousetweaks` 1.1.0 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/mousetweaks-1.1.0/mousetweaks-1.1.0.hnmod`)
- SHA-256 of the `.hnmod`: `5b74114ada20c434408b533bd57dec2725d081e8a8cba4d7de96c3d1502c5a6d` (checked against the registry index)
- Written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: DeportedToHell. Licence: BSD-3-Clause (per the registry; the licence file shipped in the mod is kept here). Upstream: https://github.com/DeportedToHell/MCRE-Mods/tree/main/mousetweaks

## Changes

- Targets moved: `tweaks::tweak_press`/`tweak_scroll` to `extras::mouse_tweaks`, `inventory_drag_move`/`inventory_mouse_release` to `game::inventory_input`.
