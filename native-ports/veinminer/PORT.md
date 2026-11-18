# Veinminer (veinminer) — Lumos port

- Source: registry release `veinminer` 0.3.2 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/veinminer-0.3.2/veinminer-0.3.2.hnmod`)
- SHA-256 of the `.hnmod`: `31a4249da1b35972ae8b34201373c86352f4adc2f5cb3e015749d1fbfecf00a0` (checked against the registry index)
- Written for Horizon `d91cab68ded29bd2b004f7050c468fb98c41a00b` with horizon-mixins `4342a0bfbe71d4d14185528d64e496d6022f16a1`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: levzzz5154. Licence: MIT (per the registry; the release ships no licence file in its mixins folder). Upstream: https://github.com/levzzz5154/horizon-veinminer

## Changes

- `mining` hook: `Game::break_block` moved to `game::block_breaking` (target path only).
