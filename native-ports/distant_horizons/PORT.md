# distant_horizons: Lumos port

Started from the registry release **distant_horizons 0.1.0** (game 26.3), `distant_horizons-0.1.0.hnmod`,
SHA-256 `c53a2501f14efb624839329aac6999ddf8c64a399d41bec42a0c878902feea81`. It was written for Horizon `d009ebd140476b5bdc958c00f56a75e82abec691` with horizon-mixins `0d6b103c70633d8fd9443d8d0acd3aadaacedb06`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `LGPL-3.0-only` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: denizsahin7744, Deniz7744. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- Injection `distance`: `Game::terrain_distance` moved to `game::render_settings`; target updated.
- `bridge.rs` `add_commands`: Horizon no longer has `Remote::commands` or `Game::merge_remote_tree`; it keeps the chat's merged tree in `Remote::chat_tree` (rebuilt when the server sends its tree). `/dh` is now added to that tree, once the server's tree has arrived (`chat_tree_received`), as before.
