fn lumos_fullbright_key_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::lumos_fullbright_key_glue::on_click(game, mod_id, button);
}
