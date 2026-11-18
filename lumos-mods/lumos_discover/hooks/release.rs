fn lumos_discover_release(game: &crate::game::Game<'_>, screen: &NativeScreenView<'_>, canceled: &mut bool) {
    crate::game::lumos_discover_glue::on_release(game, screen, canceled);
}
