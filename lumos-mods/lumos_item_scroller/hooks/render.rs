fn lumos_item_scroller_render(game: &crate::game::Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    crate::game::lumos_item_scroller_glue::on_render(game, batch, screen);
}
