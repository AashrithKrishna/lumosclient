// Lumos Paginated Advancements: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_paginated_advancements`;
// `glue.rs` and the Advancements screen's hooks call in here. It names nothing outside `std`, so
// it also compiles alone: `rustc --edition 2024 --test runtime.rs` runs the tests below.
use std::cell::RefCell;

pub const ID: &str = "lumos_paginated_advancements";
pub const NAME: &str = "Paginated Advancements";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "The Advancements screen pages its tabs: arrows, Page Up/Down \
and the wheel over the tabs reach every tab, however many data packs and mods add.";

/// The Advancements screen's id (`Screen::id`).
pub const SCREEN: &str = "advancements";
/// Vanilla's tab places around the window: 8 above, 8 below, 5 left, 5 right.
pub const VANILLA_MAX: usize = 26;
pub const PER_PAGE: [usize; 3] = [8, 16, 26];
pub const PER_PAGE_LABELS: [&str; 3] = ["8 (top row)", "16 (top and bottom)", "26 (all sides)"];

pub const KEY_PREVIOUS: &str = "key.keyboard.page.up";
pub const KEY_NEXT: &str = "key.keyboard.page.down";

/// The window (`AdvancementsScreen`): 252 x 140, centred.
pub const WINDOW_W: f32 = 252.0;
pub const WINDOW_H: f32 = 140.0;
pub const ARROW: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Index in `PER_PAGE`.
    pub per_page: usize,
    /// The wheel over the tabs (outside the tree) turns the page.
    pub wheel: bool,
    /// "Page 2 / 3" in the window's title line.
    pub label: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            per_page: 0,
            wheel: true,
            label: true,
        }
    }
}

impl Settings {
    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        let flag = |key: &str, default: bool| get(key).map_or(default, |v| v.trim() == "true");
        Self {
            per_page: get("per_page")
                .and_then(|v| v.trim().parse::<usize>().ok())
                .filter(|v| *v < PER_PAGE.len())
                .unwrap_or(d.per_page),
            wheel: flag("wheel", d.wheel),
            label: flag("label", d.label),
        }
    }

    pub fn labels(&self) -> Vec<String> {
        let on = |b: bool| if b { "ON" } else { "OFF" };
        vec![
            format!("Tabs a page: {}", PER_PAGE_LABELS[self.per_page]),
            format!("Wheel over tabs turns page: {}", on(self.wheel)),
            format!("Page number: {}", on(self.label)),
        ]
    }

    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.per_page = (self.per_page + 1) % PER_PAGE.len(),
            1 => self.wheel = !self.wheel,
            2 => self.label = !self.label,
            _ => {}
        }
    }

    pub fn stored(&self) -> [(&'static str, String); 3] {
        [
            ("per_page", self.per_page.to_string()),
            ("wheel", self.wheel.to_string()),
            ("label", self.label.to_string()),
        ]
    }

    pub fn tabs_per_page(&self) -> usize {
        PER_PAGE[self.per_page]
    }
}

pub fn page_count(total: usize, per_page: usize) -> usize {
    total.div_ceil(per_page.max(1)).max(1)
}

/// The paging of the open Advancements screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pager {
    pub page: usize,
    /// The tab the last view showed as selected: when the selection changes other than by a
    /// click on this page (the screen opening on a remembered tab), its page comes up.
    last_selected: Option<usize>,
    /// All the tabs and the size of a page, as the last view had them.
    pub total: usize,
    pub per_page: usize,
}

impl Pager {
    pub fn pages(&self) -> usize {
        page_count(self.total, self.per_page)
    }

    /// The tabs to show from `all` (every displayed root, in the screen's order), with
    /// `selected` the screen's selected tab and `per_page` tabs a page.
    pub fn view(&mut self, all: &[usize], selected: Option<usize>, per_page: usize) -> Vec<usize> {
        let per_page = per_page.clamp(1, VANILLA_MAX);
        self.total = all.len();
        self.per_page = per_page;
        if selected != self.last_selected {
            if let Some(at) = selected.and_then(|s| all.iter().position(|t| *t == s)) {
                let page = at / per_page;
                // A click picks a tab on the page shown; anything else jumps to its page.
                if page != self.page {
                    self.page = page;
                }
            }
            self.last_selected = selected;
        }
        self.page = self.page.min(self.pages() - 1);
        let from = self.page * per_page;
        all[from..(from + per_page).min(all.len())].to_vec()
    }

    /// Turns `delta` pages, stopping at the first and the last. True when the page changed.
    pub fn turn(&mut self, delta: i32) -> bool {
        let last = self.pages() - 1;
        let page = if delta < 0 {
            self.page.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (self.page + delta as usize).min(last)
        };
        let changed = page != self.page;
        self.page = page;
        changed
    }

    /// The screen closed: it opens next time on its selected tab's page.
    pub fn closed(&mut self) {
        self.last_selected = None;
    }
}

/// The window's top left in a GUI of `width` x `height` (`AdvancementsScreen`'s centring).
pub fn window(width: f32, height: f32) -> (f32, f32) {
    (
        ((width - WINDOW_W) / 2.0).floor(),
        ((height - WINDOW_H) / 2.0).floor(),
    )
}

/// The previous and next page buttons, `[x, y, w, h]`: beside the top row of tabs, clear of
/// the tabs on every side.
pub fn arrows(width: f32, height: f32) -> ([f32; 4], [f32; 4]) {
    let (left, top) = window(width, height);
    let y = (top - 26.0).max(0.0);
    (
        [(left - ARROW - 6.0).max(0.0), y, ARROW, ARROW],
        [(left + WINDOW_W + 6.0).min(width - ARROW), y, ARROW, ARROW],
    )
}

pub fn inside(rect: [f32; 4], at: (f32, f32)) -> bool {
    at.0 >= rect[0] && at.0 < rect[0] + rect[2] && at.1 >= rect[1] && at.1 < rect[1] + rect[3]
}

/// Whether the pointer is over the tabs (around the window, or on its frame) rather than the
/// tree inside it, where the wheel scrolls the tree as before.
pub fn over_tabs(width: f32, height: f32, at: (f32, f32)) -> bool {
    let (left, top) = window(width, height);
    let around = [left - 32.0, top - 32.0, WINDOW_W + 64.0, WINDOW_H + 64.0];
    let tree = [left + 9.0, top + 18.0, 234.0, 113.0];
    inside(around, at) && !inside(tree, at)
}

/// "Page 2 / 3".
pub fn page_label(page: usize, pages: usize) -> String {
    format!("Page {} / {}", page + 1, pages)
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
    static PAGER: RefCell<Pager> = RefCell::new(Pager::default());
}

pub fn settings(get: impl Fn(&str) -> Option<String>) -> Settings {
    SETTINGS.with_borrow_mut(|s| *s.get_or_insert_with(|| Settings::read(get)))
}

pub fn set_settings(new: Settings) {
    SETTINGS.with_borrow_mut(|s| *s = Some(new));
}

pub fn with_pager<R>(f: impl FnOnce(&mut Pager) -> R) -> R {
    PAGER.with_borrow_mut(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |key| pairs.iter().find(|p| p.0 == key).map(|p| p.1.to_owned())
    }

    #[test]
    fn settings_round_trip() {
        assert_eq!(Settings::read(stored(&[])), Settings::default());
        let mut s = Settings::default();
        for b in 0..3 {
            s.click(b);
        }
        assert_eq!(
            s,
            Settings {
                per_page: 1,
                wheel: false,
                label: false
            }
        );
        let pairs = s.stored();
        let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(Settings::read(stored(&refs)), s);
        assert_eq!(s.tabs_per_page(), 16);
    }

    #[test]
    fn pages_cover_every_tab() {
        assert_eq!(page_count(0, 8), 1);
        assert_eq!(page_count(5, 8), 1);
        assert_eq!(page_count(8, 8), 1);
        assert_eq!(page_count(9, 8), 2);
        assert_eq!(page_count(60, 26), 3);
        let all: Vec<usize> = (100..140).collect();
        let mut p = Pager::default();
        let mut seen = Vec::new();
        let first = p.view(&all, Some(100), 8);
        assert_eq!(first, (100..108).collect::<Vec<_>>());
        seen.extend(first);
        while p.turn(1) {
            seen.extend(p.view(&all, Some(100), 8));
        }
        assert_eq!(seen, all);
        assert_eq!(p.page, 4);
        assert!(!p.turn(1));
        assert!(p.turn(-10));
        assert_eq!(p.page, 0);
    }

    #[test]
    fn few_tabs_stay_as_vanilla_shows_them() {
        let all = [3, 1, 4, 1, 5];
        let mut p = Pager::default();
        assert_eq!(p.view(&all, Some(3), 8), all.to_vec());
        assert_eq!(p.pages(), 1);
        assert!(!p.turn(1));
    }

    #[test]
    fn the_selected_tabs_page_comes_up_and_paging_keeps_the_selection() {
        let all: Vec<usize> = (0..20).collect();
        let mut p = Pager::default();
        // Opening on a remembered tab on the third page.
        assert_eq!(p.view(&all, Some(17), 8), (16..20).collect::<Vec<_>>());
        // Paging back keeps tab 17 selected without jumping again.
        assert!(p.turn(-1));
        assert_eq!(p.view(&all, Some(17), 8), (8..16).collect::<Vec<_>>());
        // Clicking a tab on the page shown stays on it.
        assert_eq!(p.view(&all, Some(9), 8), (8..16).collect::<Vec<_>>());
        // Reopening jumps to the selection's page again.
        p.turn(-1);
        p.closed();
        assert_eq!(p.view(&all, Some(9), 8), (8..16).collect::<Vec<_>>());
    }

    #[test]
    fn fewer_tabs_later_clamp_the_page() {
        let mut p = Pager::default();
        let many: Vec<usize> = (0..30).collect();
        p.view(&many, None, 8);
        p.turn(3);
        assert_eq!(p.page, 3);
        let few: Vec<usize> = (0..10).collect();
        assert_eq!(p.view(&few, None, 8), vec![8, 9]);
        assert_eq!(p.page, 1);
        // A page size past vanilla's 26 places is held to 26.
        assert_eq!(p.view(&many, None, 99).len(), 4);
    }

    #[test]
    fn arrows_sit_beside_the_window_and_on_screen() {
        let (prev, next) = arrows(427.0, 240.0);
        let (left, top) = window(427.0, 240.0);
        assert!(prev[0] + prev[2] <= left - 4.0);
        assert!(next[0] >= left + WINDOW_W + 4.0);
        assert!(prev[1] + prev[3] <= top);
        let (p2, n2) = arrows(256.0, 160.0);
        assert!(p2[0] >= 0.0 && n2[0] + n2[2] <= 256.0 && p2[1] >= 0.0);
        assert!(inside(prev, (prev[0] + 1.0, prev[1] + 1.0)));
        assert!(!inside(prev, (prev[0] + ARROW, prev[1])));
    }

    #[test]
    fn wheel_area_is_the_tabs_not_the_tree() {
        let (left, top) = window(427.0, 240.0);
        assert!(over_tabs(427.0, 240.0, (left + 10.0, top - 10.0)));
        assert!(over_tabs(427.0, 240.0, (left - 10.0, top + 50.0)));
        assert!(!over_tabs(427.0, 240.0, (left + 100.0, top + 60.0)));
        assert!(!over_tabs(427.0, 240.0, (2.0, 2.0)));
    }

    #[test]
    fn label() {
        assert_eq!(page_label(1, 3), "Page 2 / 3");
    }
}
