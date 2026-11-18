fn lumos_hud_press(game: &crate::game::Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    crate::game::lumos_hud_glue::on_press(game, screen, button, canceled);
}
