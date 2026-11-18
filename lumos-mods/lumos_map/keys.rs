// Lumos Map: every key the mod reads, and the only place that reads them.
//
// Horizon 9f42064c has no Controls-screen API for a mod's keys, so they are listed in
// `NativeMod::keys`: Options > Mods shows them on the mod's page, rebinds them there and keeps
// the choice as `key.<mod id>.<name>` in `horizon-mods.txt` (`native_mod_key` reads it). Moving
// to `Registrar::register_key_mapping` later changes this file only.
use crate::game::Game;
use crate::game::lumos_map as m;

/// Opens (and closes) the world map.
pub(super) const MAP: &str = "map";
/// Adds a waypoint where you stand.
pub(super) const WAYPOINT: &str = "waypoint";

/// Each key: its name, its label on the page, and its default (`key.keyboard.g`).
const KEYS: [[&str; 3]; 2] = [
    [MAP, "World Map", "key.keyboard.m"],
    [WAYPOINT, "Add Waypoint", "key.keyboard.b"],
];

/// The keys for `NativeMod::keys`.
pub(super) fn listed() -> Vec<[String; 3]> {
    KEYS.iter().map(|k| k.map(str::to_owned)).collect()
}

/// The key the player chose for `name`, else its default.
pub(super) fn bound(game: &Game<'_>, name: &str) -> String {
    let default = KEYS.iter().find(|k| k[0] == name).map_or("", |k| k[2]);
    game.native_mod_key(m::ID, name, default)
}

/// Whether the `native_key` event (`key` went down when `down`) is `name`'s key going down.
/// F3 held makes it a debug combination (F3+B draws hitboxes), not the mod's key.
pub(super) fn pressed(game: &Game<'_>, name: &str, key: &str, down: bool) -> bool {
    down && !game
        .physical_keys
        .contains(&winit::keyboard::KeyCode::F3)
        && key == bound(game, name)
}

/// Whether `name`'s key (or mouse button) is held down now.
#[allow(dead_code)]
pub(super) fn held(game: &Game<'_>, name: &str) -> bool {
    let key = bound(game, name);
    if let Some(code) = horizon_client::keybinds::key_code(&key) {
        return game.physical_keys.contains(&code);
    }
    horizon_client::keybinds::mouse_button(&key).is_some_and(|b| game.physical_buttons.contains(&b))
}
