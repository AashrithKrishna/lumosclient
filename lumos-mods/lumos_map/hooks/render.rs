fn lumos_map_render(game: &crate::game::Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    crate::game::lumos_map_glue::on_render(game, batch, screen);
}
