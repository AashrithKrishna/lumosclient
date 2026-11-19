// Item Scroller's hotkeys: the one place that reads them.
//
// Horizon 9f42064c has no key mappings for native mods in Options > Controls, so the keys are
// the mod's own (`NativeMod.keys`): Options > Mods lists them on the mod's page, where the player
// rebinds them (Lumos Controls also shows them in Key Binds), and `native_mod_key` reads the
// choice. The table is `HOTKEYS` in runtime.rs, with the modifiers each one needs (`key_mods`,
// fixed as Item Scroller's defaults). Moving to `Registrar::register_key_mapping` (a newer
// Horizon) only changes this file.
use crate::game::Game;
use crate::game::lumos_item_scroller as m;

/// `NativeMod.keys`: name, label and default key of each hotkey.
pub(super) fn listed() -> Vec<[String; 3]> {
    m::HOTKEYS
        .iter()
        .map(|(id, label, key)| [(*id).to_owned(), (*label).to_owned(), (*key).to_owned()])
        .collect()
}

/// The key hotkey `id` is bound to (`key.keyboard.q`).
pub(super) fn bound(game: &Game<'_>, id: &str) -> String {
    let default = m::HOTKEYS.iter().find(|k| k.0 == id).map_or("key.keyboard.unknown", |k| k.2);
    game.native_mod_key(m::ID, id, default)
}

/// A key press (`key.keyboard.c`, with `modifiers`) is hotkey `id`: its key, exactly its
/// modifiers. An unbound hotkey matches nothing; F3 held leaves the key to the debug keys.
pub(super) fn pressed(game: &Game<'_>, id: &str, key: &str, modifiers: u32) -> bool {
    key != "key.keyboard.unknown"
        && !game.physical_keys.contains(&winit::keyboard::KeyCode::F3)
        && modifiers & m::MODS == m::key_mods(id)
        && bound(game, id) == key
}

/// Whether `id`'s key (or mouse button) is held down now. A modifier key counts on either side.
pub(super) fn held(game: &Game<'_>, id: &str) -> bool {
    let key = bound(game, id);
    match key.as_str() {
        "key.keyboard.left.shift" | "key.keyboard.right.shift" => return game.has_shift_down(),
        "key.keyboard.left.control" | "key.keyboard.right.control" => return game.has_control_down(),
        "key.keyboard.left.alt" | "key.keyboard.right.alt" => return game.has_alt_down(),
        _ => {}
    }
    if let Some(code) = horizon_client::keybinds::key_code(&key) {
        return game.physical_keys.contains(&code);
    }
    horizon_client::keybinds::mouse_button(&key).is_some_and(|b| game.physical_buttons.contains(&b))
}

/// A mouse press of GLFW `button` is hotkey `id`'s (bound to `key.mouse.middle` and the like).
pub(super) fn button_is(game: &Game<'_>, id: &str, button: i32) -> bool {
    let name = match button {
        0 => "key.mouse.left",
        1 => "key.mouse.right",
        2 => "key.mouse.middle",
        3 => "key.mouse.4",
        4 => "key.mouse.5",
        _ => return false,
    };
    bound(game, id) == name
}

/// The keys Item Scroller's clicks look at besides the modifiers.
pub(super) fn held_keys(game: &Game<'_>) -> m::Held {
    m::Held {
        drop: held(game, "drop_key"),
        up: held(game, "move_up"),
        down: held(game, "move_down"),
        space: held(game, "move_everything_old"),
    }
}
