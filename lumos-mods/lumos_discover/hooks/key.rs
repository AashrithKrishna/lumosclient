fn lumos_discover_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::lumos_discover_glue::on_key(game, key, pressed);
}
