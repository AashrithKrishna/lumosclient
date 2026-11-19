// Lumos World Map: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_world_map`; `glue.rs` reads the
// game and calls in here. It names nothing outside `std`, so it also compiles alone:
// `rustc --edition 2024 --test runtime.rs` runs the tests below.
//
// What is here: the settings; the map's view (smooth zoom about the pointer, panning); the
// pictures the map is drawn with (128 x 128 textures at levels of detail, composed from the
// explored tiles near the view and from small per-region overviews far out); the region cache's
// memory limit; cave layers; the saved formats shared with Lumos Map (its `LMR` region files,
// waypoint files, sub-worlds and footsteps, copied here so neither mod needs the other to
// build) and this mod's own `LWX1` side files (block, biome and light per column); waypoints;
// and a PNG writer.
//
// Lumos Map's formats below are kept byte for byte as `lumos_map/runtime.rs` writes them: a
// change to one must be made to both.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

pub const ID: &str = "lumos_world_map";
pub const NAME: &str = "World Map";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "A fullscreen map (M) of everywhere you have been, saved per world, server and \
dimension: smooth zoom, cave layers, waypoints shared with Lumos Map, other players, dimensions with Nether \
coordinates, PNG export. With Lumos Map on, its minimap uses this map's chunks.";

/// Every mod screen's id is the game's one for mod screens.
pub const SCREEN: &str = "mod";
/// Lumos Map (the minimap), which this map works with when it is on.
pub const MINIMAP_ID: &str = "lumos_map";

// ---------------------------------------------------------------- settings

pub const PAGES: [&str; 8] = ["General", "Look", "Caves", "Waypoints", "Players", "Performance", "Minimap", "Keys"];
pub const KEYS_PAGE: usize = PAGES.len() - 1;

/// How a server's maps are told apart: asked (a confirm screen when the world changes), by the
/// world's spawn, or one map for the address.
pub const MAP_MODES: [&str; 3] = ["Ask to confirm", "By spawn", "Single"];
/// Slopes: flat, gentle relief (2D), strong relief (3D).
pub const SLOPES: [&str; 3] = ["Off", "2D", "3D"];
pub const CAVE_MODES: [&str; 2] = ["Auto", "Off"];
/// How far below a cave layer's top it looks for a floor (as Lumos Map's).
pub const CAVE_DEPTHS: [usize; 5] = [8, 16, 30, 48, 64];
/// The closest and farthest zoom, blocks per GUI pixel.
pub const MIN_ZOOMS: [f64; 4] = [0.0625, 0.125, 0.25, 0.5];
pub const MAX_ZOOMS: [f64; 4] = [16.0, 32.0, 64.0, 128.0];
pub const WP_NAMES: [&str; 3] = ["Pointed at", "Zoomed in", "Always"];
/// Which waypoints the map and its list show.
pub const FILTERS: [&str; 4] = ["All", "This set", "Death points", "Hidden"];
pub const DEATHS: [usize; 4] = [1, 3, 5, 10];
/// A new waypoint's colour: the next in turn, or one of `COLOURS`.
pub const NEW_COLOURS: [&str; 11] =
    ["Next in turn", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Pink", "White", "Grey"];
/// A waypoint's marker text: its name's first letter, or a symbol.
pub const SYMBOLS: [&str; 8] = ["First letter", "*", "+", "!", "#", "X", "$", "@"];
/// Regions (512 x 512 blocks) of full detail kept in memory, with roughly what they cost at most.
pub const CACHE_SIZES: [(&str, usize); 4] = [("16 (60 MB)", 16), ("32 (120 MB)", 32), ("64 (240 MB)", 64), ("128 (480 MB)", 128)];
/// Time a frame may spend mapping chunks, in microseconds (closed, open).
pub const SPEEDS: [(&str, u64, u64); 3] = [("Light", 400, 1500), ("Normal", 700, 2500), ("Fast", 1500, 5000)];
pub const EXPORTS: [&str; 2] = ["The view, 1:1", "Whole map, 1:8"];
pub const ARROW_COLOURS: [(&str, u32); 4] = [("White", 0xFFFFFF), ("Red", 0xFF5555), ("Yellow", 0xFFFF55), ("Aqua", 0x55FFFF)];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    // General
    pub follow: bool,
    pub cursor: bool,
    pub cursor_biome: bool,
    pub grid: bool,
    pub regions: bool,
    pub smooth: bool,
    pub zoom_min: usize,
    pub zoom_max: usize,
    pub map_mode: usize,
    pub convert: bool,
    pub footsteps: bool,
    pub export: usize,
    // Look
    pub textured: bool,
    pub slopes: usize,
    pub lighting: bool,
    pub biomes: bool,
    pub blend: bool,
    pub glass: bool,
    pub flowers: bool,
    pub water_depth: bool,
    pub palette_biomes: bool,
    pub dim_unloaded: bool,
    // Caves
    pub cave_mode: usize,
    pub cave_depth: usize,
    pub cave_legible: bool,
    pub cave_scan: bool,
    pub cave_shared: bool,
    pub layer_y: bool,
    // Waypoints
    pub waypoints: bool,
    pub wp_names: usize,
    pub filter: usize,
    pub deaths: bool,
    pub deaths_kept: usize,
    pub teleport: bool,
    pub wp_colour: usize,
    pub share: bool,
    // Players
    pub arrow: bool,
    pub arrow_colour: usize,
    pub players: bool,
    pub player_names: bool,
    // Performance
    pub cache: usize,
    pub map_new: bool,
    pub map_update: bool,
    pub speed: usize,
    pub overviews: bool,
    // Minimap
    pub take_m: bool,
    pub enlarge_opens: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            follow: true,
            cursor: true,
            cursor_biome: true,
            grid: false,
            regions: false,
            smooth: true,
            zoom_min: 1,
            zoom_max: 1,
            map_mode: 1,
            convert: true,
            footsteps: true,
            export: 0,
            textured: true,
            slopes: 1,
            lighting: false,
            biomes: true,
            blend: true,
            glass: true,
            flowers: true,
            water_depth: true,
            palette_biomes: false,
            dim_unloaded: false,
            cave_mode: 0,
            cave_depth: 2,
            cave_legible: true,
            cave_scan: true,
            cave_shared: true,
            layer_y: true,
            waypoints: true,
            wp_names: 1,
            filter: 0,
            deaths: true,
            deaths_kept: 1,
            teleport: true,
            wp_colour: 0,
            share: true,
            arrow: true,
            arrow_colour: 0,
            players: true,
            player_names: true,
            cache: 1,
            map_new: true,
            map_update: true,
            speed: 1,
            overviews: true,
            take_m: true,
            enlarge_opens: true,
        }
    }
}

/// A settings table entry as a page button shows it.
pub trait ShowEntry {
    fn show(&self) -> String;
}
impl ShowEntry for &str {
    fn show(&self) -> String {
        (*self).to_owned()
    }
}
impl ShowEntry for f64 {
    fn show(&self) -> String {
        if *self < 1.0 { format!("1/{}", (1.0 / self).round()) } else { format!("{self}") }
    }
}
impl ShowEntry for usize {
    fn show(&self) -> String {
        format!("{self}")
    }
}
impl ShowEntry for (&str, u32) {
    fn show(&self) -> String {
        self.0.to_owned()
    }
}
impl ShowEntry for (&str, usize) {
    fn show(&self) -> String {
        self.0.to_owned()
    }
}
impl ShowEntry for (&str, u64, u64) {
    fn show(&self) -> String {
        self.0.to_owned()
    }
}

pub const SETTINGS_VERSION: u32 = 1;
pub const SETTINGS_VERSION_KEY: &str = "settings_version";

macro_rules! settings_table {
    ($( $field:ident: $kind:ident $(($table:ident))?, $page:expr, $label:expr; )*) => {
        impl Settings {
            pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
                let d = Self::default();
                let flag = |key: &str, default: bool| match get(key).as_deref().map(str::trim) {
                    Some("true") => true,
                    Some("false") => false,
                    _ => default,
                };
                let index = |key: &str, default: usize, len: usize| {
                    get(key)
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .filter(|i| *i < len)
                        .unwrap_or(default)
                };
                let _ = (&flag, &index);
                Self { $( $field: settings_table!(@read $kind, flag, index, stringify!($field), d.$field $(, $table)?), )* }
            }
            pub fn stored(&self) -> Vec<(&'static str, String)> {
                vec![ $( (stringify!($field), self.$field.to_string()), )* (SETTINGS_VERSION_KEY, SETTINGS_VERSION.to_string()) ]
            }
            /// Every setting's page.
            pub const PAGE_OF: [usize; Self::COUNT] = [$($page),*];
            /// Every setting's button text.
            pub fn labels(&self) -> Vec<String> {
                let on = |b: bool| if b { "ON".to_owned() } else { "OFF".to_owned() };
                let _ = &on;
                vec![ $( format!("{}: {}", $label, settings_table!(@show $kind, on, self.$field $(, $table)?)), )* ]
            }
            /// Setting `button` one step on (`back`: one step back).
            pub fn click_dir(&mut self, button: usize, back: bool) {
                let names = [$(stringify!($field)),*];
                let Some(name) = names.get(button) else { return };
                $(
                    if *name == stringify!($field) {
                        settings_table!(@click $kind, self.$field, back $(, $table)?);
                    }
                )*
            }
            pub fn click(&mut self, button: usize) {
                self.click_dir(button, false);
            }
            pub const COUNT: usize = [$(stringify!($field)),*].len();
        }
    };
    (@read flag, $flag:ident, $index:ident, $key:expr, $d:expr) => { $flag($key, $d) };
    (@read index, $flag:ident, $index:ident, $key:expr, $d:expr, $table:ident) => { $index($key, $d, $table.len()) };
    (@show flag, $on:ident, $v:expr) => { $on($v) };
    (@show index, $on:ident, $v:expr, $table:ident) => { ShowEntry::show(&$table[$v]) };
    (@click flag, $v:expr, $back:expr) => { $v = !$v };
    (@click index, $v:expr, $back:expr, $table:ident) => {
        $v = if $back { ($v + $table.len() - 1) % $table.len() } else { ($v + 1) % $table.len() }
    };
}

settings_table! {
    follow: flag, 0, "Follow you when opened";
    cursor: flag, 0, "Cursor coordinates, chunk and region";
    cursor_biome: flag, 0, "Biome and block at the cursor";
    grid: flag, 0, "Chunk grid";
    regions: flag, 0, "Region lines";
    smooth: flag, 0, "Smooth zoom";
    zoom_min: index(MIN_ZOOMS), 0, "Closest zoom (blocks a pixel)";
    zoom_max: index(MAX_ZOOMS), 0, "Farthest zoom (blocks a pixel)";
    map_mode: index(MAP_MODES), 0, "Server maps";
    convert: flag, 0, "Nether coordinates x8 between dimensions";
    footsteps: flag, 0, "Footsteps (your recent path)";
    export: index(EXPORTS), 0, "PNG export";
    textured: flag, 1, "Block colours from textures (off: map colours)";
    slopes: index(SLOPES), 1, "Terrain slopes";
    lighting: flag, 1, "Lighting (darker at night, lit blocks glow)";
    biomes: flag, 1, "Biome tints";
    blend: flag, 1, "Blend biome colours";
    glass: flag, 1, "See through stained glass";
    flowers: flag, 1, "Show flowers";
    water_depth: flag, 1, "Water depth (seabed shows through)";
    palette_biomes: flag, 1, "Biome colours with map colours";
    dim_unloaded: flag, 1, "Dim ground not loaded now";
    cave_mode: index(CAVE_MODES), 2, "Cave mode";
    cave_depth: index(CAVE_DEPTHS), 2, "Layer depth (blocks)";
    cave_legible: flag, 2, "Legible caves";
    cave_scan: flag, 2, "Map caves while underground";
    cave_shared: flag, 2, "Cave depth and legibility from the minimap";
    layer_y: flag, 2, "Show the layer's Y";
    waypoints: flag, 3, "Waypoints on the map";
    wp_names: index(WP_NAMES), 3, "Waypoint names";
    filter: index(FILTERS), 3, "Waypoints shown";
    deaths: flag, 3, "Death points (when the minimap is off)";
    deaths_kept: index(DEATHS), 3, "Death points kept";
    teleport: flag, 3, "Teleport from the map (where allowed)";
    wp_colour: index(NEW_COLOURS), 3, "New waypoints' colour";
    share: flag, 3, "Share waypoints in chat";
    arrow: flag, 4, "Your arrow";
    arrow_colour: index(ARROW_COLOURS), 4, "Arrow colour";
    players: flag, 4, "Other players";
    player_names: flag, 4, "Player names";
    cache: index(CACHE_SIZES), 5, "Regions kept in memory";
    map_new: flag, 5, "Map new chunks";
    map_update: flag, 5, "Update changed chunks";
    speed: index(SPEEDS), 5, "Mapping speed";
    overviews: flag, 5, "Overviews when zoomed far out";
    take_m: flag, 6, "Open this map in place of the minimap's";
    enlarge_opens: flag, 6, "The minimap's enlarge key opens this map";
}

impl Settings {
    /// The settings on page `page`: (setting, button text).
    pub fn page(&self, page: usize) -> Vec<(usize, String)> {
        self.labels()
            .into_iter()
            .enumerate()
            .filter(|(i, _)| Self::PAGE_OF[*i] == page)
            .collect()
    }

    /// The closest and farthest zoom, blocks per GUI pixel.
    pub fn zooms(&self) -> (f64, f64) {
        (MIN_ZOOMS[self.zoom_min.min(MIN_ZOOMS.len() - 1)], MAX_ZOOMS[self.zoom_max.min(MAX_ZOOMS.len() - 1)])
    }

    /// How far below a layer's top its floors are looked for.
    pub fn depth_blocks(&self) -> i32 {
        CAVE_DEPTHS[self.cave_depth.min(CAVE_DEPTHS.len() - 1)] as i32
    }

    /// Regions of full detail kept in memory.
    pub fn cache_regions(&self) -> usize {
        CACHE_SIZES[self.cache.min(CACHE_SIZES.len() - 1)].1
    }

    /// Mapping time a frame, microseconds, with the map closed and open.
    pub fn budget_us(&self, open: bool) -> u64 {
        let s = SPEEDS[self.speed.min(SPEEDS.len() - 1)];
        if open { s.2 } else { s.1 }
    }

    /// How the map's pictures are coloured (`night`: the sky's light level step with lighting on).
    pub fn look(&self, night: u8) -> Look {
        Look {
            textured: self.textured,
            slopes: self.slopes.min(2) as u8,
            legible: self.cave_legible,
            biomes: self.palette_biomes,
            night: if self.lighting { night.min(NIGHT_STEPS) } else { NIGHT_STEPS },
        }
    }

    /// Lumos Map's cave settings taken over (its `cave_depth` and `cave_legible`, as it stores
    /// them), when "from the minimap" is on.
    pub fn with_minimap_caves(mut self, get: impl Fn(&str) -> Option<String>) -> Self {
        if !self.cave_shared {
            return self;
        }
        if let Some(d) = get("cave_depth").and_then(|v| v.trim().parse::<usize>().ok()).filter(|d| *d < CAVE_DEPTHS.len()) {
            self.cave_depth = d;
        }
        match get("cave_legible").as_deref().map(str::trim) {
            Some("true") => self.cave_legible = true,
            Some("false") => self.cave_legible = false,
            _ => {}
        }
        self
    }
}

// ---------------------------------------------------------------- the view

/// What the map shows: its centre in blocks and its zoom in blocks per GUI pixel, easing
/// towards `target` with smooth zoom, about the point the wheel was turned at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapView {
    pub x: f64,
    pub z: f64,
    pub bpp: f64,
    pub target: f64,
    /// Where the wheel was turned: the GUI point and the world point under it then.
    pub anchor: Option<([f32; 2], [f64; 2])>,
}

impl Default for MapView {
    fn default() -> Self {
        Self { x: 0.0, z: 0.0, bpp: 1.0, target: 1.0, anchor: None }
    }
}

/// A wheel notch zooms by this factor.
pub const ZOOM_NOTCH: f64 = std::f64::consts::SQRT_2;

impl MapView {
    /// The world point under GUI point `p` of the map's `rect` (`[x, y, w, h]`).
    pub fn world_at(&self, rect: [f32; 4], p: [f32; 2]) -> (f64, f64) {
        let (cx, cy) = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
        (self.x + f64::from(p[0] - cx) * self.bpp, self.z + f64::from(p[1] - cy) * self.bpp)
    }

    /// Where world point `x, z` is on the map's `rect`, in GUI pixels.
    pub fn screen_of(&self, rect: [f32; 4], x: f64, z: f64) -> [f32; 2] {
        let (cx, cy) = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
        [cx + ((x - self.x) / self.bpp) as f32, cy + ((z - self.z) / self.bpp) as f32]
    }

    /// Dragged by `delta` GUI pixels: the map moves with the pointer.
    pub fn pan(&mut self, delta: [f32; 2]) {
        self.x -= f64::from(delta[0]) * self.bpp;
        self.z -= f64::from(delta[1]) * self.bpp;
        if let Some((_, w)) = &mut self.anchor {
            w[0] -= f64::from(delta[0]) * self.bpp;
            w[1] -= f64::from(delta[1]) * self.bpp;
        }
    }

    /// `notches` wheel steps in (> 0) or out within `min..=max`, about GUI point `p`; at once
    /// unless `smooth`.
    pub fn zoom_at(&mut self, rect: [f32; 4], p: [f32; 2], notches: f64, min: f64, max: f64, smooth: bool) {
        let target = (self.target / ZOOM_NOTCH.powf(notches)).clamp(min, max);
        self.target = target;
        let (wx, wz) = self.world_at(rect, p);
        self.anchor = Some((p, [wx, wz]));
        if !smooth {
            self.set_bpp(rect, target);
            self.anchor = None;
        }
    }

    /// The zoom set to `bpp`, the anchor (if any) kept under its GUI point.
    fn set_bpp(&mut self, rect: [f32; 4], bpp: f64) {
        self.bpp = bpp;
        if let Some((p, w)) = self.anchor {
            let (cx, cy) = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
            self.x = w[0] - f64::from(p[0] - cx) * bpp;
            self.z = w[1] - f64::from(p[1] - cy) * bpp;
        }
    }

    /// `dt` seconds of the smooth zoom; whether the view changed.
    pub fn step(&mut self, rect: [f32; 4], dt: f64) -> bool {
        if (self.bpp / self.target - 1.0).abs() < 1e-3 {
            if self.bpp != self.target {
                self.set_bpp(rect, self.target);
                self.anchor = None;
                return true;
            }
            self.anchor = None;
            return false;
        }
        let k = (dt * 14.0).clamp(0.0, 1.0);
        let next = self.bpp * (self.target / self.bpp).powf(k);
        self.set_bpp(rect, next);
        true
    }

    /// The world rectangle the map's `rect` shows: min x, min z, max x, max z.
    pub fn bounds(&self, rect: [f32; 4]) -> (f64, f64, f64, f64) {
        let (x0, z0) = self.world_at(rect, [rect[0], rect[1]]);
        let (x1, z1) = self.world_at(rect, [rect[0] + rect[2], rect[1] + rect[3]]);
        (x0, z0, x1, z1)
    }

    /// Centred on `x, z` (the zoom's anchor dropped).
    pub fn centre_on(&mut self, x: f64, z: f64) {
        self.x = x;
        self.z = z;
        self.anchor = None;
    }
}

/// Unexplored ground (as Lumos Map's world map shows it).
pub const UNKNOWN: [u8; 4] = [18, 18, 22, 255];

/// A key pans the map by a tenth of its `rect` in direction `dir` (x, z).
pub fn key_pan(view: &mut MapView, rect: [f32; 4], dir: (i32, i32)) {
    view.pan([-rect[2] / 10.0 * dir.0 as f32, -rect[3] / 10.0 * dir.1 as f32]);
}

/// How far a press may move and still be a click, in GUI pixels.
pub const CLICK_SLOP: f32 = 3.0;

/// Where the view goes when the map switches from dimension `from` to `to`: the same place
/// (an eighth in the Nether, eight times out of it) when `scale`, else the same numbers.
pub fn convert_view(x: f64, z: f64, from: &str, to: &str, scale: bool) -> (f64, f64) {
    if !scale {
        return (x, z);
    }
    match (from, to) {
        (OVERWORLD, NETHER) => (x / 8.0, z / 8.0),
        (NETHER, OVERWORLD) => (x * 8.0, z * 8.0),
        _ => (x, z),
    }
}

/// The cursor's place as the bottom bar says it: block, chunk (and the block in it), region.
pub fn cursor_line(x: i32, z: i32, y: Option<i32>) -> String {
    let y = y.map_or_else(String::new, |y| format!(" Y {y}"));
    format!(
        "X {x}{y} Z {z}   Chunk {} {} ({} {})   Region {} {}",
        x.div_euclid(16),
        z.div_euclid(16),
        x.rem_euclid(16),
        z.rem_euclid(16),
        x.div_euclid(512),
        z.div_euclid(512)
    )
}

/// The same place in the other of the Overworld and the Nether, for the bottom bar.
pub fn other_dim_line(dim: &str, x: i32, z: i32) -> Option<String> {
    match dim {
        OVERWORLD => Some(format!("Nether {} {}", x.div_euclid(8), z.div_euclid(8))),
        NETHER => Some(format!("Overworld {} {}", x * 8, z * 8)),
        _ => None,
    }
}

/// "1/4", "2.8" blocks a pixel.
pub fn zoom_text(bpp: f64) -> String {
    if bpp < 1.0 { format!("x{:.1}", 1.0 / bpp) } else { format!("1:{:.1}", bpp) }
}

// ---------------------------------------------------------------- cave layers

/// What the map shows of a dimension: its surface (under the roof of a roofed one), a cave
/// layer (a 16-block band, Lumos Map's `caves/<band>`), or the full cave map (`caves/full`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Layer {
    #[default]
    Surface,
    Band(i32),
    Full,
}

impl Layer {
    /// The folder under the dimension's, as Lumos Map names it.
    pub fn folder(self) -> Option<String> {
        match self {
            Layer::Surface => None,
            Layer::Band(b) => Some(cave_folder(Some(b))),
            Layer::Full => Some(cave_folder(None)),
        }
    }

    /// The band a region file of this layer says it is (None: not a band's).
    pub fn band(self) -> Option<i32> {
        match self {
            Layer::Band(b) => Some(b),
            _ => None,
        }
    }
}

/// A band's top Y: a layer is scanned from here down.
pub fn band_top(band: i32) -> i32 {
    band * 16 + 15
}

/// The layer a step down or up from `now` in a world from `min_y` to `max_y` (exclusive):
/// down from the surface (or the full cave map) goes to the band of `feet`; up past the top
/// band, the surface; up from the surface, the full cave map (`full`: an open dimension's).
pub fn layer_step(now: Layer, feet: i32, down: bool, min_y: i32, max_y: i32, full: bool) -> Layer {
    let (lo, hi) = (band_of(min_y), band_of(max_y - 1));
    match now {
        Layer::Surface | Layer::Full if down => Layer::Band(band_of(feet).clamp(lo, hi)),
        Layer::Surface if full => Layer::Full,
        Layer::Surface => Layer::Surface,
        Layer::Full => Layer::Surface,
        Layer::Band(b) if down => Layer::Band((b - 1).max(lo)),
        Layer::Band(b) if b >= hi => Layer::Surface,
        Layer::Band(b) => Layer::Band(b + 1),
    }
}

/// What the map shows now: the layer chosen by hand, else (cave mode Auto) the band of the
/// player's head while underground, else the surface.
pub fn shown_layer(manual: Option<Layer>, auto: bool, underground: bool, head: i32) -> Layer {
    if let Some(l) = manual {
        return l;
    }
    if auto && underground { Layer::Band(band_of(head)) } else { Layer::Surface }
}

/// Underground: a roof over every column round the player, held a second before it switches
/// (walking through a doorway doesn't flip the map).
#[derive(Clone, Copy, Debug, Default)]
pub struct CaveSwitch {
    pub on: bool,
    pending: Option<(bool, u64)>,
}

/// Ticks (50 ms) a new answer must hold.
pub const CAVE_HOLD: u64 = 20;

impl CaveSwitch {
    pub fn step(&mut self, roofed: bool, tick: u64) -> bool {
        if roofed == self.on {
            self.pending = None;
        } else {
            match self.pending {
                Some((v, since)) if v == roofed => {
                    if tick.saturating_sub(since) >= CAVE_HOLD {
                        self.on = roofed;
                        self.pending = None;
                    }
                }
                _ => self.pending = Some((roofed, tick)),
            }
        }
        self.on
    }
}

/// Every column of the square round the player has a roof.
pub fn roofed(columns: &[Option<i32>]) -> bool {
    !columns.is_empty() && columns.iter().all(Option::is_some)
}

// ---------------------------------------------------------------- shading

/// How a column is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Look {
    /// The game's colours (textures, biome tints), else the map palette.
    pub textured: bool,
    /// 0 flat, 1 gentle relief, 2 strong relief.
    pub slopes: u8,
    /// Cave floors darken less with depth.
    pub legible: bool,
    /// The map palette's biome-tinted colours take the column's tint.
    pub biomes: bool,
    /// The sky's light, 0 (night) to `NIGHT_STEPS` (day; lighting off is always day).
    pub night: u8,
}

pub const NIGHT_STEPS: u8 = 8;

/// Map colours the game tints by biome: grass, plants (leaves) and water.
pub const BIOME_TINTED: [u8; 3] = [1, 7, WATER];

/// Palette colour `palette` in the hue of `tinted`, as bright as the palette colour.
pub fn palette_biome(palette: u32, tinted: u32) -> u32 {
    let lum = |c: u32| 0.299 * ((c >> 16) & 255) as f32 + 0.587 * ((c >> 8) & 255) as f32 + 0.114 * (c & 255) as f32;
    let (want, have) = (lum(palette), lum(tinted));
    if have < 1.0 {
        return palette;
    }
    let k = want / have;
    let ch = |s: u32| ((((tinted >> s) & 255) as f32 * k).round().clamp(0.0, 255.0) as u32) << s;
    ch(16) | ch(8) | ch(0)
}

/// The ground under stained glass, seen through it.
pub fn glass_over(ground: u32, glass: u32) -> u32 {
    lerp_rgb(ground, glass, 0.35)
}

/// How bright the sky is, 0..=1 (1 at day, about 0.25 at midnight, less in rain and thunder).
pub fn sky_light(day_time: i64, rain: f32, thunder: f32) -> f32 {
    let t = (day_time.rem_euclid(24000) as f64) / 24000.0;
    let f = (t - 0.25).rem_euclid(1.0);
    let g = 0.5 - (f * std::f64::consts::PI).cos() / 2.0;
    let angle = (f * 2.0 + g) / 3.0;
    let mut light = 1.0 - (1.0 - ((angle * std::f64::consts::TAU).cos() * 2.0 + 0.5)).clamp(0.0, 1.0);
    light *= 1.0 - f64::from(rain.clamp(0.0, 1.0)) * 5.0 / 16.0;
    light *= 1.0 - f64::from(thunder.clamp(0.0, 1.0)) * 5.0 / 16.0;
    light as f32
}

/// The sky's light as a step for `Look::night` (so the pictures are drawn again only a few
/// times a day).
pub fn night_step(sky: f32) -> u8 {
    (sky.clamp(0.0, 1.0) * f32::from(NIGHT_STEPS)).round() as u8
}

/// How bright a column is with lighting: the sky's step, or the block light over it when
/// brighter (torches glow at night).
pub fn light_factor(night: u8, block_light: u8) -> f32 {
    let sky = 0.25 + 0.75 * f32::from(night.min(NIGHT_STEPS)) / f32::from(NIGHT_STEPS);
    let lamp = 0.25 + 0.75 * f32::from(block_light.min(15)) / 15.0;
    sky.max(lamp)
}

// `CaveShade`, `depth_factor`, `WALL_RGB`, `WALL_EDGE_RGB`, `DEEP_RGB` and `FIRE` are with the tiles below.

/// One sampled column: the tile's column and the block light over it.
pub type Sample = Option<(Col, u8)>;

/// A column's colour: its own (or its map colour), lit where the ground rises towards the
/// north-west and shaded where it falls away (`n`, `w`: the heights there), darker in a cave
/// layer with depth, and lit by the sky or its blocks with lighting.
pub fn shade(palette: &[u32], look: Look, cave: Option<CaveShade>, c: Col, light: u8, n: Option<i16>, w: Option<i16>, wall_edge: bool) -> [u8; 4] {
    let px = |rgb: u32| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255];
    if c.kind == DEEP {
        return px(DEEP_RGB);
    }
    if cave.is_some() && c.kind == WALL {
        return px(if wall_edge { WALL_EDGE_RGB } else { WALL_RGB });
    }
    let own = look.textured && c.rgb != 0;
    let base = if own {
        c.rgb
    } else {
        let flat = palette.get(usize::from(c.color)).copied().unwrap_or(0);
        if look.biomes && c.rgb != 0 && BIOME_TINTED.contains(&c.color) { palette_biome(flat, c.rgb) } else { flat }
    };
    let water = c.color == WATER || c.depth > 0;
    let slopes = if cave.is_some() { look.slopes.max(1) } else { look.slopes };
    let mut factor = if water {
        if own { 1.0 } else { 1.0 - f32::from(c.depth.min(14)) * 0.03 }
    } else if slopes > 0 {
        let d = |h: Option<i16>| f32::from(c.height - h.unwrap_or(c.height)).clamp(-6.0, 6.0);
        let slope = d(n) + d(w);
        if slopes == 1 { (1.0 + slope * 0.045).clamp(0.7, 1.16) } else { (1.0 + slope * 0.11).clamp(0.5, 1.35) }
    } else {
        1.0
    };
    if let Some(cave) = cave
        && c.color != FIRE
    {
        factor *= depth_factor(cave.start - i32::from(c.height), cave.depth, look.legible);
    }
    if look.night < NIGHT_STEPS && cave.is_none() {
        factor *= light_factor(look.night, light);
    }
    let ch = |s: u32| ((((base >> s) & 255) as f32 * factor).round().clamp(0.0, 255.0)) as u8;
    [ch(16), ch(8), ch(0), 255]
}

// ---------------------------------------------------------------- the map's pictures

// The map is drawn with 128 x 128 textures in Horizon's map slots (the atlas slots of framed
// maps), each covering `128 << lod` blocks a side. Near zooms (lod 0 to 2) are composed from
// the explored tiles in memory; far ones (lod 3 and up) from each region's 64 x 64 overview
// (8 blocks a texel), which stays in memory for every region of the dimension, so zooming far
// out never loads whole regions. Unknown ground is clear, over the map's background.

/// A texture's side, texels.
pub const TEX: u32 = 128;
/// An overview's side, texels (a region is 512 blocks: 8 blocks a texel).
pub const OVERVIEW: usize = 64;
pub const OVERVIEW_STEP: i32 = 512 / OVERVIEW as i32;
/// The first level drawn from overviews.
pub const BASE_LOD: u8 = 3;
pub const MAX_LOD: u8 = 10;

/// One texture: what it shows (`source`: a dimension's layer), its level and its place in
/// steps of its own span.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexKey {
    pub source: u32,
    pub lod: u8,
    pub x: i32,
    pub z: i32,
}

impl TexKey {
    /// Blocks a side.
    pub fn span(self) -> i64 {
        i64::from(TEX) << self.lod
    }
    /// Its north-west block.
    pub fn origin(self) -> (i64, i64) {
        (i64::from(self.x) * self.span(), i64::from(self.z) * self.span())
    }
    /// The textures at `lod` covering world `x0..=x1, z0..=z1`.
    pub fn covering(source: u32, lod: u8, x0: f64, z0: f64, x1: f64, z1: f64) -> Vec<TexKey> {
        let span = (i64::from(TEX) << lod) as f64;
        let c = |v: f64| (v / span).floor() as i32;
        let mut out = Vec::new();
        for z in c(z0)..=c(z1) {
            for x in c(x0)..=c(x1) {
                out.push(TexKey { source, lod, x, z });
            }
        }
        out
    }
    /// The texture at `lod` holding chunk `cx, cz`.
    pub fn of_chunk(source: u32, lod: u8, cx: i32, cz: i32) -> TexKey {
        let shift = 3 + u32::from(lod);
        TexKey { source, lod, x: cx >> shift, z: cz >> shift }
    }
    /// Whether it is drawn from overviews.
    pub fn from_overviews(self) -> bool {
        self.lod >= BASE_LOD
    }
}

/// The level for `bpp` blocks a GUI pixel: a texel at least a GUI pixel.
pub fn fine_lod(bpp: f64) -> u8 {
    if bpp <= 1.0 { 0 } else { (bpp.log2().ceil() as i32).clamp(0, i32::from(MAX_LOD)) as u8 }
}

/// Composes `size` x `size` RGBA texels from `column`, texel `(i, j)` showing block
/// `(ox + i * step, oz + j * step)`; unknown ground clear. Slopes compare each sample with the
/// one north and west of it (`step` blocks away).
#[allow(clippy::too_many_arguments)]
pub fn compose(
    column: &mut dyn FnMut(i32, i32) -> Sample,
    palette: &[u32],
    look: Look,
    cave: Option<CaveShade>,
    ox: i32,
    oz: i32,
    step: i32,
    size: usize,
    out: &mut Vec<u8>,
) {
    out.clear();
    out.resize(size * size * 4, 0);
    // Samples with a ring round them: (size + 2)^2, row by row from (-1, -1).
    let side = size + 2;
    let mut grid: Vec<Sample> = Vec::with_capacity(side * side);
    for j in 0..side {
        let z = oz + (j as i32 - 1) * step;
        for i in 0..side {
            grid.push(column(ox + (i as i32 - 1) * step, z));
        }
    }
    let at = |i: usize, j: usize| grid[j * side + i];
    for j in 0..size {
        for i in 0..size {
            let Some((c, light)) = at(i + 1, j + 1) else {
                continue;
            };
            let floor_h = |s: Sample| s.filter(|(n, _)| cave.is_none() || n.kind == FLOOR).map(|(n, _)| n.height);
            let (n, w) = if c.color == WATER || c.depth > 0 { (None, None) } else { (floor_h(at(i + 1, j)), floor_h(at(i, j + 1))) };
            let edge = c.kind == WALL
                && [at(i + 1, j), at(i + 1, j + 2), at(i, j + 1), at(i + 2, j + 1)]
                    .iter()
                    .any(|s| s.is_some_and(|(n, _)| n.kind != WALL));
            let o = (j * size + i) * 4;
            out[o..o + 4].copy_from_slice(&shade(palette, look, cave, c, light, n, w, edge));
        }
    }
}

/// A region's overview: 64 x 64 texels, 8 blocks each, and the block light of each.
#[derive(Clone, Debug, PartialEq)]
pub struct Overview {
    pub rgba: Vec<u8>,
}

/// A region's overview from its columns (`column` as for `compose`, lighting left out: an
/// overview is drawn in daylight and lit when a far texture is composed).
pub fn overview_of(column: &mut dyn FnMut(i32, i32) -> Sample, palette: &[u32], look: Look, cave: Option<CaveShade>, region: (i32, i32)) -> Overview {
    let mut rgba = Vec::new();
    let day = Look { night: NIGHT_STEPS, ..look };
    compose(column, palette, day, cave, region.0 * 512, region.1 * 512, OVERVIEW_STEP, OVERVIEW, &mut rgba);
    Overview { rgba }
}

/// Composes far texture `key` (lod 3 and up) from the regions' overviews; texels of regions
/// with none are clear. `night` darkens it as lighting does the sky (no block light this far).
pub fn compose_far<'a>(overview: &dyn Fn((i32, i32)) -> Option<&'a Overview>, key: TexKey, night: u8, out: &mut Vec<u8>) {
    let size = TEX as usize;
    out.clear();
    out.resize(size * size * 4, 0);
    let (ox, oz) = key.origin();
    let step = 1i64 << key.lod;
    let k = if night < NIGHT_STEPS { light_factor(night, 0) } else { 1.0 };
    let mut last: Option<((i32, i32), Option<&Overview>)> = None;
    for j in 0..size {
        let bz = oz + j as i64 * step;
        for i in 0..size {
            let bx = ox + i as i64 * step;
            let r = ((bx.div_euclid(512)) as i32, (bz.div_euclid(512)) as i32);
            let ov = match last {
                Some((lr, o)) if lr == r => o,
                _ => {
                    let o = overview(r);
                    last = Some((r, o));
                    o
                }
            };
            let Some(ov) = ov else {
                continue;
            };
            let (tx, tz) = ((bx.rem_euclid(512) / 8) as usize, (bz.rem_euclid(512) / 8) as usize);
            let s = (tz * OVERVIEW + tx) * 4;
            let o = (j * size + i) * 4;
            if ov.rgba[s + 3] == 0 {
                continue;
            }
            for c in 0..3 {
                out[o + c] = (f32::from(ov.rgba[s + c]) * k).round() as u8;
            }
            out[o + 3] = 255;
        }
    }
}

/// The ids this mod gives its textures in Horizon's map slot table, kept apart from the game's
/// map ids (which are never negative).
#[derive(Debug, Default)]
pub struct TexIds {
    ids: HashMap<TexKey, i32>,
    next: i32,
}

pub const TEX_ID_BASE: i32 = -0x4C57_0000;

impl TexIds {
    pub fn id(&mut self, key: TexKey) -> i32 {
        let next = &mut self.next;
        *self.ids.entry(key).or_insert_with(|| {
            *next = (*next + 1) % 0x0100_0000;
            TEX_ID_BASE - *next
        })
    }
}

// ---------------------------------------------------------------- the region cache

/// Regions to let go so that at most `limit` stay: never the `wanted` ones, the least
/// recently used first.
pub fn to_evict(loaded: &HashMap<(i32, i32), u64>, wanted: &HashSet<(i32, i32)>, limit: usize) -> Vec<(i32, i32)> {
    if loaded.len() <= limit {
        return Vec::new();
    }
    let mut old: Vec<((i32, i32), u64)> = loaded.iter().filter(|(r, _)| !wanted.contains(*r)).map(|(r, t)| (*r, *t)).collect();
    old.sort_by_key(|&(r, t)| (t, r));
    old.into_iter().take(loaded.len() - limit).map(|(r, _)| r).collect()
}

/// The regions the world rectangle `x0..=x1, z0..=z1` touches.
pub fn regions_in(x0: f64, z0: f64, x1: f64, z1: f64) -> Vec<(i32, i32)> {
    let r = |v: f64| (v.floor() as i64).div_euclid(512) as i32;
    let mut out = Vec::new();
    for z in r(z0)..=r(z1) {
        for x in r(x0)..=r(x1) {
            out.push((x, z));
        }
    }
    out
}

// ---------------------------------------------------------------- LWX1: what each column is

// `r.<x>.<z>.lwx` beside each `r.<x>.<z>.lmr` (Lumos Map ignores it): per explored chunk, per
// column, the block seen from above, its biome and the block light over it, for the cursor's
// readout and the map's lighting.
//
//   "LWX1"
//   u16 names; each: u8 length, UTF-8 (block and biome ids, `minecraft:grass_block`)
//   u32 chunks; each: u8 x, u8 z (in the region), 256 x u16 block, 256 x u16 biome,
//                     256 x u8 light (index into the names; 0xFFFF unknown)
//
// Little-endian; columns row by row (`z * 16 + x`), as the `.lmr` tiles.

pub const NO_NAME: u16 = 0xFFFF;
const LWX: &[u8; 4] = b"LWX1";

/// One chunk's columns: block and biome as indices into a name table, and block light.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extra {
    pub block: [u16; 256],
    pub biome: [u16; 256],
    pub light: [u8; 256],
}

impl Default for Extra {
    fn default() -> Self {
        Self { block: [NO_NAME; 256], biome: [NO_NAME; 256], light: [0; 256] }
    }
}

/// Names by index and back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    pub list: Vec<String>,
    index: HashMap<String, u16>,
}

impl Names {
    pub fn intern(&mut self, name: &str) -> u16 {
        if let Some(&i) = self.index.get(name) {
            return i;
        }
        if self.list.len() >= usize::from(NO_NAME) {
            return NO_NAME;
        }
        let i = self.list.len() as u16;
        self.list.push(name.to_owned());
        self.index.insert(name.to_owned(), i);
        i
    }
    pub fn get(&self, i: u16) -> Option<&str> {
        self.list.get(usize::from(i)).map(String::as_str)
    }
}

/// A region's side file from its chunks (`chunk` in the world, its columns) and the names the
/// indices are in.
pub fn encode_extras(chunks: &[((i32, i32), &Extra)], names: &Names) -> Vec<u8> {
    let mut local = Names::default();
    let mut remap = |i: u16| if i == NO_NAME { NO_NAME } else { names.get(i).map_or(NO_NAME, |n| local.intern(n)) };
    let mapped: Vec<((i32, i32), Extra)> = chunks
        .iter()
        .map(|(k, e)| (*k, Extra { block: e.block.map(&mut remap), biome: e.biome.map(&mut remap), light: e.light }))
        .collect();
    let mut out = LWX.to_vec();
    out.extend_from_slice(&(local.list.len() as u16).to_le_bytes());
    for n in &local.list {
        let b = &n.as_bytes()[..n.len().min(255)];
        out.push(b.len() as u8);
        out.extend_from_slice(b);
    }
    out.extend_from_slice(&(mapped.len() as u32).to_le_bytes());
    for (k, e) in &mapped {
        out.push(k.0.rem_euclid(REGION) as u8);
        out.push(k.1.rem_euclid(REGION) as u8);
        for v in e.block.iter().chain(e.biome.iter()) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&e.light);
    }
    out
}

/// The chunks of a side file of `region`, their indices moved into `names`; None when it
/// isn't one.
pub fn decode_extras(region: (i32, i32), bytes: &[u8], names: &mut Names) -> Option<Vec<((i32, i32), Extra)>> {
    if bytes.len() < 10 || &bytes[..4] != LWX {
        return None;
    }
    let mut at = 4;
    let count = usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]));
    at += 2;
    let mut local = Vec::with_capacity(count);
    for _ in 0..count {
        let len = usize::from(*bytes.get(at)?);
        let name = std::str::from_utf8(bytes.get(at + 1..at + 1 + len)?).ok()?;
        local.push(names.intern(name));
        at += 1 + len;
    }
    let chunks = u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize;
    at += 4;
    const ONE: usize = 2 + 512 + 512 + 256;
    if chunks > (REGION * REGION) as usize || bytes.len() != at + chunks * ONE {
        return None;
    }
    let map = |v: u16| if v == NO_NAME { NO_NAME } else { local.get(usize::from(v)).copied().unwrap_or(NO_NAME) };
    let mut out = Vec::with_capacity(chunks);
    for c in bytes[at..].chunks_exact(ONE) {
        let (lx, lz) = (i32::from(c[0]), i32::from(c[1]));
        if lx >= REGION || lz >= REGION {
            return None;
        }
        let mut e = Extra::default();
        for i in 0..256 {
            e.block[i] = map(u16::from_le_bytes([c[2 + i * 2], c[3 + i * 2]]));
            e.biome[i] = map(u16::from_le_bytes([c[514 + i * 2], c[515 + i * 2]]));
        }
        e.light.copy_from_slice(&c[1026..1282]);
        out.push(((region.0 * REGION + lx, region.1 * REGION + lz), e));
    }
    Some(out)
}

/// A side file written over `old` (the file on disk, if any): `new`'s chunks win, `old`'s
/// others stay.
pub fn merge_extras(region: (i32, i32), old: Option<&[u8]>, new: &[u8]) -> Vec<u8> {
    let mut names = Names::default();
    let mut all: HashMap<(i32, i32), Extra> = old.and_then(|b| decode_extras(region, b, &mut names)).unwrap_or_default().into_iter().collect();
    for (k, e) in decode_extras(region, new, &mut names).unwrap_or_default() {
        all.insert(k, e);
    }
    let mut keys: Vec<&(i32, i32)> = all.keys().collect();
    keys.sort();
    let chunks: Vec<((i32, i32), &Extra)> = keys.into_iter().map(|k| (*k, &all[k])).collect();
    encode_extras(&chunks, &names)
}

/// A region file (Lumos Map's `LMR`) written over `old`: `new`'s tiles win, `old`'s others
/// stay; `band` as `new` has it (a cave layer's) or None.
pub fn merge_region(region: (i32, i32), old: Option<&[u8]>, new: &[u8]) -> Vec<u8> {
    let band = region_band(new);
    let mut store = Store { band, ..Store::default() };
    if let Some(old) = old
        && region_band(old) == band
        && let Some(tiles) = decode_region(region, old)
    {
        store.tiles.extend(tiles);
    }
    if let Some(tiles) = decode_region(region, new) {
        store.tiles.extend(tiles);
    }
    store.encode_region(region)
}

// ---------------------------------------------------------------- waypoints on the map

/// Whether waypoint `w` passes the map's filter (`FILTERS`), `set` being the current set.
pub fn passes(filter: usize, w: &Waypoint, set: &str, visible: bool) -> bool {
    match filter {
        0 => visible,
        1 => visible && w.set == set,
        2 => w.death && w.shown,
        _ => !w.shown,
    }
}

/// Whether a waypoint's name shows: pointed at always; "Zoomed in" within 2 blocks a pixel.
pub fn shows_name(mode: usize, hovered: bool, bpp: f64) -> bool {
    hovered || mode == 2 || (mode == 1 && bpp <= 2.0)
}

/// A row of the right-click menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    WaypointHere,
    TeleportHere,
    CentreHere,
    Edit,
    Toggle,
    Delete,
    TeleportTo,
    Share,
    Close,
}

impl MenuItem {
    pub fn label(self, shown: bool) -> &'static str {
        match self {
            MenuItem::WaypointHere => "Create waypoint here",
            MenuItem::TeleportHere => "Teleport here",
            MenuItem::CentreHere => "Centre here",
            MenuItem::Edit => "Edit waypoint",
            MenuItem::Toggle if shown => "Disable waypoint",
            MenuItem::Toggle => "Enable waypoint",
            MenuItem::Delete => "Delete waypoint",
            MenuItem::TeleportTo => "Teleport to it",
            MenuItem::Share => "Share in chat",
            MenuItem::Close => "Close",
        }
    }
}

/// The right-click menu's rows: a waypoint's own over one, else the spot's; teleports only
/// where the player may use /tp (a waypoint's only in the dimension played), sharing only when
/// switched on.
pub fn menu_items(on_waypoint: bool, may_teleport: bool, same_dim: bool, share: bool) -> Vec<MenuItem> {
    let mut out = Vec::new();
    if on_waypoint {
        out.extend([MenuItem::Edit, MenuItem::Toggle, MenuItem::Delete]);
        if may_teleport && same_dim {
            out.push(MenuItem::TeleportTo);
        }
        if share {
            out.push(MenuItem::Share);
        }
    } else {
        out.push(MenuItem::WaypointHere);
        if may_teleport && same_dim {
            out.push(MenuItem::TeleportHere);
        }
        out.push(MenuItem::CentreHere);
    }
    out.push(MenuItem::Close);
    out
}

/// The menu's rows on screen: each `[x, y, w, h]`, kept inside a `w` x `h` screen.
pub fn menu_rects(at: [f32; 2], rows: usize, w: f32, h: f32) -> Vec<[f32; 4]> {
    let (rw, rh) = (120.0, 14.0);
    let x = at[0].min(w - rw - 2.0).max(0.0);
    let y = at[1].min(h - rh * rows as f32 - 2.0).max(0.0);
    (0..rows).map(|i| [x, y + i as f32 * rh, rw, rh]).collect()
}

/// How far round a spot (blocks) a known floor gives its height.
pub const SPOT_NEAR: i32 = 16;

/// The Y to stand at on block `x, z`: on the floor known there, else over the highest floor
/// known near, else `fallback`.
pub fn spot_y(floor: impl Fn(i32, i32) -> Option<i32>, x: i32, z: i32, fallback: i32) -> i32 {
    if let Some(h) = floor(x, z) {
        return h + 1;
    }
    let mut best: Option<i32> = None;
    for dz in -SPOT_NEAR..=SPOT_NEAR {
        for dx in -SPOT_NEAR..=SPOT_NEAR {
            if let Some(h) = floor(x + dx, z + dz) {
                best = Some(best.map_or(h, |b| b.max(h)));
            }
        }
    }
    best.map_or(fallback, |h| h + 1)
}

/// The teleport command to block `b`: onto its top, in its middle.
pub fn tp_command(b: [i32; 3]) -> String {
    format!("tp @s {} {} {}", f64::from(b[0]) + 0.5, b[1], f64::from(b[2]) + 0.5)
}

/// An arrow of `size` at `at` pointing along the game's yaw on a north-up map: its four
/// corners, tip first.
pub fn arrow(at: [f32; 2], angle: f32, size: f32) -> [[f32; 2]; 4] {
    let (s, c) = angle.to_radians().sin_cos();
    let dir = [-s, c];
    let side = [c, s];
    let pt = |f: f32, r: f32| [at[0] + (dir[0] * f + side[0] * r) * size, at[1] + (dir[1] * f + side[1] * r) * size];
    [pt(1.0, 0.0), pt(-0.7, 0.7), pt(-0.35, 0.0), pt(-0.7, -0.7)]
}

/// The waypoint editor's text boxes, as Lumos Map's (name, symbol, x, y, z, colour hex).
// (Form is below, with the copied waypoint code.)

// ---------------------------------------------------------------- sub-worlds: asking

/// The sub-world a spawn belongs to without changing anything: its index, or None (a new
/// world, or the current one's first spawn).
pub fn spawn_match(subs: &SubWorlds, spawn: [i32; 2]) -> Option<usize> {
    let near = |s: &Option<[i32; 2]>| s.is_some_and(|s| (s[0] - spawn[0]).abs() <= SPAWN_SLOP && (s[1] - spawn[1]).abs() <= SPAWN_SLOP);
    if subs.list[subs.current].1.is_none() {
        return Some(subs.current);
    }
    subs.list.iter().position(|w| near(&w.1))
}

/// The map-selection screen's choice: an existing map, or a new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Map(usize),
    New,
}

/// What asking settles to for a world whose spawn is `spawn`: the map it matches, else a new
/// one; and whether to ask (`ask` mode, and the answer isn't the map in use).
pub fn suggestion(subs: &SubWorlds, spawn: [i32; 2], ask: bool) -> (Choice, bool) {
    let c = spawn_match(subs, spawn).map_or(Choice::New, Choice::Map);
    (c, ask && c != Choice::Map(subs.current))
}

/// A choice taken: the sub-worlds after it (a new map made current, or one chosen, taking the
/// spawn if it had none).
pub fn choose(subs: &mut SubWorlds, choice: Choice, spawn: Option<[i32; 2]>) {
    match choice {
        Choice::New => subs.add(spawn),
        Choice::Map(i) if i < subs.list.len() => {
            subs.current = i;
            if subs.list[i].1.is_none() {
                subs.list[i].1 = spawn;
            }
        }
        Choice::Map(_) => {}
    }
}

// ---------------------------------------------------------------- dimensions

/// The game's dimensions, in the order the map steps through them.
pub const DIMENSIONS: [&str; 3] = [OVERWORLD, NETHER, "minecraft:the_end"];

/// The dimension after `now` among the game's and `extra` (ones with saved maps).
pub fn next_dimension(now: &str, extra: &[String]) -> String {
    let mut all: Vec<String> = DIMENSIONS.iter().map(|d| (*d).to_owned()).collect();
    for e in extra {
        if !all.contains(e) {
            all.push(e.clone());
        }
    }
    let i = all.iter().position(|d| d == now).map_or(0, |i| (i + 1) % all.len());
    all[i].clone()
}

/// A dimension's id from its folder name (`minecraft_the_nether`), for the saved ones.
pub fn dimension_of_folder(folder: &str) -> Option<String> {
    let known = DIMENSIONS.iter().find(|d| folder_name(d) == folder);
    if let Some(d) = known {
        return Some((*d).to_owned());
    }
    let (ns, path) = folder.split_once('_')?;
    (!ns.is_empty() && !path.is_empty()).then(|| format!("{ns}:{path}"))
}

// ---------------------------------------------------------------- the screen's state

/// What the map screen shows over the map.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Page {
    #[default]
    Map,
    /// The waypoint list beside the map.
    List { scroll: usize },
    /// Editing waypoint `index` (or a new one, `draft`).
    Edit { index: usize, form: Form, draft: Option<Box<Waypoint>> },
    /// Deleting waypoint `index`, once confirmed.
    Confirm { index: usize },
    /// Which map this world is (the map selection).
    Maps { choice: Choice },
    /// The settings, page `tab`.
    Settings { tab: usize },
    /// The keys and the mouse.
    Controls,
}

impl Default for Choice {
    fn default() -> Self {
        Choice::New
    }
}

impl Page {
    pub fn typing(&self) -> bool {
        matches!(self, Page::Edit { form, .. } if form.focus.is_some())
    }
}

/// A right-click menu: where, the block, the waypoint under it.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub at: [f32; 2],
    pub block: [i32; 3],
    pub waypoint: Option<usize>,
}

/// The map screen between frames.
#[derive(Clone, Debug, Default)]
pub struct MapScreen {
    pub view: MapView,
    pub page: Page,
    /// A left press on the map: where, and whether it moved enough to be a drag.
    pub press: Option<([f32; 2], bool)>,
    pub follow: bool,
    pub menu: Option<Menu>,
    /// The layer chosen by hand (None: as cave mode says).
    pub layer: Option<Layer>,
    /// The dimension shown (another one's saved map), None: the one played.
    pub dim: Option<String>,
    /// The bars and buttons hidden (F1).
    pub hide_ui: bool,
}

impl MapScreen {
    /// A drag of `delta` while pressed: pans once it is past the slop (and stops following).
    pub fn drag(&mut self, mouse: [f32; 2], delta: [f32; 2]) {
        let Some((at, dragged)) = &mut self.press else {
            return;
        };
        if !*dragged {
            let moved = ((mouse[0] - at[0]).powi(2) + (mouse[1] - at[1]).powi(2)).sqrt();
            if moved < CLICK_SLOP {
                return;
            }
            *dragged = true;
        }
        self.follow = false;
        self.view.pan(delta);
    }

    /// The press let go: whether it was a click.
    pub fn release(&mut self) -> bool {
        matches!(self.press.take(), Some((_, false)))
    }
}

/// Rows of the waypoint list a `height` GUI shows.
pub fn list_rows(height: f32) -> usize {
    (((height - 90.0) / 16.0).floor() as usize).max(1)
}

// ---------------------------------------------------------------- per thread state

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

pub fn settings(get: impl Fn(&str) -> Option<String>) -> Settings {
    SETTINGS.with_borrow_mut(|s| *s.get_or_insert_with(|| Settings::read(get)))
}

pub fn set_settings(new: Settings) {
    SETTINGS.with_borrow_mut(|s| *s = Some(new));
}

// ================================================================ shared with Lumos Map
//
// Copied from `lumos_map/runtime.rs` (Lumos Map 3.1.0) so the two mods read and write the same
// files without either needing the other to build: block colours, tiles and their `LMR` region
// files, biome blending, the PNG writer, waypoints and their file, sub-worlds, footsteps and
// the waypoint editor's boxes.

// ---------------------------------------------------------------- terrain colours

/// What tints a block's top: none, or a biome's grass, foliage, dry foliage or water colour, or
/// a fixed colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tint {
    #[default]
    None,
    Grass,
    Foliage,
    DryFoliage,
    Water,
    Fixed(u32),
}

/// A block state's colour from above: its top texture's average (0xRRGGBB) and its tint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BlockColour {
    pub rgb: u32,
    pub tint: Tint,
    /// Some colour was found (a clear texture leaves the map colour in its place).
    pub known: bool,
}

/// The average colour of an RGBA texture, its clear pixels left out: the top square only (an
/// animated texture is a strip of frames). None when almost every pixel is clear.
pub fn average(width: u32, height: u32, rgba: &[u8]) -> Option<u32> {
    let side = width.min(height) as usize;
    let w = width as usize;
    if side == 0 || rgba.len() < w * side * 4 {
        return None;
    }
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for y in 0..side {
        for x in 0..side {
            let o = (y * w + x) * 4;
            let a = u64::from(rgba[o + 3]);
            if a < 16 {
                continue;
            }
            r += u64::from(rgba[o]) * a;
            g += u64::from(rgba[o + 1]) * a;
            b += u64::from(rgba[o + 2]) * a;
            n += a;
        }
    }
    // Fewer than a tenth of the pixels: glass and the like.
    if n == 0 || n < (side * side) as u64 * 255 / 10 {
        return None;
    }
    Some(((r / n) as u32) << 16 | ((g / n) as u32) << 8 | (b / n) as u32)
}

/// `rgb` multiplied by `tint`, channel by channel.
pub fn tinted(rgb: u32, tint: u32) -> u32 {
    let ch = |s: u32| ((rgb >> s & 255) * (tint >> s & 255) / 255) << s;
    ch(16) | ch(8) | ch(0)
}

/// The tints of a biome: grass, foliage, dry foliage, water.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BiomeTints {
    pub grass: u32,
    pub foliage: u32,
    pub dry_foliage: u32,
    pub water: u32,
}

/// A block's colour from above in a biome.
pub fn block_rgb(c: &BlockColour, biome: &BiomeTints) -> u32 {
    match c.tint {
        Tint::None => c.rgb,
        Tint::Grass => tinted(c.rgb, biome.grass),
        Tint::Foliage => tinted(c.rgb, biome.foliage),
        Tint::DryFoliage => tinted(c.rgb, biome.dry_foliage),
        Tint::Water => tinted(c.rgb, biome.water),
        Tint::Fixed(t) => tinted(c.rgb, t),
    }
}

// ---------------------------------------------------------------- tiles

/// Map colour ids with a meaning of their own (`MapColor`).
pub const WATER: u8 = 12;
/// `MapColor.COLOR_BLACK`: the map colour a cave view's drop keeps (it has no block of its own).
pub const BLACK: u8 = 29;

/// What a cave view's column is: a floor (or the surface), rock at the start Y (a wall), or
/// open down past the depth (a drop).
pub const FLOOR: u8 = 0;
pub const WALL: u8 = 1;
pub const DEEP: u8 = 2;
/// A drop's colour: darker than walls.
pub const DEEP_RGB: u32 = 0x10_1014;
/// Rock in a cave view (columns solid at the start Y), and its lighter edge along open space.
pub const WALL_RGB: u32 = 0x2A_2A2E;
pub const WALL_EDGE_RGB: u32 = 0x4A_4A50;
/// `MapColor.FIRE`: lava's, which stays bright in caves.
pub const FIRE: u8 = 4;

/// A cave slice's place, for its depth shading: its start Y and depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaveShade {
    pub start: i32,
    pub depth: i32,
}

/// How much of its colour a cave floor `below` blocks under the start keeps: full right under
/// you, about half at the depth limit; with legible caves, never under 70%.
pub fn depth_factor(below: i32, depth: i32, legible: bool) -> f32 {
    let f = (below.max(0) as f32 / depth.max(1) as f32).min(1.0);
    let dark = 0.55 * f;
    1.0 - if legible { dark.min(0.3) } else { dark }
}

/// One chunk of the map: per column (`z * 16 + x`) its map colour id (0: nothing known), its
/// colour in the game's look (0xRRGGBB, 0: not known, use the map colour), the height of that
/// block and the water over it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub color: [u8; 256],
    pub rgb: [u32; 256],
    pub height: [i16; 256],
    pub depth: [u8; 256],
    /// `FLOOR`, `WALL` or `DEEP` (a cave view's; the surface's are all floors).
    pub kind: [u8; 256],
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            color: [0; 256],
            rgb: [0; 256],
            height: [0; 256],
            depth: [0; 256],
            kind: [FLOOR; 256],
        }
    }
}

/// One column as the images read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Col {
    pub color: u8,
    pub rgb: u32,
    pub height: i16,
    pub depth: u8,
    pub kind: u8,
}

impl Tile {
    #[inline]
    pub fn col(&self, x: i32, z: i32) -> Option<Col> {
        let i = ((z & 15) * 16 + (x & 15)) as usize;
        (self.color[i] != 0).then(|| Col {
            color: self.color[i],
            rgb: self.rgb[i],
            height: self.height[i],
            depth: self.depth[i],
            kind: self.kind[i],
        })
    }
}

/// A region file holds 32 x 32 chunks.
pub const REGION: i32 = 32;
const MAGIC_V1: &[u8; 4] = b"LMR1";
const MAGIC: &[u8; 4] = b"LMR2";
/// A saved cave layer's region: `LMR2`'s tiles with their column kinds, and the layer's band.
const MAGIC_CAVE: &[u8; 4] = b"LMR3";

/// Cave layers are kept per 16-block band of their top Y, so walking a few blocks up or down
/// stays in one layer.
pub fn band_of(top: i32) -> i32 {
    top.div_euclid(16)
}

/// A saved cave layer's folder under its dimension's: `caves/<band>`, or `caves/full` for the
/// full cave map.
pub fn cave_folder(band: Option<i32>) -> String {
    band.map_or_else(|| "caves/full".to_owned(), |b| format!("caves/{b}"))
}

pub fn region_of(chunk: (i32, i32)) -> (i32, i32) {
    (chunk.0.div_euclid(REGION), chunk.1.div_euclid(REGION))
}

pub fn region_file(region: (i32, i32)) -> String {
    format!("r.{}.{}.lmr", region.0, region.1)
}

/// The region of a region file's name.
pub fn region_from_file(name: &str) -> Option<(i32, i32)> {
    let mid = name.strip_prefix("r.")?.strip_suffix(".lmr")?;
    let (x, z) = mid.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

/// The explored map of one dimension.
#[derive(Default)]
pub struct Store {
    pub tiles: HashMap<(i32, i32), Tile>,
    /// Regions changed since they were saved.
    pub dirty: HashSet<(i32, i32)>,
    /// Goes up with each change, so a composed image knows it is old.
    pub revision: u64,
    /// Chunks changed since the maps last looked (their cells are drawn again).
    pub changed: Vec<(i32, i32)>,
    /// Goes up when many tiles changed at once (loaded from disk): every cell is drawn again.
    pub epoch: u64,
    /// A cave layer's band: its regions are written as `LMR3` with the band and the column
    /// kinds; None (the surface, the full cave map): `LMR2`.
    pub band: Option<i32>,
}

impl Store {
    /// Keeps `tile` for `chunk`; whether it changed anything.
    pub fn put(&mut self, chunk: (i32, i32), tile: Tile) -> bool {
        if tile.color.iter().all(|c| *c == 0) || self.tiles.get(&chunk) == Some(&tile) {
            return false;
        }
        self.tiles.insert(chunk, tile);
        self.dirty.insert(region_of(chunk));
        self.revision += 1;
        self.changed.push(chunk);
        true
    }

    /// Tiles read from disk, where the session has none of its own (those are newer).
    pub fn merge_loaded(&mut self, loaded: Vec<((i32, i32), Tile)>) {
        let mut any = false;
        for (k, t) in loaded {
            if let std::collections::hash_map::Entry::Vacant(e) = self.tiles.entry(k) {
                e.insert(t);
                any = true;
            }
        }
        if any {
            self.revision += 1;
            self.epoch += 1;
        }
    }

    /// The tiles of every region but those `keep` keeps and those not saved yet let go; whether
    /// any went (an epoch, so the cells showing them are drawn again).
    pub fn let_go(&mut self, keep: impl Fn((i32, i32)) -> bool) -> bool {
        let n = self.tiles.len();
        let dirty = &self.dirty;
        self.tiles.retain(|k, _| {
            let r = region_of(*k);
            keep(r) || dirty.contains(&r)
        });
        let gone = self.tiles.len() != n;
        if gone {
            self.revision += 1;
            self.epoch += 1;
        }
        gone
    }

    /// The column at block `x, z`.
    #[inline]
    pub fn column(&self, x: i32, z: i32) -> Option<Col> {
        self.tiles.get(&(x >> 4, z >> 4))?.col(x, z)
    }

    /// Region `region` as its file's bytes.
    pub fn encode_region(&self, region: (i32, i32)) -> Vec<u8> {
        let keys: Vec<&(i32, i32)> = self
            .tiles
            .keys()
            .filter(|k| region_of(**k) == region)
            .collect();
        self.encode_keys(keys)
    }

    /// The regions changed since the last save (`dirty`, emptied) as their files' bytes, the
    /// tiles sorted into their regions in one pass (not one pass over every tile per region).
    pub fn encode_dirty(&mut self) -> Vec<((i32, i32), Vec<u8>)> {
        let dirty = std::mem::take(&mut self.dirty);
        let mut by_region: HashMap<(i32, i32), Vec<&(i32, i32)>> = dirty.iter().map(|r| (*r, Vec::new())).collect();
        for k in self.tiles.keys() {
            if let Some(keys) = by_region.get_mut(&region_of(*k)) {
                keys.push(k);
            }
        }
        by_region.into_iter().map(|(r, keys)| (r, self.encode_keys(keys))).collect()
    }

    fn encode_keys(&self, mut keys: Vec<&(i32, i32)>) -> Vec<u8> {
        keys.sort();
        let mut out = Vec::with_capacity(12 + keys.len() * ONE_V3);
        match self.band {
            Some(band) => {
                out.extend_from_slice(MAGIC_CAVE);
                out.extend_from_slice(&band.to_le_bytes());
            }
            None => out.extend_from_slice(MAGIC),
        }
        out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
        for k in keys {
            let t = &self.tiles[k];
            out.push(k.0.rem_euclid(REGION) as u8);
            out.push(k.1.rem_euclid(REGION) as u8);
            out.extend_from_slice(&t.color);
            for h in t.height {
                out.extend_from_slice(&h.to_le_bytes());
            }
            out.extend_from_slice(&t.depth);
            for c in t.rgb {
                out.extend_from_slice(&c.to_le_bytes()[..3]);
            }
            if self.band.is_some() {
                out.extend_from_slice(&t.kind);
            }
        }
        out
    }
}

const ONE_V1: usize = 2 + 256 + 512 + 256;
const ONE_V2: usize = ONE_V1 + 768;
const ONE_V3: usize = ONE_V2 + 256;

/// The band a cave layer's region file was saved for (None: not a cave layer's).
pub fn region_band(bytes: &[u8]) -> Option<i32> {
    (bytes.len() >= 8 && &bytes[..4] == MAGIC_CAVE).then(|| i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))
}

/// The regions within `radius` of any of `centres` (blocks): the ones a region cache keeps.
pub fn regions_near(centres: &[(f64, f64)], radius: i32) -> HashSet<(i32, i32)> {
    let mut out = HashSet::new();
    for &(x, z) in centres {
        let r = region_of(((x.floor() as i32) >> 4, (z.floor() as i32) >> 4));
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                out.insert((r.0 + dx, r.1 + dz));
            }
        }
    }
    out
}

/// The tiles of a region file (a cave layer's, this version's, or the first's without
/// colours); None when it isn't one.
pub fn decode_region(region: (i32, i32), bytes: &[u8]) -> Option<Vec<((i32, i32), Tile)>> {
    if bytes.len() < 8 {
        return None;
    }
    let (one, head) = match &bytes[..4] {
        m if m == MAGIC_CAVE => (ONE_V3, 12),
        m if m == MAGIC => (ONE_V2, 8),
        m if m == MAGIC_V1 => (ONE_V1, 8),
        _ => return None,
    };
    if bytes.len() < head {
        return None;
    }
    let count = u32::from_le_bytes(bytes[head - 4..head].try_into().ok()?) as usize;
    if count > (REGION * REGION) as usize || bytes.len() != head + count * one {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for c in bytes[head..].chunks_exact(one) {
        let (lx, lz) = (i32::from(c[0]), i32::from(c[1]));
        if lx >= REGION || lz >= REGION {
            return None;
        }
        let mut t = Tile::default();
        t.color.copy_from_slice(&c[2..258]);
        for (i, h) in c[258..770].chunks_exact(2).enumerate() {
            t.height[i] = i16::from_le_bytes([h[0], h[1]]);
        }
        t.depth.copy_from_slice(&c[770..1026]);
        if one >= ONE_V2 {
            for (i, p) in c[1026..1794].chunks_exact(3).enumerate() {
                t.rgb[i] = u32::from(p[0]) | u32::from(p[1]) << 8 | u32::from(p[2]) << 16;
            }
        }
        if one == ONE_V3 {
            t.kind.copy_from_slice(&c[1794..2050]);
        }
        out.push(((region.0 * REGION + lx, region.1 * REGION + lz), t));
    }
    Some(out)
}

// ---------------------------------------------------------------- biome blending, water

/// Biome tints of a chunk and the quarts around it: 6 x 6 cells of 4 x 4 blocks, row by row,
/// cell (1, 1) being the chunk's own north-west quart.
pub type TintGrid = [BiomeTints; 36];

fn lerp_rgb(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 255) as f32, ((b >> s) & 255) as f32);
        ((x + (y - x) * t).round().clamp(0.0, 255.0) as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// The tints at block `lx, lz` of the chunk (0..16), blended between the centres of the four
/// nearest quarts: no seam where one biome meets another, as the game's own biome blend.
pub fn blend_tints(grid: &TintGrid, lx: i32, lz: i32) -> BiomeTints {
    // Quart centres sit at 2, 6, 10, 14 (and -2, 18 in the neighbours); grid index = quart + 1.
    let at = |l: i32| {
        let f = (l as f32 + 0.5 - 2.0) / 4.0 + 1.0;
        let i = (f.floor() as i32).clamp(0, 4);
        (i as usize, (f - i as f32).clamp(0.0, 1.0))
    };
    let (ix, tx) = at(lx);
    let (iz, tz) = at(lz);
    let g = |x: usize, z: usize| grid[z * 6 + x];
    let mix = |f: fn(&BiomeTints) -> u32| {
        let top = lerp_rgb(f(&g(ix, iz)), f(&g(ix + 1, iz)), tx);
        let bottom = lerp_rgb(f(&g(ix, iz + 1)), f(&g(ix + 1, iz + 1)), tx);
        lerp_rgb(top, bottom, tz)
    };
    BiomeTints {
        grass: mix(|t| t.grass),
        foliage: mix(|t| t.foliage),
        dry_foliage: mix(|t| t.dry_foliage),
        water: mix(|t| t.water),
    }
}

/// Water seen from above: the ground under it showing through, less the deeper it is, and the
/// whole darker with depth. `depth` is the water blocks under the top one.
pub fn water_over(ground: u32, water: u32, depth: u8) -> u32 {
    let d = f32::from(depth.min(30));
    let cover = (0.42 + d * 0.07).min(0.9);
    let mixed = lerp_rgb(ground, water, cover);
    let dark = 1.0 - (d * 0.014).min(0.32);
    let ch = |s: u32| ((((mixed >> s) & 255) as f32 * dark).round() as u32) << s;
    ch(16) | ch(8) | ch(0)
}

// ---------------------------------------------------------------- PNG

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

/// A PNG of `width` x `height` RGBA pixels, stored without compression (no library needed).
pub fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in rgba.chunks_exact(width as usize * 4).take(height as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    if blocks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

// ---------------------------------------------------------------- waypoints

/// The colours a waypoint can have: name and 0xRRGGBB.
pub const COLOURS: [(&str, u32); 10] = [
    ("Red", 0xFF5555),
    ("Orange", 0xFFAA33),
    ("Yellow", 0xFFFF55),
    ("Green", 0x55FF55),
    ("Aqua", 0x55FFFF),
    ("Blue", 0x5577FF),
    ("Purple", 0xBB66FF),
    ("Pink", 0xFF77CC),
    ("White", 0xFFFFFF),
    ("Grey", 0xAAAAAA),
];
pub const DEATH_NAME: &str = "Death";
pub const NAME_MAX: usize = 32;
pub const DEFAULT_SET: &str = "Default";

#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub name: String,
    pub pos: [i32; 3],
    /// `minecraft:overworld`.
    pub dim: String,
    /// Index in `COLOURS`.
    pub colour: usize,
    pub death: bool,
    /// Off: kept, but shown nowhere for now.
    pub shown: bool,
    /// Its set's name.
    pub set: String,
    /// When it was made, seconds since 1970.
    pub created: u64,
    /// Its marker's text: a symbol, or empty for its initials.
    pub symbol: String,
    /// A colour of its own (0xRRGGBB), over `colour`.
    pub custom: Option<u32>,
    /// Removed when you leave the world.
    pub temporary: bool,
    /// Kept for every world and server, not only this one.
    pub global: bool,
    /// Where it shows (each also needs the matching setting on).
    pub on_screen: bool,
    pub on_minimap: bool,
    pub on_map: bool,
    /// Its name on screen always, not only when looked at.
    pub always_name: bool,
    /// No height of its own: it is at your height (distances are flat).
    pub two_d: bool,
    /// Removed when you reach it.
    pub vanish: bool,
}

impl Default for Waypoint {
    fn default() -> Self {
        Self {
            name: String::new(),
            pos: [0; 3],
            dim: OVERWORLD.into(),
            colour: 0,
            death: false,
            shown: true,
            set: DEFAULT_SET.into(),
            created: 0,
            symbol: String::new(),
            custom: None,
            temporary: false,
            global: false,
            on_screen: true,
            on_minimap: true,
            on_map: true,
            always_name: false,
            two_d: false,
            vanish: false,
        }
    }
}

/// `w` given the colour and symbol new waypoints take (`NEW_COLOURS`, `SYMBOLS`).
pub fn style_new(w: &mut Waypoint, colour: usize, symbol: usize) {
    if colour > 0 {
        w.colour = (colour - 1).min(COLOURS.len() - 1);
    }
    if symbol > 0 {
        w.symbol = SYMBOLS.get(symbol).map_or_else(String::new, |s| (*s).to_owned());
    }
}

/// The block a position is in: each coordinate floored, so -0.3 is block -1 (not 0). A new
/// waypoint's default place is `block_at` the player's feet.
pub fn block_at(p: [f64; 3]) -> [i32; 3] {
    p.map(|c| c.floor() as i32)
}

/// A colour typed as hex (`#3fa`, `33FFAA`), as 0xRRGGBB.
pub fn parse_hex(text: &str) -> Option<u32> {
    let t = text.trim().trim_start_matches('#');
    let full: String = match t.len() {
        3 => t.chars().flat_map(|c| [c, c]).collect(),
        6 => t.to_owned(),
        _ => return None,
    };
    u32::from_str_radix(&full, 16).ok()
}

impl Waypoint {
    pub fn rgb(&self) -> u32 {
        self.custom.unwrap_or(COLOURS[self.colour.min(COLOURS.len() - 1)].1)
    }

    /// Its flags as the file keeps them: a letter for each that isn't its default.
    fn flags(&self) -> String {
        let mut f = String::new();
        for (on, c) in [
            (self.temporary, 'T'),
            (self.global, 'G'),
            (!self.on_screen, 'S'),
            (!self.on_minimap, 'M'),
            (!self.on_map, 'W'),
            (self.always_name, 'N'),
            (self.two_d, '2'),
            (self.vanish, 'V'),
        ] {
            if on {
                f.push(c);
            }
        }
        if f.is_empty() { "-".into() } else { f }
    }

    fn read_flags(&mut self, f: &str) {
        self.temporary = f.contains('T');
        self.global = f.contains('G');
        self.on_screen = !f.contains('S');
        self.on_minimap = !f.contains('M');
        self.on_map = !f.contains('W');
        self.always_name = f.contains('N');
        self.two_d = f.contains('2');
        self.vanish = f.contains('V');
    }

    /// Where it is in `dim` as seen from height `y`: a 2D waypoint is at that height.
    pub fn place_at(&self, dim: &str, convert: bool, y: f64) -> Option<[f64; 3]> {
        let mut p = self.place_in(dim, convert)?;
        if self.two_d {
            p[1] = y;
        }
        Some(p)
    }
    /// The block centre at its foot.
    pub fn centre(&self) -> [f64; 3] {
        [
            f64::from(self.pos[0]) + 0.5,
            f64::from(self.pos[1]),
            f64::from(self.pos[2]) + 0.5,
        ]
    }
    /// Its marker's text: its symbol, else its name's first letter.
    pub fn mark(&self) -> String {
        if self.symbol.is_empty() { self.letter() } else { self.symbol.clone() }
    }

    /// Its name's first letter or digit, in capitals ("?" for a name with none).
    pub fn letter(&self) -> String {
        self.name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect())
    }
    /// Where it is in `dim`: its own place, or the converted one between the Overworld and the
    /// Nether when `convert`; None in another dimension.
    pub fn place_in(&self, dim: &str, convert: bool) -> Option<[f64; 3]> {
        if self.dim == dim {
            return Some(self.centre());
        }
        if !convert {
            return None;
        }
        let p = converted(self.pos, &self.dim, dim)?;
        Some([f64::from(p[0]) + 0.5, f64::from(p[1]), f64::from(p[2]) + 0.5])
    }
}

pub const OVERWORLD: &str = "minecraft:overworld";
pub const NETHER: &str = "minecraft:the_nether";

/// `pos` of `from` as the same place in `to`: an eighth of the way in the Nether, eight times
/// in the Overworld; None between other dimensions.
pub fn converted(pos: [i32; 3], from: &str, to: &str) -> Option<[i32; 3]> {
    match (from, to) {
        (OVERWORLD, NETHER) => Some([pos[0].div_euclid(8), pos[1], pos[2].div_euclid(8)]),
        (NETHER, OVERWORLD) => Some([pos[0] * 8, pos[1], pos[2] * 8]),
        _ => None,
    }
}

/// A name as kept: no tabs or line breaks, at most `NAME_MAX` characters.
pub fn clean_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(NAME_MAX)
        .collect();
    s.trim().to_owned()
}

/// A set: its name and whether its waypoints show.
#[derive(Clone, Debug, PartialEq)]
pub struct WaySet {
    pub name: String,
    pub shown: bool,
}

/// A world's waypoints, their sets and the set new ones go in.
#[derive(Clone, Debug, PartialEq)]
pub struct Book {
    pub list: Vec<Waypoint>,
    pub sets: Vec<WaySet>,
    /// Index in `sets`.
    pub current: usize,
    /// Every set shows for now, its own switch aside (the render-all-sets key; not saved).
    pub all_sets: bool,
}

impl Default for Book {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            sets: vec![WaySet { name: DEFAULT_SET.into(), shown: true }],
            current: 0,
            all_sets: false,
        }
    }
}

impl Book {
    /// The file: the sets, then one line per waypoint (name, x, y, z, dimension, colour,
    /// death, shown, set, created), tab separated.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# Lumos Map waypoints 2\n");
        for (i, s) in self.sets.iter().enumerate() {
            out.push_str(&format!(
                "#set\t{}\t{}\t{}\n",
                clean_name(&s.name),
                s.shown,
                i == self.current
            ));
        }
        for w in &self.list {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                clean_name(&w.name),
                w.pos[0],
                w.pos[1],
                w.pos[2],
                w.dim,
                w.colour,
                w.death,
                w.shown,
                clean_name(&w.set),
                w.created,
                clean_name(&w.symbol),
                w.custom.map_or_else(|| "-".to_owned(), |c| format!("{c:06X}")),
                w.flags()
            ));
        }
        out
    }

    /// A file of this version or the first (whose lines have no set or time); lines that don't
    /// read are skipped.
    pub fn from_text(text: &str) -> Self {
        let mut book = Book { list: Vec::new(), sets: Vec::new(), current: 0, all_sets: false };
        for l in text.lines() {
            let f: Vec<&str> = l.split('\t').collect();
            if f.first() == Some(&"#set") && f.len() >= 4 {
                let name = clean_name(f[1]);
                if name.is_empty() || book.set_index(&name).is_some() {
                    continue;
                }
                if f[3].trim() == "true" {
                    book.current = book.sets.len();
                }
                book.sets.push(WaySet { name, shown: f[2].trim() != "false" });
                continue;
            }
            if l.starts_with('#') || f.len() < 8 {
                continue;
            }
            let n = |s: &str| s.trim().parse::<i32>().ok();
            let (Some(x), Some(y), Some(z), Some(colour)) =
                (n(f[1]), n(f[2]), n(f[3]), f[5].trim().parse::<usize>().ok())
            else {
                continue;
            };
            let set = f.get(8).map(|s| clean_name(s)).filter(|s| !s.is_empty());
            let mut w = Waypoint {
                name: clean_name(f[0]),
                pos: [x, y, z],
                dim: f[4].trim().to_owned(),
                colour: colour.min(COLOURS.len() - 1),
                death: f[6].trim() == "true",
                shown: f[7].trim() != "false",
                set: set.unwrap_or_else(|| DEFAULT_SET.to_owned()),
                created: f.get(9).and_then(|s| s.trim().parse().ok()).unwrap_or(0),
                symbol: f.get(10).map(|s| clean_name(s).chars().take(2).collect()).unwrap_or_default(),
                custom: f.get(11).and_then(|c| parse_hex(c)),
                ..Waypoint::default()
            };
            w.read_flags(f.get(12).copied().unwrap_or(""));
            book.list.push(w);
        }
        // Every waypoint's set is listed.
        for i in 0..book.list.len() {
            let s = book.list[i].set.clone();
            if book.set_index(&s).is_none() {
                book.sets.push(WaySet { name: s, shown: true });
            }
        }
        if book.sets.is_empty() {
            book.sets.push(WaySet { name: DEFAULT_SET.into(), shown: true });
        }
        book.current = book.current.min(book.sets.len() - 1);
        book
    }

    pub fn set_index(&self, name: &str) -> Option<usize> {
        self.sets.iter().position(|s| s.name == name)
    }

    /// Whether `w` shows: it and its set on.
    pub fn visible(&self, w: &Waypoint) -> bool {
        w.shown && (self.all_sets || self.set_index(&w.set).is_none_or(|i| self.sets[i].shown))
    }

    /// Waypoints reached at `at` in `dim` (within 4 blocks, flat for 2D ones) that go when
    /// reached: those marked so, and (`deaths`) death points made at least `age` seconds before
    /// `now`. Removed; their names.
    pub fn remove_reached(&mut self, at: [f64; 3], dim: &str, deaths: bool, now: u64, age: u64) -> Vec<String> {
        let mut gone = Vec::new();
        self.list.retain(|w| {
            let goes = (w.vanish || (deaths && w.death && now.saturating_sub(w.created) >= age))
                && w.dim == dim
                && {
                    let c = w.centre();
                    let dy = if w.two_d { 0.0 } else { at[1] - c[1] };
                    ((at[0] - c[0]).powi(2) + dy.powi(2) + (at[2] - c[2]).powi(2)).sqrt() <= 4.0
                };
            if goes {
                gone.push(w.name.clone());
            }
            !goes
        });
        gone
    }

    pub fn current_set(&self) -> &str {
        &self.sets[self.current.min(self.sets.len() - 1)].name
    }

    /// "Waypoint N", the first N not taken.
    pub fn next_name(&self) -> String {
        (1..)
            .map(|n| format!("Waypoint {n}"))
            .find(|name| !self.list.iter().any(|w| &w.name == name))
            .unwrap_or_default()
    }

    /// A waypoint `add` would add at `pos` in `dim` (the next name and colour, the current
    /// set), not in the book yet: the "New waypoint" box edits it before it is added.
    pub fn draft(&self, pos: [i32; 3], dim: &str, now: u64) -> Waypoint {
        let colour = self.list.iter().filter(|w| !w.death).count() % (COLOURS.len() - 2);
        Waypoint {
            name: self.next_name(),
            pos,
            dim: dim.to_owned(),
            colour,
            death: false,
            shown: true,
            set: self.current_set().to_owned(),
            created: now,
            ..Waypoint::default()
        }
    }

    /// `w` put in the book (an empty name gets the next "Waypoint N", a set that is gone the
    /// current one); returns its index.
    pub fn insert(&mut self, mut w: Waypoint) -> usize {
        w.name = clean_name(&w.name);
        if w.name.is_empty() {
            w.name = self.next_name();
        }
        if self.set_index(&w.set).is_none() {
            w.set = self.current_set().to_owned();
        }
        self.list.push(w);
        self.list.len() - 1
    }

    /// A waypoint added at `pos` in `dim` to the current set; returns its index.
    pub fn add(&mut self, pos: [i32; 3], dim: &str, now: u64) -> usize {
        let w = self.draft(pos, dim, now);
        self.insert(w)
    }

    /// Where you died: kept with the last `keep` deaths, the oldest going first. The newest is
    /// "Death", older ones "Death 2", "Death 3"...
    pub fn add_death(&mut self, pos: [i32; 3], dim: &str, keep: usize, now: u64) {
        self.list.push(Waypoint {
            name: DEATH_NAME.to_owned(),
            pos,
            dim: dim.to_owned(),
            colour: 0,
            death: true,
            shown: true,
            set: self.current_set().to_owned(),
            created: now,
            ..Waypoint::default()
        });
        let mut deaths: Vec<usize> = (0..self.list.len()).filter(|&i| self.list[i].death).collect();
        // Newest first.
        deaths.sort_by_key(|&i| std::cmp::Reverse((self.list[i].created, i)));
        let drop: HashSet<usize> = deaths.iter().skip(keep.max(1)).copied().collect();
        for (n, &i) in deaths.iter().take(keep.max(1)).enumerate() {
            self.list[i].name = if n == 0 {
                DEATH_NAME.to_owned()
            } else {
                format!("{DEATH_NAME} {}", n + 1)
            };
        }
        let mut i = 0;
        self.list.retain(|_| {
            let keep = !drop.contains(&i);
            i += 1;
            keep
        });
    }

    /// Waypoint `i` given the colour and symbol new waypoints take (`NEW_COLOURS`, `SYMBOLS`).
    pub fn style_new(&mut self, i: usize, colour: usize, symbol: usize) {
        if let Some(w) = self.list.get_mut(i) {
            style_new(w, colour, symbol);
        }
    }

    /// A new set, made current; false when the name is empty or taken.
    pub fn add_set(&mut self, name: &str) -> bool {
        let name = clean_name(name);
        if name.is_empty() || self.set_index(&name).is_some() {
            return false;
        }
        self.sets.push(WaySet { name, shown: true });
        self.current = self.sets.len() - 1;
        true
    }

    /// The set after the current one becomes current.
    pub fn next_set(&mut self) {
        self.current = (self.current + 1) % self.sets.len();
    }

    /// Deletes the current set with its waypoints (the last set stays).
    pub fn remove_set(&mut self) {
        if self.sets.len() <= 1 {
            return;
        }
        let name = self.sets.remove(self.current).name;
        self.list.retain(|w| w.set != name);
        self.current = self.current.min(self.sets.len() - 1);
    }

    /// The list as shown: in `sort` order, only those whose name, set or symbol has `query` in it
    /// (any case), and only set `set` when given.
    pub fn listed(&self, sort: usize, from: Option<[f64; 3]>, query: &str, set: Option<&str>) -> Vec<usize> {
        let q = query.trim().to_lowercase();
        self.order(sort, from)
            .into_iter()
            .filter(|&i| {
                let w = &self.list[i];
                set.is_none_or(|s| w.set == s)
                    && (q.is_empty()
                        || w.name.to_lowercase().contains(&q)
                        || w.set.to_lowercase().contains(&q)
                        || w.symbol.to_lowercase() == q)
            })
            .collect()
    }

    /// Waypoints `which` removed (indices into `list`).
    pub fn remove(&mut self, which: &[usize]) {
        let drop: HashSet<usize> = which.iter().copied().collect();
        let mut i = 0;
        self.list.retain(|_| {
            let keep = !drop.contains(&i);
            i += 1;
            keep
        });
    }

    /// Waypoints `which` moved into set `set`.
    pub fn move_to_set(&mut self, which: &[usize], set: &str) {
        for &i in which {
            if let Some(w) = self.list.get_mut(i) {
                w.set = set.to_owned();
            }
        }
    }

    /// A copy of waypoint `i` beside it, named "<name> copy"; its index.
    pub fn duplicate(&mut self, i: usize, now: u64) -> Option<usize> {
        let mut w = self.list.get(i)?.clone();
        w.name = clean_name(&format!("{} copy", w.name));
        w.created = now;
        w.death = false;
        self.list.push(w);
        Some(self.list.len() - 1)
    }

    /// The current set renamed (its waypoints follow); false when the name is empty or taken.
    pub fn rename_set(&mut self, name: &str) -> bool {
        let name = clean_name(name);
        if name.is_empty() || self.set_index(&name).is_some() {
            return false;
        }
        let c = self.current.min(self.sets.len() - 1);
        let old = std::mem::replace(&mut self.sets[c].name, name.clone());
        for w in self.list.iter_mut().filter(|w| w.set == old) {
            w.set = name.clone();
        }
        true
    }

    /// The world's own waypoints and the ones kept for every world, split for their files.
    pub fn split_global(&self) -> (Book, Book) {
        let mut own = self.clone();
        own.list.retain(|w| !w.global);
        let mut all = Book::default();
        all.list = self.list.iter().filter(|w| w.global).cloned().collect();
        for w in &all.list {
            if all.set_index(&w.set).is_none() {
                all.sets.push(WaySet { name: w.set.clone(), shown: true });
            }
        }
        (own, all)
    }

    /// The waypoints kept for every world added to this world's (each marked global).
    pub fn merge_global(&mut self, global: Book) {
        for mut w in global.list {
            w.global = true;
            if self.set_index(&w.set).is_none() {
                self.sets.push(WaySet { name: w.set.clone(), shown: true });
            }
            self.list.push(w);
        }
    }

    /// Temporary waypoints dropped (leaving the world).
    pub fn drop_temporary(&mut self) -> bool {
        let n = self.list.len();
        self.list.retain(|w| !w.temporary);
        n != self.list.len()
    }

    /// Waypoints `which` shown (or hidden for now) at once; whether any changed.
    pub fn set_shown(&mut self, which: &[usize], shown: bool) -> bool {
        let mut any = false;
        for &i in which {
            if let Some(w) = self.list.get_mut(i)
                && w.shown != shown
            {
                w.shown = shown;
                any = true;
            }
        }
        any
    }

    /// The list's order: by name, by distance from `from`, or newest first.
    pub fn order(&self, sort: usize, from: Option<[f64; 3]>) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.list.len()).collect();
        match sort {
            0 => idx.sort_by_key(|&i| self.list[i].name.to_lowercase()),
            1 => {
                let d = |i: usize| from.map_or(0.0, |p| distance(p, self.list[i].centre()));
                idx.sort_by(|&a, &b| d(a).partial_cmp(&d(b)).unwrap_or(std::cmp::Ordering::Equal));
            }
            _ => idx.sort_by_key(|&i| std::cmp::Reverse((self.list[i].created, i))),
        }
        idx
    }
}

/// The text a waypoint is shared as in chat: `Name: x y z (Dimension)`.
pub fn share_text(w: &Waypoint) -> String {
    format!(
        "{}: {} {} {} ({})",
        w.name,
        w.pos[0],
        w.pos[1],
        w.pos[2],
        pretty_id(&w.dim)
    )
}

/// The first coordinates in a chat line: three whole numbers close together (`12 64 -30`,
/// `x: 12, y: 64, z: -30`, `1.5 / 64 / -2`).
pub fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// "12 m", "1.4 km".
pub fn format_distance(d: f64) -> String {
    if d < 1000.0 {
        format!("{} m", d.round() as i64)
    } else {
        format!("{:.1} km", d / 1000.0)
    }
}

/// A distance in the chosen units (`UNITS`): "12 m" / "1.4 km", or "12 blocks".
pub fn distance_text(d: f64, units: usize) -> String {
    if units == 1 {
        format!("{} blocks", d.round() as i64)
    } else {
        format_distance(d)
    }
}

/// Where a point `rel` (from the camera, in blocks) lands on a `w` x `h` screen through the
/// camera's `view` and `projection` (column-major): its screen point and whether it is in
/// front of the camera. Behind, the point is mirrored, so it still says which way to turn.
pub fn folder_name(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if out.is_empty() || out.chars().all(|c| c == '.') {
        out = "_".to_owned();
    }
    out
}

/// `minecraft:the_nether` as "The Nether"; `minecraft:dark_forest` as "Dark Forest".
pub fn pretty_id(id: &str) -> String {
    let name = id.rsplit(':').next().unwrap_or(id);
    name.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------- sub-worlds

/// The worlds kept apart on one server (a lobby and a survival world on one address): each its
/// folder name and the spawn point that tells it, once seen.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SubWorlds {
    pub list: Vec<(String, Option<[i32; 2]>)>,
    pub current: usize,
}

/// How far a spawn may move and still be the same world.
pub const SPAWN_SLOP: i32 = 48;

impl SubWorlds {
    pub fn from_text(text: &str) -> Self {
        let mut s = SubWorlds::default();
        for l in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let f: Vec<&str> = l.split('\t').collect();
            let name = folder_name(f[0].trim());
            let spawn = (|| Some([f.get(1)?.trim().parse().ok()?, f.get(2)?.trim().parse().ok()?]))();
            if f.get(3).is_some_and(|c| c.trim() == "current") {
                s.current = s.list.len();
            }
            s.list.push((name, spawn));
        }
        if s.list.is_empty() {
            s.list.push(("world-1".into(), None));
        }
        s.current = s.current.min(s.list.len() - 1);
        s
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# Lumos Map sub-worlds: folder, spawn x, spawn z, current\n");
        for (i, (name, spawn)) in self.list.iter().enumerate() {
            let (x, z) = spawn.map_or((String::new(), String::new()), |s| {
                (s[0].to_string(), s[1].to_string())
            });
            let cur = if i == self.current { "current" } else { "" };
            out.push_str(&format!("{name}\t{x}\t{z}\t{cur}\n"));
        }
        out
    }

    pub fn current_name(&self) -> &str {
        &self.list[self.current].0
    }

    /// The world whose spawn this is: the current one if it has none yet (it takes this one), one
    /// that has it, or a new one. Whether the current world changed.
    pub fn follow_spawn(&mut self, spawn: [i32; 2]) -> bool {
        let near = |s: &Option<[i32; 2]>| {
            s.is_some_and(|s| {
                (s[0] - spawn[0]).abs() <= SPAWN_SLOP && (s[1] - spawn[1]).abs() <= SPAWN_SLOP
            })
        };
        if near(&self.list[self.current].1) {
            return false;
        }
        if self.list[self.current].1.is_none() {
            self.list[self.current].1 = Some(spawn);
            return false;
        }
        if let Some(i) = self.list.iter().position(|w| near(&w.1)) {
            self.current = i;
        } else {
            self.add(Some(spawn));
        }
        true
    }

    /// A new world, made current.
    pub fn add(&mut self, spawn: Option<[i32; 2]>) {
        let n = (1..)
            .find(|n| !self.list.iter().any(|w| w.0 == format!("world-{n}")))
            .unwrap_or(1);
        self.list.push((format!("world-{n}"), spawn));
        self.current = self.list.len() - 1;
    }

    pub fn next(&mut self) {
        self.current = (self.current + 1) % self.list.len();
    }
}

// ---------------------------------------------------------------- footsteps

/// Your recent path in a dimension: a point every 2 blocks moved, the last `FOOTSTEPS` kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Footsteps {
    pub points: std::collections::VecDeque<[i32; 2]>,
}

pub const FOOTSTEPS: usize = 1000;

impl Footsteps {
    /// You stand at `x, z`: a new point once 2 blocks from the last; whether one was added.
    pub fn step(&mut self, x: f64, z: f64) -> bool {
        let p = [x.floor() as i32, z.floor() as i32];
        if let Some(last) = self.points.back()
            && (p[0] - last[0]).pow(2) + (p[1] - last[1]).pow(2) < 4
        {
            return false;
        }
        self.points.push_back(p);
        while self.points.len() > FOOTSTEPS {
            self.points.pop_front();
        }
        true
    }

    /// The file: one `x z` a line, oldest first.
    pub fn to_text(&self) -> String {
        self.points.iter().map(|p| format!("{} {}\n", p[0], p[1])).collect()
    }

    pub fn from_text(text: &str) -> Self {
        let mut out = Footsteps::default();
        for l in text.lines() {
            let mut it = l.split_whitespace().map(|v| v.parse::<i32>());
            if let (Some(Ok(x)), Some(Ok(z))) = (it.next(), it.next()) {
                out.points.push_back([x, z]);
            }
        }
        while out.points.len() > FOOTSTEPS {
            out.points.pop_front();
        }
        out
    }
}

/// How far a press may move and still be a click, in GUI pixels.

/// The waypoint editor's text boxes: name, symbol, x, y, z, colour (hex), and the one typed in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form {
    pub fields: [String; 6],
    pub focus: Option<usize>,
}

pub const F_NAME: usize = 0;
pub const F_SYMBOL: usize = 1;
pub const F_X: usize = 2;
pub const F_HEX: usize = 5;

impl Form {
    pub fn of(w: &Waypoint) -> Self {
        Self {
            fields: [
                w.name.clone(),
                w.symbol.clone(),
                w.pos[0].to_string(),
                w.pos[1].to_string(),
                w.pos[2].to_string(),
                w.custom.map_or_else(String::new, |c| format!("{c:06X}")),
            ],
            focus: Some(F_NAME),
        }
    }

    /// The boxes of a waypoint not made yet: its name box empty, typed into.
    pub fn of_new(w: &Waypoint) -> Self {
        let mut f = Self::of(w);
        f.fields[F_NAME].clear();
        f
    }

    /// A character typed into the focused box, if that box takes it.
    pub fn type_char(&mut self, c: char) {
        let Some(f) = self.focus else { return };
        let text = &mut self.fields[f];
        let ok = match f {
            F_NAME => !c.is_control() && text.chars().count() < NAME_MAX,
            F_SYMBOL => !c.is_control() && !c.is_whitespace() && text.chars().count() < 2,
            F_HEX => c.is_ascii_hexdigit() && text.len() < 6,
            _ => (c.is_ascii_digit() || (c == '-' && text.is_empty())) && text.len() < 9,
        };
        if ok {
            text.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if let Some(f) = self.focus {
            self.fields[f].pop();
        }
    }

    /// The boxes put on `w`: what doesn't read (an empty name, a bad number) is left as it was.
    pub fn apply(&self, w: &mut Waypoint) {
        let name = clean_name(&self.fields[F_NAME]);
        if !name.is_empty() {
            w.name = name;
        }
        w.symbol = clean_name(&self.fields[F_SYMBOL]).chars().take(2).collect();
        for k in 0..3 {
            if let Ok(v) = self.fields[F_X + k].trim().parse::<i32>() {
                w.pos[k] = v;
            }
        }
        w.custom = parse_hex(&self.fields[F_HEX]);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_pages() {
        assert_eq!(Settings::read(|_| None), Settings::default());
        let mut s = Settings::default();
        for b in 0..Settings::COUNT {
            s.click(b);
        }
        let stored = s.stored();
        let read = Settings::read(|k| stored.iter().find(|p| p.0 == k).map(|p| p.1.clone()));
        assert_eq!(read, s);
        let d = Settings::default();
        let total: usize = (0..PAGES.len()).map(|p| d.page(p).len()).sum();
        assert_eq!(total, Settings::COUNT);
        assert!(d.page(KEYS_PAGE).is_empty());
        for p in 0..KEYS_PAGE {
            assert!(!d.page(p).is_empty(), "page {p} has settings");
        }
        let mut b = Settings::default();
        let speed = b.labels().iter().position(|l| l.starts_with("Mapping speed")).unwrap();
        b.click_dir(speed, true);
        assert_eq!(b.speed, 0);
        assert_eq!(Settings::read(|k| (k == "slopes").then(|| "9".into())).slopes, 1);
        assert_eq!(d.zooms(), (0.125, 32.0));
        assert_eq!(d.cache_regions(), 32);
        assert!(d.budget_us(true) > d.budget_us(false));
    }

    #[test]
    fn the_minimaps_cave_settings_are_taken_over() {
        let d = Settings::default();
        let get = |k: &str| match k {
            "cave_depth" => Some("4".to_owned()),
            "cave_legible" => Some("false".to_owned()),
            _ => None,
        };
        let s = d.with_minimap_caves(get);
        assert_eq!((s.depth_blocks(), s.cave_legible), (64, false));
        let own = Settings { cave_shared: false, ..d }.with_minimap_caves(get);
        assert_eq!(own.depth_blocks(), 30);
        assert_eq!(d.with_minimap_caves(|_| Some("99".into())).cave_depth, d.cave_depth);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let rect = [0.0, 0.0, 400.0, 200.0];
        let mut v = MapView { x: 100.0, z: -50.0, ..MapView::default() };
        let p = [300.0, 40.0];
        let before = v.world_at(rect, p);
        v.zoom_at(rect, p, 2.0, 0.125, 32.0, false);
        assert!((v.bpp - 0.5).abs() < 1e-9);
        let after = v.world_at(rect, p);
        assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6);
        // Smooth: eases there over frames, the point staying put.
        let mut s = MapView::default();
        s.zoom_at(rect, p, -2.0, 0.125, 32.0, true);
        let w0 = s.world_at(rect, p);
        let mut frames = 0;
        while s.step(rect, 1.0 / 60.0) {
            frames += 1;
            let w = s.world_at(rect, p);
            assert!((w.0 - w0.0).abs() < 1e-6 && (w.1 - w0.1).abs() < 1e-6);
            assert!(frames < 300);
        }
        assert!((s.bpp - 2.0).abs() < 1e-9 && frames > 3);
        // Clamped.
        s.zoom_at(rect, p, -100.0, 0.125, 32.0, false);
        assert_eq!(s.bpp, 32.0);
    }

    #[test]
    fn panning_and_screen_points_agree() {
        let rect = [10.0, 20.0, 300.0, 100.0];
        let mut v = MapView { x: 5.0, z: 5.0, bpp: 2.0, target: 2.0, anchor: None };
        let sp = v.screen_of(rect, 25.0, -15.0);
        let (x, z) = v.world_at(rect, sp);
        assert!((x - 25.0).abs() < 1e-4 && (z + 15.0).abs() < 1e-4);
        v.pan([10.0, -5.0]);
        assert_eq!((v.x, v.z), (-15.0, 15.0));
        key_pan(&mut v, rect, (1, 0));
        assert_eq!(v.x, -15.0 + 30.0 * 2.0);
    }

    #[test]
    fn readouts() {
        assert_eq!(cursor_line(-1, 530, Some(64)), "X -1 Y 64 Z 530   Chunk -1 33 (15 2)   Region -1 1");
        assert_eq!(other_dim_line(OVERWORLD, -9, 16).as_deref(), Some("Nether -2 2"));
        assert_eq!(other_dim_line(NETHER, 3, -1).as_deref(), Some("Overworld 24 -8"));
        assert_eq!(other_dim_line("minecraft:the_end", 3, -1), None);
        assert_eq!(convert_view(80.0, -16.0, OVERWORLD, NETHER, true), (10.0, -2.0));
        assert_eq!(convert_view(10.0, -2.0, NETHER, OVERWORLD, true), (80.0, -16.0));
        assert_eq!(convert_view(10.0, -2.0, NETHER, OVERWORLD, false), (10.0, -2.0));
        assert_eq!(zoom_text(0.25), "x4.0");
        assert_eq!(zoom_text(2.0), "1:2.0");
    }

    #[test]
    fn layers_step_and_follow_caves() {
        assert_eq!(layer_step(Layer::Surface, 40, true, -64, 320, true), Layer::Band(2));
        assert_eq!(layer_step(Layer::Band(2), 40, true, -64, 320, true), Layer::Band(1));
        assert_eq!(layer_step(Layer::Band(-4), 40, true, -64, 320, true), Layer::Band(-4));
        assert_eq!(layer_step(Layer::Band(19), 40, false, -64, 320, true), Layer::Surface);
        assert_eq!(layer_step(Layer::Band(3), 40, false, -64, 320, true), Layer::Band(4));
        assert_eq!(layer_step(Layer::Surface, 40, false, -64, 320, true), Layer::Full);
        assert_eq!(layer_step(Layer::Full, 40, false, -64, 320, true), Layer::Surface);
        assert_eq!(layer_step(Layer::Surface, 40, false, 0, 256, false), Layer::Surface);
        assert_eq!(band_top(-1), -1);
        assert_eq!(band_top(2), 47);
        assert_eq!(shown_layer(None, true, true, -20), Layer::Band(-2));
        assert_eq!(shown_layer(None, false, true, -20), Layer::Surface);
        assert_eq!(shown_layer(Some(Layer::Full), true, true, -20), Layer::Full);
        assert_eq!(Layer::Band(-2).folder().as_deref(), Some("caves/-2"));
        assert_eq!(Layer::Full.folder().as_deref(), Some("caves/full"));
        let mut sw = CaveSwitch::default();
        assert!(!sw.step(true, 0));
        assert!(!sw.step(true, 10));
        assert!(sw.step(true, 20));
        assert!(sw.step(false, 21), "holds while the new answer is young");
        assert!(!roofed(&[Some(70), None]) && roofed(&[Some(70), Some(72)]) && !roofed(&[]));
    }

    fn col(height: i16) -> Col {
        Col { color: 1, rgb: 0x808080, height, depth: 0, kind: FLOOR }
    }

    #[test]
    fn shading_modes() {
        let pal = [0u32; 64];
        let day = Look { textured: true, slopes: 1, night: NIGHT_STEPS, ..Look::default() };
        let flat = shade(&pal, Look { slopes: 0, ..day }, None, col(64), 0, Some(60), Some(60), false);
        assert_eq!(flat, [128, 128, 128, 255]);
        let gentle = shade(&pal, day, None, col(64), 0, Some(60), Some(60), false);
        let strong = shade(&pal, Look { slopes: 2, ..day }, None, col(64), 0, Some(60), Some(60), false);
        assert!(gentle[0] > 128 && strong[0] > gentle[0]);
        // Night, and a torch's light.
        let night = Look { night: 0, ..day };
        let dark = shade(&pal, night, None, col(64), 0, None, None, false);
        let lit = shade(&pal, night, None, col(64), 14, None, None, false);
        assert!(dark[0] < 50 && lit[0] > 110);
        // Caves: rock, its edge, drops, depth.
        let cave = Some(CaveShade { start: 40, depth: 30 });
        let wall = Col { kind: WALL, ..col(40) };
        assert_eq!(shade(&pal, day, cave, wall, 0, None, None, false)[0], (WALL_RGB >> 16) as u8);
        assert_eq!(shade(&pal, day, cave, wall, 0, None, None, true)[0], (WALL_EDGE_RGB >> 16) as u8);
        assert_eq!(shade(&pal, day, cave, Col { kind: DEEP, ..col(10) }, 0, None, None, false)[0], (DEEP_RGB >> 16) as u8);
        let deep_floor = shade(&pal, Look { legible: false, ..day }, cave, col(10), 0, None, None, false);
        assert!(deep_floor[0] < 70);
        // Vanilla colours: the palette's.
        let mut p2 = pal;
        p2[1] = 0x112233;
        assert_eq!(shade(&p2, Look { textured: false, slopes: 0, ..day }, None, col(64), 0, None, None, false), [0x11, 0x22, 0x33, 255]);
        assert_eq!(night_step(1.0), NIGHT_STEPS);
        assert!(sky_light(18000, 0.0, 0.0) < 0.1 && sky_light(6000, 0.0, 0.0) > 0.9);
    }

    #[test]
    fn textures_cover_the_view_at_their_level() {
        assert_eq!(fine_lod(0.25), 0);
        assert_eq!(fine_lod(1.0), 0);
        assert_eq!(fine_lod(1.5), 1);
        assert_eq!(fine_lod(8.0), 3);
        assert_eq!(fine_lod(1e9), MAX_LOD);
        let keys = TexKey::covering(7, 0, -1.0, 0.0, 127.0, 128.0);
        assert_eq!(keys.len(), 4);
        assert!(keys.contains(&TexKey { source: 7, lod: 0, x: -1, z: 0 }));
        assert_eq!(TexKey::of_chunk(0, 0, 9, -1), TexKey { source: 0, lod: 0, x: 1, z: -1 });
        assert_eq!(TexKey::of_chunk(0, 3, 64, 63), TexKey { source: 0, lod: 3, x: 1, z: 0 });
        assert_eq!(TexKey { source: 0, lod: 2, x: -1, z: 1 }.origin(), (-512, 512));
        let mut ids = TexIds::default();
        let a = ids.id(keys[0]);
        assert!(a < 0 && ids.id(keys[0]) == a && ids.id(keys[1]) != a);
    }

    #[test]
    fn composing_leaves_unknown_ground_clear() {
        let pal = [0u32; 64];
        let look = Look { textured: true, slopes: 1, night: NIGHT_STEPS, ..Look::default() };
        let mut column = |x: i32, z: i32| (x >= 0 && z >= 0).then(|| (col(64 + (x % 3) as i16), 0));
        let mut out = Vec::new();
        compose(&mut column, &pal, look, None, -2, -2, 1, 4, &mut out);
        assert_eq!(out.len(), 64);
        assert_eq!(out[3], 0, "(-2, -2) unknown");
        assert_eq!(out[(2 * 4 + 2) * 4 + 3], 255, "(0, 0) known");
        // An overview, and a far texture made from it.
        let ov = overview_of(&mut column, &pal, look, None, (0, 0));
        assert_eq!(ov.rgba.len(), OVERVIEW * OVERVIEW * 4);
        let far_key = TexKey { source: 0, lod: 3, x: 0, z: 0 };
        let get = |r: (i32, i32)| (r == (0, 0)).then_some(&ov);
        compose_far(&get, far_key, NIGHT_STEPS, &mut out);
        assert_eq!(out[3], 255);
        assert_eq!(out[(64 * 128) * 4 + 3], 0, "the region south of it has no overview");
        let lit = out[0];
        compose_far(&get, far_key, 0, &mut out);
        assert!(out[0] < lit, "darker at night");
    }

    #[test]
    fn the_cache_lets_the_oldest_unwanted_regions_go() {
        let loaded: HashMap<(i32, i32), u64> = [((0, 0), 5), ((1, 0), 1), ((2, 0), 3), ((3, 0), 9)].into();
        let wanted: HashSet<(i32, i32)> = [(1, 0)].into();
        assert_eq!(to_evict(&loaded, &wanted, 2), vec![(2, 0), (0, 0)]);
        assert!(to_evict(&loaded, &wanted, 4).is_empty());
        assert_eq!(regions_in(-1.0, 0.0, 512.0, 10.0), vec![(-1, 0), (0, 0), (1, 0)]);
    }

    fn tile(c: u8) -> Tile {
        let mut t = Tile::default();
        t.color = [c; 256];
        t.rgb = [0x123456; 256];
        t.height = [70; 256];
        t
    }

    #[test]
    fn region_files_merge_with_what_is_on_disk() {
        let mut a = Store::default();
        a.put((0, 0), tile(1));
        a.put((1, 0), tile(1));
        let old = a.encode_region((0, 0));
        let mut b = Store::default();
        b.put((1, 0), tile(2));
        b.put((2, 5), tile(3));
        let new = b.encode_region((0, 0));
        let merged = merge_region((0, 0), Some(&old), &new);
        let tiles: HashMap<(i32, i32), Tile> = decode_region((0, 0), &merged).unwrap().into_iter().collect();
        assert_eq!(tiles.len(), 3);
        assert_eq!(tiles[&(0, 0)].color[0], 1);
        assert_eq!(tiles[&(1, 0)].color[0], 2, "the new tile wins");
        // A cave band's file keeps its band, and a surface file isn't merged into it.
        let mut c = Store { band: Some(-2), ..Store::default() };
        c.put((0, 0), tile(4));
        let cave = c.encode_region((0, 0));
        let m2 = merge_region((0, 0), Some(&old), &cave);
        assert_eq!(region_band(&m2), Some(-2));
        assert_eq!(decode_region((0, 0), &m2).unwrap().len(), 1);
        assert_eq!(merge_region((0, 0), None, &new), new);
    }

    #[test]
    fn side_files_round_trip_and_merge() {
        let mut names = Names::default();
        let mut e = Extra::default();
        e.block[0] = names.intern("minecraft:grass_block");
        e.biome[0] = names.intern("minecraft:plains");
        e.light[0] = 9;
        let bytes = encode_extras(&[((33, -1), &e)], &names);
        let mut other = Names::default();
        other.intern("minecraft:stone");
        let back = decode_extras((1, -1), &bytes, &mut other).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].0, (33, -1));
        assert_eq!(other.get(back[0].1.block[0]), Some("minecraft:grass_block"));
        assert_eq!(other.get(back[0].1.biome[0]), Some("minecraft:plains"));
        assert_eq!(back[0].1.light[0], 9);
        assert_eq!(back[0].1.block[1], NO_NAME);
        let mut e2 = Extra::default();
        e2.block[5] = names.intern("minecraft:sand");
        let new = encode_extras(&[((34, -1), &e2)], &names);
        let merged = merge_extras((1, -1), Some(&bytes), &new);
        let mut n3 = Names::default();
        let all = decode_extras((1, -1), &merged, &mut n3).unwrap();
        assert_eq!(all.len(), 2);
        assert!(decode_extras((0, 0), b"LMR2xxxxxx", &mut n3).is_none());
    }

    #[test]
    fn lumos_maps_waypoint_file_reads_and_writes_the_same() {
        let text = "# Lumos Map waypoints 2\n#set\tDefault\ttrue\ttrue\n#set\tMine\tfalse\tfalse\n\
Home\t10\t64\t-5\tminecraft:overworld\t3\tfalse\ttrue\tDefault\t100\t-\t-\t-\n\
Death\t1\t2\t3\tminecraft:the_nether\t0\ttrue\ttrue\tMine\t200\t-\t-\tG\n";
        let book = Book::from_text(text);
        assert_eq!(book.list.len(), 2);
        assert_eq!(book.sets.len(), 2);
        assert_eq!(Book::from_text(&book.to_text()), book);
        let w = &book.list[0];
        assert!(passes(0, w, "Default", book.visible(w)));
        assert!(passes(1, w, "Default", true) && !passes(1, w, "Mine", true));
        assert!(!passes(2, w, "Default", true) && passes(2, &book.list[1], "", false));
        let hidden = Waypoint { shown: false, ..w.clone() };
        assert!(passes(3, &hidden, "", false) && !passes(3, w, "", true));
        assert!(shows_name(0, true, 30.0) && !shows_name(1, false, 4.0) && shows_name(1, false, 1.0) && shows_name(2, false, 9.0));
        assert_eq!(share_text(w), "Home: 10 64 -5 (Overworld)");
        assert_eq!(tp_command([1, 70, -2]), "tp @s 1.5 70 -1.5");
    }

    #[test]
    fn the_menu_offers_what_is_allowed() {
        assert_eq!(menu_items(false, false, true, true), vec![MenuItem::WaypointHere, MenuItem::CentreHere, MenuItem::Close]);
        assert!(menu_items(false, true, true, true).contains(&MenuItem::TeleportHere));
        assert!(!menu_items(false, true, false, true).contains(&MenuItem::TeleportHere));
        let wp = menu_items(true, true, true, false);
        assert!(wp.contains(&MenuItem::TeleportTo) && !wp.contains(&MenuItem::Share));
        assert_eq!(MenuItem::Toggle.label(true), "Disable waypoint");
        let r = menu_rects([390.0, 190.0], 3, 400.0, 200.0);
        assert!(r[2][1] + r[2][3] <= 200.0 && r[0][0] + r[0][2] <= 400.0);
        assert_eq!(spot_y(|x, _| (x == 5).then_some(70), 0, 0, 64), 71);
        assert_eq!(spot_y(|_, _| None, 0, 0, 64), 64);
    }

    #[test]
    fn maps_of_a_server_are_suggested_and_chosen() {
        let mut subs = SubWorlds::from_text("world-1\t0\t0\tcurrent\nworld-2\t1000\t1000\t\n");
        assert_eq!(suggestion(&subs, [10, -10], true), (Choice::Map(0), false));
        assert_eq!(suggestion(&subs, [1010, 990], true), (Choice::Map(1), true));
        assert_eq!(suggestion(&subs, [1010, 990], false), (Choice::Map(1), false));
        assert_eq!(suggestion(&subs, [-5000, 0], true), (Choice::New, true));
        choose(&mut subs, Choice::New, Some([-5000, 0]));
        assert_eq!(subs.current_name(), "world-3");
        choose(&mut subs, Choice::Map(1), Some([0, 0]));
        assert_eq!(subs.list[1].1, Some([1000, 1000]), "a known spawn stays");
        let fresh = SubWorlds::from_text("");
        assert_eq!(spawn_match(&fresh, [5, 5]), Some(0));
    }

    #[test]
    fn dimensions_step_round() {
        assert_eq!(next_dimension(OVERWORLD, &[]), NETHER);
        assert_eq!(next_dimension("minecraft:the_end", &[]), OVERWORLD);
        let extra = vec!["mod:moon".to_owned()];
        assert_eq!(next_dimension("minecraft:the_end", &extra), "mod:moon");
        assert_eq!(dimension_of_folder("minecraft_the_nether").as_deref(), Some(NETHER));
        assert_eq!(dimension_of_folder("mod_moon").as_deref(), Some("mod:moon"));
        assert_eq!(dimension_of_folder("exports"), None);
    }

    #[test]
    fn png_has_its_header() {
        let p = png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]);
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&p[12..16], b"IHDR");
    }

    #[test]
    fn footsteps_and_screen_clicks() {
        let mut f = Footsteps::default();
        assert!(f.step(0.0, 0.0) && !f.step(1.0, 0.0) && f.step(2.0, 0.0));
        assert_eq!(Footsteps::from_text(&f.to_text()), f);
        let mut s = MapScreen { press: Some(([10.0, 10.0], false)), follow: true, ..MapScreen::default() };
        s.drag([11.0, 10.0], [1.0, 0.0]);
        assert!(s.follow, "inside the slop it is still a click");
        s.drag([20.0, 10.0], [9.0, 0.0]);
        assert!(!s.follow && !s.release());
        assert!(list_rows(250.0) >= 9);
    }
}
