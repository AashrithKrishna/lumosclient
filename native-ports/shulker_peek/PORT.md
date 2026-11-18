# ShulkerPeek (shulker_peek) — Lumos port

- Source: registry release `shulker_peek` 0.1.0 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/shulker_peek-0.1.0/shulker_peek-0.1.0.hnmod`)
- SHA-256 of the `.hnmod`: `e4e33cb698d91042b2ed2f93a1c935f279d16d83988115d53c0f33530cabeab5` (checked against the registry index)
- Written for Horizon `5e391eb67392941734df7ddd648728c4ed5302f6` with horizon-mixins `0d6b103c70633d8fd9443d8d0acd3aadaacedb06`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: vstxx. Licence: MIT (per the registry; the release ships no licence file in its mixins folder). Upstream: https://github.com/vstxx/shulkerpeek

## Changes

- `tooltip.rs`: `Game::tooltip_box` is a method at 9f42064c and takes the stack (for mod tooltip colours); the helper calls `game.tooltip_box(batch, Some(stack), ...)`.
