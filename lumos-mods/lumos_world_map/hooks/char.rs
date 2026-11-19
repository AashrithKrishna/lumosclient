fn lumos_world_map_char(game: &crate::game::Game<'_>, screen: &NativeScreenView<'_>, c: char, canceled: &mut bool) {
    crate::game::lumos_world_map_glue::on_char(game, screen, c, canceled);
}
