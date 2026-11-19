fn lumos_item_scroller_drag(
    game: &mut crate::game::Game<'_>,
    screen: &str,
    button: i32,
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::lumos_item_scroller_glue::on_drag(game, screen, button, modifiers, canceled);
}
