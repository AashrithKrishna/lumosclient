fn lumos_durability_tooltip_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::lumos_durability_tooltip_glue::on_click(game, mod_id, button);
}
