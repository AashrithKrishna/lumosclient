fn lumos_discover_char(game: &crate::game::Game<'_>, screen: &NativeScreenView<'_>, c: char, canceled: &mut bool) {
    crate::game::lumos_discover_glue::on_char(game, screen, c, canceled);
}
