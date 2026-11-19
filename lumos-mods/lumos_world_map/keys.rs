// Lumos World Map: every key the mod reads, and the only place that reads them.
//
// Horizon 9f42064c has no Controls-screen API for a mod's keys, so they are listed in
// `NativeMod::keys`: Options > Mods shows them on the mod's page, rebinds them there and keeps
// the choice as `key.<mod id>.<name>` in `horizon-mods.txt` (`native_mod_key` reads it), and
// Lumos Controls lists them in Controls. Moving to `Registrar::register_key_mapping` later
// changes this file only. The keys marked "on the map" act only while the map is open.
use crate::game::Game;
use crate::game::lumos_world_map as m;

/// Opens (and closes) the map.
pub(super) const MAP: &str = "map";
/// Confirms the map the world was matched to, without opening the map.
pub(super) const CONFIRM: &str = "quick_confirm";
/// On the map: zoom in and out.
pub(super) const ZOOM_IN: &str = "zoom_in";
pub(super) const ZOOM_OUT: &str = "zoom_out";
/// On the map: cave mode Auto or Off.
pub(super) const CAVE: &str = "cave";
/// On the map: the cave layer a band down or up.
pub(super) const LAYER_DOWN: &str = "layer_down";
pub(super) const LAYER_UP: &str = "layer_up";
/// On the map: the next dimension's map.
pub(super) const DIMENSION: &str = "dimension";
/// On the map: back to you.
pub(super) const CENTRE: &str = "centre";
/// On the map: following you on or off.
pub(super) const FOLLOW: &str = "follow";
/// On the map: a waypoint where the pointer is.
pub(super) const WAYPOINT: &str = "waypoint";
/// On the map: the waypoint list.
pub(super) const WAYPOINTS: &str = "waypoints";
/// On the map: bars and buttons hidden.
pub(super) const HIDE_UI: &str = "hide_ui";
/// On the map: saved as a PNG.
pub(super) const EXPORT: &str = "export";

/// Each key: its name, its label, and its default (`key.keyboard.m`).
const KEYS: [[&str; 3]; 14] = [
    [MAP, "Open World Map", "key.keyboard.m"],
    [CONFIRM, "Quick Confirm Map", "key.keyboard.unknown"],
    [ZOOM_IN, "Map: Zoom In", "key.keyboard.page.up"],
    [ZOOM_OUT, "Map: Zoom Out", "key.keyboard.page.down"],
    [CAVE, "Map: Cave Mode Auto/Off", "key.keyboard.v"],
    [LAYER_DOWN, "Map: Cave Layer Down", "key.keyboard.left.bracket"],
    [LAYER_UP, "Map: Cave Layer Up", "key.keyboard.right.bracket"],
    [DIMENSION, "Map: Next Dimension", "key.keyboard.n"],
    [CENTRE, "Map: Centre on You", "key.keyboard.home"],
    [FOLLOW, "Map: Follow You", "key.keyboard.unknown"],
    [WAYPOINT, "Map: Waypoint at Pointer", "key.keyboard.b"],
    [WAYPOINTS, "Map: Waypoint List", "key.keyboard.u"],
    [HIDE_UI, "Map: Hide Bars", "key.keyboard.f1"],
    [EXPORT, "Map: Export PNG", "key.keyboard.unknown"],
];

/// Every key's name and label, for the Keys pages.
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

/// Whether the `native_key` event (`key` going down when `down`) is `name`'s key going down.
/// F3 held makes it a debug combination, not the mod's key.
pub(super) fn pressed(game: &Game<'_>, name: &str, key: &str, down: bool) -> bool {
    down && !game.physical_keys.contains(&winit::keyboard::KeyCode::F3) && key == bound(game, name) && key != "key.keyboard.unknown"
}

/// Whether a key pressed on the map's screen (`key`, as the screen hook gives it) is `name`'s.
pub(super) fn on_screen(game: &Game<'_>, name: &str, key: &str) -> bool {
    key != "key.keyboard.unknown" && key == bound(game, name)
}
