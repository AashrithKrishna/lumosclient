fn lumos_paginated_advancements_page_buttons(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    crate::game::lumos_paginated_advancements_glue::on_settings(game, mod_id, buttons);
}
