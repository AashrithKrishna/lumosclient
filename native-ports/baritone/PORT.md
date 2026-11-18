# baritone: Lumos port

Started from the registry release **baritone 1.1.52** (game 26.3), `baritone-1.1.52.hnmod`,
SHA-256 `d46c541d1e673a95f343960b6fb1646a0669ec07df11958af97f1a76b3b14ffa`. It was written for Horizon `6475f62f3c834b7489e1816bf47f47ea5eb2b879` with horizon-mixins `0d6b103c70633d8fd9443d8d0acd3aadaacedb06`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `LGPL-3.0-or-later (a derivative of Baritone, https://github.com/cabaletta/baritone)` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: luhnox (this port); Baritone by leijurv and the Baritone contributors. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- Targets in `game::tweaks` (`sort_mouse_press`, `sort_key_press`) moved to `game::extras::sorting`.
