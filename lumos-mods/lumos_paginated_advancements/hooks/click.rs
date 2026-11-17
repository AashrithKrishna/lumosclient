fn lumos_paginated_advancements_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::lumos_paginated_advancements_glue::on_click(game, mod_id, button);
}
