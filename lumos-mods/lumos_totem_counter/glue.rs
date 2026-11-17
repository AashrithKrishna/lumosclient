// Lumos Totem Counter: between Horizon's `Game` and the rules in `lumos_totem_counter`.
use crate::game::Game;
use crate::game::lumos_totem_counter as m;
use crate::mod_api::NativeMod;
use horizon_client::extras::Corner;
use horizon_client::extras_hud::Corners;
use horizon_client::hud::HudBatch;
use horizon_client::particles::TrackingTarget;
use horizon_client::text::Text;
use horizon_items::items::ItemStack;
use std::time::Instant;

/// The totems the player carries: the inventory (hotbar included) and the off hand.
fn carried(game: &Game<'_>) -> u32 {
    game.inventory
        .iter()
        .chain(std::iter::once(&game.offhand))
        .flatten()
        .filter(|s| &*s.item == m::TOTEM)
        .map(|s| s.count)
        .sum()
}

/// `native_hud`: the counter, and the notice of a pop once the inventory shows it.
pub(super) fn on_hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    if !game.native_on(m::ID) || game.player.is_none() {
        return;
    }
    let settings = {
        let g = &*game;
        m::settings(|key| g.native_setting(m::ID, key))
    };
    let now = Instant::now();
    let count = carried(game);
    let notice = m::with_tracker(|t| t.frame(count, now));
    if let Some((pops, left)) = notice
        && settings.notify
    {
        let colour = if left == 0 { m::RED } else { m::GOLD };
        game.native_client_action_bar(Text::colored(m::notice(pops, left), colour), false);
    }
    let (flashing, blink_on) = m::with_tracker(|t| (t.flashing(now), t.blink_on(now)));
    if count == 0 && !settings.show_zero && !flashing {
        return;
    }
    let label = format!("{count}");
    let text_w = game.native_text_width(&label);
    let (w, h) = (16.0 + 2.0 + text_w + 4.0, 18.0);
    let offhand = game.offhand.is_some();
    let left_hand = game.settings.main_hand_left;
    let (x, y) = match m::hotbar_place(
        settings.position,
        batch.width,
        batch.height,
        w,
        h,
        offhand && !left_hand,
        offhand && left_hand,
    ) {
        Some(at) => at,
        None => {
            let corner = match settings.position {
                3 => Corner::TopLeft,
                4 => Corner::TopRight,
                5 => Corner::BottomLeft,
                _ => Corner::BottomRight,
            };
            if !game.native_corner_shown(corner) {
                return;
            }
            corners.place(corner, w, h)
        }
    };
    if settings.background {
        game.native_fill(batch, [x, y, w, h], m::BACKGROUND);
    }
    let icon = ItemStack::new(m::TOTEM, 1);
    game.draw_item_model(batch, &icon, x + 1.0, y + 1.0);
    let colour = m::count_colour(count, settings.warn_at, flashing, blink_on);
    game.native_text(batch, &label, [x + 19.0, y + 5.0], colour, true);
}

/// `totem_event`: the totem particles and sound of entity event 35. Only the window's own
/// player counts as a pop of yours.
pub(super) fn on_totem(game: &Game<'_>, target: TrackingTarget) {
    if target == TrackingTarget::LocalPlayer && game.native_on(m::ID) {
        m::with_tracker(|t| t.popped(Instant::now()));
    }
}

/// The client left its level: the next one starts without a flash.
pub(super) fn on_level_unload() {
    m::with_tracker(m::Tracker::reset_count);
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
