fn lumos_hud_layer(game: &crate::game::Game<'_>, layer: &str, canceled: &mut bool) {
    crate::game::lumos_hud_glue::on_layer(game, layer, canceled);
}
