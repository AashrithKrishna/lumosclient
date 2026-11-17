fn lumos_particle_rain_sheets(
    game: &crate::game::Game<'_>,
) -> std::ops::ControlFlow<Vec<horizon_client::weather::Vertex>> {
    if crate::game::lumos_particle_rain_glue::hide_sheets(game) {
        std::ops::ControlFlow::Break(Vec::new())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
