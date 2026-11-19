fn lumos_item_scroller_drag(
    game: &mut crate::game::Game<'_>,
    screen: &str,
    mouse: [f32; 2],
    button: i32,
    delta: [f32; 2],
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::lumos_item_scroller_glue::on_drag(game, screen, mouse, button, delta, modifiers, canceled);
}
