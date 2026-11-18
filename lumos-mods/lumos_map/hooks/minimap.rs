fn lumos_map_minimap(game: &crate::game::Game<'_>, minimap: &mut Option<NativeMinimap>) {
    crate::game::lumos_map_glue::on_minimap(game, minimap);
}
