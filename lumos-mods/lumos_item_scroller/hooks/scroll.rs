fn lumos_item_scroller_scroll(
    game: &mut crate::game::Game<'_>,
    screen: &str,
    scroll: [f64; 2],
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::lumos_item_scroller_glue::on_scroll(game, screen, scroll, modifiers, canceled);
}
