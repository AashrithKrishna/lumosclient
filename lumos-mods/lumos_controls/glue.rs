// Lumos Controls: between the options screen (`OptionsUi`, `Settings`) and the rules in
// `lumos_controls`.
//
// Inserted into `crates/horizon-client-render/src/options_ui.rs` as `mod lumos_controls_glue`, so
// it builds the same widgets as Key Binds' own rows. The options screen has no `Game`: its hooks
// check the switch `native_on` reads (`mods.disabled` in `Settings`), or a flag the layout hook
// set after checking it.
//
// Key Binds gets a section per enabled native mod with keys, after vanilla's categories. A key's
// button is the one its page in Options > Mods has (`Act::NativeKey`): a click waits for a key,
// Escape unbinds, and the key lands in `mods.keys` (`key.<id>.<name>` in horizon-mods.txt), where
// `native_mod_key` reads it at once. Reset removes the line, as `native_set_mod_key(None)` does.
use super::*;
use super::lumos_controls as m;
use std::cell::Cell;
use std::ops::ControlFlow;

thread_local! {
    /// While Key Binds is laid out with the mod on: how much the mods' rows add to the list.
    static EXTRA: Cell<Option<f32>> = const { Cell::new(None) };
    /// Key Binds was last laid out with the mod on (its Reset buttons are on screen).
    static SHOWN: Cell<bool> = const { Cell::new(false) };
    /// A Reset button pressed: (row in Options > Mods, key), for the click or key that pressed it.
    static RESET: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

/// The mod is on (`Game::native_on`).
fn on(s: &Settings) -> bool {
    !s.mods.disabled.contains(m::ID)
}

/// The mods' lines as they are now.
fn lines(ui: &OptionsUi, s: &Settings) -> Vec<m::Line> {
    let mods: Vec<m::ModKeys<'_>> = s
        .mods
        .installed
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let page = row.native.as_ref()?;
            let shown = !page.keys.is_empty() && !s.mods.disabled.contains(&row.id);
            shown.then_some(m::ModKeys {
                index,
                id: &row.id,
                title: &row.title,
                keys: &page.keys,
            })
        })
        .collect();
    // Every vanilla mapping's key, and Horizon's own keys while what they do is on.
    let mut others: Vec<&str> = all_mappings().map(|(name, _, _)| s.keys.get(name)).collect();
    if s.extras.zoom {
        others.push(&s.extras.zoom_key);
    }
    if s.extras.sort {
        others.push(&s.extras.sort_key);
    }
    others.push(&s.extras.pvp.editor_key);
    m::lines(&mods, &s.mods.keys, &others, &ui.settings_search.value)
}

/// `layout`'s start: on Key Binds with the mod on, the list's extra height, so that the scroll
/// limit `layout` sets (`clamp_scroll`) reaches the mods' rows.
pub(crate) fn before_layout(ui: &mut OptionsUi, s: &Settings) {
    let shown = on(s) && ui.page() == Page::KeyBinds;
    SHOWN.set(shown);
    EXTRA.set(shown.then(|| m::height(&lines(ui, s))));
}

/// `clamp_scroll` inside `layout`: the limit with the mods' rows counted.
pub(crate) fn clamp(ui: &mut OptionsUi, top: f32, bottom: f32, content: f32) -> ControlFlow<()> {
    let Some(extra) = EXTRA.get() else {
        return ControlFlow::Continue(());
    };
    let max = (content + extra - (bottom - top)).max(0.0);
    ui.scroll = ui.scroll.clamp(0.0, max);
    ControlFlow::Break(())
}

/// A widget in the list, in list row `row`.
fn listed(mut widget: Widget, row: usize) -> Widget {
    widget.list = true;
    widget.row = row;
    widget
}

/// Text in the list, as Key Binds' labels and headers.
fn label(x: f32, y: f32, w: f32, text: Text, centered: bool, row: usize) -> Widget {
    Widget {
        x,
        y,
        w,
        h: 9.0,
        kind: WKind::Text {
            text,
            color: u32::MAX,
            centered,
        },
        list: true,
        row,
    }
}

/// `layout`'s result on Key Binds: the mods' sections after vanilla's categories, in the same
/// scrolled list, and the list that much taller.
pub(crate) fn after_layout(
    ui: &mut OptionsUi,
    s: &Settings,
    w: f32,
    value: (Vec<Widget>, f32, f32, f32),
) -> (Vec<Widget>, f32, f32, f32) {
    if EXTRA.take().is_none() || !on(s) {
        return value;
    }
    let lines = lines(ui, s);
    let (mut out, top, bottom, content) = value;
    if lines.is_empty() {
        return (out, top, bottom, content);
    }
    let (label_x, change_x, reset_x) = m::columns(w);
    let mut y = m::list_end(top, content, ui.scroll);
    let mut mine = Vec::new();
    for line in &lines {
        let row = ui.row_bands.len();
        match line {
            m::Line::Header(title) => {
                // KeyBindsList.CategoryEntry: bold and underlined, in the middle.
                let text = Text::literal(title.clone()).with_style(&Style {
                    bold: Some(true),
                    underlined: Some(true),
                    ..Style::default()
                });
                mine.push(label(0.0, y + 7.0, w, text, true, row));
            }
            m::Line::Key {
                index,
                key,
                label: caption,
                bound,
                default,
                clash,
            } => {
                mine.push(label(
                    label_x,
                    y + 6.0,
                    200.0,
                    Text::literal(caption.clone()),
                    false,
                    row,
                ));
                let mut message = keybinds::display_name(bound);
                if *clash {
                    message = Text::literal("[ ")
                        .append(message.with_color(text::RED))
                        .append(" ]")
                        .with_color(text::YELLOW);
                }
                if ui.selected_key == Some(NATIVE_KEY)
                    && ui.selected_native_key == Some((*index, *key))
                {
                    message = Text::literal("> ")
                        .append(message.with_style(&Style {
                            color: Some(text::WHITE),
                            underlined: Some(true),
                            ..Style::default()
                        }))
                        .append(" <")
                        .with_color(text::YELLOW);
                }
                let change = button(change_x, y, 75.0, message, Act::NativeKey(*index, *key));
                mine.push(listed(change, row));
                if *clash {
                    // KeyBindsList.KeyEntry's red mark left of a key that clashes.
                    mine.push(Widget {
                        x: change_x - 6.0,
                        y: y + 1.0,
                        w: 3.0,
                        h: 18.0,
                        kind: WKind::Text {
                            text: Text::empty(),
                            color: rgba(255, 85, 85, 255),
                            centered: false,
                        },
                        list: true,
                        row,
                    });
                }
                let reset = button(
                    reset_x,
                    y,
                    50.0,
                    tr("controls.reset"),
                    Act::NativeKey(*index, m::reset_act(*key)),
                );
                mine.push(listed(if *default { inactive(reset) } else { reset }, row));
            }
        }
        ui.row_bands.push([y, y + m::LINE]);
        y += m::LINE;
    }
    // In the list, before the footer's Reset Keys and Done, as Tab and the controller go.
    let at = out.iter().rposition(|wd| wd.list).map_or(out.len(), |i| i + 1);
    out.splice(at..at, mine);
    (out, top, bottom, content + m::height(&lines))
}

/// `wait_native_key`: a Reset button says which key to reset instead of waiting for one.
/// Only this mod's Reset buttons carry such a key index, and only while it is on.
pub(crate) fn wait(i: usize, k: usize) -> ControlFlow<()> {
    if !SHOWN.get() {
        return ControlFlow::Continue(());
    }
    let Some(key) = m::reset_of(k) else {
        return ControlFlow::Continue(());
    };
    RESET.set(Some((i, key)));
    ControlFlow::Break(())
}

/// The end of a click or key on the options screen: a Reset it pressed removes the key's
/// `key.<id>.<name>` line (the mod's default again), and the settings are saved and applied.
pub(crate) fn after_input(s: &mut Settings, mut events: Vec<Event>) -> Vec<Event> {
    let Some((i, k)) = RESET.take() else {
        return events;
    };
    if !on(s) {
        return events;
    }
    let Some(row) = s.mods.installed.get(i) else {
        return events;
    };
    let Some([name, ..]) = row.native.as_ref().and_then(|page| page.keys.get(k)) else {
        return events;
    };
    let (id, name) = (row.id.clone(), name.clone());
    if let Some(keys) = s.mods.keys.get_mut(&id)
        && keys.remove(&name).is_some()
    {
        events.push(Event::Changed);
    }
    events
}
