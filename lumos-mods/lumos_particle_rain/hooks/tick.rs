fn lumos_particle_rain_tick(game: &mut crate::game::Game<'_>) {
    crate::game::lumos_particle_rain_glue::on_tick(game);
}
