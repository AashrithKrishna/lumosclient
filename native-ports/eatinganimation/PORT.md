# Eating Animation (eatinganimation) — Lumos port

- Source: registry release `eatinganimation` 0.2.0 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/eatinganimation-0.2.0/eatinganimation-0.2.0.hnmod`)
- SHA-256 of the `.hnmod`: `e9df7c923723b3b047e6442529dac057806521d988ab2694c43fdcc3104ecc5f` (checked against the registry index)
- Written for Horizon `6d99e14ff0c70ae60bc56b67829b526409fa068e` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: sillysoufi. Licence: MIT (per the registry; the licence file shipped in the mod is kept here). Upstream: https://github.com/soufied/EatingAnimation-Horizon

## Changes

- `embedded-content` hook: `mods::content_root` takes a `disabled` set at 9f42064c. The hook binds it and passes it on (`horizon-mixin.toml`, `embedded_content.rs`).
