fn lumos_fullbright_key_page(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    crate::game::lumos_fullbright_key_glue::on_settings(game, mod_id, buttons);
}
