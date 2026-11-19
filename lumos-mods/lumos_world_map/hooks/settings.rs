fn lumos_world_map_page(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    crate::game::lumos_world_map_glue::on_settings(game, mod_id, buttons);
}
