fn lumos_paginated_advancements_mouse(
    game: &mut crate::game::Game<'_>,
    pressed: bool,
    size: [u32; 2],
) -> std::ops::ControlFlow<()> {
    if crate::game::lumos_paginated_advancements_glue::on_mouse(game, pressed, size) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
