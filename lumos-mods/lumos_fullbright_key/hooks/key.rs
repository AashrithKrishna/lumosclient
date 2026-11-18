fn lumos_fullbright_key_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::lumos_fullbright_key_glue::on_key(game, key, pressed);
}
