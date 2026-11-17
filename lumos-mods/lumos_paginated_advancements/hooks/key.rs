fn lumos_paginated_advancements_key(
    game: &crate::game::Game<'_>,
    screen: &NativeScreenView<'_>,
    key: &str,
    canceled: &mut bool,
) {
    crate::game::lumos_paginated_advancements_glue::on_key(game, screen, key, canceled);
}
