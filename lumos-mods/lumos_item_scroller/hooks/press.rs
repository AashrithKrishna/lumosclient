fn lumos_item_scroller_press(
    game: &crate::game::Game<'_>,
    screen: &NativeScreenView<'_>,
    button: i32,
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::lumos_item_scroller_glue::on_press(game, screen, button, modifiers, canceled);
}
