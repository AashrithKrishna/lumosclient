fn lumos_durability_tooltip_lines(
    game: &crate::game::Game<'_>,
    stack: &ItemStack,
    lines: &mut Vec<(Text, u32)>,
) {
    crate::game::lumos_durability_tooltip_glue::on_tooltip(game, stack, lines);
}
