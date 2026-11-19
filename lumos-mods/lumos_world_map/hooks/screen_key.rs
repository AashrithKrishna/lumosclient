fn lumos_world_map_screen_key(
    game: &crate::game::Game<'_>,
    screen: &NativeScreenView<'_>,
    key: &str,
    canceled: &mut bool,
) {
    crate::game::lumos_world_map_glue::on_key_pressed(game, screen, key, canceled);
}
