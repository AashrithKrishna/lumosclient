# worldedit: Lumos port

Started from the registry release **worldedit 1.2.1** (game 26.3), `worldedit-1.2.1.hnmod`,
SHA-256 `b2bb3019ecd4d5392644c54c70a87d6c3277538a6e18d0c311f3a051f7d2f015`. It was written for Horizon `83f15fa276c44711276b6ceb5f0fe0c9c534dc66` with horizon-mixins `cb1ad23022392d88f006cf94cbf6552a73f5df12`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: _cykoo_. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- The chat screen moved to the `horizon-client-render` crate (still re-exported as `horizon_client::chat`): modules `paths` and `chat` now go into `crates/horizon-client-render/src/chat.rs` and the `chat-suggestions` target is `horizon_client_render::chat::ChatScreen::update_command_info`.
- Targets `key_pressed`, `open_world` and `play_mouse` moved to `game::play_keys`, `game::world_session` and `game::play_input`.
- `chat-suggestions.rs`: `ChatScreen::show_suggestions` takes an `immediate` flag now; passes `false` (not Tab-triggered), as Horizon's own `update_command_info` does.
- `runtime.rs`: `Dim::min_y`/`max_y` are gone; the vanilla heights come from `Dim::shape()` (`min_y`, and `max_y() - 1` for the highest block, as the old inclusive `max_y` was used).
- `runtime.rs`: an item's `custom_name` is a text component (`Tag`) now; the wand is named `Tag::String("WorldEdit Wand")` and recognised by that string.
