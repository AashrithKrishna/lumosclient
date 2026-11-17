fn lumos_paginated_advancements_scroll(game: &mut crate::game::Game<'_>, y: f32) -> std::ops::ControlFlow<()> {
    if crate::game::lumos_paginated_advancements_glue::on_scroll(game, y) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
