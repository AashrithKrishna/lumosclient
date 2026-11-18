# PhysicsModHorizon (horizon_physics) — Lumos port

- Source: registry release `horizon_physics` 0.20.7 for game 26.3 (`https://github.com/horizon-rust/horizon-mods/releases/download/horizon_physics-0.20.7/horizon_physics-0.20.7.hnmod`)
- SHA-256 of the `.hnmod`: `8eeba82ac0e7755298269fd328679473720a9894cfe10df5c8e3aaf88f59c34c` (checked against the registry index)
- Written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `d9c83d7096b7ea5b83678da2efdfd6161f74997e`
- Ported to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` with horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`
- Authors: tariksabanuz. Licence: MIT (per the registry; the release ships no licence file in its mixins folder). Upstream: https://github.com/tariksabanuz/PhysicsModHorizon

## Changes

- Targets moved: `break_block` to `game::block_breaking`, `attack` to `game::melee`, `destroy_particles`/`crack_particle` to `game::block_particles`.
- `shader` hook: `native_material::module` takes `line: (Api, bool)` instead of `api`; the binding and `shader.rs` pass that tuple on to `shader_source` unchanged.
