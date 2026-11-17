fn lumos_paginated_advancements_render(
    game: &crate::game::Game<'_>,
    batch: &mut HudBatch,
    screen: &NativeScreenView<'_>,
) {
    crate::game::lumos_paginated_advancements_glue::on_render(game, batch, screen);
}
