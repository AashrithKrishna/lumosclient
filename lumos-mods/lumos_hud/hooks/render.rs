fn lumos_hud_render(game: &crate::game::Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    crate::game::lumos_hud_glue::on_render(game, batch, screen);
}
