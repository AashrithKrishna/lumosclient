fn lumos_hud_attacked(game: &crate::game::Game<'_>, id: i32, distance: f64) {
    crate::game::lumos_hud_glue::on_attacked(game, id, distance);
}
