fn lumos_hud_release(game: &crate::game::Game<'_>, screen: &NativeScreenView<'_>, canceled: &mut bool) {
    crate::game::lumos_hud_glue::on_release(game, screen, canceled);
}
