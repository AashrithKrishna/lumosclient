fn lumos_hud_page(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    crate::game::lumos_hud_glue::on_settings(game, mod_id, buttons);
}
