fn lumos_discover_render(game: &crate::game::Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    crate::game::lumos_discover_glue::on_render(game, batch, screen);
}
