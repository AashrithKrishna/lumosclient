fn lumos_freelook_turn(game: &mut crate::game::Game<'_>, x: f64, y: f64) -> std::ops::ControlFlow<()> {
    if crate::game::lumos_freelook_glue::on_turn(game, x, y) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
