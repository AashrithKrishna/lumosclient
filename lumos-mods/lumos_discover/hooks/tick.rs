fn lumos_discover_tick(game: &mut crate::game::Game<'_>) {
    crate::game::lumos_discover_glue::on_tick(game);
}
