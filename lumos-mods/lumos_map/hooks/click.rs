fn lumos_map_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::lumos_map_glue::on_click(game, mod_id, button);
}
