// Lumos Paginated Advancements: between Horizon's `Game` and the rules in
// `lumos_paginated_advancements`.
use crate::game::Game;
use crate::game::lumos_paginated_advancements as m;
use crate::mod_api::{NativeMod, NativeScreenView};
use horizon_client::hud::HudBatch;
use horizon_client::text::Text;
use horizon_client::ui;

fn settings(game: &Game<'_>) -> m::Settings {
    m::settings(|key| game.native_setting(m::ID, key))
}

/// More pages than one, with the mod on.
fn paging(game: &Game<'_>) -> bool {
    game.native_on(m::ID) && m::with_pager(|p| p.pages() > 1)
}

/// `adv_view`'s result: the tabs of the page shown. `all` is every displayed root in the
/// screen's order (the screen no longer cuts them at 26). With the mod off, vanilla's 26.
pub(super) fn page_tabs(game: &Game<'_>, all: Vec<usize>, selected: Option<usize>) -> Vec<usize> {
    if !game.native_on(m::ID) {
        let mut all = all;
        all.truncate(m::VANILLA_MAX);
        return all;
    }
    let per_page = settings(game).tabs_per_page();
    m::with_pager(|p| p.view(&all, selected, per_page))
}

/// The screen opens (`open_advancements`): on its selected tab's page.
pub(super) fn on_open() {
    m::with_pager(m::Pager::closed);
}

/// `native_screen_render_post`: the page buttons and the page number over the finished screen.
pub(super) fn on_render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if screen.id != m::SCREEN || !paging(game) {
        return;
    }
    let (page, pages) = m::with_pager(|p| (p.page, p.pages()));
    let (prev, next) = m::arrows(batch.width, batch.height);
    let atlas = &game.gui.atlas;
    for (rect, label, active) in [(prev, "<", page > 0), (next, ">", page + 1 < pages)] {
        let hover = active && m::inside(rect, game.cursor);
        ui::button(
            batch,
            atlas,
            rect[0],
            rect[1],
            rect[2],
            rect[3],
            &Text::literal(label),
            active,
            hover,
        );
    }
    if settings(game).label {
        let (left, top) = m::window(batch.width, batch.height);
        let text = m::page_label(page, pages);
        let w = game.native_text_width(&text);
        game.native_text(
            batch,
            &text,
            [left + m::WINDOW_W - 8.0 - w, top + 6.0],
            0x404040,
            false,
        );
    }
}

/// `advancements_mouse`: a press on a page button turns the page (and goes no further).
pub(super) fn on_mouse(game: &mut Game<'_>, pressed: bool, size: [u32; 2]) -> bool {
    if !pressed || !paging(game) {
        return false;
    }
    let b = HudBatch::new(size);
    let (prev, next) = m::arrows(b.width, b.height);
    let delta = if m::inside(prev, game.cursor) {
        -1
    } else if m::inside(next, game.cursor) {
        1
    } else {
        return false;
    };
    if m::with_pager(|p| p.turn(delta)) {
        game.audio
            .play("minecraft:ui.button.click", None, 0.25, 1.0);
    }
    true
}

/// `advancements_scroll`: the wheel over the tabs turns the page; over the tree it scrolls.
pub(super) fn on_scroll(game: &mut Game<'_>, y: f32) -> bool {
    if y == 0.0 || !paging(game) || !settings(game).wheel {
        return false;
    }
    let (width, height) = game.gui_size();
    if !m::over_tabs(width, height, game.cursor) {
        return false;
    }
    m::with_pager(|p| p.turn(if y > 0.0 { -1 } else { 1 }));
    true
}

/// `native_screen_key_pressed_pre`: Page Up and Page Down turn the page.
pub(super) fn on_key(game: &Game<'_>, screen: &NativeScreenView<'_>, key: &str, canceled: &mut bool) {
    if screen.id != m::SCREEN || *canceled || !paging(game) {
        return;
    }
    let delta = match key {
        m::KEY_PREVIOUS => -1,
        m::KEY_NEXT => 1,
        _ => return,
    };
    m::with_pager(|p| p.turn(delta));
    *canceled = true;
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
    buttons.extend(settings(game).labels());
}

pub(super) fn on_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != m::ID {
        return;
    }
    let mut new = settings(game);
    new.click(button);
    m::set_settings(new);
    for (key, value) in new.stored() {
        game.native_set_setting(m::ID, key, &value);
    }
}
