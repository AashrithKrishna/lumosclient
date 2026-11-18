fn lumos_hud_tick(game: &mut crate::game::Game<'_>) {
    crate::game::lumos_hud_glue::on_tick(game);
}
