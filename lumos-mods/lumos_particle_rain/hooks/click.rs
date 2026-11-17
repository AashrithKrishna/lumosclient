fn lumos_particle_rain_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::lumos_particle_rain_glue::on_click(game, mod_id, button);
}
