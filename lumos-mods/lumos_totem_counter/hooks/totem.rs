fn lumos_totem_counter_popped(game: &crate::game::Game<'_>, target: TrackingTarget) {
    crate::game::lumos_totem_counter_glue::on_totem(game, target);
}
