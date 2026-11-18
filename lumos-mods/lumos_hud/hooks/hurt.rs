fn lumos_hud_hurt(game: &crate::game::Game<'_>, id: i32, own: bool, by_me: Option<bool>) {
    crate::game::lumos_hud_glue::on_hurt(game, id, own, by_me);
}
