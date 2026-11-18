fn lumos_freelook_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::lumos_freelook_glue::on_click(game, mod_id, button);
}
