fn lumos_totem_counter_hud(
    game: &mut crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    corners: &mut horizon_client::extras_hud::Corners,
) {
    crate::game::lumos_totem_counter_glue::on_hud(game, batch, corners);
}
