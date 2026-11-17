fn lumos_totem_counter_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::lumos_totem_counter_glue::on_click(game, mod_id, button);
}
