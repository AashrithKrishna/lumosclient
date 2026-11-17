fn lumos_particle_rain_fog(
    game: &crate::game::Game<'_>,
    partial: f32,
    camera: [f64; 3],
    fog_type: horizon_client::fog::FogType,
    fog: &mut horizon_client::fog::FogData,
) {
    crate::game::lumos_particle_rain_glue::on_fog(game, partial, camera, fog_type, fog);
}
