fn lumos_hud_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::lumos_hud_glue::on_key(game, key, pressed);
}
