fn lumos_hud_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::lumos_hud_glue::on_click(game, mod_id, button);
}
