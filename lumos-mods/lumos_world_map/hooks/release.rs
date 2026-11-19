fn lumos_world_map_release(
    game: &crate::game::Game<'_>,
    screen: &NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    crate::game::lumos_world_map_glue::on_release(game, screen, button, canceled);
}
