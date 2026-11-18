fn lumos_map_chat(game: &crate::game::Game<'_>, overlay: bool, message: &Text) {
    crate::game::lumos_map_glue::on_chat(game, overlay, message);
}
