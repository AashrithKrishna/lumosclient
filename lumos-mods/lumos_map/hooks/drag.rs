fn lumos_map_drag(
    game: &mut crate::game::Game<'_>,
    screen: &str,
    mouse: [f32; 2],
    delta: [f32; 2],
    canceled: &mut bool,
) {
    crate::game::lumos_map_glue::on_drag(game, screen, mouse, delta, canceled);
}
