# ClientSort (clientsort) — Lumos port

- Source: registry release `clientsort` 1.2.1 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/clientsort-1.2.1/clientsort-1.2.1.hnmod`)
- SHA-256 of the `.hnmod`: `e07a64a27ec386a12916b3d685cccb9d2e0d5f8273dc61344df3667c93ce1c87` (checked against the registry index)
- Written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: DeportedToHell. Licence: Apache-2.0 AND LGPL-3.0-only (per the registry; the licence file shipped in the mod is kept here). Upstream: https://github.com/DeportedToHell/MCRE-Mods/tree/b033b17c8a7d623fe3be4713f52e9b6b9021ee23/clientsort

## Changes

- Targets moved: `tweaks::draw_sort_buttons`/`sort_buttons` to `extras::sorting`.
- `glue.rs`: `tweaks::tweak_menu` moved to `extras::mouse_tweaks::tweak_menu`.
- `glue.rs`: `SoundEngine::unseeded()` is gone; the same thing is written out: `LegacyRandom::new(random_source::unique_seed())`.
