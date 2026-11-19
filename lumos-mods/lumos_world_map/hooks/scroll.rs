fn lumos_world_map_scroll(
    game: &mut crate::game::Game<'_>,
    screen: &str,
    mouse: [f32; 2],
    scroll: [f64; 2],
    canceled: &mut bool,
) {
    crate::game::lumos_world_map_glue::on_scroll(game, screen, mouse, scroll, canceled);
}
