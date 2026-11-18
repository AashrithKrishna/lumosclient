fn lumos_hud_drag(game: &crate::game::Game<'_>, screen: &NativeScreenView<'_>, modifiers: u32, canceled: &mut bool) {
    crate::game::lumos_hud_glue::on_drag(game, screen, modifiers, canceled);
}
