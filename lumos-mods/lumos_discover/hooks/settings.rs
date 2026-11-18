fn lumos_discover_page(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    crate::game::lumos_discover_glue::on_settings(game, mod_id, buttons);
}
