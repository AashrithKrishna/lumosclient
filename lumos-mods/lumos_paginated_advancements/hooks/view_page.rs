fn lumos_paginated_advancements_page(value: View, game: &crate::game::Game<'_>) -> View {
    let View {
        tabs,
        widgets,
        parent,
        bounds,
    } = value;
    let tabs = crate::game::lumos_paginated_advancements_glue::page_tabs(game, tabs, game.adv_ui.tab);
    View {
        tabs,
        widgets,
        parent,
        bounds,
    }
}
