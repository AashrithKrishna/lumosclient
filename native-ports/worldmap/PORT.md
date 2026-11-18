# World Map (worldmap) — Lumos port

- Source: registry release `worldmap` 0.7.1 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/worldmap-0.7.1/worldmap-0.7.1.hnmod`)
- SHA-256 of the `.hnmod`: `ae6fa84ce4d7b00df776d0f619a927b9903c28b18f3233d6560bb3d7244190f1` (checked against the registry index)
- Written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: Px228-Da-Da. Licence: MIT (per the registry; the release ships no licence file in its mixins folder). Upstream: https://github.com/Px228-Da-Da/horizon-worldmap

## Changes

- `chat-tree` hook: `chat::Game::chat_tree_for` ends in a call chain at 9f42064c that horizon-mixins can't wrap with a RETURN hook (`E_UNSUPPORTED_CONTROL_FLOW`). It is a cancellable HEAD hook now that builds the same tree as the function body (native commands, studio commands, profiler) and then adds /wp's words, as before.
