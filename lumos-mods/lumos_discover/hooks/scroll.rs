fn lumos_discover_scroll(game: &crate::game::Game<'_>, screen: &NativeScreenView<'_>, scroll: [f64; 2], canceled: &mut bool) {
    crate::game::lumos_discover_glue::on_scroll(game, screen, scroll, canceled);
}
