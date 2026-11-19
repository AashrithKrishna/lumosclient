fn lumos_item_scroller_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::lumos_item_scroller_glue::on_click(game, mod_id, button);
}
