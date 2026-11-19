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
pub const VERSION: &str = "1.1.0";
pub const DESCRIPTION: &str = "Browse and install mods, resource packs and shaders from \
Horizon's registry without leaving the game. Right Control (or the pause menu's Discover \
button) opens it. Packs and shaders apply at once; native mods apply after a restart. The \
Shaders tab also lists Modrinth's Iris/OptiFine shader packs.";

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
    ("lumos_world_map", "World Map", "A fullscreen map of everywhere you explored, with cave layers, dimensions and waypoints shared with Lumos Map. M opens it.", &["utility", "adventure"]),
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
/// Modrinth's tag on its rows.
pub const MODRINTH: u32 = 0xFF1B_D96A;

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
    /// A Modrinth shader pack (not Horizon's registry): its slug. Its `id` is
    /// `modrinth:<project id>` and its `file` the pack's name in `shaderpacks/` once known.
    pub modrinth: Option<String>,
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
            // Modrinth's hits are already its answer to the search.
            e.kind.tab() == tab
                && (e.modrinth.is_some() || matches(e, query))
                && category.is_none_or(|c| e.categories.iter().any(|x| x.eq_ignore_ascii_case(c)))
        })
        .collect();
    out.sort_by(|&a, &b| {
        let (a, b) = (&entries[a], &entries[b]);
        let rank = |e: &Entry| match e.kind {
            _ if e.modrinth.is_some() => 3,
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
            modrinth: None,
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
        return (vec![Act::Install], if e.modrinth.is_some() { "Modrinth" } else { "" }, MODRINTH);
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
    /// The Shaders tab also lists Modrinth's shader packs (read from api.modrinth.com).
    pub modrinth: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { pause_button: true, unbuilt: true, modrinth: true }
    }
}

pub fn settings(read: impl Fn(&str) -> Option<String>) -> Settings {
    let flag = |key: &str, default: bool| read(key).map_or(default, |v| v.trim() == "true");
    let d = Settings::default();
    Settings {
        pause_button: flag("pause_button", d.pause_button),
        unbuilt: flag("unbuilt", d.unbuilt),
        modrinth: flag("modrinth", d.modrinth),
    }
}

impl Settings {
    pub fn labels(&self) -> Vec<String> {
        vec![
            format!("Pause menu button: {}", if self.pause_button { "Shown" } else { "Hidden" }),
            format!("Native mods not built yet: {}", if self.unbuilt { "Listed" } else { "Hidden" }),
            format!("Modrinth shaders: {}", if self.modrinth { "On" } else { "Off" }),
        ]
    }

    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.pause_button = !self.pause_button,
            1 => self.unbuilt = !self.unbuilt,
            2 => self.modrinth = !self.modrinth,
            _ => {}
        }
    }

    pub fn stored(&self) -> [(&'static str, String); 3] {
        [
            ("pause_button", self.pause_button.to_string()),
            ("unbuilt", self.unbuilt.to_string()),
            ("modrinth", self.modrinth.to_string()),
        ]
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
    /// A line after the rows (Modrinth's state on the Shaders tab), with its colour.
    pub tail: Option<(String, u32)>,
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

/// The rows, and the tail line when there is one.
pub fn list_len(ui: &Ui, v: &View<'_>) -> usize {
    rows(ui, v).len() + usize::from(v.tail.is_some())
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
        if list.is_empty() && v.tail.is_none() {
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
            if e.modrinth.is_some() && !e.author.is_empty() {
                title = format!("{title} by {}", e.author);
            }
            push_text(&mut out, fit(&title, text_w, w), [lr[0] + 3.0, y + 3.0], WHITE);
            let mut second = e.summary.clone();
            if e.kind == Kind::Native && !e.own {
                second = format!("Native. {second}");
            }
            if e.modrinth.is_some() {
                // Pointed at, the row says what a Modrinth pack is.
                second = if inside(rect, mouse) { MODRINTH_NOTE.to_owned() } else { format!("{} downloads - {second}", short_count(e.downloads)) };
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
        // The tail line, as the row after the last.
        if let Some((text, colour)) = &v.tail
            && let Some(k) = list.len().checked_sub(ui.scroll).filter(|k| *k < fits)
        {
            push_text(&mut out, fit(text, lr[2] - 8.0, w), [lr[0] + 3.0, lr[1] + k as f32 * ROW_H + 8.0], *colour);
        }
        // Where the list is when it is longer than the box.
        let total = list.len() + usize::from(v.tail.is_some());
        if total > fits {
            let track = [lr[0] + lr[2] + 1.0, lr[1], 2.0, lr[3]];
            let thumb_h = (lr[3] * fits as f32 / total as f32).max(8.0);
            let at = (lr[3] - thumb_h) * ui.scroll.min(total - fits) as f32 / (total - fits) as f32;
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

// ── Modrinth's shader packs ──
//
// The Shaders tab also lists Modrinth's shader packs (OptiFine/Iris `shaders/` zips, which
// Horizon loads). Read from Modrinth's public API with no key: a search page at a time as the
// list is scrolled or searched, kept for the session (`Feed`); a pack's versions when it is
// installed. Its file is taken only from `cdn.modrinth.com` over HTTPS and only when its size
// and SHA-512 are Modrinth's, under a plain name that stays in `shaderpacks/`.

pub const MODRINTH_API: &str = "https://api.modrinth.com/v2";
/// The one place a pack's file is downloaded from.
pub const MODRINTH_CDN: &str = "https://cdn.modrinth.com/";
/// A generic client name: nothing about the player goes to Modrinth.
pub const MODRINTH_AGENT: &str = "LumosClient/1.1.0 (github.com/AashrithKrishna/lumosclient)";
/// What a Modrinth row says when pointed at, and what an install of one says.
pub const MODRINTH_NOTE: &str = "Iris/OptiFine pack: may not look exactly as in Java.";
/// The line after Modrinth's last row.
pub const MODRINTH_END: &str = "Modrinth's are Iris/OptiFine packs: not always as in Java.";
/// Hits a search page asks for.
pub const MODRINTH_PAGE: usize = 20;
/// The most hits kept for one search (25 pages).
pub const MODRINTH_MAX_HITS: usize = 500;
/// The largest pack file taken.
pub const MODRINTH_MAX_FILE: u64 = 256 << 20;
/// The prefix of a Modrinth entry's id.
pub const MODRINTH_PREFIX: &str = "modrinth:";

/// A JSON value, read by `Json::parse` (std only, so the tests run alone).
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

static JSON_NULL: Json = Json::Null;

impl Json {
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = JsonReader { b: text.as_bytes(), i: 0 };
        let v = p.value(0)?;
        p.space();
        if p.i != p.b.len() {
            return Err(format!("unexpected text at {}", p.i));
        }
        Ok(v)
    }

    /// Member `key` of an object (Null when absent or not an object).
    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(m) => m.iter().find(|(k, _)| k == key).map_or(&JSON_NULL, |(_, v)| v),
            _ => &JSON_NULL,
        }
    }

    pub fn str(&self) -> Option<&str> {
        if let Json::Str(s) = self { Some(s) } else { None }
    }

    pub fn num(&self) -> Option<f64> {
        if let Json::Num(n) = self { Some(*n) } else { None }
    }

    /// A whole number of at least 0.
    pub fn count(&self) -> Option<u64> {
        self.num().filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n < 9.0e15).map(|n| n as u64)
    }

    pub fn arr(&self) -> &[Json] {
        if let Json::Arr(a) = self { a } else { &[] }
    }

    /// The strings of an array.
    pub fn strs(&self) -> Vec<String> {
        self.arr().iter().filter_map(|v| v.str().map(str::to_owned)).collect()
    }
}

struct JsonReader<'a> {
    b: &'a [u8],
    i: usize,
}

impl JsonReader<'_> {
    fn space(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, word: &str) -> bool {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > 64 {
            return Err("too deeply nested".into());
        }
        self.space();
        let Some(&c) = self.b.get(self.i) else { return Err("unexpected end".into()) };
        match c {
            b'{' => {
                self.i += 1;
                let mut members = Vec::new();
                self.space();
                if self.eat("}") {
                    return Ok(Json::Obj(members));
                }
                loop {
                    self.space();
                    if self.b.get(self.i) != Some(&b'"') {
                        return Err(format!("expected a key at {}", self.i));
                    }
                    let key = self.string()?;
                    self.space();
                    if !self.eat(":") {
                        return Err(format!("expected ':' at {}", self.i));
                    }
                    let v = self.value(depth + 1)?;
                    members.push((key, v));
                    self.space();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("}") {
                        return Ok(Json::Obj(members));
                    }
                    return Err(format!("expected ',' or '}}' at {}", self.i));
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.space();
                if self.eat("]") {
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.space();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("]") {
                        return Ok(Json::Arr(items));
                    }
                    return Err(format!("expected ',' or ']' at {}", self.i));
                }
            }
            b'"' => self.string().map(Json::Str),
            b't' if self.eat("true") => Ok(Json::Bool(true)),
            b'f' if self.eat("false") => Ok(Json::Bool(false)),
            b'n' if self.eat("null") => Ok(Json::Null),
            b'-' | b'0'..=b'9' => {
                let start = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                let s = std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?;
                s.parse::<f64>().map(Json::Num).map_err(|_| format!("bad number {s}"))
            }
            _ => Err(format!("unexpected '{}' at {}", c as char, self.i)),
        }
    }

    /// A string, the reader on its opening quote.
    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&c) = self.b.get(self.i) else { return Err("unterminated string".into()) };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(&e) = self.b.get(self.i) else { return Err("unterminated string".into()) };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                if !self.eat("\\u") {
                                    return Err("lone surrogate".into());
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err("bad surrogate".into());
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(code).unwrap_or('\u{FFFD}')
                        }
                        _ => return Err(format!("bad escape at {}", self.i)),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c if c < 0x20 => return Err("control character in a string".into()),
                c => out.push(c),
            }
        }
        // Split only at ASCII bytes of valid UTF-8, so this holds.
        String::from_utf8(out).map_err(|e| e.to_string())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let s = self.b.get(self.i..self.i + 4).ok_or("short \\u escape")?;
        let s = std::str::from_utf8(s).map_err(|e| e.to_string())?;
        let v = u32::from_str_radix(s, 16).map_err(|_| format!("bad \\u escape {s}"))?;
        self.i += 4;
        Ok(v)
    }
}

/// Percent-encodes `s` for a URL's query (everything but `A-Za-z0-9-_.~`).
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A search key: the search's words, lower case, single spaces.
pub fn feed_key(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Modrinth's shader packs for search `key`, the most downloaded first, from hit `offset`.
/// Only packs for Iris or OptiFine (the format Horizon loads), not Canvas or core shaders.
pub fn search_url(key: &str, offset: usize) -> String {
    let facets = r#"[["project_type:shader"],["categories:iris","categories:optifine"]]"#;
    let mut url = format!("{MODRINTH_API}/search?facets={}&index=downloads&offset={offset}&limit={MODRINTH_PAGE}", url_encode(facets));
    if !key.is_empty() {
        url.push_str(&format!("&query={}", url_encode(key)));
    }
    url
}

/// A Modrinth project id: base62, as its ids are.
pub fn valid_project_id(id: &str) -> bool {
    (1..=24).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The project's versions, or None for an id that isn't one.
pub fn versions_url(project_id: &str) -> Option<String> {
    valid_project_id(project_id).then(|| format!("{MODRINTH_API}/project/{project_id}/version"))
}

/// Text from Modrinth for a row: no control characters, at most `max` characters.
pub fn clean(text: &str, max: usize) -> String {
    let one_line: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let words = one_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() <= max { words } else { format!("{}...", words.chars().take(max).collect::<String>().trim_end()) }
}

/// Loader names among a pack's categories: they say what runs it, not what it is.
const LOADERS: [&str; 4] = ["iris", "optifine", "canvas", "vanilla"];

/// A search page: its hits as Shaders-tab entries, and how many hits the search has.
pub fn parse_search(text: &str) -> Result<(Vec<Entry>, usize), String> {
    let v = Json::parse(text).map_err(|e| format!("Modrinth's answer wasn't read ({e})"))?;
    let Json::Arr(hits) = v.get("hits") else { return Err("Modrinth's answer has no hits".into()) };
    let total = v.get("total_hits").count().unwrap_or(hits.len() as u64) as usize;
    let mut out: Vec<Entry> = Vec::new();
    for h in hits {
        let Some(pid) = h.get("project_id").str().filter(|p| valid_project_id(p)) else { continue };
        let id = format!("{MODRINTH_PREFIX}{pid}");
        if out.iter().any(|e| e.id == id) {
            continue;
        }
        let title = clean(h.get("title").str().unwrap_or(""), 64);
        let slug = clean(h.get("slug").str().unwrap_or(""), 64);
        let shown = if h.get("display_categories").arr().is_empty() { h.get("categories") } else { h.get("display_categories") };
        out.push(Entry {
            id,
            name: if title.is_empty() { slug.clone() } else { title },
            summary: clean(h.get("description").str().unwrap_or(""), 200),
            author: clean(h.get("author").str().unwrap_or(""), 40),
            kind: Kind::Shader,
            categories: shown.strs().into_iter().filter(|c| !LOADERS.contains(&c.as_str())).map(|c| clean(&c, 32).to_lowercase()).collect(),
            tags: vec!["modrinth".into()],
            downloads: h.get("downloads").count().unwrap_or(0),
            version: String::new(),
            file: String::new(),
            own: false,
            cloud: false,
            modrinth: Some(slug),
        });
    }
    Ok((out, total))
}

/// A file of a Modrinth version.
#[derive(Clone, Debug, PartialEq)]
pub struct MrFile {
    pub url: String,
    pub name: String,
    pub size: u64,
    /// Lower-case hex.
    pub sha512: String,
    pub primary: bool,
}

/// A version of a Modrinth project.
#[derive(Clone, Debug, PartialEq)]
pub struct MrVersion {
    pub id: String,
    pub number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    /// ISO 8601, so it sorts as text.
    pub published: String,
    pub files: Vec<MrFile>,
}

/// `/project/{id}/version`: the project's versions (files with no size or SHA-512 left out).
pub fn parse_versions(text: &str) -> Result<Vec<MrVersion>, String> {
    let v = Json::parse(text).map_err(|e| format!("Modrinth's versions weren't read ({e})"))?;
    let Json::Arr(list) = v else { return Err("Modrinth's versions aren't a list".into()) };
    Ok(list
        .iter()
        .map(|x| MrVersion {
            id: x.get("id").str().unwrap_or("").to_owned(),
            number: clean(x.get("version_number").str().unwrap_or(""), 40),
            game_versions: x.get("game_versions").strs(),
            loaders: x.get("loaders").strs().into_iter().map(|l| l.to_lowercase()).collect(),
            published: x.get("date_published").str().unwrap_or("").to_owned(),
            files: x
                .get("files")
                .arr()
                .iter()
                .filter_map(|f| {
                    Some(MrFile {
                        url: f.get("url").str()?.to_owned(),
                        name: f.get("filename").str()?.to_owned(),
                        size: f.get("size").count()?,
                        sha512: f.get("hashes").get("sha512").str()?.to_ascii_lowercase(),
                        primary: f.get("primary") == &Json::Bool(true),
                    })
                })
                .collect(),
        })
        .collect())
}

/// `1.21.11` → its numbers; None for a snapshot or anything not all numbers.
fn game_numbers(v: &str) -> Option<[u64; 3]> {
    let parts: Vec<u64> = v.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| *p > 999) {
        return None;
    }
    let mut out = [0; 3];
    out[..parts.len()].copy_from_slice(&parts);
    Some(out)
}

/// How far a version's games are from `game`: 0 for `game` itself, then the nearest older game,
/// then the nearest newer one, then versions with no game this can read.
fn game_distance(v: &MrVersion, game: &str) -> (u8, u64) {
    let num = |n: [u64; 3]| n[0] * 1_000_000 + n[1] * 1_000 + n[2];
    let Some(at) = game_numbers(game).map(num) else { return (3, 0) };
    v.game_versions
        .iter()
        .filter_map(|g| game_numbers(g).map(num))
        .map(|g| if g == at { (0, 0) } else if g < at { (1, at - g) } else { (2, g - at) })
        .min()
        .unwrap_or((3, 0))
}

/// The version to install for `game`: of those for Iris or OptiFine, the one made for the game
/// closest to it, the newest of those; with none for Iris or OptiFine, the newest. Only versions
/// with a file count.
pub fn pick_version<'a>(versions: &'a [MrVersion], game: &str) -> Option<&'a MrVersion> {
    let usable: Vec<&MrVersion> = versions.iter().filter(|v| primary_file(v).is_some()).collect();
    let packs: Vec<&MrVersion> = usable.iter().copied().filter(|v| v.loaders.iter().any(|l| l == "iris" || l == "optifine")).collect();
    if packs.is_empty() {
        return usable.into_iter().max_by(|a, b| a.published.cmp(&b.published));
    }
    packs.into_iter().min_by(|a, b| game_distance(a, game).cmp(&game_distance(b, game)).then(b.published.cmp(&a.published)))
}

/// A version's primary file (its first when none is marked).
pub fn primary_file(v: &MrVersion) -> Option<&MrFile> {
    v.files.iter().find(|f| f.primary).or_else(|| v.files.first())
}

/// Whether `url` is an HTTPS address on `cdn.modrinth.com` (nothing else: no other host, port,
/// user or `..`).
pub fn cdn_url_ok(url: &str) -> bool {
    url.strip_prefix(MODRINTH_CDN)
        .is_some_and(|path| !path.is_empty() && !path.contains("..") && !path.contains(['\\', '@', '#']) && !path.chars().any(|c| c.is_control() || c.is_whitespace()))
}

/// Whether `name` is a plain file name for `shaderpacks/`: a `.zip` (the pack format Horizon
/// reads from a file), no folder in it, nothing hidden, nothing Windows refuses.
pub fn safe_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower.split('.').next().unwrap_or("");
    let reserved = matches!(stem, "con" | "prn" | "aux" | "nul") || ((stem.starts_with("com") || stem.starts_with("lpt")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    (5..=128).contains(&name.chars().count())
        && lower.ends_with(".zip")
        && !name.starts_with(['.', ' '])
        && !name.ends_with([' ', '.'])
        && !name.contains("..")
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        && !reserved
}

/// The checks a downloaded file passes before it goes into `shaderpacks/`: Modrinth's size,
/// Modrinth's SHA-512 (`actual_sha512`, hex, of the bytes) and a zip's first bytes.
pub fn check_download(file: &MrFile, bytes: &[u8], actual_sha512: &str) -> Result<(), String> {
    if bytes.len() as u64 != file.size {
        return Err(format!("the file is {} bytes, Modrinth says {}", bytes.len(), file.size));
    }
    let expected = file.sha512.trim();
    if expected.len() != 128 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Modrinth gave no usable SHA-512".into());
    }
    if !expected.eq_ignore_ascii_case(actual_sha512.trim()) {
        return Err("the file's SHA-512 isn't Modrinth's".into());
    }
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err("the file isn't a zip".into());
    }
    Ok(())
}

/// Letters and digits, lower case: `Complementary-Reimagined` and `ComplementaryReimagined_r5.zip`
/// compare as `complementaryreimagined...`.
fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The file in `shaderpacks/` that is Modrinth pack `slug` (`name`): the one recorded when it was
/// installed here if it is still there, else a zip whose name starts with the slug or name.
pub fn installed_file(slug: &str, name: &str, files: &[String], recorded: Option<&str>) -> Option<String> {
    if let Some(r) = recorded.filter(|r| files.iter().any(|f| f == *r)) {
        return Some(r.to_owned());
    }
    let keys: Vec<String> = [squash(slug), squash(name)].into_iter().filter(|k| k.len() >= 4).collect();
    files
        .iter()
        .filter(|f| f.to_ascii_lowercase().ends_with(".zip"))
        .find(|f| {
            let s = squash(f);
            keys.iter().any(|k| s.starts_with(k.as_str()))
        })
        .cloned()
}

/// The record of Modrinth installs (`<project id> <file>` a line), as a map.
pub fn parse_records(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|(id, file)| valid_project_id(id) && safe_file_name(file))
        .map(|(id, file)| (id.to_owned(), file.to_owned()))
        .collect()
}

pub fn records_text(records: &BTreeMap<String, String>) -> String {
    records.iter().map(|(id, file)| format!("{id} {file}\n")).collect()
}

/// `73302308` → `73.3M`, `12400` → `12.4k`.
pub fn short_count(n: u64) -> String {
    let one = |v: f64, unit: &str| {
        let s = if v < 100.0 { format!("{v:.1}") } else { format!("{v:.0}") };
        format!("{}{unit}", s.strip_suffix(".0").unwrap_or(&s))
    };
    match n {
        0..=999 => n.to_string(),
        1_000..=999_949 => one(n as f64 / 1e3, "k"),
        1_000_000..=999_949_999 => one(n as f64 / 1e6, "M"),
        _ => one(n as f64 / 1e9, "B"),
    }
}

/// One search's hits so far.
#[derive(Debug, Default)]
pub struct Page {
    pub hits: Vec<Entry>,
    /// The hits the search has (None until a page arrived).
    pub total: Option<usize>,
    pub error: Option<String>,
    /// When the last request for it failed (`Feed::now`).
    pub failed_at: u64,
}

/// Ticks (1/20 s) after typing before a search is sent.
pub const FEED_DEBOUNCE: u64 = 8;
/// Ticks between any two requests.
pub const FEED_SPACING: u64 = 10;
/// Ticks before a failed search is tried again (when still wanted).
pub const FEED_RETRY: u64 = 600;
/// Searches kept; past it, all but the open one and the empty search go.
pub const FEED_KEEP: usize = 48;

/// Modrinth's search pages for the session, and when to ask for the next one: one request at a
/// time, none until typing pauses, none sooner than `FEED_SPACING` after the last, a failed
/// one again only after `FEED_RETRY`, and a next page only when the list nears its end.
#[derive(Debug, Default)]
pub struct Feed {
    pub pages: BTreeMap<String, Page>,
    /// The request out: (search key, offset).
    pub busy: Option<(String, usize)>,
    /// Ticks, counted by the glue.
    pub now: u64,
    /// When the search last changed.
    pub typed_at: u64,
    /// When the last request went out (None: none yet).
    pub last_sent: Option<u64>,
    /// The search seen last tick.
    pub last_key: String,
}

impl Feed {
    /// A tick: the time, and the search as it is now.
    pub fn tick(&mut self, query: &str) {
        self.now += 1;
        let key = feed_key(query);
        if key != self.last_key {
            self.last_key = key;
            self.typed_at = self.now;
        }
    }

    /// The request to send now, if any: (search key, offset). `want_more`: the list is scrolled
    /// near its end.
    pub fn next(&self, query: &str, want_more: bool) -> Option<(String, usize)> {
        let key = feed_key(query);
        let typing = self.typed_at > 0 && self.now < self.typed_at + FEED_DEBOUNCE;
        if self.busy.is_some() || typing || self.last_sent.is_some_and(|t| self.now < t + FEED_SPACING) {
            return None;
        }
        let Some(p) = self.pages.get(&key) else { return Some((key, 0)) };
        let have = p.hits.len();
        if p.error.is_some() {
            return (self.now >= p.failed_at + FEED_RETRY && (want_more || have == 0)).then_some((key, have));
        }
        match p.total {
            Some(total) if want_more && have < total.min(MODRINTH_MAX_HITS) => Some((key, have)),
            _ => None,
        }
    }

    pub fn sent(&mut self, key: &str, offset: usize) {
        self.busy = Some((key.to_owned(), offset));
        self.last_sent = Some(self.now);
    }

    /// A search page arrived (or failed).
    pub fn landed(&mut self, key: &str, offset: usize, result: Result<(Vec<Entry>, usize), String>) {
        self.busy = None;
        let now = self.now;
        let p = self.pages.entry(key.to_owned()).or_default();
        match result {
            // A page for an offset already passed (or not reached) is dropped.
            Ok((hits, total)) if offset == p.hits.len() => {
                for h in hits {
                    if !p.hits.iter().any(|e| e.id == h.id) {
                        p.hits.push(h);
                    }
                }
                // A page with nothing new ends the list, whatever the total says.
                let ended = p.hits.len() <= offset;
                p.total = Some(if ended { p.hits.len() } else { total });
                p.error = None;
            }
            Ok(_) => {}
            Err(e) => {
                p.error = Some(e);
                p.failed_at = now;
            }
        }
        if self.pages.len() > FEED_KEEP {
            let open = self.last_key.clone();
            self.pages.retain(|k, _| k.is_empty() || *k == open || k == key);
        }
    }

    /// The hits so far for search `query`.
    pub fn hits(&self, query: &str) -> &[Entry] {
        self.pages.get(&feed_key(query)).map_or(&[], |p| &p.hits)
    }

    /// The line after the rows: what Modrinth's list is doing, or the note on its packs.
    pub fn tail(&self, query: &str) -> (String, u32) {
        let key = feed_key(query);
        let asking = self.busy.as_ref().is_some_and(|(k, _)| *k == key);
        match self.pages.get(&key) {
            _ if asking => ("Loading shader packs from Modrinth...".into(), GREY),
            None => ("Searching Modrinth...".into(), GREY),
            Some(p) if p.error.is_some() => (format!("Modrinth: {}", p.error.as_deref().unwrap_or("")), RED),
            Some(p) if p.hits.is_empty() => ("Modrinth has no shader pack for this search.".into(), GREY),
            Some(p) if p.total.is_some_and(|t| p.hits.len() < t.min(MODRINTH_MAX_HITS)) => ("More from Modrinth below...".into(), GREY),
            Some(_) => (MODRINTH_END.into(), GREY),
        }
    }
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
    /// Horizon's registry and Lumos's own mods; `entries` is these, then Modrinth's hits for
    /// the search (`compose`).
    pub registry: Vec<Entry>,
    pub feed: Feed,
    /// What `entries` was last made from: (search key, Modrinth hits, Modrinth on, registry size).
    pub composed: Option<(String, usize, bool, usize)>,
}

impl State {
    /// Makes `entries` again when the registry, the search or Modrinth's hits for it changed;
    /// true when it did. Modrinth's hits come after the registry's, so a registry row keeps its
    /// index while Modrinth pages arrive.
    pub fn compose(&mut self, modrinth: bool) -> bool {
        let key = feed_key(&self.ui.query);
        let hits = if modrinth { self.feed.hits(&key) } else { &[] };
        let sig = (key.clone(), hits.len(), modrinth, self.registry.len());
        if self.composed.as_ref() == Some(&sig) {
            return false;
        }
        let mut entries = self.registry.clone();
        entries.extend(hits.iter().filter(|h| !self.registry.iter().any(|r| r.id == h.id)).cloned());
        self.entries = entries;
        self.composed = Some(sig);
        true
    }
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
            modrinth: None,
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
        assert_eq!(s.labels().len(), 3);
        assert!(s.modrinth, "Modrinth shaders are on by default");
        s.click(2);
        let stored = s.stored();
        assert!(!settings(|k| stored.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone())).modrinth);
    }

    #[test]
    fn frame_hits_match_what_is_drawn() {
        let mut es = vec![entry("faithful", Kind::ResourcePack, 9, &["16x"])];
        es.extend(own_entries(&|_| true));
        let have = |id: &str| if id == "faithful" { Have { installed: true, ..Have::default() } } else { Have::default() };
        let v = View { entries: &es, have: &have, width: &width, settings: Settings::default(), tail: None };
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

    // ── Modrinth ──

    const SEARCH: &str = r#"{"hits":[
        {"project_id":"HVnmMxH1","project_type":"shader","slug":"complementary-reimagined","author":"EminGT",
         "title":"Complementary Shaders - Reimagined","description":"Preserving the elements of Minecraft\nwith exceptional quality.",
         "categories":["atmosphere","iris","optifine","shadows","vanilla-like"],
         "display_categories":["colored-lighting","iris","optifine","vanilla-like"],
         "versions":["1.21.11","26.3"],"downloads":73302308,"icon_url":"https://cdn.modrinth.com/data/HVnmMxH1/icon.webp"},
        {"project_id":"Q1vvjJYV","slug":"bsl-shaders","author":"capttatsu","title":"BSL Shaders é😀",
         "description":"Bright, \"soft\" & smooth","categories":["iris","optifine","cartoon"],"display_categories":[],"downloads":41000000.0},
        {"project_id":"../../etc","slug":"bad","title":"Bad id","downloads":5},
        {"project_id":"HVnmMxH1","slug":"dupe","title":"Twice","downloads":1}
    ],"offset":0,"limit":20,"total_hits":1168}"#;

    fn mr_file(name: &str, primary: bool) -> MrFile {
        MrFile { url: format!("{MODRINTH_CDN}data/x/versions/y/{name}"), name: name.into(), size: 4, sha512: "ab".repeat(64), primary }
    }

    fn mr_version(id: &str, loaders: &[&str], games: &[&str], published: &str) -> MrVersion {
        MrVersion {
            id: id.into(),
            number: id.into(),
            game_versions: games.iter().map(|g| (*g).to_owned()).collect(),
            loaders: loaders.iter().map(|l| (*l).to_owned()).collect(),
            published: published.into(),
            files: vec![mr_file(&format!("{id}.zip"), true)],
        }
    }

    #[test]
    fn json_reads_what_modrinth_sends() {
        let v = Json::parse(r#" {"a": [1, -2.5e2, true, false, null], "s": "q\"\\\/\nA", "o": {}} "#).unwrap();
        assert_eq!(v.get("a").arr().len(), 5);
        assert_eq!(v.get("a").arr()[1].num(), Some(-250.0));
        assert_eq!(v.get("s").str(), Some("q\"\\/\nA"));
        assert_eq!(v.get("missing"), &Json::Null);
        assert_eq!(v.get("a").get("x"), &Json::Null);
        for bad in ["", "{", "[1,]", r#"{"a" 1}"#, r#""open"#, "tru", "{} x", "\"\u{1}\"", r#""\ud800""#] {
            assert!(Json::parse(bad).is_err(), "{bad:?}");
        }
        assert!(Json::parse(&"[".repeat(100)).is_err(), "nesting is bounded");
    }

    #[test]
    fn search_page_becomes_shader_entries() {
        let (hits, total) = parse_search(SEARCH).unwrap();
        assert_eq!(total, 1168);
        assert_eq!(hits.len(), 2, "a bad id and a repeated project are left out");
        let c = &hits[0];
        assert_eq!(c.id, "modrinth:HVnmMxH1");
        assert_eq!(c.modrinth.as_deref(), Some("complementary-reimagined"));
        assert_eq!((c.name.as_str(), c.author.as_str(), c.downloads), ("Complementary Shaders - Reimagined", "EminGT", 73302308));
        assert_eq!(c.summary, "Preserving the elements of Minecraft with exceptional quality.");
        assert_eq!(c.kind, Kind::Shader);
        assert_eq!(c.categories, vec!["colored-lighting", "vanilla-like"], "loaders aren't categories");
        let b = &hits[1];
        assert_eq!(b.name, "BSL Shaders \u{e9}\u{1F600}");
        assert_eq!(b.summary, "Bright, \"soft\" & smooth");
        assert_eq!(b.categories, vec!["cartoon"], "categories when display_categories is empty");
        assert_eq!(b.downloads, 41_000_000);
        assert!(parse_search("{}").is_err() && parse_search("<html>").is_err());
        assert_eq!(parse_search(r#"{"hits":[]}"#).unwrap(), (Vec::new(), 0));
    }

    #[test]
    fn modrinth_urls() {
        let u = search_url("", 0);
        assert!(u.starts_with("https://api.modrinth.com/v2/search?facets=%5B%5B%22project_type%3Ashader%22%5D"), "{u}");
        assert!(u.contains("&index=downloads&offset=0&limit=20") && !u.contains("query="));
        assert!(search_url(&feed_key("  BSL   Shaders "), 40).ends_with("&offset=40&limit=20&query=bsl%20shaders"));
        assert_eq!(url_encode("a&b=c/é"), "a%26b%3Dc%2F%C3%A9");
        assert_eq!(versions_url("HVnmMxH1").as_deref(), Some("https://api.modrinth.com/v2/project/HVnmMxH1/version"));
        assert_eq!(versions_url("../x"), None);
        assert_eq!(versions_url(""), None);
        assert!(MODRINTH_AGENT.starts_with("LumosClient/"));
    }

    #[test]
    fn versions_parse_and_pick() {
        let text = r#"[
          {"id":"new","version_number":"r5.10","loaders":["iris","optifine"],"game_versions":["1.21.11","26.3"],
           "date_published":"2026-11-18T17:18:09Z","files":[{"url":"https://cdn.modrinth.com/data/H/versions/new/C_r5.10.zip",
           "filename":"C_r5.10.zip","primary":true,"size":567387,"hashes":{"sha1":"x","sha512":"EDCED01F"}}]},
          {"id":"nofile","loaders":["iris"],"game_versions":["26.3"],"date_published":"2026-12-01T00:00:00Z","files":[{"url":"u"}]}
        ]"#;
        let vs = parse_versions(text).unwrap();
        assert_eq!(vs.len(), 2);
        assert_eq!(vs[0].files[0].sha512, "edced01f");
        assert_eq!(vs[0].files[0].size, 567387);
        assert!(vs[1].files.is_empty(), "a file with no hash or size is left out");
        assert_eq!(pick_version(&vs, "26.3").unwrap().id, "new", "a version with no file isn't picked");
        assert!(parse_versions("{}").is_err());

        let vs = vec![
            mr_version("old_exact", &["iris"], &["26.3"], "2026-01-01"),
            mr_version("new_exact", &["optifine"], &["26.3", "1.21.11"], "2026-06-01"),
            mr_version("newest_older_game", &["iris"], &["26.2"], "2026-10-01"),
            mr_version("canvas", &["canvas"], &["26.3"], "2026-11-01"),
        ];
        assert_eq!(pick_version(&vs, "26.3").unwrap().id, "new_exact", "the game itself, then the newest");
        assert_eq!(pick_version(&vs[2..], "26.3").unwrap().id, "newest_older_game", "an Iris pack before a Canvas one");
        let near = vec![
            mr_version("far_old", &["iris"], &["1.20.1"], "2026-11-01"),
            mr_version("near_old", &["iris"], &["1.21.11"], "2025-01-01"),
            mr_version("near_new", &["iris"], &["26.4"], "2026-11-02"),
        ];
        assert_eq!(pick_version(&near, "26.3").unwrap().id, "near_old", "the nearest older game before a newer one");
        assert_eq!(pick_version(&near[2..], "26.3").unwrap().id, "near_new");
        let others = vec![mr_version("a", &["canvas"], &["1.0"], "2025-01-01"), mr_version("b", &["vanilla"], &["25w14a"], "2025-02-01")];
        assert_eq!(pick_version(&others, "26.3").unwrap().id, "b", "no Iris or OptiFine version: the newest");
        assert_eq!(pick_version(&[], "26.3"), None);

        let mut v = mr_version("p", &["iris"], &[], "");
        v.files = vec![mr_file("extra.zip", false), mr_file("main.zip", true)];
        assert_eq!(primary_file(&v).unwrap().name, "main.zip");
        v.files[1].primary = false;
        assert_eq!(primary_file(&v).unwrap().name, "extra.zip");
    }

    #[test]
    fn files_come_only_from_modrinths_cdn() {
        assert!(cdn_url_ok("https://cdn.modrinth.com/data/HVnmMxH1/versions/RKfUB9VC/ComplementaryReimagined_r5.10.zip"));
        for bad in [
            "http://cdn.modrinth.com/data/x.zip",
            "https://cdn.modrinth.com.evil.example/x.zip",
            "https://cdn.modrinth.com@evil.example/x.zip",
            "https://cdn.modrinth.com:8443/x.zip",
            "https://evil.example/https://cdn.modrinth.com/x.zip",
            "https://cdn.modrinth.com/",
            "https://cdn.modrinth.com/data/../../x.zip",
            "https://cdn.modrinth.com/a b.zip",
            "https://cdn.modrinth.com/x@y.zip",
        ] {
            assert!(!cdn_url_ok(bad), "{bad}");
        }
    }

    #[test]
    fn file_names_stay_in_shaderpacks() {
        for ok in ["ComplementaryReimagined_r5.10.zip", "BSL_v8.2.09.zip", "Sildur's Vibrant Shaders v1.54 Lite.ZIP", "a.zip"] {
            assert!(safe_file_name(ok), "{ok}");
        }
        for bad in [
            "", ".zip", ".hidden.zip", "../x.zip", "..\\x.zip", "a/b.zip", "a\\b.zip", "C:x.zip", "pack.jar", "pack.zip.exe",
            "pack..zip", "con.zip", "COM1.zip", "nul.zip", "a\u{0}.zip", " lead.zip", "trail.zip.", &format!("{}.zip", "x".repeat(130)),
        ] {
            assert!(!safe_file_name(bad), "{bad:?}");
        }
        assert!(safe_file_name("company.zip"), "only exact reserved names are refused");
    }

    #[test]
    fn downloads_must_match_modrinths_hash_and_size() {
        let bytes = b"PK\x03\x04rest";
        let file = MrFile { url: String::new(), name: "p.zip".into(), size: bytes.len() as u64, sha512: "AB".repeat(64), primary: true };
        assert_eq!(check_download(&file, bytes, &"ab".repeat(64)), Ok(()), "hex case doesn't matter");
        assert!(check_download(&file, bytes, &"ac".repeat(64)).unwrap_err().contains("SHA-512"));
        assert!(check_download(&file, &bytes[..7], &"ab".repeat(64)).unwrap_err().contains("bytes"));
        let no_hash = MrFile { sha512: "ab".into(), ..file.clone() };
        assert!(check_download(&no_hash, bytes, "ab").is_err(), "a short hash never matches");
        let empty = MrFile { sha512: String::new(), ..file.clone() };
        assert!(check_download(&empty, bytes, "").is_err());
        assert!(check_download(&file, b"NOTAZIP!", &"ab".repeat(64)).is_err() || file.size != 8);
        let notzip = b"MZ\x00\x00rest";
        assert!(check_download(&file, notzip, &"ab".repeat(64)).unwrap_err().contains("zip"));
    }

    #[test]
    fn installed_packs_are_found() {
        let files: Vec<String> = ["ComplementaryReimagined_r5.10.zip", "BSL_v8.2.09.zip", "notes.txt", "aurora.hshader"].iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(installed_file("complementary-reimagined", "Complementary Shaders - Reimagined", &files, None).as_deref(), Some("ComplementaryReimagined_r5.10.zip"));
        assert_eq!(installed_file("bsl-shaders", "BSL Shaders", &files, None), None, "no recorded file and no matching name");
        assert_eq!(installed_file("bsl-shaders", "BSL Shaders", &files, Some("BSL_v8.2.09.zip")).as_deref(), Some("BSL_v8.2.09.zip"));
        assert_eq!(installed_file("x", "Y", &files, Some("gone.zip")), None, "a recorded file that was deleted");
        assert_eq!(installed_file("not", "Not", &files, None), None, "short names match nothing");
        let rec = parse_records("HVnmMxH1 ComplementaryReimagined_r5.10.zip\n../x evil.zip\nQ1vvjJYV ../../x.zip\nbroken\n");
        assert_eq!(rec.len(), 1);
        assert_eq!(parse_records(&records_text(&rec)), rec);
    }

    #[test]
    fn counts_read_short() {
        assert_eq!(short_count(0), "0");
        assert_eq!(short_count(999), "999");
        assert_eq!(short_count(12_400), "12.4k");
        assert_eq!(short_count(150_000), "150k");
        assert_eq!(short_count(73_302_308), "73.3M");
        assert_eq!(short_count(1_000_000), "1M");
        assert_eq!(short_count(2_500_000_000), "2.5B");
    }

    #[test]
    fn feed_asks_politely() {
        let mut f = Feed::default();
        f.tick("");
        assert_eq!(f.next("", false), Some((String::new(), 0)), "the first page at once");
        f.sent("", 0);
        assert_eq!(f.next("", true), None, "one request at a time");
        assert_eq!(f.tail("").0, "Loading shader packs from Modrinth...");
        let (hits, _) = parse_search(SEARCH).unwrap();
        f.landed("", 0, Ok((hits.clone(), 30)));
        assert_eq!(f.hits("").len(), 2);
        assert_eq!(f.next("", true), None, "not again straight away");
        for _ in 0..FEED_SPACING {
            f.tick("");
        }
        assert_eq!(f.next("", false), None, "no next page until the list nears its end");
        assert_eq!(f.next("", true), Some((String::new(), 2)));
        assert!(f.tail("").0.starts_with("More from Modrinth"));
        // A page for an old offset is dropped; a page with nothing new ends the list.
        f.sent("", 2);
        f.landed("", 0, Ok((hits.clone(), 30)));
        assert_eq!(f.hits("").len(), 2);
        f.sent("", 2);
        f.landed("", 2, Ok((hits.clone(), 30)));
        assert_eq!(f.pages[""].total, Some(2));
        for _ in 0..FEED_SPACING {
            f.tick("");
        }
        assert_eq!(f.next("", true), None, "the end of the list");
        assert_eq!(f.tail("").0, MODRINTH_END);

        // Typing: nothing until it pauses, then the search; the old one stays kept.
        for c in ["b", "bs", "bsl"] {
            f.tick(c);
            assert_eq!(f.next(c, false), None, "{c}: still typing");
        }
        assert_eq!(f.tail("bsl").0, "Searching Modrinth...");
        for _ in 0..FEED_DEBOUNCE {
            f.tick("bsl");
        }
        assert_eq!(f.next("BSL ", false), Some(("bsl".into(), 0)));
        f.sent("bsl", 0);
        f.landed("bsl", 0, Err("can't be reached (offline?)".into()));
        assert_eq!(f.tail("bsl"), ("Modrinth: can't be reached (offline?)".into(), RED));
        for _ in 0..FEED_RETRY - 1 {
            f.tick("bsl");
        }
        assert_eq!(f.next("bsl", true), None, "a failed search waits before it is tried again");
        f.tick("bsl");
        assert_eq!(f.next("bsl", false), Some(("bsl".into(), 0)));
        f.sent("bsl", 0);
        f.landed("bsl", 0, Ok((Vec::new(), 0)));
        assert_eq!(f.tail("bsl").0, "Modrinth has no shader pack for this search.");
        assert_eq!(f.hits("").len(), 2, "the first search is still kept");
    }

    #[test]
    fn modrinth_rows_follow_the_registry() {
        let mut s = State::default();
        s.registry = vec![entry("aurora", Kind::Shader, 3, &["atmosphere"])];
        let (hits, _) = parse_search(SEARCH).unwrap();
        s.feed.landed("", 0, Ok((hits, 2)));
        assert!(s.compose(true));
        assert!(!s.compose(true), "made again only when something changed");
        assert_eq!(s.entries.len(), 3);
        s.ui.set_tab(Tab::Shaders);
        s.ui.query = "aurora".into();
        assert!(s.compose(true));
        assert_eq!(s.entries.len(), 1, "another search: its own Modrinth hits (none yet)");
        s.ui.query.clear();
        s.compose(true);
        let order = visible(&s.entries, Tab::Shaders, "", None);
        assert_eq!(order, vec![0, 1, 2], "the registry's first, then Modrinth's by downloads");
        assert!(s.compose(false));
        assert_eq!(s.entries.len(), 1, "switched off, no Modrinth rows");

        s.compose(true);
        let have = |id: &str| if id == "modrinth:Q1vvjJYV" { Have { installed: true, on: true, ..Have::default() } } else { Have::default() };
        let tail = s.feed.tail("");
        let v = View { entries: &s.entries, have: &have, width: &width, settings: Settings::default(), tail: Some(tail.clone()) };
        assert_eq!(offer(&s.entries[1], &have(&s.entries[1].id), false), (vec![Act::Install], "Modrinth", MODRINTH));
        assert_eq!(offer(&s.entries[2], &have(&s.entries[2].id), false).0, vec![Act::Off, Act::Remove]);
        assert_eq!(list_len(&s.ui, &v), 4, "three rows and the tail line");
        let (prims, hits) = frame(&s.ui, &v, [0.0, 0.0], 480.0, 270.0);
        let texts: Vec<&str> = prims.iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.as_str()) } else { None }).collect();
        assert!(texts.iter().any(|t| t.starts_with("Complementary Shaders - Reimagined by")), "{texts:?}");
        assert!(texts.iter().any(|t| t.starts_with("73.3M downloads - Preserving")), "{texts:?}");
        assert!(texts.contains(&"Modrinth"));
        assert!(texts.contains(&MODRINTH_END), "the tail line, whole: {texts:?}");
        assert!(texts.contains(&"BSL Shaders \u{e9}\u{1F600} by capttatsu"));
        assert!(hits.iter().any(|(_, h)| *h == Hit::Row(1, Act::Install)));
        // Pointed at, a Modrinth row says what its pack is.
        let row = hits.iter().find(|(_, h)| *h == Hit::Row(1, Act::Install)).unwrap().0;
        let (prims, _) = frame(&s.ui, &v, [row[0] - 100.0, row[1] + 5.0], 480.0, 270.0);
        assert!(prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text.starts_with("Iris/OptiFine pack"))));
    }
}
