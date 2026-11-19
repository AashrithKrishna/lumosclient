// Item Scroller's hotkeys: the one place that reads them.
//
// Horizon 9f42064c has no key mappings for native mods in Options > Controls, so the keys are
// the mod's own (`NativeMod.keys`): Options > Mods lists them on the mod's page, where the player
// rebinds them (Lumos Controls also shows them in Key Binds), and `native_mod_key` reads the
// choice. The table is `HOTKEYS` in runtime.rs. Moving to `Registrar::register_key_mapping` (a
// newer Horizon) only changes this file.
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

/// A key press (`key.keyboard.q`) is hotkey `id`'s key; an unbound hotkey matches nothing.
pub(super) fn pressed(game: &Game<'_>, id: &str, key: &str) -> bool {
    key != "key.keyboard.unknown" && bound(game, id) == key
}
