fn lumos_world_map_render(game: &crate::game::Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    crate::game::lumos_world_map_glue::on_render(game, batch, screen);
}
