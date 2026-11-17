fn lumos_totem_counter_page(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    crate::game::lumos_totem_counter_glue::on_settings(game, mod_id, buttons);
}
