# Dynamic FPS for rust (dynamic_fps_for_rust) — Lumos port

- Source: registry release `dynamic_fps_for_rust` 0.1.0 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/dynamic_fps_for_rust-0.1.0/dynamic_fps_for_rust-0.1.0.hnmod`)
- SHA-256 of the `.hnmod`: `50fad9a8d1d481a3862479b313c04e0dc6f8494a84d4cd007583822f5671e17e` (checked against the registry index)
- Written for Horizon `998b1d8301928c0559a0f368809860a30d6d9e53` with horizon-mixins `e8cd0aa38cffef49cb2e088ef716baa107148c02`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: Lyvt (dc: _.lyvt). Licence: MIT (per the registry; the licence file shipped in the mod is kept here). Upstream: https://github.com/Lyvt-Dev/dynamic-fps-for-rust

## Changes

- Target moved: `Game::handle_window_event` to `game::window_handler` (target path only).
