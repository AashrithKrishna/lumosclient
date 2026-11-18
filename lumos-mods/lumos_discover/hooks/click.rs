fn lumos_discover_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::lumos_discover_glue::on_click(game, mod_id, button);
}
