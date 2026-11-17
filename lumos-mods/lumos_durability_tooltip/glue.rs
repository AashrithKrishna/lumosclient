// Lumos Durability Tooltip: between Horizon's `Game` and the rules in `lumos_durability_tooltip`.
use crate::game::Game;
use crate::game::lumos_durability_tooltip as m;
use crate::mod_api::NativeMod;
use horizon_client::hud::rgba;
use horizon_client::text::Text;
use horizon_items::items::ItemStack;

/// `native_tooltip`: every item tooltip the game shows (slots, creative tabs, the recipe
/// book, chat hovers), once its own lines are built.
pub(super) fn on_tooltip(game: &Game<'_>, stack: &ItemStack, lines: &mut Vec<(Text, u32)>) {
    if !game.native_on(m::ID) || lines.is_empty() {
        return;
    }
    let settings = m::settings(|key| game.native_setting(m::ID, key));
    // `ItemStack.getMaxDamage` when the stack is damageable: 0 for an unbreakable one.
    let Some(durability) = m::Durability::of(stack.damage, game.items().stack_max_damage(stack))
    else {
        return;
    };
    if durability.full() && !settings.show_full {
        return;
    }
    // Advanced tooltips (F3+H) already print vanilla's line for a damaged item.
    if stack.damage > 0 && game.settings.file.bool("advancedItemTooltips") {
        return;
    }
    let numbers = || {
        Text::translate_with_fallback(
            "item.durability",
            m::FALLBACK,
            &[
                Text::literal(durability.remaining.to_string()),
                Text::literal(durability.max.to_string()),
            ],
        )
    };
    let text = match durability.suffix(settings.style) {
        None => numbers(),
        Some(percent) if settings.style == 2 => Text::literal(format!("Durability: {percent}")),
        Some(percent) => numbers().append(Text::literal(percent)),
    };
    let rgb = durability.colour(settings.colour);
    let colour = rgba((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 0xFF);
    let at = m::insert_at(lines.len(), &stack.item, |i| lines[i].0.plain());
    lines.insert(at, (text, colour));
}

pub(super) fn on_list(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: m::ID.into(),
        title: format!("{} {}", m::NAME, m::VERSION),
        description: m::DESCRIPTION.into(),
        keys: Vec::new(),
    });
}

pub(super) fn on_settings(game: &mut Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id != m::ID {
        return;
    }
    let g = &*game;
    buttons.extend(m::settings(|key| g.native_setting(m::ID, key)).labels());
}

pub(super) fn on_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != m::ID {
        return;
    }
    let mut settings = {
        let g = &*game;
        m::settings(|key| g.native_setting(m::ID, key))
    };
    settings.click(button);
    m::set_settings(settings);
    for (key, value) in settings.stored() {
        game.native_set_setting(m::ID, key, &value);
    }
}
