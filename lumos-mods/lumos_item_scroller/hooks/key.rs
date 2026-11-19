fn lumos_item_scroller_key(
    game: &crate::game::Game<'_>,
    screen: &NativeScreenView<'_>,
    key: &str,
    repeat: bool,
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::lumos_item_scroller_glue::on_key(game, screen, key, repeat, modifiers, canceled);
}
