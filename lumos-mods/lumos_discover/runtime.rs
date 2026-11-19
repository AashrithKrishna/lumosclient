// Lumos Discover: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_discover`; `glue.rs` reads the
// game, runs the downloads on a worker and calls in here. It names nothing outside `std`, so it
// also compiles alone: `rustc --edition 2024 --test runtime.rs` runs the tests below.
//
// The screen is a list of draw primitives (`Prim`) and the places a click lands (`Hit`), built
// together by `frame`, so what is drawn and what is clicked never disagree. The catalogue
// (`Entry`) and what this instance has of each entry (`Have`) come from the glue.

use std::cell::RefCell;
use std::collections::BTreeMap;

pub const ID: &str = "lumos_discover";
pub const NAME: &str = "Lumos Discover";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "Browse and install mods, resource packs and shaders from \
Horizon's registry without leaving the game. Right Control (or the pause menu's Discover \
button) opens it. Packs and shaders apply at once; native mods apply after a restart.";

/// The mod's hotkeys: id, label (Options > Mods lists them, rebindable) and default key, by
/// vanilla's key names. `keys.rs` is the one place that reads them. Right Control has no
/// vanilla or Horizon mapping (Studio's free camera reads it only inside a replay).
pub const HOTKEYS: [(&str, &str, &str); 1] = [("open", "Open Discover", "key.keyboard.right.control")];

/// Whether key `key` (`key.keyboard.k`) types text: a letter, digit or sign, space, or a
/// keypad digit or sign. Bound to one, the hotkey must not close the screen while the search
/// box is being typed in (the key is a character of the search then).
pub fn types_text(key: &str) -> bool {
    let Some(k) = key.strip_prefix("key.keyboard.") else {
        return false;
    };
    let k = k.strip_prefix("keypad.").unwrap_or(k);
    k.chars().count() == 1
        || matches!(
            k,
            "space"
                | "apostrophe"
                | "comma"
                | "minus"
                | "period"
                | "slash"
                | "semicolon"
                | "equal"
                | "left.bracket"
                | "backslash"
                | "right.bracket"
                | "grave.accent"
                | "world.1"
                | "world.2"
                | "add"
                | "subtract"
                | "multiply"
                | "divide"
                | "decimal"
        )
}

/// What the game asks of Lumos when it closes (beside `options.txt`): native mods to add or
/// take out, content mods to delete, and whether to start the game again.
pub const REQUESTS_FILE: &str = "lumos-requests.json";
/// The instance's native mods, as Lumos writes it (`native_builds.rs`).
pub const NATIVE_FILE: &str = "lumos-native.json";

/// Lumos's own native mods (lumos-mods/): not in Horizon's registry, so listed from here. Keep
/// in step with `OWN` in obsidian-launcher-rs/src/horizon_mods.rs: (id, name, summary, categories).
pub const OWN: &[(&str, &str, &str, &[&str])] = &[
    ("lumos_durability_tooltip", "Durability Tooltip", "Shows durability as numbers on item tooltips, coloured green to red.", &["utility", "decoration"]),
    ("lumos_totem_counter", "Totem Counter", "Counts your Totems of Undying on the HUD and tells you when one pops.", &["utility", "game-mechanics"]),
    ("lumos_particle_rain", "Particle Rain", "Rain and snow as falling particles, with splashes and optional mist.", &["decoration"]),
    ("lumos_paginated_advancements", "Paginated Advancements", "Pages for the advancement tabs, so no tab is ever cut off.", &["utility"]),
    ("lumos_hud", "LumosHUD", "Lunar-style HUD modules (FPS, CPS, keystrokes, armour, effects and more). Right Shift opens the editor.", &["utility", "decoration"]),
    ("lumos_discover", "Lumos Discover", "Browse and install mods, resource packs and shaders from inside the game.", &["utility"]),
    ("lumos_item_scroller", "Item Scroller", "Move items with the mouse wheel and mass clicks, recipe slots with mass crafting, and favourite villager trades.", &["utility"]),
];

// ── colours (ARGB) ──

pub const ACCENT: u32 = 0xFFFF_6B2C;
pub const WHITE: u32 = 0xFFFF_FFFF;
pub const GREY: u32 = 0xFFA8_A8A8;
pub const DARK_GREY: u32 = 0xFF70_7070;
pub const GREEN: u32 = 0xFF6E_D46E;
pub const RED: u32 = 0xFFFF_6060;
pub const GOLD: u32 = 0xFFFF_C840;
pub const PANEL: u32 = 0xC010_1014;
pub const ROW_HOVER: u32 = 0x28FF_FFFF;
pub const FIELD: u32 = 0xFF00_0000;
pub const FIELD_EDGE: u32 = 0xFFA0_A0A0;

// ── the catalogue ──

/// The screen's tabs, as the launcher's Discover has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Mods,
    ResourcePacks,
    Shaders,
}

pub const TABS: [(Tab, &str); 3] = [(Tab::Mods, "Mods"), (Tab::ResourcePacks, "Resource Packs"), (Tab::Shaders, "Shaders")];

/// What an entry is, which says where it goes and how it applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A content mod (`.hmod`) into `horizon-mods/`.
    Content,
    /// A native mod: compiled into the game, so Lumos installs it between runs.
    Native,
    /// A resource pack into `resourcepacks/`.
    ResourcePack,
    /// A shader pack (`.hshader`, or a `shaders/` zip) into `shaderpacks/`.
    Shader,
}

impl Kind {
    pub fn tab(self) -> Tab {
        match self {
            Kind::Content | Kind::Native => Tab::Mods,
            Kind::ResourcePack => Tab::ResourcePacks,
            Kind::Shader => Tab::Shaders,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub author: String,
    pub kind: Kind,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub downloads: u64,
    /// The version this game installs ("" for a native mod).
    pub version: String,
    /// The registry's file name, which is also its name in the instance ("" for a native mod).
    pub file: String,
    /// One of Lumos's own (`OWN`).
    pub own: bool,
    /// A native mod Lumos's cloud builds have for this computer.
    pub cloud: bool,
}

/// What this instance has of an entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Have {
    pub installed: bool,
    /// Installed and switched on (a resource pack selected, the shader pack picked).
    pub on: bool,
    /// A native mod compiled into the running game: it switches with no restart.
    pub built_in: bool,
    pub pending: Pending,
}

/// A change that waits for the game to close.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pending {
    #[default]
    None,
    /// Lumos adds this native mod when the game closes.
    Install,
    /// Lumos takes it out when the game closes.
    Remove,
    /// A content mod with blocks, items or worldgen: it switches when the game starts again.
    Restart,
}

/// `[a-z0-9_]{1,64}`: Horizon's mod id rule, and all an id from the requests file may be.
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `game-mechanics` → `Game Mechanics`.
pub fn pretty(tag: &str) -> String {
    tag.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A search matches the id, name, summary, author, tags and categories, ignoring case; every
/// word of it must match somewhere.
pub fn matches(e: &Entry, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let hay = [e.id.as_str(), e.name.as_str(), e.summary.as_str(), e.author.as_str()]
        .into_iter()
        .chain(e.tags.iter().map(String::as_str))
        .chain(e.categories.iter().map(String::as_str))
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    q.split_whitespace().all(|w| hay.contains(w))
}

/// The entries a tab shows, by index: those matching the search and in `category`, the most
/// downloaded first, Lumos's own and then other native mods after the content mods (as the
/// launcher's Discover orders them).
pub fn visible(entries: &[Entry], tab: Tab, query: &str, category: Option<&str>) -> Vec<usize> {
    let mut out: Vec<usize> = (0..entries.len())
        .filter(|&i| {
            let e = &entries[i];
            e.kind.tab() == tab
                && matches(e, query)
                && category.is_none_or(|c| e.categories.iter().any(|x| x.eq_ignore_ascii_case(c)))
        })
        .collect();
    out.sort_by(|&a, &b| {
        let (a, b) = (&entries[a], &entries[b]);
        let rank = |e: &Entry| match e.kind {
            Kind::Native if e.own => 1,
            Kind::Native => 2,
            _ => 0,
        };
        rank(a)
            .cmp(&rank(b))
            .then(b.downloads.cmp(&a.downloads))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// The categories a tab's entries use, sorted, each once.
pub fn categories(entries: &[Entry], tab: Tab) -> Vec<String> {
    let mut out: Vec<String> = entries
        .iter()
        .filter(|e| e.kind.tab() == tab)
        .flat_map(|e| e.categories.iter().map(|c| c.to_lowercase()))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Lumos's own mods as entries; `cloud` says which a cloud build has.
pub fn own_entries(cloud: &dyn Fn(&str) -> bool) -> Vec<Entry> {
    OWN.iter()
        .map(|(id, name, summary, cats)| Entry {
            id: (*id).into(),
            name: (*name).into(),
            summary: (*summary).into(),
            author: "Lumos".into(),
            kind: Kind::Native,
            categories: cats.iter().map(|c| (*c).to_owned()).collect(),
            tags: Vec::new(),
            downloads: 0,
            version: String::new(),
            file: String::new(),
            own: true,
            cloud: cloud(id),
        })
        .collect()
}

// ── what a row offers ──

/// What a row's buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Install,
    Remove,
    On,
    Off,
    /// Takes back a change that waits for the restart.
    Undo,
}

impl Act {
    pub fn label(self) -> &'static str {
        match self {
            Act::Install => "Install",
            Act::Remove => "Remove",
            Act::On => "Turn on",
            Act::Off => "Turn off",
            Act::Undo => "Undo",
        }
    }
}

/// The buttons a row shows, and the word beside them (with its colour).
pub fn offer(e: &Entry, h: &Have, busy: bool) -> (Vec<Act>, &'static str, u32) {
    if busy {
        return (Vec::new(), "Working...", GOLD);
    }
    if e.kind == Kind::Native {
        if e.id == ID {
            return (Vec::new(), "Open now", GREEN);
        }
        return match h.pending {
            Pending::Install => (vec![Act::Undo], "Restart to apply", GOLD),
            Pending::Remove => (vec![Act::Undo], "Removed after restart", GOLD),
            _ if h.installed && h.built_in => (vec![if h.on { Act::Off } else { Act::On }, Act::Remove], if h.on { "On" } else { "Off" }, if h.on { GREEN } else { GREY }),
            _ if h.installed => (vec![Act::Remove], "Restart to apply", GOLD),
            _ if h.built_in || e.cloud => (vec![Act::Install], "", GREY),
            _ => (Vec::new(), "Not built for Lumos yet", DARK_GREY),
        };
    }
    if !h.installed {
        return (vec![Act::Install], "", GREY);
    }
    let toggle = if h.on { Act::Off } else { Act::On };
    let (word, colour) = match (h.pending, h.on) {
        (Pending::Restart, true) => ("On after restart", GOLD),
        (Pending::Restart, false) => ("Off after restart", GOLD),
        (_, true) => ("On", GREEN),
        (_, false) => ("Off", GREY),
    };
    (vec![toggle, Act::Remove], word, colour)
}

// ── requests to Lumos ──

/// `lumos-requests.json`: `{"install": [ids], "remove": [ids], "restart": bool}`. Lumos reads it
/// when the game it started closes, applies it and deletes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Requests {
    pub install: Vec<String>,
    pub remove: Vec<String>,
    pub restart: bool,
}

impl Requests {
    /// Reads the file's text; anything that isn't a valid id is left out.
    pub fn parse(text: &str) -> Requests {
        Requests {
            install: id_list(text, "install"),
            remove: id_list(text, "remove"),
            restart: bool_value(text, "restart"),
        }
    }

    pub fn to_json(&self) -> String {
        let list = |v: &[String]| v.iter().map(|i| format!("\"{i}\"")).collect::<Vec<_>>().join(", ");
        format!(
            "{{\n  \"install\": [{}],\n  \"remove\": [{}],\n  \"restart\": {}\n}}\n",
            list(&self.install),
            list(&self.remove),
            self.restart
        )
    }

    /// Nothing to install or remove.
    pub fn is_empty(&self) -> bool {
        self.install.is_empty() && self.remove.is_empty()
    }

    /// Asks for `id`; an earlier removal of it is taken back instead.
    pub fn add(&mut self, id: &str) {
        if !valid_id(id) {
            return;
        }
        if let Some(i) = self.remove.iter().position(|r| r == id) {
            self.remove.remove(i);
            return;
        }
        if !self.install.iter().any(|r| r == id) {
            self.install.push(id.to_owned());
        }
    }

    /// Asks Lumos to take `id` out; an earlier install of it is taken back instead.
    pub fn take_out(&mut self, id: &str) {
        if !valid_id(id) {
            return;
        }
        if let Some(i) = self.install.iter().position(|r| r == id) {
            self.install.remove(i);
            return;
        }
        if !self.remove.iter().any(|r| r == id) {
            self.remove.push(id.to_owned());
        }
    }

    /// Forgets any change asked for `id`.
    pub fn undo(&mut self, id: &str) {
        self.install.retain(|r| r != id);
        self.remove.retain(|r| r != id);
    }

    pub fn pending(&self, id: &str) -> Pending {
        if self.install.iter().any(|r| r == id) {
            Pending::Install
        } else if self.remove.iter().any(|r| r == id) {
            Pending::Remove
        } else {
            Pending::None
        }
    }
}

/// The value of `"key": [ ... ]` in a flat JSON object: its valid string ids.
fn id_list(text: &str, key: &str) -> Vec<String> {
    let Some(rest) = after_key(text, key) else { return Vec::new() };
    let Some(rest) = rest.strip_prefix('[') else { return Vec::new() };
    let Some(end) = rest.find(']') else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for item in rest[..end].split(',') {
        let item = item.trim();
        if let Some(id) = item.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            if valid_id(id) && !out.iter().any(|o| o == id) {
                out.push(id.to_owned());
            }
        }
    }
    out
}

fn bool_value(text: &str, key: &str) -> bool {
    after_key(text, key).is_some_and(|rest| rest.starts_with("true"))
}

/// What follows `"key":`, its spaces skipped.
fn after_key<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let at = text.find(&format!("\"{key}\""))?;
    let rest = text[at + key.len() + 2..].trim_start();
    Some(rest.strip_prefix(':')?.trim_start())
}

/// The `"mods"` list of `lumos-native.json`: the native mods this instance has.
pub fn native_mods(text: &str) -> Vec<String> {
    id_list(text, "mods")
}

// ── settings ──

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The pause menu's Discover button.
    pub pause_button: bool,
    /// Native mods with no Lumos build for this computer are listed (greyed).
    pub unbuilt: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { pause_button: true, unbuilt: true }
    }
}

pub fn settings(read: impl Fn(&str) -> Option<String>) -> Settings {
    let flag = |key: &str, default: bool| read(key).map_or(default, |v| v.trim() == "true");
    let d = Settings::default();
    Settings { pause_button: flag("pause_button", d.pause_button), unbuilt: flag("unbuilt", d.unbuilt) }
}

impl Settings {
    pub fn labels(&self) -> Vec<String> {
        vec![
            format!("Pause menu button: {}", if self.pause_button { "Shown" } else { "Hidden" }),
            format!("Native mods not built yet: {}", if self.unbuilt { "Listed" } else { "Hidden" }),
        ]
    }

    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.pause_button = !self.pause_button,
            1 => self.unbuilt = !self.unbuilt,
            _ => {}
        }
    }

    pub fn stored(&self) -> [(&'static str, String); 2] {
        [("pause_button", self.pause_button.to_string()), ("unbuilt", self.unbuilt.to_string())]
    }
}

// ── the screen ──

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Fill { rect: [f32; 4], argb: u32 },
    Text { text: String, at: [f32; 2], argb: u32, shadow: bool },
    /// A vanilla button, its label centred.
    Button { rect: [f32; 4], label: String, active: bool, hover: bool },
}

/// What a click at a place does.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Tab(Tab),
    Category(Option<String>),
    /// Entry `usize` (an index into the catalogue), and the button's act.
    Row(usize, Act),
    Search,
    Restart,
    Retry,
    Done,
}

/// Where the screen's parts are, in GUI pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub panel: [f32; 4],
    pub search: [f32; 4],
    pub cats: [f32; 4],
    pub list: [f32; 4],
    pub footer: [f32; 4],
}

pub const ROW_H: f32 = 25.0;
pub const CAT_H: f32 = 12.0;
pub const TAB_W: f32 = 78.0;

pub fn layout(sw: f32, sh: f32) -> Layout {
    let w = (sw - 16.0).clamp(200.0, 470.0);
    let x = ((sw - w) / 2.0).floor();
    let (top, bottom) = (6.0, (sh - 6.0).max(120.0));
    let search = [x + 6.0, top + 28.0, w - 12.0, 14.0];
    let body = top + 48.0;
    let footer = [x + 6.0, bottom - 24.0, w - 12.0, 20.0];
    // The categories column goes on narrow windows.
    let cats_w = if w >= 330.0 { 88.0 } else { 0.0 };
    let body_h = (footer[1] - 4.0 - body).max(ROW_H);
    Layout {
        panel: [x, top, w, bottom - top],
        search,
        cats: [x + 6.0, body, cats_w, body_h],
        list: [x + 6.0 + cats_w + if cats_w > 0.0 { 6.0 } else { 0.0 }, body, w - 12.0 - cats_w - if cats_w > 0.0 { 6.0 } else { 0.0 }, body_h],
        footer,
    }
}

pub fn inside(r: [f32; 4], p: [f32; 2]) -> bool {
    p[0] >= r[0] && p[0] < r[0] + r[2] && p[1] >= r[1] && p[1] < r[1] + r[3]
}

/// `text` cut with "..." to fit `max` pixels.
pub fn fit(text: &str, max: f32, width: &dyn Fn(&str) -> f32) -> String {
    if width(text) <= max {
        return text.to_owned();
    }
    let mut s: String = text.to_owned();
    while !s.is_empty() && width(&format!("{s}...")) > max {
        s.pop();
    }
    format!("{}...", s.trim_end())
}

/// The screen's state between frames.
#[derive(Debug)]
pub struct Ui {
    pub tab: Tab,
    pub query: String,
    pub category: Option<String>,
    /// The first row shown.
    pub scroll: usize,
    pub cat_scroll: usize,
    /// The search box has the keyboard.
    pub typing: bool,
    pub loading: bool,
    pub error: Option<String>,
    /// The last thing that happened: text, and whether it went well.
    pub message: Option<(String, bool)>,
    /// The entry a download or change runs for.
    pub busy: Option<String>,
    /// Something waits for the game to start again.
    pub restart: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            tab: Tab::Mods,
            query: String::new(),
            category: None,
            scroll: 0,
            cat_scroll: 0,
            typing: true,
            loading: false,
            error: None,
            message: None,
            busy: None,
            restart: false,
        }
    }
}

impl Ui {
    pub fn set_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.category = None;
            self.scroll = 0;
            self.cat_scroll = 0;
        }
    }

    pub fn type_char(&mut self, c: char) {
        if !c.is_control() && self.query.chars().count() < 40 {
            self.query.push(c);
            self.scroll = 0;
        }
    }

    pub fn backspace(&mut self) {
        if self.query.pop().is_some() {
            self.scroll = 0;
        }
    }

    /// The wheel over the list (`up`: towards the top), `rows` in it and `fits` shown.
    pub fn wheel(&mut self, up: bool, rows: usize, fits: usize) {
        let max = rows.saturating_sub(fits);
        self.scroll = if up { self.scroll.saturating_sub(2) } else { (self.scroll + 2).min(max) };
    }
}

/// What `frame` needs from the catalogue.
pub struct View<'a> {
    pub entries: &'a [Entry],
    pub have: &'a dyn Fn(&str) -> Have,
    pub width: &'a dyn Fn(&str) -> f32,
    pub settings: Settings,
}

/// The rows shown under the current tab, search and category.
pub fn rows(ui: &Ui, v: &View<'_>) -> Vec<usize> {
    visible(v.entries, ui.tab, &ui.query, ui.category.as_deref())
        .into_iter()
        .filter(|&i| {
            let e = &v.entries[i];
            v.settings.unbuilt || e.kind != Kind::Native || e.cloud || (v.have)(&e.id).built_in || (v.have)(&e.id).installed
        })
        .collect()
}

pub fn rows_fit(l: &Layout) -> usize {
    ((l.list[3] / ROW_H).floor() as usize).max(1)
}

fn push_text(out: &mut Vec<Prim>, text: impl Into<String>, at: [f32; 2], argb: u32) {
    out.push(Prim::Text { text: text.into(), at, argb, shadow: true });
}

fn push_button(out: &mut Vec<Prim>, hits: &mut Vec<([f32; 4], Hit)>, rect: [f32; 4], label: &str, hit: Hit, mouse: [f32; 2]) {
    out.push(Prim::Button { rect, label: label.into(), active: true, hover: inside(rect, mouse) });
    hits.push((rect, hit));
}

/// The whole screen: what to draw, and where clicks land.
pub fn frame(ui: &Ui, v: &View<'_>, mouse: [f32; 2], sw: f32, sh: f32) -> (Vec<Prim>, Vec<([f32; 4], Hit)>) {
    let l = layout(sw, sh);
    let w = v.width;
    let mut out = Vec::new();
    let mut hits = Vec::new();
    let [px, py, pw, _] = l.panel;
    out.push(Prim::Fill { rect: l.panel, argb: PANEL });
    out.push(Prim::Fill { rect: [px, py, pw, 1.0], argb: ACCENT });
    push_text(&mut out, "Discover", [px + 7.0, py + 8.0], WHITE);

    // Tabs, right-aligned in the title row.
    let tab_w = TAB_W.min(((pw - 80.0) / 3.0).floor());
    for (k, (tab, label)) in TABS.iter().enumerate() {
        let rect = [px + pw - 6.0 - (3 - k) as f32 * (tab_w + 2.0), py + 4.0, tab_w, 20.0];
        out.push(Prim::Button { rect, label: (*label).into(), active: ui.tab != *tab, hover: ui.tab != *tab && inside(rect, mouse) });
        if ui.tab == *tab {
            out.push(Prim::Fill { rect: [rect[0] + 2.0, rect[1] + 18.0, rect[2] - 4.0, 1.0], argb: ACCENT });
        } else {
            hits.push((rect, Hit::Tab(*tab)));
        }
    }

    // The search box.
    let s = l.search;
    out.push(Prim::Fill { rect: [s[0] - 1.0, s[1] - 1.0, s[2] + 2.0, s[3] + 2.0], argb: if ui.typing { WHITE } else { FIELD_EDGE } });
    out.push(Prim::Fill { rect: s, argb: FIELD });
    hits.push((s, Hit::Search));
    let shown = if ui.query.is_empty() {
        ("Search...".to_owned(), DARK_GREY)
    } else {
        let mut q = ui.query.clone();
        // The end of a long search stays in view.
        while !q.is_empty() && w(&q) > s[2] - 12.0 {
            q.remove(0);
        }
        (q, WHITE)
    };
    push_text(&mut out, &shown.0, [s[0] + 4.0, s[1] + 3.0], shown.1);
    if ui.typing {
        let x = s[0] + 4.0 + if ui.query.is_empty() { 0.0 } else { w(&shown.0) + 1.0 };
        out.push(Prim::Fill { rect: [x, s[1] + 2.0, 1.0, 10.0], argb: WHITE });
    }

    // Categories.
    if l.cats[2] > 0.0 {
        let c = l.cats;
        let names = categories(v.entries, ui.tab);
        let fit_n = ((c[3] / CAT_H).floor() as usize).max(1);
        let all = std::iter::once(None).chain(names.into_iter().map(Some)).collect::<Vec<_>>();
        for (k, cat) in all.iter().skip(ui.cat_scroll).take(fit_n).enumerate() {
            let rect = [c[0], c[1] + k as f32 * CAT_H, c[2], CAT_H];
            let chosen = ui.category.as_deref() == cat.as_deref();
            if chosen {
                out.push(Prim::Fill { rect, argb: 0x40FF_6B2C });
            } else if inside(rect, mouse) {
                out.push(Prim::Fill { rect, argb: ROW_HOVER });
            }
            let label = cat.as_deref().map_or("All".to_owned(), pretty);
            push_text(&mut out, fit(&label, c[2] - 6.0, w), [c[0] + 3.0, rect[1] + 2.0], if chosen { WHITE } else { GREY });
            hits.push((rect, Hit::Category(cat.clone())));
        }
    }

    // The list.
    let lr = l.list;
    if ui.loading && v.entries.is_empty() {
        push_text(&mut out, "Reading Horizon's registry...", [lr[0] + 4.0, lr[1] + 4.0], GREY);
    } else if let Some(err) = ui.error.as_ref().filter(|_| v.entries.is_empty()) {
        push_text(&mut out, fit(&format!("The registry couldn't be read: {err}"), lr[2] - 8.0, w), [lr[0] + 4.0, lr[1] + 4.0], RED);
        push_button(&mut out, &mut hits, [lr[0] + 4.0, lr[1] + 18.0, 60.0, 20.0], "Retry", Hit::Retry, mouse);
    } else {
        let list = rows(ui, v);
        if list.is_empty() {
            push_text(&mut out, "Nothing here matches.", [lr[0] + 4.0, lr[1] + 4.0], GREY);
        }
        let fits = rows_fit(&l);
        for (k, &i) in list.iter().skip(ui.scroll).take(fits).enumerate() {
            let e = &v.entries[i];
            let h = (v.have)(&e.id);
            let y = lr[1] + k as f32 * ROW_H;
            let rect = [lr[0], y, lr[2], ROW_H - 1.0];
            if inside(rect, mouse) {
                out.push(Prim::Fill { rect, argb: ROW_HOVER });
            }
            let (acts, word, colour) = offer(e, &h, ui.busy.as_deref() == Some(e.id.as_str()));
            let bw = 46.0;
            let buttons_w = acts.len() as f32 * (bw + 2.0);
            let word_w = if word.is_empty() { 0.0 } else { w(word) + 6.0 };
            let text_w = (lr[2] - 6.0 - buttons_w - word_w).max(40.0);
            let mut title = e.name.clone();
            if !e.version.is_empty() {
                title = format!("{title} {}", e.version);
            }
            push_text(&mut out, fit(&title, text_w, w), [lr[0] + 3.0, y + 3.0], WHITE);
            let mut second = e.summary.clone();
            if e.kind == Kind::Native && !e.own {
                second = format!("Native. {second}");
            }
            push_text(&mut out, fit(&second, text_w, w), [lr[0] + 3.0, y + 13.0], GREY);
            let mut bx = lr[0] + lr[2] - buttons_w;
            if !word.is_empty() {
                push_text(&mut out, word, [bx - word_w + 2.0, y + 8.0], colour);
            }
            for act in acts {
                let label = if act == Act::Remove && h.pending == Pending::Install { "Cancel" } else { act.label() };
                push_button(&mut out, &mut hits, [bx, y + 2.0, bw, 20.0], label, Hit::Row(i, act), mouse);
                bx += bw + 2.0;
            }
        }
        // Where the list is when it is longer than the box.
        if list.len() > fits {
            let track = [lr[0] + lr[2] + 1.0, lr[1], 2.0, lr[3]];
            let thumb_h = (lr[3] * fits as f32 / list.len() as f32).max(8.0);
            let at = (lr[3] - thumb_h) * ui.scroll as f32 / (list.len() - fits) as f32;
            out.push(Prim::Fill { rect: track, argb: 0x40FF_FFFF });
            out.push(Prim::Fill { rect: [track[0], lr[1] + at, 2.0, thumb_h], argb: 0xC0FF_FFFF });
        }
    }

    // The footer: what happened, Restart when something waits for it, Done.
    let f = l.footer;
    let done = [f[0] + f[2] - 50.0, f[1], 50.0, 20.0];
    push_button(&mut out, &mut hits, done, "Done", Hit::Done, mouse);
    let mut text_right = done[0] - 4.0;
    if ui.restart {
        let r = [done[0] - 104.0, f[1], 100.0, 20.0];
        push_button(&mut out, &mut hits, r, "Restart to apply", Hit::Restart, mouse);
        text_right = r[0] - 4.0;
    }
    if let Some((msg, ok)) = &ui.message {
        push_text(&mut out, fit(msg, text_right - f[0], w), [f[0], f[1] + 6.0], if *ok { GREEN } else { RED });
    }
    (out, hits)
}

/// Where a click at `mouse` lands.
pub fn hit(hits: &[([f32; 4], Hit)], mouse: [f32; 2]) -> Option<Hit> {
    hits.iter().rev().find(|(r, _)| inside(*r, mouse)).map(|(_, h)| h.clone())
}

/// The pause menu's Discover button: top left, clear of vanilla's centred grid and icon row.
pub fn pause_button() -> [f32; 4] {
    [6.0, 6.0, 80.0, 20.0]
}

// ── state ──

#[derive(Debug, Default)]
pub struct State {
    pub ui: Ui,
    pub entries: Vec<Entry>,
    /// What this instance has of each entry, by id; refreshed by the glue.
    pub have: BTreeMap<String, Have>,
    /// Clicks and keys from the screen hooks (which only read the game), for the next tick.
    pub queue: Vec<Hit>,
    /// The hotkey or the pause button asked for the screen.
    pub open: bool,
    /// Ticks since `have` was read.
    pub stale: u32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, kind: Kind, downloads: u64, cats: &[&str]) -> Entry {
        Entry {
            id: id.into(),
            name: id.replace('_', " "),
            summary: format!("{id} summary"),
            author: "someone".into(),
            kind,
            categories: cats.iter().map(|c| (*c).to_owned()).collect(),
            tags: vec!["shiny".into()],
            downloads,
            version: "1.0".into(),
            file: format!("{id}.zip"),
            own: false,
            cloud: false,
        }
    }

    fn width(t: &str) -> f32 {
        t.chars().count() as f32 * 6.0
    }

    #[test]
    fn ids_follow_horizons_rule() {
        assert!(valid_id("lumos_hud") && valid_id("a1"));
        for bad in ["", "../x", "A", "a-b", "a b", "a/b", "a.json", &"x".repeat(65)] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn search_tabs_and_categories() {
        let es = vec![
            entry("zoom", Kind::Native, 50, &["utility"]),
            entry("ruby", Kind::Content, 10, &["equipment"]),
            entry("aurora", Kind::Shader, 3, &["atmosphere"]),
            entry("faithful", Kind::ResourcePack, 9, &["16x"]),
            entry("lanterns", Kind::Content, 30, &["decoration", "utility"]),
        ];
        // Content first, most downloaded first, then native.
        assert_eq!(visible(&es, Tab::Mods, "", None), vec![4, 1, 0]);
        assert_eq!(visible(&es, Tab::Mods, "", Some("Utility")), vec![4, 0]);
        assert_eq!(visible(&es, Tab::Mods, "ZOOM", None), vec![0]);
        assert_eq!(visible(&es, Tab::Mods, "shiny lanterns", None), vec![4]);
        assert_eq!(visible(&es, Tab::Mods, "shiny nothing", None), Vec::<usize>::new());
        assert_eq!(visible(&es, Tab::Shaders, "", None), vec![2]);
        assert_eq!(categories(&es, Tab::Mods), vec!["decoration", "equipment", "utility"]);
        assert_eq!(pretty("game-mechanics"), "Game Mechanics");
        assert_eq!(pretty("16x"), "16x");
    }

    #[test]
    fn own_mods_list_before_other_natives() {
        let mut es = own_entries(&|id| id == "lumos_hud");
        es.push(entry("zoom", Kind::Native, 999, &[]));
        let order = visible(&es, Tab::Mods, "", None);
        assert_eq!(es[*order.last().unwrap()].id, "zoom");
        assert!(es.iter().any(|e| e.id == ID), "this mod lists itself");
        assert!(es.iter().find(|e| e.id == "lumos_hud").unwrap().cloud);
    }

    #[test]
    fn requests_round_trip_and_cancel() {
        let mut r = Requests::default();
        r.add("zoom");
        r.add("zoom");
        r.take_out("minimap");
        r.restart = true;
        let text = r.to_json();
        assert_eq!(Requests::parse(&text), r);
        assert_eq!(r.pending("zoom"), Pending::Install);
        assert_eq!(r.pending("minimap"), Pending::Remove);
        // Removing what waits to be installed takes the install back, and the other way round.
        r.take_out("zoom");
        r.add("minimap");
        assert!(r.is_empty());
        // Paths and odd names never get in.
        r.add("../../etc");
        let parsed = Requests::parse(r#"{"install": ["ok_id", "../x", "a/b", 7], "remove": [], "restart": false}"#);
        assert_eq!(parsed.install, vec!["ok_id"]);
        assert!(!parsed.restart && r.is_empty());
        assert_eq!(Requests::parse("garbage"), Requests::default());
    }

    #[test]
    fn native_file_lists_the_instance_mods() {
        let text = r#"{"build": "all-9f42064c", "mods": ["zoom", "lumos_hud"], "known": [["zoom", "Zoom", true]]}"#;
        assert_eq!(native_mods(text), vec!["zoom", "lumos_hud"]);
        assert!(native_mods("{}").is_empty());
    }

    #[test]
    fn rows_offer_what_applies() {
        let pack = entry("faithful", Kind::ResourcePack, 1, &[]);
        assert_eq!(offer(&pack, &Have::default(), false).0, vec![Act::Install]);
        let on = Have { installed: true, on: true, ..Have::default() };
        assert_eq!(offer(&pack, &on, false).0, vec![Act::Off, Act::Remove]);
        assert!(offer(&pack, &on, true).0.is_empty());
        let content = entry("ruby", Kind::Content, 1, &[]);
        let waits = Have { installed: true, on: true, pending: Pending::Restart, ..Have::default() };
        assert_eq!(offer(&content, &waits, false).1, "On after restart");

        let mut native = entry("zoom", Kind::Native, 1, &[]);
        assert!(offer(&native, &Have::default(), false).0.is_empty(), "no build, no install");
        native.cloud = true;
        assert_eq!(offer(&native, &Have::default(), false).0, vec![Act::Install]);
        let asked = Have { pending: Pending::Install, ..Have::default() };
        assert_eq!(offer(&native, &asked, false), (vec![Act::Undo], "Restart to apply", GOLD));
        let built = Have { installed: true, on: true, built_in: true, ..Have::default() };
        assert_eq!(offer(&native, &built, false).0, vec![Act::Off, Act::Remove]);
        let me = Entry { id: ID.into(), ..native };
        assert!(offer(&me, &built, false).0.is_empty(), "never offers to remove itself");
    }

    #[test]
    fn settings_round_trip() {
        let mut s = settings(|_| None);
        assert_eq!(s, Settings::default());
        s.click(0);
        let stored = s.stored();
        let back = settings(|k| stored.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone()));
        assert_eq!(back, s);
        assert!(!back.pause_button);
        assert_eq!(s.labels().len(), 2);
    }

    #[test]
    fn frame_hits_match_what_is_drawn() {
        let mut es = vec![entry("faithful", Kind::ResourcePack, 9, &["16x"])];
        es.extend(own_entries(&|_| true));
        let have = |id: &str| if id == "faithful" { Have { installed: true, ..Have::default() } } else { Have::default() };
        let v = View { entries: &es, have: &have, width: &width, settings: Settings::default() };
        let mut ui = Ui::default();
        ui.set_tab(Tab::ResourcePacks);
        ui.restart = true;
        let (prims, hits) = frame(&ui, &v, [0.0, 0.0], 480.0, 270.0);
        assert!(prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text.starts_with("faithful"))));
        // Each button drawn has a place a click lands, and the other way round for buttons.
        let buttons: Vec<[f32; 4]> = prims.iter().filter_map(|p| match p { Prim::Button { rect, active: true, .. } => Some(*rect), _ => None }).collect();
        for b in &buttons {
            let centre = [b[0] + b[2] / 2.0, b[1] + b[3] / 2.0];
            assert!(hit(&hits, centre).is_some(), "{b:?}");
        }
        let turn_on = hits.iter().find(|(_, h)| *h == Hit::Row(0, Act::On)).expect("the pack's switch");
        assert_eq!(hit(&hits, [turn_on.0[0] + 1.0, turn_on.0[1] + 1.0]), Some(Hit::Row(0, Act::On)));
        assert!(hits.iter().any(|(_, h)| *h == Hit::Restart));
        assert!(hits.iter().any(|(_, h)| *h == Hit::Tab(Tab::Mods)));
        assert!(!hits.iter().any(|(_, h)| *h == Hit::Tab(Tab::ResourcePacks)), "the open tab isn't a button");
        // Everything stays inside the window, even a small one.
        for (sw, sh) in [(320.0, 240.0), (480.0, 270.0), (960.0, 540.0)] {
            let (prims, _) = frame(&ui, &v, [0.0, 0.0], sw, sh);
            for p in prims {
                if let Prim::Fill { rect, .. } | Prim::Button { rect, .. } = p {
                    assert!(rect[0] >= 0.0 && rect[1] >= 0.0 && rect[0] + rect[2] <= sw + 0.5 && rect[1] + rect[3] <= sh + 0.5, "{rect:?} in {sw}x{sh}");
                }
            }
        }
    }

    #[test]
    fn typing_and_scrolling() {
        let mut ui = Ui::default();
        for c in "zoom\u{8}".chars() {
            ui.type_char(c);
        }
        assert_eq!(ui.query, "zoom");
        ui.backspace();
        assert_eq!(ui.query, "zoo");
        ui.wheel(false, 30, 8);
        ui.wheel(false, 30, 8);
        assert_eq!(ui.scroll, 4);
        for _ in 0..20 {
            ui.wheel(false, 30, 8);
        }
        assert_eq!(ui.scroll, 22);
        ui.type_char('x');
        assert_eq!(ui.scroll, 0, "a new search starts at the top");
        ui.set_tab(Tab::Shaders);
        assert_eq!((ui.tab, ui.category.clone()), (Tab::Shaders, None));
    }

    #[test]
    fn fit_cuts_long_text() {
        assert_eq!(fit("short", 100.0, &width), "short");
        let cut = fit("a very long summary indeed", 60.0, &width);
        assert!(cut.ends_with("...") && width(&cut) <= 60.0, "{cut}");
    }

    #[test]
    fn keys_that_type_text() {
        for k in ["key.keyboard.d", "key.keyboard.7", "key.keyboard.space", "key.keyboard.keypad.3", "key.keyboard.minus", "key.keyboard.keypad.add"] {
            assert!(types_text(k), "{k}");
        }
        for k in ["key.keyboard.right.control", "key.keyboard.f6", "key.keyboard.escape", "key.keyboard.keypad.enter", "key.mouse.left", "key.keyboard.unknown"] {
            assert!(!types_text(k), "{k}");
        }
    }
}
