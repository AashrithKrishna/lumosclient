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
/// The minimap's zoom, in and out.
pub(super) const ZOOM_IN: &str = "zoom_in";
pub(super) const ZOOM_OUT: &str = "zoom_out";
/// The minimap's cave view on or off.
pub(super) const CAVE: &str = "cave";
/// The cave view in Manual, starting where you are now.
pub(super) const CAVE_HERE: &str = "cave_here";
/// Shows or hides the minimap.
pub(super) const MINIMAP: &str = "minimap";
/// Shows or hides every waypoint.
pub(super) const WAYPOINTS: &str = "waypoints";
/// Opens the waypoint list.
pub(super) const WAYPOINT_LIST: &str = "waypoint_list";
/// Opens the map's settings.
pub(super) const SETTINGS: &str = "settings";

/// Each key: its name, its label on the page, and its default (`key.keyboard.g`).
const KEYS: [[&str; 3]; 10] = [
    [MAP, "World Map", "key.keyboard.m"],
    [WAYPOINT, "Add Waypoint", "key.keyboard.b"],
    [ZOOM_IN, "Minimap Zoom In", "key.keyboard.equal"],
    [ZOOM_OUT, "Minimap Zoom Out", "key.keyboard.minus"],
    [CAVE, "Cave View Auto/Manual/Off", "key.keyboard.k"],
    [CAVE_HERE, "Cave Start Y Here", "key.keyboard.unknown"],
    [MINIMAP, "Show/Hide Minimap", "key.keyboard.h"],
    [WAYPOINTS, "Show/Hide Waypoints", "key.keyboard.unknown"],
    [WAYPOINT_LIST, "Waypoint List", "key.keyboard.u"],
    [SETTINGS, "Map Settings", "key.keyboard.unknown"],
];

/// Every key's name and label, for the settings' Keys page.
pub(super) fn labels() -> Vec<(&'static str, &'static str)> {
    KEYS.iter().map(|k| (k[0], k[1])).collect()
}

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

/// Whether vanilla's key mapping `mapping` (`key.playerlist`) is held now, on the key the
/// player bound it to in Controls.
pub(super) fn vanilla_held(game: &Game<'_>, mapping: &str) -> bool {
    let key = game.settings.keys.get(mapping).to_owned();
    if let Some(code) = horizon_client::keybinds::key_code(&key) {
        return game.physical_keys.contains(&code);
    }
    horizon_client::keybinds::mouse_button(&key).is_some_and(|b| game.physical_buttons.contains(&b))
}
