fn lumos_freelook_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::lumos_freelook_glue::on_key(game, key, pressed);
}
