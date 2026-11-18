fn lumos_map_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::lumos_map_glue::on_key(game, key, pressed);
}
