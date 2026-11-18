fn lumos_discover_screen_key(game: &crate::game::Game<'_>, screen: &NativeScreenView<'_>, key: &str, canceled: &mut bool) {
    crate::game::lumos_discover_glue::on_screen_key(game, screen, key, canceled);
}
