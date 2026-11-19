fn lumos_item_scroller_release(game: &crate::game::Game<'_>, button: i32, canceled: &mut bool) {
    crate::game::lumos_item_scroller_glue::on_release(game, button, canceled);
}
