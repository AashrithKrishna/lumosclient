// Lumos Map: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_map`; `glue.rs` reads the game
// and calls in here. It names nothing outside `std`, so it also compiles alone:
// `rustc --edition 2024 --test runtime.rs` runs the tests below.
//
// What is here: the settings; the terrain colours (texture averages, tints, shading); the
// explored map's tiles and their region files; the minimap's and the world map's images and
// geometry; waypoints with their sets, deaths, sorting and sharing; sub-worlds of a server;
// slime chunks; and a PNG writer for exporting the map.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

pub const ID: &str = "lumos_map";
pub const NAME: &str = "Map";
pub const VERSION: &str = "3.1.0";
pub const DESCRIPTION: &str = "A crisp minimap and world map in the game's own colours, \
entities as dots with faces (and players' names) while Tab is held, waypoints as on-screen markers with a \
full manager, and a world map (M) of everywhere you have been, saved per world and server.";

/// The mod's own screen id is the game's for every mod screen.
pub const SCREEN: &str = "mod";

// ---------------------------------------------------------------- settings

/// The settings' pages, on the mod's page in Options > Mods and on the map's settings screen.
pub const PAGES: [&str; 8] = ["General", "Minimap", "Caves", "World map", "Entities", "Waypoints", "Extras", "Keys"];
/// The page listing the keys (it has no settings of its own).
pub const KEYS_PAGE: usize = PAGES.len() - 1;
pub const CORNERS: [&str; 4] = ["Top left", "Top right", "Bottom left", "Bottom right"];
pub const OFFSETS: [f32; 7] = [0.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0];
pub const SIZES: [f32; 8] = [64.0, 80.0, 96.0, 112.0, 128.0, 160.0, 192.0, 224.0];
/// The minimap's zoom: blocks per GUI pixel, in steps of a square root of two. The map's own
/// texture is a block a texel, drawn with crisp (nearest) pixels at any of these.
pub const ZOOMS: [f32; 9] = [0.25, 0.35, 0.5, 0.71, 1.0, 1.41, 2.0, 2.83, 4.0];
/// How many region files (512 blocks each) round you and the world map's view stay in memory;
/// farther ones are let go once saved and read again when you come near.
pub const CACHE_RADII: [usize; 3] = [2, 4, 8];
/// How a server's maps are told apart: by the world's spawn (sub-worlds), or one map for all.
pub const MAP_MODES: [&str; 2] = ["By spawn", "Single"];
/// How the enlarged minimap (held key) zooms: as the minimap, or out by 2 or 4.
pub const ENLARGE_ZOOMS: [&str; 3] = ["Same", "Out x2", "Out x4"];
/// The light overlay marks floors whose block light is at most this (mobs spawn at 0).
pub const LIGHT_MAXES: [usize; 4] = [0, 3, 7, 11];
pub const OPACITY: [u8; 5] = [255, 217, 178, 140, 102];
pub const FRAMES: [&str; 3] = ["Rim and shadow", "Thin rim", "None"];
/// The frame's rim colour: name and 0xRRGGBB ("Dark" is the rim it always had).
pub const FRAME_COLOURS: [(&str, u32); 8] = [
    ("Dark", 0x0C_0C0E),
    ("Black", 0x00_0000),
    ("Grey", 0x80_8088),
    ("White", 0xF0_F0F0),
    ("Gold", 0xC8_A040),
    ("Red", 0xB0_3030),
    ("Green", 0x40_9040),
    ("Blue", 0x40_60C0),
];
/// The minimap's unexplored ground: name and 0xAARRGGBB.
pub const BACKGROUNDS: [(&str, u32); 4] = [
    ("Dark", 0xFF1A_1C20),
    ("Black", 0xFF00_0000),
    ("Grey", 0xFF45_474C),
    ("See-through", 0x5010_1014),
];
pub const COMPASS: [&str; 3] = ["Off", "N only", "N E S W"];
/// The cave view: Auto (a roof over you turns it on), Manual (on, from the start Y setting or
/// where you are; the cave key switches it), Off. 3.0 stored "Always" at index 1: it is Manual now.
pub const CAVE_MODES: [&str; 3] = ["Auto", "Manual", "Off"];
/// How many columns round you need a roof for Auto to turn the cave view on: 1x1, 3x3, 5x5.
pub const CAVE_ROOFS: [&str; 3] = ["1x1", "3x3", "5x5"];
/// How far below the start Y the cave view looks for a floor, in blocks.
pub const CAVE_DEPTHS: [usize; 5] = [8, 16, 30, 48, 64];
/// How far above the start Y the cave view also shows open floors (lava lakes, caves over you).
pub const CAVE_ABOVES: [usize; 4] = [0, 2, 4, 8];
/// The start Y the settings button steps through (the cave key sets any Y).
pub const CAVE_STARTS: [i32; 9] = [96, 64, 48, 32, 16, 0, -16, -32, -48];
/// How long (ticks) a new roof state must hold before the cave view switches: about a second.
pub const CAVE_HOLD: u64 = 20;
/// Entering the cave view zooms the minimap in by this many steps (each a square root of two).
pub const CAVE_ZOOMS: [&str; 3] = ["Off", "x1.41", "x2"];
/// The info lines' orders: C coordinates, B biome, F facing, L light, T time, D dimension.
pub const INFO_ORDERS: [&str; 4] = ["C B F L T D", "B C F L T D", "D C B F L T", "T C B F L D"];
pub const MAP_MINS: [&str; 4] = ["1/8", "1/4", "1/2", "1"];
pub const MAP_MAXES: [&str; 4] = ["4", "8", "16", "32"];
pub const DEATHS: [usize; 4] = [1, 3, 5, 10];
pub const SORTS: [&str; 3] = ["Name", "Distance", "Newest"];
/// Entity icons' side on the minimap, GUI pixels.
pub const ICON_SIZES: [f32; 5] = [6.0, 8.0, 10.0, 12.0, 16.0];
/// How far from you entities show on the minimap: to its edge, or within so many blocks.
pub const RANGES: [&str; 6] = ["Map edge", "16", "32", "64", "128", "256"];
/// Entities this far above or below you (blocks) fade; Auto is 8.
pub const DEPTH_FADES: [&str; 5] = ["Auto", "4", "8", "16", "32"];
/// Entities farther above or below you than this are left off.
pub const HEIGHT_LIMITS: [&str; 4] = ["Off", "16", "32", "64"];
/// The most entities drawn.
pub const DOT_LIMITS: [usize; 4] = [100, 200, 400, 1000];
/// When a waypoint marker shows its name.
pub const WP_NAMES: [&str; 3] = ["Looked at", "Always", "Never"];
pub const WP_SHAPES: [&str; 2] = ["Diamond", "Square"];
pub const WP_LABELS: [f32; 3] = [0.75, 1.0, 1.25];
pub const WP_RANGES: [&str; 6] = ["Any distance", "100", "250", "500", "1000", "5000"];
pub const UNITS: [&str; 2] = ["Metres", "Blocks"];
/// A new waypoint's colour: the next in turn, or one of `COLOURS`.
pub const NEW_COLOURS: [&str; 11] =
    ["Next in turn", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Pink", "White", "Grey"];
/// A waypoint's marker text: its name's first letter, or a symbol.
pub const SYMBOLS: [&str; 8] = ["First letter", "*", "+", "!", "#", "X", "$", "@"];
/// Name labels' size in the detail view.
pub const LABEL_SIZES: [f32; 3] = [0.5, 0.75, 1.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// The minimap shows.
    pub minimap: bool,
    /// The game's colours (textures and biome tints), or the flat map palette.
    pub textured: bool,
    /// Slopes lit and shaded.
    pub shading: bool,
    /// Index in `CAVE_MODES`: underground, the floor around you.
    pub cave: usize,
    /// Index in `CAVE_ROOFS`.
    pub cave_roof: usize,
    /// Index in `CAVE_DEPTHS`.
    pub cave_depth: usize,
    /// Index in `CAVE_ABOVES`.
    pub cave_above: usize,
    /// Manual cave view's start Y (None: where you are).
    pub cave_start: Option<i32>,
    /// Caves darken less with depth, so deep floors stay readable.
    pub cave_legible: bool,
    /// Index in `CAVE_ZOOMS`.
    pub cave_zoom: usize,
    /// Overworld waypoints shown in the Nether at their Nether place, and the other way.
    pub convert: bool,
    /// Index in `CORNERS`.
    pub corner: usize,
    /// Index in `OFFSETS`, GUI pixels from the screen's side.
    pub offset_x: usize,
    /// Index in `OFFSETS`, GUI pixels from the top or bottom.
    pub offset_y: usize,
    /// Index in `SIZES`.
    pub size: usize,
    /// Index in `ZOOMS`.
    pub zoom: usize,
    /// Round, or square.
    pub round: bool,
    /// Turns with the player (forward is up), or north is up.
    pub rotate: bool,
    /// Index in `OPACITY`.
    pub opacity: usize,
    /// The enlarge key switches the big minimap on and off (else: big while held).
    pub enlarge_toggle: bool,
    /// Index in `ENLARGE_ZOOMS`.
    pub enlarge_zoom: usize,
    /// The big minimap turns with you as the small one does (off: north up).
    pub enlarge_rotate: bool,
    /// Index in `FRAMES`.
    pub frame: usize,
    /// Index in `BACKGROUNDS`: the ground not explored yet.
    pub background: usize,
    /// Index in `COMPASS`.
    pub compass: usize,
    /// Index in `FRAME_COLOURS`.
    pub frame_colour: usize,
    /// The minimap's size follows the GUI scale (`auto_size`), instead of `size`.
    pub auto_size: bool,
    /// The minimap's ground darker the lower it is below you.
    pub depth_shade: bool,
    /// The minimap's surface darker at night (and in rain), as the sky is.
    pub night: bool,
    /// The map palette's grass, leaves and water in their biome's colours.
    pub palette_biomes: bool,
    /// Stained glass seen through: the ground under it, tinted (chunks are mapped again).
    pub glass: bool,
    /// The chunk grid on the minimap.
    pub mini_grid: bool,
    /// Slime chunks on the minimap (where the seed is known).
    pub mini_slime: bool,
    /// The light overlay on the minimap: dark floors near you marked.
    pub light_overlay: bool,
    /// Index in `LIGHT_MAXES`.
    pub light_max: usize,
    /// Info line: X Y Z.
    pub coords: bool,
    /// Info line: the biome.
    pub biome: bool,
    /// Info line: the way you face.
    pub facing: bool,
    /// Info line: the light level.
    pub light: bool,
    /// Info line: the clock.
    pub time: bool,
    /// Info line: the dimension.
    pub dimension: bool,
    /// Info line: the weather.
    pub weather: bool,
    /// Info line: the chunk you are in and your place in it.
    pub chunk: bool,
    /// Info line: the same place in the other of the Overworld and the Nether.
    pub other_coords: bool,
    /// Info line: the camera's yaw and pitch.
    pub angles: bool,
    /// Info line: the time on your computer's clock.
    pub real_time: bool,
    /// Clocks as 12-hour (3:05 PM), else 24-hour.
    pub clock_12h: bool,
    /// Index in `INFO_ORDERS`.
    pub info_order: usize,
    /// Index in `MAP_MINS`.
    pub map_min: usize,
    /// Index in `MAP_MAXES`.
    pub map_max: usize,
    /// The chunk grid is on when the map opens.
    pub map_grid: bool,
    /// The bottom bar shows the coordinates under the pointer.
    pub map_cursor: bool,
    /// The map follows you when it opens.
    pub map_follow: bool,
    /// Explored ground outside the loaded chunks is dimmed.
    pub map_shade: bool,
    /// Your recent path on the world map.
    pub footsteps: bool,
    /// Index in `MAP_MODES`.
    pub map_mode: usize,
    /// Index in `CACHE_RADII`.
    pub map_cache: usize,
    /// Chunks scanned again as they change (off: each kept as first seen).
    pub map_update: bool,
    /// New chunks mapped (off: the map stays as it is).
    pub map_new: bool,
    /// Biome colours blended across biome borders (off: a little faster, with seams).
    pub blend: bool,
    /// No entities and no cave view on the maps (fair play).
    pub fair_play: bool,
    /// Other players on the world map.
    pub map_players: bool,
    /// Players on the minimap.
    pub players: bool,
    /// Players' names in the detail view (kept as `names`, as 3.0 stored it).
    pub names: bool,
    /// Mobs' names in the detail view (players' only by default: mobs show their faces).
    pub mob_names: bool,
    /// Hostile mobs on the minimap.
    pub hostile: bool,
    /// Passive mobs on the minimap.
    pub passive: bool,
    /// Dropped items on the minimap.
    pub items: bool,
    /// Entities that are neither players nor hostile or passive mobs.
    pub others: bool,
    /// Faces and names while the player list key is held (else dots).
    pub tab_details: bool,
    /// Faces and names all the time.
    pub always_details: bool,
    /// Index in `ICON_SIZES`.
    pub icon_size: usize,
    /// Index in `LABEL_SIZES`.
    pub label_size: usize,
    /// Index in `RANGES`.
    pub range: usize,
    /// Entities on the minimap at all (the radar key turns them off and on).
    pub radar: bool,
    /// Index in `DEPTH_FADES`.
    pub depth_fade: usize,
    /// Index in `HEIGHT_LIMITS`.
    pub height_limit: usize,
    /// A small up or down tick on entities above or below the fade distance.
    pub show_depth: bool,
    /// Entities' Y in their labels (the detail view).
    pub entity_y: bool,
    /// Index in `DOT_LIMITS`.
    pub dot_limit: usize,
    /// Dot colour, index in `COLOURS`.
    pub player_colour: usize,
    /// Dot colour, index in `COLOURS`.
    pub hostile_colour: usize,
    /// Dot colour, index in `COLOURS`.
    pub passive_colour: usize,
    /// Dot colour, index in `COLOURS`.
    pub other_colour: usize,
    /// Every waypoint shown (the key turns them all off and on).
    pub waypoints: bool,
    /// Waypoint markers on screen, where each waypoint is.
    pub markers: bool,
    /// Waypoints on the minimap.
    pub wp_minimap: bool,
    /// Waypoints on the world map.
    pub wp_worldmap: bool,
    /// Index in `WP_NAMES`: when a marker shows its name.
    pub wp_names: usize,
    /// Index in `WP_NAMES`: when a marker shows its distance.
    pub wp_dist: usize,
    /// Index in `WP_LABELS`.
    pub wp_label: usize,
    /// Index in `WP_SHAPES`.
    pub wp_shape: usize,
    /// Markers off screen kept at its edge with an arrow.
    pub wp_edge: bool,
    /// Markers smaller the farther away.
    pub wp_scale: bool,
    /// Markers fade right beside you and far away.
    pub fade: bool,
    /// Index in `WP_RANGES`: how far markers show.
    pub wp_range: usize,
    /// Index in `UNITS`.
    pub units: usize,
    /// Index in `NEW_COLOURS`.
    pub wp_colour: usize,
    /// Index in `SYMBOLS`.
    pub wp_symbol: usize,
    /// A waypoint where you die.
    pub death: bool,
    /// Death points go when you walk back to them.
    pub death_reached: bool,
    /// Coordinates hidden on screen (streaming): info line, list, map bar.
    pub hide_coords: bool,
    /// Index in `DEATHS`.
    pub deaths: usize,
    /// Index in `SORTS`, the waypoint list's order.
    pub sort: usize,
    /// A beam over each waypoint in the world.
    pub beams: bool,
    /// The add-waypoint key adds one where you stand at once, without the "New waypoint" box.
    pub instant_add: bool,
    /// With Lumos World Map on: the region files it writes are read again as they change, so
    /// the minimap shows what it mapped (its chunks) where this mod has none of its own.
    pub wm_chunks: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            minimap: true,
            textured: true,
            shading: true,
            cave: 0,
            cave_roof: 1,
            cave_depth: 2,
            cave_above: 0,
            cave_start: None,
            cave_legible: true,
            cave_zoom: 0,
            convert: false,
            corner: 1,
            offset_x: 0,
            offset_y: 0,
            size: 2,
            zoom: 4,
            round: false,
            rotate: true,
            opacity: 0,
            enlarge_toggle: false,
            enlarge_zoom: 1,
            enlarge_rotate: true,
            frame: 0,
            background: 0,
            compass: 2,
            frame_colour: 0,
            auto_size: false,
            depth_shade: false,
            night: false,
            palette_biomes: false,
            glass: false,
            mini_grid: false,
            mini_slime: false,
            light_overlay: false,
            light_max: 2,
            coords: true,
            biome: true,
            facing: false,
            light: false,
            time: false,
            dimension: false,
            weather: false,
            chunk: false,
            other_coords: false,
            angles: false,
            real_time: false,
            clock_12h: false,
            info_order: 0,
            map_min: 0,
            map_max: 3,
            map_grid: false,
            map_cursor: true,
            map_follow: false,
            map_shade: true,
            footsteps: true,
            map_mode: 0,
            map_cache: 1,
            map_update: true,
            map_new: true,
            blend: true,
            fair_play: false,
            map_players: true,
            players: true,
            names: true,
            mob_names: false,
            hostile: true,
            passive: true,
            items: false,
            others: false,
            tab_details: true,
            always_details: false,
            icon_size: 1,
            label_size: 1,
            range: 0,
            radar: true,
            depth_fade: 0,
            height_limit: 0,
            show_depth: true,
            entity_y: false,
            dot_limit: 2,
            player_colour: 8,
            hostile_colour: 0,
            passive_colour: 3,
            other_colour: 9,
            waypoints: true,
            markers: true,
            wp_minimap: true,
            wp_worldmap: true,
            wp_names: 0,
            wp_dist: 0,
            wp_label: 1,
            wp_shape: 0,
            wp_edge: false,
            wp_scale: false,
            fade: true,
            wp_range: 0,
            units: 0,
            wp_colour: 0,
            wp_symbol: 0,
            death: true,
            deaths: 1,
            death_reached: true,
            hide_coords: false,
            sort: 1,
            beams: false,
            instant_add: false,
            wm_chunks: true,
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
impl ShowEntry for f32 {
    fn show(&self) -> String {
        format!("{self}")
    }
}
impl ShowEntry for u8 {
    fn show(&self) -> String {
        format!("{}%", (u32::from(*self) * 100 + 127) / 255)
    }
}
impl ShowEntry for (&str, u32) {
    fn show(&self) -> String {
        self.0.to_owned()
    }
}
impl ShowEntry for usize {
    fn show(&self) -> String {
        format!("{self}")
    }
}

/// The settings file's version (`settings_version`): saved with every setting.
pub const SETTINGS_VERSION: u32 = 4;
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
                // A Y kept as text: "auto" or a whole number.
                let y = |key: &str, default: Option<i32>| match get(key).as_deref().map(str::trim) {
                    Some("auto") => None,
                    Some(v) => v.parse::<i32>().ok().or(default),
                    None => default,
                };
                let _ = (&flag, &index, &y);
                let mut s = Self { $( $field: settings_table!(@read $kind, flag, index, y, stringify!($field), d.$field $(, $table)?), )* };
                // Before 3.0 `markers` was the beams ("Waypoints in the world"): turning those off
                // must not hide 3.0's markers. Settings saved before the version key are reset once.
                let version = get(SETTINGS_VERSION_KEY).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
                if version < 3 {
                    s.markers = d.markers;
                }
                // 4: on-screen markers as Xaero's: only where they are (none pinned to the edges),
                // only this dimension's (no converted Overworld/Nether ones).
                if version < 4 {
                    s.wp_edge = d.wp_edge;
                    s.convert = d.convert;
                }
                s
            }
            pub fn stored(&self) -> Vec<(&'static str, String)> {
                vec![ $( (stringify!($field), settings_table!(@store $kind, self.$field)), )* (SETTINGS_VERSION_KEY, SETTINGS_VERSION.to_string()) ]
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
    (@read flag, $flag:ident, $index:ident, $y:ident, $key:expr, $d:expr) => { $flag($key, $d) };
    (@read index, $flag:ident, $index:ident, $y:ident, $key:expr, $d:expr, $table:ident) => { $index($key, $d, $table.len()) };
    (@read y, $flag:ident, $index:ident, $y:ident, $key:expr, $d:expr) => { $y($key, $d) };
    (@store flag, $v:expr) => { $v.to_string() };
    (@store index, $v:expr) => { $v.to_string() };
    (@store y, $v:expr) => { $v.map_or_else(|| "auto".to_owned(), |y: i32| y.to_string()) };
    (@show flag, $on:ident, $v:expr) => { $on($v) };
    (@show index, $on:ident, $v:expr, $table:ident) => { ShowEntry::show(&$table[$v]) };
    (@show y, $on:ident, $v:expr) => { $v.map_or_else(|| "Where you are".to_owned(), |y: i32| y.to_string()) };
    (@click flag, $v:expr, $back:expr) => { $v = !$v };
    (@click y, $v:expr, $back:expr) => { $v = next_start($v, $back) };
    (@click index, $v:expr, $back:expr, $table:ident) => {
        $v = if $back { ($v + $table.len() - 1) % $table.len() } else { ($v + 1) % $table.len() }
    };
}

settings_table! {
    minimap: flag, 0, "Minimap";
    textured: flag, 0, "Game colours (off: map palette)";
    shading: flag, 0, "Terrain shading";
    cave: index(CAVE_MODES), 2, "Cave view";
    cave_roof: index(CAVE_ROOFS), 2, "Roof needed over";
    cave_depth: index(CAVE_DEPTHS), 2, "Depth (blocks below)";
    cave_above: index(CAVE_ABOVES), 2, "Show floors above (blocks)";
    cave_start: y, 2, "Manual start Y";
    cave_legible: flag, 2, "Legible caves";
    cave_zoom: index(CAVE_ZOOMS), 2, "Zoom in when entering";
    convert: flag, 0, "Overworld/Nether waypoints converted";
    corner: index(CORNERS), 1, "Corner";
    offset_x: index(OFFSETS), 1, "Offset from the side";
    offset_y: index(OFFSETS), 1, "Offset from the top/bottom";
    size: index(SIZES), 1, "Size";
    zoom: index(ZOOMS), 1, "Zoom (blocks per pixel)";
    round: flag, 1, "Round";
    rotate: flag, 1, "Turns with you";
    opacity: index(OPACITY), 1, "Opacity";
    enlarge_toggle: flag, 6, "Enlarge key toggles";
    enlarge_zoom: index(ENLARGE_ZOOMS), 6, "Enlarged zoom";
    enlarge_rotate: flag, 6, "Enlarged turns with you";
    frame: index(FRAMES), 1, "Frame";
    background: index(BACKGROUNDS), 1, "Unexplored ground";
    compass: index(COMPASS), 1, "Compass letters";
    frame_colour: index(FRAME_COLOURS), 1, "Frame colour";
    auto_size: flag, 1, "Auto size (by GUI scale)";
    depth_shade: flag, 1, "Terrain depth (lower is darker)";
    night: flag, 1, "Darker at night";
    palette_biomes: flag, 0, "Biome colours in the map palette";
    glass: flag, 0, "See through stained glass";
    mini_grid: flag, 6, "Chunk grid";
    mini_slime: flag, 6, "Slime chunks";
    light_overlay: flag, 6, "Light overlay (dark floors)";
    light_max: index(LIGHT_MAXES), 6, "Light overlay: block light up to";
    coords: flag, 1, "Coordinates";
    biome: flag, 1, "Biome";
    facing: flag, 1, "Facing";
    light: flag, 1, "Light level";
    time: flag, 1, "Time of day";
    dimension: flag, 1, "Dimension";
    weather: flag, 6, "Info: weather";
    chunk: flag, 6, "Info: chunk";
    other_coords: flag, 6, "Info: Overworld/Nether place";
    angles: flag, 6, "Info: yaw and pitch";
    real_time: flag, 6, "Info: real time";
    clock_12h: flag, 6, "12-hour clocks";
    info_order: index(INFO_ORDERS), 1, "Info line order";
    map_min: index(MAP_MINS), 3, "Closest zoom (blocks per pixel)";
    map_max: index(MAP_MAXES), 3, "Farthest zoom (blocks per pixel)";
    map_grid: flag, 3, "Chunk grid when opened";
    map_cursor: flag, 3, "Cursor coordinates";
    map_follow: flag, 3, "Follow you when opened";
    map_shade: flag, 3, "Dim ground not loaded now";
    footsteps: flag, 3, "Footsteps (your recent path)";
    map_mode: index(MAP_MODES), 3, "Server maps";
    map_cache: index(CACHE_RADII), 3, "Regions kept round you";
    map_update: flag, 3, "Update changed chunks";
    map_new: flag, 3, "Map new chunks";
    blend: flag, 0, "Blend biome colours";
    fair_play: flag, 0, "Fair play (no entities, no caves)";
    map_players: flag, 3, "Other players";
    players: flag, 4, "Players";
    names: flag, 4, "Show player names";
    mob_names: flag, 4, "Show mob names";
    hostile: flag, 4, "Hostile mobs";
    passive: flag, 4, "Passive mobs";
    items: flag, 4, "Dropped items";
    others: flag, 4, "Other entities";
    tab_details: flag, 4, "Faces and names while the player list key is held";
    always_details: flag, 4, "Faces and names always";
    icon_size: index(ICON_SIZES), 4, "Entity icon size";
    label_size: index(LABEL_SIZES), 4, "Name label size";
    range: index(RANGES), 4, "Entities within (blocks)";
    radar: flag, 4, "Entities shown";
    depth_fade: index(DEPTH_FADES), 4, "Fade above/below (blocks)";
    height_limit: index(HEIGHT_LIMITS), 4, "Hide above/below (blocks)";
    show_depth: flag, 4, "Above/below marks";
    entity_y: flag, 4, "Y in labels";
    dot_limit: index(DOT_LIMITS), 4, "Most entities drawn";
    player_colour: index(COLOURS), 4, "Player dots";
    hostile_colour: index(COLOURS), 4, "Hostile mob dots";
    passive_colour: index(COLOURS), 4, "Passive mob dots";
    other_colour: index(COLOURS), 4, "Other entity dots";
    waypoints: flag, 5, "Waypoints shown";
    markers: flag, 5, "On screen";
    wp_minimap: flag, 5, "On the minimap";
    wp_worldmap: flag, 5, "On the world map";
    wp_names: index(WP_NAMES), 5, "Names on screen";
    wp_dist: index(WP_NAMES), 5, "Distances on screen";
    wp_label: index(WP_LABELS), 5, "Name size";
    wp_shape: index(WP_SHAPES), 5, "Marker";
    wp_edge: flag, 5, "Off-screen ones at the edge";
    wp_scale: flag, 5, "Shrink with distance";
    fade: flag, 5, "Fade near and far";
    wp_range: index(WP_RANGES), 5, "Shown within (blocks)";
    units: index(UNITS), 5, "Distances in";
    wp_colour: index(NEW_COLOURS), 5, "New waypoints' colour";
    wp_symbol: index(SYMBOLS), 5, "New waypoints' symbol";
    death: flag, 5, "Death points";
    death_reached: flag, 5, "Death points go when reached";
    hide_coords: flag, 6, "Hide coordinates (streaming)";
    deaths: index(DEATHS), 5, "Death points kept";
    sort: index(SORTS), 5, "List order";
    beams: flag, 5, "Beams (old style)";
    instant_add: flag, 5, "B adds instantly";
    wm_chunks: flag, 3, "With World Map: use its chunks";
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

    /// The world map's closest and farthest zoom steps (`map_zoom`).
    pub fn map_zooms(&self) -> (usize, usize) {
        (self.map_min.min(3) * 2, MAP_ZOOM_ONE + 4 + self.map_max.min(3) * 2)
    }

    /// The info lines' order, as letters (`INFO_ORDERS`), then the newer lines: K chunk, O the
    /// other dimension's place, A angles, W weather, R real time.
    pub fn info_letters(&self) -> Vec<char> {
        let mut out: Vec<char> = INFO_ORDERS[self.info_order.min(INFO_ORDERS.len() - 1)].chars().filter(|c| c.is_alphabetic()).collect();
        out.extend(['K', 'O', 'A', 'W', 'R']);
        out
    }
    pub fn shows(&self, kind: DotKind) -> bool {
        match kind {
            DotKind::Player => self.players,
            DotKind::Hostile => self.hostile,
            DotKind::Passive => self.passive,
            DotKind::Other => self.others,
        }
    }

    /// The dot colour (0xRRGGBB) of an entity of `kind`.
    pub fn dot_rgb(&self, kind: DotKind) -> u32 {
        let i = match kind {
            DotKind::Player => self.player_colour,
            DotKind::Hostile => self.hostile_colour,
            DotKind::Passive => self.passive_colour,
            DotKind::Other => self.other_colour,
        };
        COLOURS[i.min(COLOURS.len() - 1)].1
    }

    /// How an entity `dy` blocks above (or below) you is drawn: None when it is past the height
    /// limit, else its opacity and its mark (Some(true) above, Some(false) below the fade
    /// distance, None within it or with marks off).
    pub fn depth_look(&self, dy: f64) -> Option<(u8, Option<bool>)> {
        let limit: Option<f64> = HEIGHT_LIMITS.get(self.height_limit).and_then(|v| v.parse().ok());
        if limit.is_some_and(|l| dy.abs() > l) {
            return None;
        }
        let fade: f64 = DEPTH_FADES.get(self.depth_fade).and_then(|v| v.parse().ok()).unwrap_or(8.0);
        if dy.abs() <= fade {
            return Some((255, None));
        }
        Some((95, self.show_depth.then_some(dy > 0.0)))
    }

    /// The farthest an entity shows, in blocks (None: the map's edge).
    pub fn max_range(&self) -> Option<f64> {
        RANGES.get(self.range).and_then(|r| r.parse().ok())
    }

    /// How many columns a side the roof test looks at (1, 3 or 5).
    pub fn roof_side(&self) -> i32 {
        [1, 3, 5][self.cave_roof.min(2)]
    }

    /// How far below the start Y the cave view looks for a floor.
    pub fn depth_blocks(&self) -> i32 {
        CAVE_DEPTHS[self.cave_depth.min(CAVE_DEPTHS.len() - 1)] as i32
    }

    /// How far above the start Y the cave view also shows open floors.
    pub fn above_blocks(&self) -> i32 {
        CAVE_ABOVES[self.cave_above.min(CAVE_ABOVES.len() - 1)] as i32
    }

    /// The minimap's zoom step now: the setting's, stepped in by the cave zoom in the cave view.
    pub fn zoom_now(&self, cave: bool) -> usize {
        let z = self.zoom.min(ZOOMS.len() - 1);
        if cave { z.saturating_sub(self.cave_zoom.min(2)) } else { z }
    }

    /// The minimap's blocks per GUI pixel now: its zoom (stepped in in the cave view), times
    /// the enlarged map's zoom out when it is big.
    pub fn bpp_now(&self, cave: bool, big: bool) -> f32 {
        let out = if big { [1.0, 2.0, 4.0][self.enlarge_zoom.min(2)] } else { 1.0 };
        ZOOMS[self.zoom_now(cave)] * out
    }

    /// One zoom step in (`in_`) or out; whether it changed.
    pub fn zoom_step(&mut self, in_: bool) -> bool {
        let z = if in_ {
            self.zoom.saturating_sub(1)
        } else {
            (self.zoom + 1).min(ZOOMS.len() - 1)
        };
        let changed = z != self.zoom;
        self.zoom = z;
        changed
    }
}

/// The manual start Y after `now` on the settings button: where you are (None), then the
/// presets from the top down (`back`: the other way). A Y set with the key goes to the nearest
/// preset under it.
pub fn next_start(now: Option<i32>, back: bool) -> Option<i32> {
    let n = CAVE_STARTS.len();
    // Positions: 0 is "where you are", k + 1 is preset k.
    let at = match now {
        None => 0,
        Some(y) => CAVE_STARTS.iter().position(|p| *p <= y).map_or(n + 1, |k| k + 1),
    };
    let exact = now.is_none_or(|y| CAVE_STARTS.contains(&y));
    let next = match (exact, back) {
        (true, false) => (at + 1) % (n + 1),
        (true, true) => (at + n) % (n + 1),
        // Between presets: the one under it, or (back) the one over it.
        (false, false) => at % (n + 1),
        (false, true) => at - 1,
    };
    (next > 0).then(|| CAVE_STARTS[next - 1])
}

// ---------------------------------------------------------------- cave view: when and where

/// Whether the columns round the player are all under a roof: each is the Y of the first roof
/// block over the head (blocks at different heights still make one roof), or None for open sky.
pub fn roofed(columns: &[Option<i32>]) -> bool {
    !columns.is_empty() && columns.iter().all(Option::is_some)
}

/// The cave view's automatic switch: a new roof state has to hold `CAVE_HOLD` ticks before the
/// view follows it, so walking through a doorway or under a tree doesn't flicker the map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaveSwitch {
    /// Whether the cave view is on now.
    pub on: bool,
    /// The other state and the tick it was first seen.
    pending: Option<(bool, u64)>,
}

impl CaveSwitch {
    /// Takes the roof test's answer at `tick`; whether the cave view is on.
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

/// What the minimap's cave view shows: the slice from `start` (the head's block) down `depth`
/// blocks, with open floors up to `above` blocks over it; or, on a roofed dimension's roof, the
/// roof's top from above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaveView {
    Slice { start: i32, depth: i32, above: i32 },
    RoofTop,
}

/// The cave view for the player's head block `head` in a dimension with or without a ceiling
/// (`ceiling`: the logical top of a roofed dimension), with `roofed` the switch's state now.
/// None: the surface map.
pub fn cave_view(s: &Settings, head: i32, roofed: bool, ceiling: Option<i32>) -> Option<CaveView> {
    if s.fair_play {
        // Fair play: the surface only (a roofed dimension shows its under-roof map, as always).
        return None;
    }
    if let Some(roof) = ceiling
        && head > roof
        && !(s.cave == 1 && s.cave_start.is_some())
    {
        // Standing on the bedrock roof: its top, as the surface would be.
        return (s.cave != 2).then_some(CaveView::RoofTop);
    }
    let start = match s.cave {
        0 if roofed || ceiling.is_some() => head,
        1 => s.cave_start.unwrap_or(head),
        _ => return None,
    };
    Some(CaveView::Slice { start, depth: s.depth_blocks(), above: s.above_blocks() })
}

// ---------------------------------------------------------------- entity dots

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DotKind {
    Player,
    Hostile,
    Passive,
    Other,
}

/// A mob by its type and category.
pub fn dot_kind(kind: &str, category: &str) -> DotKind {
    if kind == "minecraft:player" {
        return DotKind::Player;
    }
    match category {
        "monster" => DotKind::Hostile,
        "creature" | "ambient" | "water_creature" | "water_ambient" | "underground_water_creature"
        | "axolotls" => DotKind::Passive,
        _ => DotKind::Other,
    }
}

/// An arrow of `size` at `at` pointing along `angle` degrees on screen, measured as the game's
/// yaw on a north-up map (0 points down, 90 points left): its four corners, tip first.
pub fn arrow(at: [f32; 2], angle: f32, size: f32) -> [[f32; 2]; 4] {
    let (s, c) = angle.to_radians().sin_cos();
    let dir = [-s, c];
    let side = [c, s];
    let pt = |f: f32, r: f32| {
        [
            at[0] + (dir[0] * f + side[0] * r) * size,
            at[1] + (dir[1] * f + side[1] * r) * size,
        ]
    };
    [pt(1.0, 0.0), pt(-0.7, 0.7), pt(-0.35, 0.0), pt(-0.7, -0.7)]
}

// ---------------------------------------------------------------- entity faces

/// One face of a model part: its normal and its four corners' texture coordinates (0 to 1).
pub type Face = ([f32; 3], [[f32; 2]; 4]);

fn uv_box(f: &Face) -> [f32; 4] {
    let (mut u0, mut v0, mut u1, mut v1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for [u, v] in f.1 {
        u0 = u0.min(u);
        v0 = v0.min(v);
        u1 = u1.max(u);
        v1 = v1.max(v);
    }
    [u0, v0, u1, v1]
}

/// The front of a model part: its biggest face looking north (-Z, where a model's face is), as
/// a texture rectangle `[u0, v0, u1, v1]`.
pub fn front(faces: &[Face]) -> Option<[f32; 4]> {
    faces
        .iter()
        .filter(|f| f.0[2] < -0.9)
        .map(uv_box)
        .filter(|b| b[2] > b[0] && b[3] > b[1])
        .max_by(|a, b| {
            let area = |b: &[f32; 4]| (b[2] - b[0]) * (b[3] - b[1]);
            area(a).partial_cmp(&area(b)).unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// An icon cut from a `w` x `h` RGBA texture: rectangle `rect` (texture coordinates), with
/// `over` (a hat layer) laid over it, fitted into `ICON` x `ICON` texels by nearest sampling.
/// Its size in texels and the `ICON` x `ICON` RGBA cell; None for a clear or empty cut.
pub fn cut_icon(w: u32, h: u32, rgba: &[u8], rect: [f32; 4], over: Option<[f32; 4]>) -> Option<(u32, u32, Vec<u8>)> {
    if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
        return None;
    }
    let px = |r: [f32; 4]| {
        let x0 = (r[0] * w as f32).round() as i64;
        let y0 = (r[1] * h as f32).round() as i64;
        let x1 = (r[2] * w as f32).round() as i64;
        let y1 = (r[3] * h as f32).round() as i64;
        (x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    };
    let (x0, y0, fw, fh) = px(rect);
    if fw == 0 || fh == 0 {
        return None;
    }
    let scale = (fw.max(fh) as f32 / ICON as f32).max(1.0);
    let (iw, ih) = (((fw as f32 / scale).round() as u32).max(1), ((fh as f32 / scale).round() as u32).max(1));
    let read = |x: i64, y: i64| -> [u8; 4] {
        if x < 0 || y < 0 || x >= i64::from(w) || y >= i64::from(h) {
            return [0; 4];
        }
        let o = ((y as u32 * w + x as u32) * 4) as usize;
        [rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]]
    };
    let over = over.map(px).filter(|o| o.2 > 0 && o.3 > 0);
    let mut out = vec![0u8; (ICON * ICON * 4) as usize];
    let mut seen = false;
    for j in 0..ih {
        for i in 0..iw {
            let (fx, fy) = ((i as f32 + 0.5) / iw as f32, (j as f32 + 0.5) / ih as f32);
            let mut c = read(x0 + (fx * fw as f32) as i64, y0 + (fy * fh as f32) as i64);
            if let Some((ox, oy, ow, oh)) = over {
                let o = read(ox + (fx * ow as f32) as i64, oy + (fy * oh as f32) as i64);
                let a = u32::from(o[3]);
                for k in 0..3 {
                    c[k] = ((u32::from(c[k]) * (255 - a) + u32::from(o[k]) * a) / 255) as u8;
                }
                c[3] = c[3].max(o[3]);
            }
            if c[3] > 16 {
                seen = true;
            }
            let o = ((j * ICON + i) * 4) as usize;
            out[o..o + 4].copy_from_slice(&c);
        }
    }
    seen.then_some((iw, ih, out))
}

/// The slot of icon `i` in the mod image: its top left texel.
pub fn icon_origin(i: usize) -> [u32; 2] {
    let i = i as u32;
    [(i % ICON_COLS) * ICON, ICONS_Y + (i / ICON_COLS) * ICON]
}

/// A model layer's key as the model file names it: `cow#main` is `minecraft:cow#main`, and the
/// player's `main` is `minecraft:player#main`.
pub fn model_key(key: &str) -> String {
    if key == "main" {
        "minecraft:player#main".into()
    } else if key.contains(':') {
        key.into()
    } else {
        format!("minecraft:{key}")
    }
}

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

// ---------------------------------------------------------------- the world map's cave maps

/// What the world map shows underground, kept per dimension: Off (the surface; on a roofed
/// dimension, the roof's top), Layered (the slice under a top Y, saved per band) or Full (the
/// floors under the roof: the Nether's under-roof map, the first caves under the ground
/// elsewhere).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaveType {
    Off,
    Layered,
    Full,
}

pub const CAVE_TYPES: [&str; 3] = ["Off", "Layered", "Full"];

impl CaveType {
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(i: usize) -> Self {
        [CaveType::Off, CaveType::Layered, CaveType::Full][i % 3]
    }
    /// A new dimension's: Full under a roof (as the Nether's map always was), else Off.
    pub fn default_for(roofed: bool) -> Self {
        if roofed { CaveType::Full } else { CaveType::Off }
    }
}

/// The cave map types chosen per dimension (`<world>/cave-maps.txt`: `dimension<TAB>type`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaveTypes(pub Vec<(String, CaveType)>);

impl CaveTypes {
    pub fn from_text(text: &str) -> Self {
        let mut out = CaveTypes::default();
        for l in text.lines().filter(|l| !l.starts_with('#')) {
            if let Some((dim, t)) = l.split_once('\t')
                && let Some(i) = CAVE_TYPES.iter().position(|n| *n == t.trim())
            {
                out.set(dim.trim(), CaveType::from_index(i));
            }
        }
        out
    }
    pub fn to_text(&self) -> String {
        let mut out = String::from("# Lumos Map cave maps: dimension, type (Off, Layered, Full)\n");
        for (dim, t) in &self.0 {
            out.push_str(&format!("{dim}\t{}\n", CAVE_TYPES[t.index()]));
        }
        out
    }
    pub fn get(&self, dim: &str, roofed: bool) -> CaveType {
        self.0.iter().find(|(d, _)| d == dim).map_or_else(|| CaveType::default_for(roofed), |(_, t)| *t)
    }
    pub fn set(&mut self, dim: &str, t: CaveType) {
        match self.0.iter_mut().find(|(d, _)| d == dim) {
            Some(e) => e.1 = t,
            None => self.0.push((dim.to_owned(), t)),
        }
    }
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

// ---------------------------------------------------------------- shading

/// How a column is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub struct Look {
    pub textured: bool,
    pub shading: bool,
    /// Cave floors darken less with depth.
    pub legible: bool,
    /// The map palette's biome-tinted colours (grass, leaves, water) take the column's tint.
    pub biomes: bool,
    /// The surface darkens below this Y (the minimap's terrain depth, from your height).
    pub depth_from: Option<i32>,
}

/// Map colours the game tints by biome: grass, plants (leaves) and water.
pub const BIOME_TINTED: [u8; 3] = [1, 7, WATER];

/// Palette colour `palette` in the hue of `tinted` (the column's own colour, its biome's
/// tint in it), as bright as the palette colour: palette mode with biome colours.
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

/// How much the surface `below` blocks under you darkens (terrain depth): not at all at or over
/// your height, down to 0.6 at 48 blocks below.
pub fn surface_depth_factor(below: i32) -> f32 {
    1.0 - (below.clamp(0, 48) as f32 / 48.0) * 0.4
}

/// The terrain depth's reference height for feet at `y`: in steps of 4 blocks, so the minimap
/// is drawn again only when you have climbed or dropped a few blocks.
pub fn depth_step(y: f64) -> i32 {
    (y.floor() as i32).div_euclid(4) * 4
}

/// How bright the sky is (1 at day, about 0.4 at midnight, less in rain and thunder): the
/// game's own sky darkening at `day_time`, as a factor for the minimap's surface at night.
pub fn sky_brightness(day_time: i64, rain: f32, thunder: f32) -> f32 {
    let t = (day_time.rem_euclid(24000) as f64) / 24000.0;
    let f = (t - 0.25).rem_euclid(1.0);
    let g = 0.5 - (f * std::f64::consts::PI).cos() / 2.0;
    let angle = (f * 2.0 + g) / 3.0;
    let mut light = (1.0 - ((angle * std::f64::consts::TAU).cos() * 2.0 + 0.5)).clamp(0.0, 1.0);
    light = 1.0 - light;
    light *= 1.0 - f64::from(rain.clamp(0.0, 1.0)) * 5.0 / 16.0;
    light *= 1.0 - f64::from(thunder.clamp(0.0, 1.0)) * 5.0 / 16.0;
    // The game darkens the sky light by up to 11 of 15 levels; the map keeps some colour.
    (0.4 + 0.6 * light) as f32
}

/// The minimap's auto size for a GUI `w` x `h` (pixels after the GUI scale): about a quarter
/// of the short side, in steps of 8, within the sizes the setting offers.
pub fn auto_size(w: f32, h: f32) -> f32 {
    ((w.min(h) * 0.27 / 8.0).round() * 8.0).clamp(SIZES[0], SIZES[SIZES.len() - 1])
}

/// The ground under stained glass, seen through it: tinted by the glass's colour.
pub fn glass_over(ground: u32, glass: u32) -> u32 {
    lerp_rgb(ground, glass, 0.35)
}

/// A column's colour as the maps show it: its own colour (or its map colour), lit where the
/// ground rises towards the north-west and shaded where it falls away, gently, as a relief map.
/// Water carries its depth in its colour already (`water_over`); with the flat palette it is
/// darkened here.
pub fn shade(
    palette: &[u32],
    look: Look,
    c: Col,
    north: Option<i16>,
    west: Option<i16>,
) -> [u8; 4] {
    if c.kind == DEEP {
        return [(DEEP_RGB >> 16) as u8, (DEEP_RGB >> 8) as u8, DEEP_RGB as u8, 255];
    }
    let own = look.textured && c.rgb != 0;
    let base = if own {
        c.rgb
    } else {
        let flat = palette.get(usize::from(c.color)).copied().unwrap_or(0);
        if look.biomes && c.rgb != 0 && BIOME_TINTED.contains(&c.color) { palette_biome(flat, c.rgb) } else { flat }
    };
    let water = c.color == WATER || c.depth > 0;
    let factor = if water {
        if own { 1.0 } else { 1.0 - f32::from(c.depth.min(14)) * 0.03 }
    } else if look.shading {
        let d = |n: Option<i16>| f32::from(c.height - n.unwrap_or(c.height)).clamp(-6.0, 6.0);
        let slope = d(north) + d(west);
        (1.0 + slope * 0.045).clamp(0.7, 1.16)
    } else {
        1.0
    };
    let ch = |s: u32| ((((base >> s) & 255) as f32 * factor).round().clamp(0.0, 255.0)) as u8;
    [ch(16), ch(8), ch(0), 255]
}

/// A column with its north and west neighbours' heights for its shading. In a cave slice
/// (`cave`), rock is flat with a lighter edge along open space, floors darken with depth (lava
/// doesn't) and slopes are always shaded.
fn shaded(
    column: &mut impl FnMut(i32, i32) -> Option<Col>,
    palette: &[u32],
    look: Look,
    cave: Option<CaveShade>,
    x: i32,
    z: i32,
) -> Option<[u8; 4]> {
    let c = column(x, z)?;
    if let Some(cave) = cave {
        let px = |rgb: u32| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255];
        match c.kind {
            DEEP => return Some(px(DEEP_RGB)),
            WALL => {
                let open = [(0, -1), (0, 1), (-1, 0), (1, 0)]
                    .iter()
                    .any(|(dx, dz)| column(x + dx, z + dz).is_some_and(|n| n.kind != WALL));
                return Some(px(if open { WALL_EDGE_RGB } else { WALL_RGB }));
            }
            _ => {}
        }
        let lit = Look { shading: true, ..look };
        let mut rgba = if c.color == WATER || c.depth > 0 {
            shade(palette, lit, c, None, None)
        } else {
            let n = column(x, z - 1).filter(|n| n.kind == FLOOR).map(|c| c.height);
            let w = column(x - 1, z).filter(|n| n.kind == FLOOR).map(|c| c.height);
            shade(palette, lit, c, n, w)
        };
        if c.color != FIRE {
            let k = depth_factor(cave.start - i32::from(c.height), cave.depth, look.legible);
            for ch in &mut rgba[..3] {
                *ch = (f32::from(*ch) * k).round() as u8;
            }
        }
        return Some(rgba);
    }
    let mut rgba = if !look.shading || c.color == WATER || c.depth > 0 {
        shade(palette, look, c, None, None)
    } else {
        let n = column(x, z - 1).map(|c| c.height);
        let w = column(x - 1, z).map(|c| c.height);
        shade(palette, look, c, n, w)
    };
    if let Some(from) = look.depth_from
        && c.kind != DEEP
    {
        let k = surface_depth_factor(from - i32::from(c.height));
        for ch in &mut rgba[..3] {
            *ch = (f32::from(*ch) * k).round() as u8;
        }
    }
    Some(rgba)
}

// ---------------------------------------------------------------- the map texture: cells

// The mod image slot (1024 square) holds 16 x 16 texel cells of terrain in its top 896 rows,
// shared by the minimap and the world map (never shown at once), and entity icons below them.
// A cell at level of detail `lod` covers 16 << lod blocks square, a texel every 1 << lod blocks.
pub const CELL: u32 = 16;
pub const CELL_COLS: u32 = 64;
pub const CELL_ROWS: u32 = 56;
pub const CELLS: usize = (CELL_COLS * CELL_ROWS) as usize;
/// The deepest level of detail (a cell of 1024 blocks).
pub const MAX_LOD: u8 = 6;
/// Where the icon cells start, and their size.
pub const ICONS_Y: u32 = CELL_ROWS * CELL;
pub const ICON: u32 = 16;
pub const ICON_COLS: u32 = 1024 / ICON;
pub const ICON_SLOTS: usize = (ICON_COLS * ((1024 - ICONS_Y) / ICON)) as usize;

/// A new epoch for a live layer, from one counter for the whole game: a layer made after
/// another (the cave view turned off and on, another slice) never starts at an epoch an old one
/// had, so the cells cached from the old layer are never taken for the new one's.
pub fn fresh_epoch() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The epoch a cell is checked against, from all the counts it depends on (mixed so that
/// different counts never add up to the same epoch).
pub fn epoch_mix(parts: &[u64]) -> u64 {
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15;
    for &p in parts {
        // splitmix64 over each part in turn.
        let mut z = h ^ p.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        h = z ^ (z >> 31);
    }
    h
}

/// What a cell shows: the explored surface (0), the cave layer (1) or the world map's slice (2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellKey {
    pub source: u8,
    pub lod: u8,
    pub x: i32,
    pub z: i32,
}

impl CellKey {
    /// Blocks a side.
    pub fn span(&self) -> i32 {
        16 << self.lod
    }
}

#[derive(Clone, Copy, Debug)]
struct CellSlot {
    key: CellKey,
    /// The source's epoch it was drawn in.
    epoch: u64,
    /// Nothing explored in it.
    empty: bool,
    /// Its tiles changed since it was drawn.
    stale: bool,
    used: u64,
}

/// Which cells of the slot hold what, kept as the maps ask for them.
pub struct CellCache {
    slots: Vec<Option<CellSlot>>,
    index: HashMap<CellKey, usize>,
    /// Counts frames, for the least recently used.
    pub frame: u64,
    /// Asked for this frame and not drawn yet (or stale): (key, missing).
    pub wanted: Vec<(CellKey, bool)>,
    wanted_set: HashSet<CellKey>,
}

impl Default for CellCache {
    fn default() -> Self {
        Self {
            slots: vec![None; CELLS],
            index: HashMap::new(),
            frame: 0,
            wanted: Vec::new(),
            wanted_set: HashSet::new(),
        }
    }
}

/// The texel at the top left of cell slot `slot`.
pub fn slot_origin(slot: usize) -> [u32; 2] {
    let s = slot as u32;
    [(s % CELL_COLS) * CELL, (s / CELL_COLS) * CELL]
}

impl CellCache {
    /// A new frame: the wanted list starts over.
    pub fn next_frame(&mut self) {
        self.frame += 1;
        self.wanted.clear();
        self.wanted_set.clear();
    }

    /// The slot drawing `key` (and whether it is empty), asking for it to be drawn (again) when
    /// it isn't or is out of date. A stale cell still shows until it is redrawn.
    pub fn get(&mut self, key: CellKey, epoch: u64) -> Option<(usize, bool)> {
        let found = self.index.get(&key).copied();
        let mut out = None;
        let mut redo = true;
        if let Some(i) = found
            && let Some(slot) = self.slots[i].as_mut()
        {
            slot.used = self.frame;
            redo = slot.stale || slot.epoch != epoch;
            out = Some((i, slot.empty));
        }
        if redo && self.wanted_set.insert(key) {
            self.wanted.push((key, found.is_none()));
        }
        out
    }

    /// A slot for `key`, drawn now: its own, a free one, or the one used longest ago (never one
    /// used this frame); None when every slot is in use this frame.
    pub fn place(&mut self, key: CellKey, epoch: u64, empty: bool) -> Option<usize> {
        let i = match self.index.get(&key) {
            Some(&i) => i,
            None => {
                let free = self.slots.iter().position(Option::is_none);
                let i = match free {
                    Some(i) => i,
                    None => {
                        let (i, used) = self
                            .slots
                            .iter()
                            .enumerate()
                            .filter_map(|(i, s)| s.map(|s| (i, s.used)))
                            .min_by_key(|&(_, used)| used)?;
                        if used == self.frame {
                            return None;
                        }
                        i
                    }
                };
                if let Some(old) = self.slots[i].take() {
                    self.index.remove(&old.key);
                }
                self.index.insert(key, i);
                i
            }
        };
        self.slots[i] = Some(CellSlot { key, epoch, empty, stale: false, used: self.frame });
        Some(i)
    }

    /// Chunk `chunk` of `source` changed: every cell showing it is out of date, and the cells
    /// east and south of it at its edge (their shading looks at it).
    pub fn chunk_changed(&mut self, source: u8, chunk: (i32, i32)) {
        for lod in 0..=MAX_LOD {
            let (x, z) = (chunk.0 >> lod, chunk.1 >> lod);
            let last = (1 << lod) - 1;
            let mut mark = |x: i32, z: i32| {
                if let Some(&i) = self.index.get(&CellKey { source, lod, x, z })
                    && let Some(s) = self.slots[i].as_mut()
                {
                    s.stale = true;
                }
            };
            mark(x, z);
            if chunk.0 & last == last {
                mark(x + 1, z);
            }
            if chunk.1 & last == last {
                mark(x, z + 1);
            }
        }
    }

    /// Every cell of `source` forgotten: a new layer took its place, and the old one's cells
    /// must not show (not even until they are redrawn, as a stale cell of the same layer does).
    pub fn forget(&mut self, source: u8) {
        for slot in &mut self.slots {
            if slot.is_some_and(|s| s.key.source == source) {
                *slot = None;
            }
        }
        self.index.retain(|k, _| k.source != source);
    }

    /// Everything forgotten (the slot was rebuilt).
    pub fn clear(&mut self) {
        self.slots.iter_mut().for_each(|s| *s = None);
        self.index.clear();
    }

    /// The cells to draw now, the missing ones first, at most `n`.
    pub fn take_wanted(&mut self, n: usize) -> Vec<CellKey> {
        self.wanted.sort_by_key(|w| !w.1);
        self.wanted.iter().take(n).map(|w| w.0).collect()
    }
}

/// Draws cell `key` (16 x 16 RGBA into `out`) from `column`: each texel the block at its middle,
/// shaded against its neighbours a block north and west. Clear where nothing is known; whether
/// all of it is.
pub fn compose_cell(
    mut column: impl FnMut(i32, i32) -> Option<Col>,
    palette: &[u32],
    look: Look,
    cave: Option<CaveShade>,
    key: CellKey,
    out: &mut [u8; 1024],
) -> bool {
    let step = 1i32 << key.lod;
    let (x0, z0) = (key.x * key.span(), key.z * key.span());
    let half = step >> 1;
    let mut empty = true;
    for j in 0..16 {
        for i in 0..16 {
            let (x, z) = (x0 + i * step + half, z0 + j * step + half);
            let o = ((j * 16 + i) * 4) as usize;
            match shaded(&mut column, palette, look, cave, x, z) {
                Some(rgba) => {
                    out[o..o + 4].copy_from_slice(&rgba);
                    empty = false;
                }
                None => out[o..o + 4].copy_from_slice(&[0, 0, 0, 0]),
            }
        }
    }
    empty
}

/// The level of detail for a map at `bpp` blocks per GUI pixel: a texel no bigger than a GUI
/// pixel, coarser when `cells(lod)` (how many cells it would need) passes `limit`.
pub fn lod_for(bpp: f64, limit: usize, cells: impl Fn(u8) -> usize) -> u8 {
    let mut lod = bpp.max(1.0).log2().floor().clamp(0.0, f64::from(MAX_LOD)) as u8;
    while lod < MAX_LOD && cells(lod) > limit {
        lod += 1;
    }
    lod
}

// ---------------------------------------------------------------- geometry

/// A point on screen with its texture coordinates.
pub type Vert = [f32; 4];

/// `poly` cut to the inside of the convex polygon `clip` (any winding), texture coordinates
/// carried along.
pub fn clip_polygon(poly: &[Vert], clip: &[[f32; 2]]) -> Vec<Vert> {
    let n = clip.len();
    if n < 3 {
        return poly.to_vec();
    }
    // The clip polygon's winding, so "inside" is the same side of every edge.
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (clip[i], clip[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut out = poly.to_vec();
    let mut next: Vec<Vert> = Vec::with_capacity(poly.len() + 8);
    for i in 0..n {
        if out.is_empty() {
            break;
        }
        let (a, b) = (clip[i], clip[(i + 1) % n]);
        let side = |p: &Vert| sign * ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]));
        // Most edges cut nothing: wholly inside them, the polygon stays as it is.
        if out.iter().all(|p| side(p) >= 0.0) {
            continue;
        }
        next.clear();
        for k in 0..out.len() {
            let (p, q) = (out[k], out[(k + 1) % out.len()]);
            let (sp, sq) = (side(&p), side(&q));
            if sp >= 0.0 {
                next.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                next.push(std::array::from_fn(|c| p[c] + (q[c] - p[c]) * t));
            }
        }
        std::mem::swap(&mut out, &mut next);
    }
    out
}

/// The outline the maps are cut to: a circle (centre, radius) or a rectangle.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Circle([f32; 2], f32),
    Rect([f32; 4]),
}

impl Shape {
    /// Its outline as a convex polygon (a circle as 64 sides).
    pub fn polygon(&self) -> Vec<[f32; 2]> {
        match *self {
            Shape::Circle(c, r) => outline(c, r, true),
            Shape::Rect([x, y, w, h]) => vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
        }
    }

    /// `poly` (a quad) cut to the shape, as quads: those wholly inside go through untouched,
    /// those wholly outside give none, only the ones across the edge are clipped.
    pub fn cut(&self, poly: &[Vert; 4], outline: &[[f32; 2]]) -> Vec<[Vert; 4]> {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for v in poly {
            x0 = x0.min(v[0]);
            y0 = y0.min(v[1]);
            x1 = x1.max(v[0]);
            y1 = y1.max(v[1]);
        }
        let (inside, outside) = match *self {
            Shape::Circle(c, r) => {
                // Inside the 64-gon for sure; out of the circle's reach altogether.
                let r_in = r * (std::f32::consts::PI / 64.0).cos();
                let inside = poly.iter().all(|v| (v[0] - c[0]).powi(2) + (v[1] - c[1]).powi(2) <= r_in * r_in);
                let nx = c[0].clamp(x0, x1) - c[0];
                let ny = c[1].clamp(y0, y1) - c[1];
                (inside, nx * nx + ny * ny > r * r)
            }
            Shape::Rect([x, y, w, h]) => (
                x0 >= x && y0 >= y && x1 <= x + w && y1 <= y + h,
                x1 <= x || y1 <= y || x0 >= x + w || y0 >= y + h,
            ),
        };
        if outside {
            Vec::new()
        } else if inside {
            vec![*poly]
        } else {
            fan_quads(&clip_polygon(poly, outline))
        }
    }
}

/// A convex polygon as quads (the last may repeat a corner): a fan from its first corner.
pub fn fan_quads(poly: &[Vert]) -> Vec<[Vert; 4]> {
    let mut out = Vec::new();
    let mut i = 1;
    while i + 1 < poly.len() {
        let c = poly.get(i + 2).copied().unwrap_or(poly[i + 1]);
        out.push([poly[0], poly[i], poly[i + 1], c]);
        i += 2;
    }
    out
}

/// The minimap's outline: a circle (as 64 sides) or a square, centred on `c`, `r` across half.
pub fn outline(c: [f32; 2], r: f32, round: bool) -> Vec<[f32; 2]> {
    if round {
        (0..64)
            .map(|i| {
                let a = i as f32 / 64.0 * std::f32::consts::TAU;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect()
    } else {
        vec![[c[0] - r, c[1] - r], [c[0] + r, c[1] - r], [c[0] + r, c[1] + r], [c[0] - r, c[1] + r]]
    }
}

/// A ring between radii `r0` and `r1` round `c` as quads: a circle's frame.
pub fn ring(c: [f32; 2], r0: f32, r1: f32) -> Vec<[[f32; 2]; 4]> {
    (0..64)
        .map(|i| {
            let at = |k: i32, r: f32| {
                let a = k as f32 / 64.0 * std::f32::consts::TAU;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            };
            [at(i, r0), at(i, r1), at(i + 1, r1), at(i + 1, r0)]
        })
        .collect()
}

// ---------------------------------------------------------------- the minimap

/// The runs of set bits in a row of 16 columns (bit `x` set: column `x` marked), as (first,
/// length): one quad each instead of one a block.
pub fn runs(row: u16) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let mut x = 0;
    while x < 16 {
        if row >> x & 1 == 1 {
            let start = x;
            while x < 16 && row >> x & 1 == 1 {
                x += 1;
            }
            out.push((start, x - start));
        } else {
            x += 1;
        }
    }
    out
}

/// A line from `a` to `b` as a quad `width` across.
pub fn line_quad(a: [f32; 2], b: [f32; 2], width: f32) -> [[f32; 2]; 4] {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
    [[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]]
}

/// Where a world offset `dx, dz` (blocks from the player) is on the minimap, in pixels from its
/// centre (x right, y down), turned with `yaw` or north up, at `bpp` blocks per pixel.
pub fn minimap_point(dx: f64, dz: f64, yaw: f32, rotate: bool, bpp: f32) -> (f32, f32) {
    let (r, f) = axes(yaw, rotate);
    let bpp = f64::from(bpp);
    let u = (dx * r[0] + dz * r[1]) / bpp;
    let v = -(dx * f[0] + dz * f[1]) / bpp;
    (u as f32, v as f32)
}

/// The world's directions as the map's right and up.
fn axes(yaw: f32, rotate: bool) -> ([f64; 2], [f64; 2]) {
    if rotate {
        let (s, c) = f64::from(yaw).to_radians().sin_cos();
        ([-c, -s], [-s, c])
    } else {
        ([1.0, 0.0], [0.0, -1.0])
    }
}

/// `p` (from the centre) kept inside the map: within `radius` of the centre when round, else
/// within the square of half side `radius`. Whether it had to move.
pub fn clamp_to_map(p: (f32, f32), radius: f32, round: bool) -> ((f32, f32), bool) {
    if round {
        let d = (p.0 * p.0 + p.1 * p.1).sqrt();
        if d <= radius {
            (p, false)
        } else {
            ((p.0 / d * radius, p.1 / d * radius), true)
        }
    } else {
        let m = p.0.abs().max(p.1.abs());
        if m <= radius {
            (p, false)
        } else {
            ((p.0 / m * radius, p.1 / m * radius), true)
        }
    }
}

/// Where north, east, south and west are on the minimap's rim, as unit directions (x right, y
/// down), for N, E, S, W.
pub fn compass(yaw: f32, rotate: bool) -> [(char, f32, f32); 4] {
    let at = |dx: f64, dz: f64| {
        let (u, v) = minimap_point(dx, dz, yaw, rotate, 1.0);
        let d = (u * u + v * v).sqrt().max(1e-6);
        (u / d, v / d)
    };
    let n = at(0.0, -1.0);
    let e = at(1.0, 0.0);
    [('N', n.0, n.1), ('E', e.0, e.1), ('S', -n.0, -n.1), ('W', -e.0, -e.1)]
}

/// The way `yaw` faces as text: "Facing North (-Z)", eight ways.
pub fn facing(yaw: f32) -> String {
    const WAYS: [&str; 8] = [
        "South (+Z)", "South-west", "West (-X)", "North-west", "North (-Z)", "North-east", "East (+X)", "South-east",
    ];
    let i = ((yaw.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8;
    format!("Facing {}", WAYS[i])
}

/// An angle in degrees as the debug screen shows yaw: -180 to 180.
pub fn wrap_degrees(a: f32) -> f32 {
    (a + 180.0).rem_euclid(360.0) - 180.0
}

/// Hours and minutes as a clock: "15:05", or "3:05 PM" when `h12`.
pub fn hm(hour: u32, minute: u32, h12: bool) -> String {
    if !h12 {
        return format!("{hour:02}:{minute:02}");
    }
    let h = match hour % 12 {
        0 => 12,
        h => h,
    };
    format!("{h}:{minute:02} {}", if hour < 12 { "AM" } else { "PM" })
}

/// The game's time of day as `hm` shows it.
pub fn game_clock(day_time: i64, h12: bool) -> String {
    let t = (day_time + 6000).rem_euclid(24000);
    hm((t / 1000) as u32, ((t % 1000) * 60 / 1000) as u32, h12)
}

/// The chunk line: the chunk's X and Z and the block's place in it.
pub fn chunk_line(block: [i32; 3]) -> String {
    format!("Chunk {} {} ({} {})", block[0] >> 4, block[2] >> 4, block[0] & 15, block[2] & 15)
}

/// The weather as the info line says it, from the rain and thunder levels (0 to 1).
pub fn weather(rain: f32, thunder: f32) -> &'static str {
    if thunder > 0.9 && rain > 0.2 {
        "Thunder"
    } else if rain > 0.2 {
        "Rain"
    } else {
        "Clear"
    }
}

// ---------------------------------------------------------------- the world map

/// Unexplored ground on the world map.
pub const UNKNOWN: [u8; 4] = [18, 18, 22, 255];

/// Zoom steps of the world map (a square root of two apart), from 1/8 block to 32 blocks a GUI
/// pixel.
pub const MAP_ZOOM_STEPS: usize = 17;
/// The step showing a block a GUI pixel.
pub const MAP_ZOOM_ONE: usize = 6;

pub fn map_zoom(step: usize) -> f64 {
    0.125 * 2f64.powf(step.min(MAP_ZOOM_STEPS - 1) as f64 / 2.0)
}

/// What the world map shows: its centre in blocks and its zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapView {
    pub x: f64,
    pub z: f64,
    /// A zoom step (`map_zoom`).
    pub zoom: usize,
}

impl Default for MapView {
    fn default() -> Self {
        Self { x: 0.0, z: 0.0, zoom: MAP_ZOOM_ONE }
    }
}

impl MapView {
    pub fn bpp(&self) -> f64 {
        map_zoom(self.zoom)
    }

    /// The world point under GUI point `p` of the map's `rect` (`[x, y, w, h]`).
    pub fn world_at(&self, rect: [f32; 4], p: [f32; 2]) -> (f64, f64) {
        let (cx, cy) = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
        (
            self.x + f64::from(p[0] - cx) * self.bpp(),
            self.z + f64::from(p[1] - cy) * self.bpp(),
        )
    }

    /// Where world point `x, z` is on the map's `rect`, in GUI pixels.
    pub fn screen_of(&self, rect: [f32; 4], x: f64, z: f64) -> [f32; 2] {
        let (cx, cy) = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
        [
            cx + ((x - self.x) / self.bpp()) as f32,
            cy + ((z - self.z) / self.bpp()) as f32,
        ]
    }

    /// Dragged by `delta` GUI pixels: the map moves with the pointer.
    pub fn pan(&mut self, delta: [f32; 2]) {
        self.x -= f64::from(delta[0]) * self.bpp();
        self.z -= f64::from(delta[1]) * self.bpp();
    }

    /// Zoom steps in (`steps` > 0) or out within `min..=max`, keeping the point under `p` where
    /// it is.
    pub fn zoom_at(&mut self, rect: [f32; 4], p: [f32; 2], steps: i32, min: usize, max: usize) {
        let before = self.world_at(rect, p);
        let zoom = (self.zoom as i32 - steps).clamp(min as i32, max.min(MAP_ZOOM_STEPS - 1) as i32) as usize;
        if zoom == self.zoom {
            return;
        }
        self.zoom = zoom;
        let after = self.world_at(rect, p);
        self.x += before.0 - after.0;
        self.z += before.1 - after.1;
    }

    /// The world rectangle the map's `rect` shows: min x, min z, max x, max z.
    pub fn bounds(&self, rect: [f32; 4]) -> (f64, f64, f64, f64) {
        let (x0, z0) = self.world_at(rect, [rect[0], rect[1]]);
        let (x1, z1) = self.world_at(rect, [rect[0] + rect[2], rect[1] + rect[3]]);
        (x0, z0, x1, z1)
    }
}

/// Composes an `iw` x `ih` RGBA image of `column` for `view`, each texel `gui_per_texel` GUI
/// pixels (the PNG export).
#[allow(clippy::too_many_arguments)]
pub fn compose(
    mut column: impl FnMut(i32, i32) -> Option<Col>,
    palette: &[u32],
    look: Look,
    view: &MapView,
    iw: u32,
    ih: u32,
    gui_per_texel: f32,
    out: &mut Vec<u8>,
) {
    out.clear();
    out.resize((iw * ih * 4) as usize, 0);
    let bpp = view.bpp() * f64::from(gui_per_texel);
    let (hw, hh) = (f64::from(iw) / 2.0, f64::from(ih) / 2.0);
    for j in 0..ih {
        let wz = (view.z + (f64::from(j) + 0.5 - hh) * bpp).floor() as i32;
        for i in 0..iw {
            let wx = (view.x + (f64::from(i) + 0.5 - hw) * bpp).floor() as i32;
            let o = ((j * iw + i) * 4) as usize;
            let rgba = shaded(&mut column, palette, look, None, wx, wz).unwrap_or(UNKNOWN);
            out[o..o + 4].copy_from_slice(&rgba);
        }
    }
}

// ---------------------------------------------------------------- slime chunks

/// `java.util.Random`, as slime chunks are picked with it.
pub struct JavaRandom(i64);

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        Self((seed ^ 0x5DEE_CE66D) & ((1 << 48) - 1))
    }
    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(0x5DEE_CE66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.0 >> (48 - bits)) as i32
    }
    #[cfg(test)]
    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }
    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        if bound & -bound == bound {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }
}

/// Whether chunk `x, z` of a world with `seed` is a slime chunk.
pub fn slime_chunk(seed: i64, x: i32, z: i32) -> bool {
    let s = seed
        .wrapping_add(i64::from(x.wrapping_mul(x).wrapping_mul(0x4c1906)))
        .wrapping_add(i64::from(x.wrapping_mul(0x5ac0db)))
        .wrapping_add(i64::from(z.wrapping_mul(z)).wrapping_mul(0x4307a7))
        .wrapping_add(i64::from(z.wrapping_mul(0x5f24f)))
        ^ 0x3ad8025f;
    JavaRandom::new(s).next_int_bound(10) == 0
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
pub fn chat_coords(line: &str) -> Option<[i32; 3]> {
    let chars: Vec<char> = line.chars().collect();
    let mut nums: Vec<(usize, i32)> = Vec::new();
    let mut i = 0;
    let mut word = 0usize;
    while i < chars.len() {
        let c = chars[i];
        let starts = c.is_ascii_digit()
            || (c == '-' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()));
        let after_letter = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '.');
        if starts && !after_letter {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            // A number ending in ':' is a label ("Waypoint 1: ..."), not a coordinate.
            if chars.get(i) != Some(&':')
                && let Ok(n) = text.parse::<i32>()
            {
                nums.push((word, n));
            }
            continue;
        }
        if c.is_whitespace() {
            word += 1;
        }
        i += 1;
    }
    nums.windows(3)
        .find(|w| w[1].0 - w[0].0 <= 2 && w[2].0 - w[1].0 <= 2)
        .map(|w| [w[0].1, w[1].1, w[2].1])
}

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
pub fn project(view: &[f32; 16], projection: &[f32; 16], rel: [f64; 3], w: f32, h: f32) -> ([f32; 2], bool) {
    let mul = |m: &[f32; 16], v: [f32; 4]| -> [f32; 4] {
        std::array::from_fn(|r| (0..4).map(|c| m[c * 4 + r] * v[c]).sum())
    };
    let v = mul(view, [rel[0] as f32, rel[1] as f32, rel[2] as f32, 1.0]);
    let c = mul(projection, v);
    let front = c[3] > 1e-4;
    let wv = if c[3].abs() < 1e-4 { 1e-4 } else { c[3].abs() };
    let (mut x, mut y) = (c[0] / wv, c[1] / wv);
    if !front {
        // Behind: push it to the edge on the side it is on.
        x = -x * 1e3 + if x == 0.0 { 1.0 } else { 0.0 };
        y = -y.abs() * 1e3;
    }
    ([(x + 1.0) / 2.0 * w, (1.0 - y) / 2.0 * h], front)
}

/// A marker at `p` kept `margin` inside a `w` x `h` screen: its place, the direction it was
/// pushed towards (radians, 0 right, clockwise as y grows down) and whether it was pushed.
pub fn to_edge(p: [f32; 2], w: f32, h: f32, margin: f32) -> ([f32; 2], f32, bool) {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (dx, dy) = (p[0] - cx, p[1] - cy);
    let (hx, hy) = ((cx - margin).max(1.0), (cy - margin).max(1.0));
    let k = (dx.abs() / hx).max(dy.abs() / hy);
    let angle = dy.atan2(dx);
    if k <= 1.0 {
        return (p, angle, false);
    }
    ([cx + dx / k, cy + dy / k], angle, true)
}

/// A marker's size factor at `d` blocks when it shrinks with distance.
pub fn marker_scale(d: f64) -> f32 {
    (1.45 - (d.max(1.0).ln() as f32) / 6.5).clamp(0.6, 1.3)
}

/// A marker's opacity at `d` blocks: faint (never gone, so a waypoint just made shows) right
/// beside it, full from 8 blocks, fading again far away to a third at 2000 blocks.
pub fn fade(d: f64) -> u8 {
    let near = 0.4 + 0.6 * ((d - 2.0) / 6.0).clamp(0.0, 1.0);
    let far = 1.0 - ((d - 300.0) / 1700.0).clamp(0.0, 1.0) * 0.66;
    (near * far * 255.0).round() as u8
}

/// A file or folder name from a world's id or a server's address: letters, digits, `-`, `_`
/// and `.` kept, the rest `_`, at most 80 characters.
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

// ---------------------------------------------------------------- the world map screen

/// What the map screen shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Page {
    #[default]
    Map,
    List {
        scroll: usize,
    },
    /// Editing waypoint `index` with its text boxes; with `draft`, the "New waypoint" box
    /// (the waypoint isn't in the book until Create, and `index` means nothing).
    Edit {
        index: usize,
        form: Form,
        draft: Option<Box<Draft>>,
    },
    /// Deleting these waypoints, once confirmed.
    Confirm {
        delete: Vec<usize>,
    },
    /// Renaming the current set.
    RenameSet {
        name: String,
    },
    /// Naming a new set.
    NewSet {
        name: String,
    },
    /// The settings, page `tab` of `PAGES`.
    Settings {
        tab: usize,
    },
}

impl Page {
    /// Keys type into a text box on this page.
    pub fn typing(&self) -> bool {
        match self {
            // The "New waypoint" box takes every key while it is open (none moves or acts).
            Page::Edit { form, draft, .. } => form.focus.is_some() || draft.is_some(),
            Page::NewSet { .. } | Page::RenameSet { .. } => true,
            _ => false,
        }
    }
}

/// A waypoint being made in the "New waypoint" box, and whether its Create or Cancel closes
/// the screen (it was opened with the key while playing) or goes back to the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub wp: Waypoint,
    pub close: bool,
}

impl Page {
    /// The "New waypoint" box for `wp`: the name box empty (its hint the name it will get)
    /// and focused.
    pub fn new_waypoint(wp: Waypoint, close: bool) -> Self {
        Page::Edit { index: 0, form: Form::of_new(&wp), draft: Some(Box::new(Draft { wp, close })) }
    }
}

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

/// What the waypoint list shows.
pub const LIST_VIEWS: [&str; 3] = ["All sets", "This set", "Death points"];

/// Which waypoints the world map shows.
pub const MAP_FILTERS: [&str; 3] = ["All", "This set", "None"];

/// The map screen's state between frames.
#[derive(Clone, Debug, Default)]
pub struct MapScreen {
    pub view: MapView,
    pub page: Page,
    /// A left press on the map: where, and whether it has moved enough to be a drag.
    pub press: Option<([f32; 2], bool)>,
    /// The view follows the player.
    pub follow: bool,
    /// Showing the layer under this height (`None`: the surface).
    pub slice: Option<i32>,
    pub grid: bool,
    pub slime: bool,
    /// Index in `MAP_FILTERS`.
    pub filter: usize,
    /// The waypoint list's search text, and whether keys type into it.
    pub search: String,
    pub searching: bool,
    /// Index in `LIST_VIEWS`.
    pub list_view: usize,
    /// Waypoints ticked in the list for the bulk buttons.
    pub selected: Vec<usize>,
    /// The right-click menu, while it is open.
    pub menu: Option<Menu>,
}

/// The world map's right-click menu: where it was opened (GUI), the block there and the
/// waypoint under the pointer, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub at: [f32; 2],
    pub block: [i32; 3],
    pub waypoint: Option<usize>,
}

/// A row of the right-click menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    WaypointHere,
    TeleportHere,
    Edit,
    Hide,
    Delete,
    TeleportTo,
    Close,
}

impl MenuItem {
    pub fn label(self) -> &'static str {
        match self {
            MenuItem::WaypointHere => "Waypoint here",
            MenuItem::TeleportHere => "Teleport here",
            MenuItem::Edit => "Edit waypoint",
            MenuItem::Hide => "Hide waypoint",
            MenuItem::Delete => "Delete waypoint",
            MenuItem::TeleportTo => "Teleport to it",
            MenuItem::Close => "Close",
        }
    }
}

/// The rows of `menu`: on a waypoint its own, else the spot's; the teleports only where the
/// player may use /tp (and a waypoint only in this dimension).
pub fn menu_items(menu: &Menu, may_teleport: bool, waypoint_here: bool) -> Vec<MenuItem> {
    let mut out = Vec::new();
    if menu.waypoint.is_some() {
        out.extend([MenuItem::Edit, MenuItem::Hide, MenuItem::Delete]);
        if may_teleport && waypoint_here {
            out.push(MenuItem::TeleportTo);
        }
    } else {
        out.push(MenuItem::WaypointHere);
        if may_teleport {
            out.push(MenuItem::TeleportHere);
        }
    }
    out.push(MenuItem::Close);
    out
}

/// The menu's rows on screen: each `[x, y, w, h]`, kept inside a `w` x `h` screen.
pub fn menu_rects(at: [f32; 2], rows: usize, w: f32, h: f32) -> Vec<[f32; 4]> {
    let (rw, rh) = (110.0, 14.0);
    let x = at[0].min(w - rw - 2.0).max(0.0);
    let y = at[1].min(h - rh * rows as f32 - 2.0).max(0.0);
    (0..rows).map(|i| [x, y + i as f32 * rh, rw, rh]).collect()
}

/// How far round a spot the map isn't known at (blocks) a known floor gives its height.
pub const SPOT_NEAR: i32 = 16;

/// The Y to stand at on block `x, z` of the map: on the floor known there (`floor` gives a
/// column's floor height); else a block over the highest floor known within `SPOT_NEAR`
/// blocks (unexplored ground beside a mountain or over a valley: not into the hill, and a fall
/// at worst); else `fallback` (nothing known near: where you are).
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
pub const CLICK_SLOP: f32 = 3.0;

impl MapScreen {
    /// Keys type into a text box now.
    pub fn typing(&self) -> bool {
        self.page.typing() || (self.searching && matches!(self.page, Page::List { .. }))
    }

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

    /// The press let go: whether it was a click (not a drag).
    pub fn release(&mut self) -> bool {
        matches!(self.press.take(), Some((_, false)))
    }

    /// A key pans the map: `dir` is (x, z) in steps of a tenth of the map's `rect`.
    pub fn key_pan(&mut self, rect: [f32; 4], dir: (i32, i32)) {
        self.follow = false;
        self.view.pan([-rect[2] / 10.0 * dir.0 as f32, -rect[3] / 10.0 * dir.1 as f32]);
    }
}

/// Rows of the waypoint list a page shows.
pub fn list_rows(height: f32) -> usize {
    (((height - 114.0) / 22.0).floor() as usize).max(1)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        assert_eq!(Settings::read(|_| None), Settings::default());
        let mut s = Settings::default();
        for b in 0..Settings::COUNT {
            s.click(b);
        }
        assert_eq!(s.labels().len(), Settings::COUNT);
        let stored = s.stored();
        let read = Settings::read(|k| stored.iter().find(|p| p.0 == k).map(|p| p.1.clone()));
        assert_eq!(read, s);
        assert!(!s.minimap && s.corner == 2 && s.size == 3 && s.zoom == 5 && s.round && !s.textured);
        assert_eq!(Settings::read(|k| (k == "corner").then(|| "9".into())).corner, 1);
        assert!(Settings::default().labels().contains(&"Opacity: 100%".to_owned()));
        // Pages: every setting on one of the first five, the keys' page has none.
        let d = Settings::default();
        let total: usize = (0..PAGES.len()).map(|p| d.page(p).len()).sum();
        assert_eq!(total, Settings::COUNT);
        assert!(d.page(KEYS_PAGE).is_empty() && !d.page(KEYS_PAGE - 1).is_empty());
        let mut b = Settings::default();
        let opacity = b.labels().iter().position(|l| l.starts_with("Opacity")).unwrap();
        b.click_dir(opacity, true);
        assert_eq!(b.opacity, OPACITY.len() - 1, "back wraps round");
        assert_eq!(d.map_zooms(), (0, MAP_ZOOM_STEPS - 1));
        assert_eq!(d.info_letters(), vec!['C', 'B', 'F', 'L', 'T', 'D', 'K', 'O', 'A', 'W', 'R']);
        // 2.0's settings: an old flag kept as an index falls back to the default.
        assert_eq!(Settings::read(|k| (k == "cave").then(|| "true".into())).cave, 0);
        let mut z = Settings::default();
        assert!(z.zoom_step(true));
        assert_eq!(z.zoom, 3);
        z.zoom = 0;
        assert!(!z.zoom_step(true));
    }

    #[test]
    fn mob_kinds_and_arrows() {
        assert_eq!(dot_kind("minecraft:player", "misc"), DotKind::Player);
        assert_eq!(dot_kind("minecraft:zombie", "monster"), DotKind::Hostile);
        assert_eq!(dot_kind("minecraft:cow", "creature"), DotKind::Passive);
        assert_eq!(dot_kind("minecraft:armor_stand", "misc"), DotKind::Other);
        let s = Settings { passive: false, ..Settings::default() };
        assert!(!s.shows(DotKind::Passive) && s.shows(DotKind::Hostile) && !s.shows(DotKind::Other));
        assert!(Settings { others: true, ..s }.shows(DotKind::Other));
        let a = arrow([0.0, 0.0], 0.0, 4.0);
        assert!(a[0][1] > 3.9 && a[0][0].abs() < 1e-4, "yaw 0 points down");
    }

    #[test]
    fn faces_are_cut_from_the_front() {
        let n = [0.0, 0.0, -1.0];
        let side = ([1.0, 0.0, 0.0], [[0.0, 0.0], [0.1, 0.0], [0.1, 0.1], [0.0, 0.1]]);
        let small = (n, [[0.5, 0.5], [0.55, 0.5], [0.55, 0.55], [0.5, 0.55]]);
        let big = (n, [[0.125, 0.125], [0.25, 0.125], [0.25, 0.25], [0.125, 0.25]]);
        assert_eq!(front(&[side, small, big]), Some([0.125, 0.125, 0.25, 0.25]));
        assert_eq!(front(&[side]), None);
        // A 64x64 texture: red face at 8..16, a half-clear blue hat at 40..48.
        let mut tex = vec![0u8; 64 * 64 * 4];
        for y in 8..16 {
            for x in 8..16 {
                let o = (y * 64 + x) * 4;
                tex[o..o + 4].copy_from_slice(&[255, 0, 0, 255]);
                let h = (y * 64 + x + 32) * 4;
                if x < 12 {
                    tex[h..h + 4].copy_from_slice(&[0, 0, 255, 255]);
                }
            }
        }
        let (w, h, cell) = cut_icon(64, 64, &tex, [0.125, 0.125, 0.25, 0.25], Some([0.625, 0.125, 0.75, 0.25])).unwrap();
        assert_eq!((w, h), (8, 8));
        assert_eq!(&cell[..4], &[0, 0, 255, 255]);
        assert_eq!(&cell[7 * 4..7 * 4 + 4], &[255, 0, 0, 255]);
        assert_eq!(cell[8 * 4 + 3], 0, "outside the face is clear");
        assert!(cut_icon(64, 64, &tex, [0.5, 0.5, 0.6, 0.6], None).is_none(), "clear");
        let (w, h, _) = cut_icon(64, 64, &tex, [0.0, 0.0, 1.0, 0.5], None).unwrap();
        assert_eq!((w, h), (16, 8), "bigger faces shrink to fit");
        assert_eq!(icon_origin(65), [16, ICONS_Y + 16]);
        assert_eq!(model_key("main"), "minecraft:player#main");
        assert_eq!(model_key("llama#main"), "minecraft:llama#main");
        assert_eq!(model_key("mymod:golem#main"), "mymod:golem#main");
        let s = Settings { range: 3, ..Settings::default() };
        assert_eq!(s.max_range(), Some(64.0));
        assert_eq!(Settings::default().max_range(), None);
        assert_eq!(Settings::default().dot_rgb(DotKind::Hostile), 0xFF5555);
        assert_eq!(Settings::default().dot_rgb(DotKind::Player), 0xFFFFFF);
    }

    #[test]
    fn colours_average_and_tint() {
        let sq = [255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(average(2, 2, &sq), Some(0x7F007F));
        // An animated strip: the first frame only.
        let strip = [10, 20, 30, 255, 200, 200, 200, 255];
        assert_eq!(average(1, 2, &strip), Some(0x0A141E));
        assert_eq!(average(1, 1, &[1, 2, 3, 0]), None);
        assert_eq!(average(0, 0, &[]), None);
        assert_eq!(tinted(0xFFFFFF, 0x7FB238), 0x7FB238);
        assert_eq!(tinted(0x808080, 0xFF0000), 0x800000);
        let grass = BlockColour { rgb: 0x9C9C9C, tint: Tint::Grass, known: true };
        let tints = BiomeTints { grass: 0x91BD59, foliage: 0, dry_foliage: 0, water: 0x3F76E4 };
        assert_eq!(block_rgb(&grass, &tints), tinted(0x9C9C9C, 0x91BD59));
        let stone = BlockColour { rgb: 0x7F7F7F, tint: Tint::None, known: true };
        assert_eq!(block_rgb(&stone, &tints), 0x7F7F7F);
    }

    #[test]
    fn minimap_points_and_compass() {
        assert_eq!(minimap_point(4.0, 0.0, 0.0, false, 1.0), (4.0, 0.0));
        assert_eq!(minimap_point(0.0, 4.0, 0.0, false, 1.0), (0.0, 4.0));
        let (u, v) = minimap_point(0.0, 4.0, 0.0, true, 1.0);
        assert!(u.abs() < 1e-5 && (v + 4.0).abs() < 1e-5);
        let c = compass(0.0, false);
        assert_eq!(c[0].0, 'N');
        assert!((c[0].2 + 1.0).abs() < 1e-5, "north is up");
        assert!((c[1].1 - 1.0).abs() < 1e-5, "east is right");
        let t = compass(0.0, true);
        assert!((t[0].2 - 1.0).abs() < 1e-5, "facing south, north is down");
        assert_eq!(facing(180.0), "Facing North (-Z)");
        assert_eq!(facing(-90.0), "Facing East (+X)");
        assert_eq!(game_clock(0, false), "06:00");
        assert_eq!(game_clock(18000, false), "00:00");
        assert_eq!(game_clock(6500, false), "12:30");
        assert_eq!(game_clock(6500, true), "12:30 PM");
        assert_eq!(game_clock(18000, true), "12:00 AM");
        assert_eq!(hm(15, 5, true), "3:05 PM");
        assert_eq!(hm(9, 7, false), "09:07");
        assert_eq!((wrap_degrees(190.0), wrap_degrees(-90.0)), (-170.0, -90.0));
        assert_eq!(chunk_line([-1, 64, 33]), "Chunk -1 2 (15 1)");
        assert_eq!((weather(0.0, 0.0), weather(1.0, 0.0), weather(1.0, 1.0)), ("Clear", "Rain", "Thunder"));
    }

    #[test]
    fn clamping_keeps_points_on_the_map() {
        assert_eq!(clamp_to_map((3.0, 4.0), 10.0, true), ((3.0, 4.0), false));
        let ((x, y), moved) = clamp_to_map((30.0, 40.0), 10.0, true);
        assert!(moved && (x - 6.0).abs() < 1e-5 && (y - 8.0).abs() < 1e-5);
        let ((x, y), moved) = clamp_to_map((20.0, 5.0), 10.0, false);
        assert!(moved && x == 10.0 && y == 2.5);
    }

    fn tile(c: u8, h: i16, rgb: u32) -> Tile {
        Tile { color: [c; 256], rgb: [rgb; 256], height: [h; 256], depth: [0; 256], kind: [FLOOR; 256] }
    }

    #[test]
    fn dirty_regions_encode_as_each_region_alone() {
        let mut s = Store::default();
        s.put((-1, 2), tile(3, 70, 0x123456));
        s.put((-32, 33), tile(7, -10, 0));
        s.put((0, 0), tile(1, 64, 0));
        s.put((31, 31), tile(2, 65, 0));
        let mut want: Vec<_> = s.dirty.iter().map(|r| (*r, s.encode_region(*r))).collect();
        let mut got = s.encode_dirty();
        want.sort();
        got.sort();
        assert_eq!(got, want);
        assert!(s.dirty.is_empty());
        assert!(s.encode_dirty().is_empty());
    }

    #[test]
    fn regions_round_trip_and_read_the_first_version() {
        let mut s = Store::default();
        assert!(s.put((-1, 2), tile(3, 70, 0x123456)));
        assert!(!s.put((-1, 2), tile(3, 70, 0x123456)));
        assert!(!s.put((5, 5), tile(0, 0, 0)));
        assert!(s.put((-32, 33), tile(7, -10, 0)));
        assert_eq!(s.dirty.len(), 2);
        let bytes = s.encode_region((-1, 0));
        let back = decode_region((-1, 0), &bytes).unwrap();
        assert_eq!(back, vec![((-1, 2), tile(3, 70, 0x123456))]);
        assert_eq!(decode_region((0, 0), &bytes[..bytes.len() - 1]), None);
        let mut v1 = b"LMR1".to_vec();
        v1.extend_from_slice(&1u32.to_le_bytes());
        v1.extend_from_slice(&[1, 2]);
        v1.extend_from_slice(&[5; 256]);
        v1.extend_from_slice(&[0; 512]);
        v1.extend_from_slice(&[0; 256]);
        let old = decode_region((0, 0), &v1).unwrap();
        assert_eq!(old[0].0, (1, 2));
        assert_eq!((old[0].1.rgb[0], old[0].1.color[0]), (0, 5));
        assert_eq!(region_from_file(&region_file((-3, 12))), Some((-3, 12)));
        let mut t = Store::default();
        t.put((-1, 2), tile(9, 9, 1));
        t.merge_loaded(back);
        assert_eq!(t.tiles[&(-1, 2)].color[0], 9);
        assert_eq!(t.column(-16, 32).map(|c| c.height), Some(9));
    }

    #[test]
    fn shading_lights_rises_and_darkens_water() {
        let palette = [0, 0x808080, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x4040FF];
        let look = Look { textured: true, shading: true, legible: true, ..Look::default() };
        let col = |h: i16, rgb: u32| Col { color: 1, rgb, height: h, depth: 0, kind: FLOOR };
        assert_eq!(shade(&palette, look, col(64, 0), Some(64), Some(64)), [128, 128, 128, 255]);
        assert!(shade(&palette, look, col(66, 0), Some(64), Some(64))[0] > 128);
        assert!(shade(&palette, look, col(62, 0), Some(64), Some(64))[0] < 128);
        assert_eq!(shade(&palette, look, col(64, 0x00FF00), None, None), [0, 255, 0, 255]);
        let flat = Look { textured: false, shading: false, legible: true, ..Look::default() };
        assert_eq!(shade(&palette, flat, col(70, 0x00FF00), Some(10), None), [128, 128, 128, 255]);
        let water = |d: u8| Col { color: WATER, rgb: 0, height: 60, depth: d, kind: FLOOR };
        assert!(shade(&palette, look, water(10), None, None)[2] < shade(&palette, look, water(1), None, None)[2]);
    }

    #[test]
    fn the_view_pans_zooms_and_maps_points_both_ways() {
        let rect = [0.0, 20.0, 400.0, 200.0];
        let mut v = MapView { x: 100.0, z: -50.0, zoom: MAP_ZOOM_ONE };
        assert_eq!(v.world_at(rect, [200.0, 120.0]), (100.0, -50.0));
        let p = v.screen_of(rect, 110.0, -45.0);
        assert_eq!(v.world_at(rect, p), (110.0, -45.0));
        v.pan([10.0, -4.0]);
        assert_eq!((v.x, v.z), (90.0, -46.0));
        let under = v.world_at(rect, [50.0, 60.0]);
        v.zoom_at(rect, [50.0, 60.0], 1, 0, MAP_ZOOM_STEPS - 1);
        assert_eq!(v.zoom, MAP_ZOOM_ONE - 1);
        let after = v.world_at(rect, [50.0, 60.0]);
        assert!((under.0 - after.0).abs() < 1e-9 && (under.1 - after.1).abs() < 1e-9);
        let (x0, z0, x1, z1) = v.bounds(rect);
        assert!(x0 < x1 && z0 < z1);
    }

    #[test]
    fn presses_drags_and_keys() {
        let mut s = MapScreen { follow: true, ..MapScreen::default() };
        s.press = Some(([10.0, 10.0], false));
        s.drag([11.0, 11.0], [1.0, 1.0]);
        assert!(s.follow);
        assert!(s.release());
        s.press = Some(([10.0, 10.0], false));
        s.drag([20.0, 10.0], [10.0, 0.0]);
        assert_eq!(s.view.x, -10.0);
        assert!(!s.follow);
        assert!(!s.release());
        s.key_pan([0.0, 0.0, 100.0, 50.0], (1, 0));
        assert_eq!(s.view.x, 0.0);
    }

    #[test]
    fn composing_draws_known_ground() {
        let palette = [0u32, 0xFF0000];
        let mut s = Store::default();
        s.put((0, 0), tile(1, 64, 0));
        let look = Look { textured: true, shading: true, legible: true, ..Look::default() };
        let view = MapView { x: 0.0, z: 0.0, zoom: MAP_ZOOM_ONE };
        let mut out = Vec::new();
        compose(|x, z| s.column(x, z), &palette, look, &view, 64, 64, 1.0, &mut out);
        let at = |o: &Vec<u8>, i: usize, j: usize| o[(j * 64 + i) * 4..(j * 64 + i) * 4 + 4].to_vec();
        assert_eq!(at(&out, 32, 32), vec![255, 0, 0, 255]);
        assert_eq!(at(&out, 0, 0), UNKNOWN.to_vec());
        assert_eq!(map_zoom(MAP_ZOOM_ONE), 1.0);
    }

    #[test]
    fn cells_are_drawn_cached_and_marked_stale() {
        let palette = [0u32, 0xFF0000];
        let mut s = Store::default();
        s.put((0, 0), tile(1, 64, 0));
        assert_eq!(s.changed, vec![(0, 0)]);
        let look = Look { textured: true, shading: true, legible: true, ..Look::default() };
        let mut px = [0u8; 1024];
        let key = CellKey { source: 0, lod: 0, x: 0, z: 0 };
        assert!(!compose_cell(|x, z| s.column(x, z), &palette, look, None, key, &mut px));
        assert_eq!(&px[..4], &[255, 0, 0, 255]);
        let far = CellKey { x: 5, ..key };
        assert!(compose_cell(|x, z| s.column(x, z), &palette, look, None, far, &mut px));
        let mut c = CellCache::default();
        assert_eq!(c.get(key, 1), None);
        assert_eq!(c.wanted, vec![(key, true)]);
        assert_eq!(c.take_wanted(9), vec![key]);
        let slot = c.place(key, 1, false).unwrap();
        c.next_frame();
        assert_eq!(c.get(key, 1), Some((slot, false)));
        assert!(c.wanted.is_empty());
        c.chunk_changed(0, (0, 0));
        assert_eq!(c.get(key, 1), Some((slot, false)), "stale still shows");
        assert_eq!(c.wanted, vec![(key, false)]);
        c.next_frame();
        assert_eq!(c.get(key, 2), Some((slot, false)));
        assert_eq!(c.wanted.len(), 1, "a new epoch draws it again");
        // East of a changed chunk's edge: its shading looks west.
        let east = CellKey { x: 1, ..key };
        let e = c.place(east, 2, false).unwrap();
        c.next_frame();
        c.chunk_changed(0, (0, 0));
        assert_eq!(c.get(east, 2), Some((e, false)));
        assert_eq!(c.wanted.len(), 1);
        assert_eq!(slot_origin(65), [16, 16]);
        assert!(ICONS_Y + 8 * ICON <= 1024 && ICON_SLOTS == 512);
        assert_eq!(lod_for(1.0, 10, |_| 4), 0);
        assert_eq!(lod_for(5.0, 10, |_| 4), 2);
        assert_eq!(lod_for(0.5, 10, |l| if l < 3 { 99 } else { 1 }), 3);
    }

    #[test]
    fn clipping_keeps_texture_coordinates() {
        let sq = [[0.0, 0.0, 0.0, 0.0], [10.0, 0.0, 1.0, 0.0], [10.0, 10.0, 1.0, 1.0], [0.0, 10.0, 0.0, 1.0]];
        let clip = outline([0.0, 0.0], 5.0, false);
        let out = clip_polygon(&sq, &clip);
        assert_eq!(out.len(), 4);
        for v in &out {
            assert!(v[0] <= 5.0 + 1e-4 && v[1] <= 5.0 + 1e-4);
            assert!((v[2] - v[0] / 10.0).abs() < 1e-5 && (v[3] - v[1] / 10.0).abs() < 1e-5);
        }
        let rev: Vec<[f32; 2]> = clip.iter().rev().copied().collect();
        assert_eq!(clip_polygon(&sq, &rev).len(), 4, "either winding");
        assert!(clip_polygon(&sq, &outline([50.0, 50.0], 5.0, true)).is_empty());
        let round = clip_polygon(&sq, &outline([0.0, 0.0], 8.0, true));
        assert!(round.len() > 4);
        let quads = fan_quads(&round);
        assert_eq!(quads.len(), (round.len() - 1) / 2);
        assert_eq!(ring([0.0, 0.0], 1.0, 2.0).len(), 64);
        let circle = Shape::Circle([0.0, 0.0], 50.0);
        let ol = circle.polygon();
        let quad = |x: f32, y: f32| [[x, y, 0.0, 0.0], [x + 4.0, y, 1.0, 0.0], [x + 4.0, y + 4.0, 1.0, 1.0], [x, y + 4.0, 0.0, 1.0]];
        assert_eq!(circle.cut(&quad(0.0, 0.0), &ol), vec![quad(0.0, 0.0)], "inside: as it is");
        assert!(circle.cut(&quad(100.0, 0.0), &ol).is_empty(), "outside: nothing");
        assert!(!circle.cut(&quad(48.0, 0.0), &ol).is_empty(), "across the rim: clipped");
        let rect = Shape::Rect([0.0, 0.0, 10.0, 10.0]);
        let rl = rect.polygon();
        assert_eq!(rect.cut(&quad(2.0, 2.0), &rl).len(), 1);
        assert!(rect.cut(&quad(20.0, 2.0), &rl).is_empty());
        assert_eq!(rect.cut(&quad(8.0, 2.0), &rl)[0][1][0], 10.0);
    }

    #[test]
    fn biome_tints_blend_and_water_shows_the_ground() {
        let a = BiomeTints { grass: 0x000000, foliage: 0, dry_foliage: 0, water: 0 };
        let b = BiomeTints { grass: 0xFFFFFF, ..a };
        let mut grid = [a; 36];
        for z in 0..6 {
            for x in 3..6 {
                grid[z * 6 + x] = b;
            }
        }
        // West half dark, east half light: a smooth ramp between quart centres, no seam.
        let g = |x: i32| blend_tints(&grid, x, 8).grass & 255;
        assert_eq!(g(0), 0);
        assert_eq!(g(15), 255);
        assert!(g(7) > 0 && g(7) < 255 && g(6) < g(7) && g(7) < g(8));
        let shallow = water_over(0xFF0000, 0x0000FF, 0);
        let deep = water_over(0xFF0000, 0x0000FF, 20);
        assert!(shallow >> 16 > deep >> 16, "the ground shows less when deeper");
        assert!((deep & 255) < 255);
    }

    #[test]
    fn java_random_and_slime_chunks() {
        assert_eq!(JavaRandom::new(42).next_int(), -1170105035);
        let n = (0..100)
            .flat_map(|x| (0..100).map(move |z| (x, z)))
            .filter(|&(x, z)| slime_chunk(12345, x, z))
            .count();
        assert!((800..1200).contains(&n), "{n}");
    }

    #[test]
    fn new_waypoints_start_at_the_block_you_stand_in() {
        // Floored, not truncated: -0.3 is block -1.
        assert_eq!(block_at([-0.3, 64.0, -0.3]), [-1, 64, -1]);
        assert_eq!(block_at([0.3, 63.9999, 0.7]), [0, 63, 0]);
        assert_eq!(block_at([-16.0, -59.5, 15.99]), [-16, -60, 15]);
        assert_eq!(block_at([-1e-9, 0.0, -1.0]), [-1, 0, -1]);
        assert_eq!(block_at([1234.5, 70.2, -9876.5]), [1234, 70, -9877]);
        // The "New waypoint" box: a draft with the next name, its name box empty and typed into,
        // keys kept from the game; nothing is added until Create.
        let mut b = Book::default();
        b.add([0, 0, 0], OVERWORLD, 1);
        let mut d = b.draft(block_at([-0.3, 64.0, 2.5]), NETHER, 2);
        style_new(&mut d, 3, 1);
        assert_eq!((d.name.as_str(), d.pos, d.colour, d.symbol.as_str()), ("Waypoint 2", [-1, 64, 2], 2, "*"));
        assert_eq!(b.list.len(), 1);
        let page = Page::new_waypoint(d.clone(), true);
        let Page::Edit { form, draft: Some(draft), .. } = &page else { panic!() };
        assert_eq!((form.fields[F_NAME].as_str(), form.fields[F_X].as_str(), form.focus), ("", "-1", Some(F_NAME)));
        assert!(draft.close && page.typing());
        let mut unfocused = page.clone();
        if let Page::Edit { form, .. } = &mut unfocused {
            form.focus = None;
        }
        assert!(unfocused.typing(), "the box keeps keys from the game without a focus too");
        // Left empty, the name is the next one; typed, it is what was typed.
        let mut f = Form::of_new(&d);
        f.apply(&mut d);
        assert_eq!(d.name, "Waypoint 2");
        for c in "Base".chars() {
            f.type_char(c);
        }
        f.apply(&mut d);
        let i = b.insert(d);
        assert_eq!((b.list[i].name.as_str(), b.list[i].mark().as_str(), b.list[i].dim.as_str()), ("Base", "*", NETHER));
        let mut gone = b.draft([0; 3], OVERWORLD, 3);
        gone.set = "Removed".into();
        gone.name = " ".into();
        let j = b.insert(gone);
        assert_eq!((b.list[j].set.as_str(), b.list[j].name.as_str()), (DEFAULT_SET, "Waypoint 2"));
    }

    #[test]
    fn marker_and_name_defaults() {
        let d = Settings::default();
        assert!(d.markers && d.waypoints && d.wp_shape == 0 && WP_SHAPES[d.wp_shape] == "Diamond");
        assert_eq!(WP_RANGES[d.wp_range], "Any distance");
        assert!(d.names && !d.mob_names && !d.instant_add && d.tab_details && !d.always_details);
        // 2.0 kept its beams as `markers`: one turned off there doesn't hide 3.0's markers...
        let old = Settings::read(|k| (k == "markers").then(|| "false".into()));
        assert!(old.markers);
        // ...but one turned off since is kept.
        let mut s = Settings::default();
        s.markers = false;
        let stored = s.stored();
        assert!(stored.iter().any(|(k, v)| *k == SETTINGS_VERSION_KEY && v == "4"));
        let read = Settings::read(|k| stored.iter().find(|p| p.0 == k).map(|p| p.1.clone()));
        assert!(!read.markers);
        // 4: markers as Xaero's: none pinned to the edges, no converted Overworld/Nether ones,
        // distance only on the one looked at; settings saved by 3 get these once.
        assert!(!d.wp_edge && !d.convert && WP_NAMES[d.wp_dist] == "Looked at");
        let v3 = Settings::read(|k| match k {
            "wp_edge" | "convert" => Some("true".into()),
            SETTINGS_VERSION_KEY => Some("3".into()),
            _ => None,
        });
        assert!(!v3.wp_edge && !v3.convert);
        let mut kept = Settings::default();
        kept.wp_edge = true;
        let stored = kept.stored();
        assert!(Settings::read(|k| stored.iter().find(|p| p.0 == k).map(|p| p.1.clone())).wp_edge);
        let w = Waypoint { name: "nether hub".into(), ..Waypoint::default() };
        assert_eq!(w.mark(), "N");
        assert_eq!(Waypoint::default().mark(), "?");
    }

    #[test]
    fn png_is_well_formed() {
        let p = png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]);
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(&p[p.len() - 8..p.len() - 4], b"IEND");
        assert_eq!(&p[p.len() - 4..], &0xAE42_6082u32.to_be_bytes());
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn books_round_trip_with_sets_and_read_the_first_file() {
        let mut b = Book::default();
        assert_eq!(b.add([1, 64, -3], OVERWORLD, 10), 0);
        assert!(b.add_set("Mining"));
        assert!(!b.add_set("Mining"));
        b.add([5, 5, 5], NETHER, 20);
        b.sets[0].shown = false;
        b.list[1].name = "Base\twith tabs".into();
        let back = Book::from_text(&b.to_text());
        assert_eq!(back.sets.len(), 2);
        assert_eq!(back.current, 1);
        assert_eq!(back.list[1].set, "Mining");
        assert_eq!(back.list[1].name, "Base with tabs");
        let mut st = back.clone();
        st.style_new(0, 2, 1);
        assert_eq!((st.list[0].colour, st.list[0].mark().as_str()), (1, "*"));
        assert_eq!(Book::from_text(&st.to_text()).list[0].symbol, "*");
        assert_eq!(st.list[1].mark(), "B");
        // Every field of a waypoint survives the file.
        let mut full = Book::default();
        full.add([1, 2, 3], NETHER, 7);
        {
            let w = &mut full.list[0];
            w.custom = Some(0x12AB34);
            w.temporary = true;
            w.on_minimap = false;
            w.always_name = true;
        }
        assert_eq!(Book::from_text(&full.to_text()).list, full.list);
        assert_eq!(full.list[0].rgb(), 0x12AB34);
        assert_eq!(parse_hex("#3fa"), Some(0x33FFAA));
        assert_eq!(parse_hex("zz"), None);
        // Global ones go to their own file and come back marked.
        full.add([9, 9, 9], OVERWORLD, 8);
        full.list[1].global = true;
        let (own, all) = full.split_global();
        assert_eq!((own.list.len(), all.list.len()), (1, 1));
        let mut merged = own.clone();
        merged.merge_global(all);
        assert!(merged.list[1].global);
        assert!(merged.drop_temporary() && merged.list.len() == 1);
        let d = merged.duplicate(0, 50).unwrap();
        assert_eq!(merged.list[d].name, "Waypoint 2 copy");
        merged.add_set("Base");
        merged.move_to_set(&[0], "Base");
        assert!(merged.rename_set("Home"));
        assert_eq!(merged.list[0].set, "Home");
        merged.remove(&[0, 1]);
        assert!(merged.list.is_empty());
        assert!(!back.visible(&back.list[0]), "its set is hidden");
        assert!(back.visible(&back.list[1]));
        let old = Book::from_text(
            "# Lumos Map waypoints\nHome\t1\t2\t3\tminecraft:overworld\t4\tfalse\ttrue\n",
        );
        assert_eq!(old.list[0].set, DEFAULT_SET);
        assert_eq!(old.sets[0].name, DEFAULT_SET);
        assert_eq!(old.list[0].colour, 4);
        let mut r = back.clone();
        r.remove_set();
        assert!(r.list.iter().all(|w| w.set != "Mining") && r.sets.len() == 1);
        r.remove_set();
        assert_eq!(r.sets.len(), 1);
    }

    #[test]
    fn deaths_keep_the_last_few() {
        let mut b = Book::default();
        b.add([0, 0, 0], OVERWORLD, 1);
        for t in 0..5u64 {
            b.add_death([t as i32, 0, 0], OVERWORLD, 3, 100 + t);
        }
        let deaths: Vec<&Waypoint> = b.list.iter().filter(|w| w.death).collect();
        assert_eq!(deaths.len(), 3);
        assert_eq!(deaths.iter().find(|w| w.name == DEATH_NAME).unwrap().pos[0], 4);
        assert!(deaths.iter().any(|w| w.name == "Death 3" && w.pos[0] == 2));
        assert_eq!(b.list.iter().filter(|w| !w.death).count(), 1);
    }

    #[test]
    fn order_initials_conversion_and_fade() {
        let mut b = Book::default();
        b.add([100, 0, 0], OVERWORLD, 5);
        b.add([10, 0, 0], OVERWORLD, 9);
        b.list[0].name = "beta".into();
        b.list[1].name = "Alpha base".into();
        assert_eq!(b.order(0, None), vec![1, 0]);
        assert_eq!(b.order(1, Some([0.0, 0.0, 0.0])), vec![1, 0]);
        assert_eq!(b.order(2, None), vec![1, 0]);
        assert_eq!(b.list[1].letter(), "A");
        b.list[0].name = "  #3 east".into();
        assert_eq!(b.list[0].mark(), "3");
        b.list[0].name = "beta".into();
        assert_eq!(b.listed(0, None, "ALPHA", None), vec![1]);
        assert_eq!(b.listed(0, None, "", Some("Nope")), Vec::<usize>::new());
        assert_eq!(b.listed(0, None, " ", Some(DEFAULT_SET)), vec![1, 0]);
        assert!(b.set_shown(&[0, 1], false));
        assert!(!b.set_shown(&[0, 1], false));
        assert!(b.list.iter().all(|w| !w.shown));
        let mut f = Form::of(&b.list[0]);
        assert!(Page::Edit { index: 0, form: f.clone(), draft: None }.typing());
        f.focus = Some(F_X);
        f.fields[F_X].clear();
        for c in "-12a3".chars() {
            f.type_char(c);
        }
        assert_eq!(f.fields[F_X], "-123");
        f.focus = Some(F_SYMBOL);
        for c in "ABC".chars() {
            f.type_char(c);
        }
        f.focus = Some(F_HEX);
        for c in "00ff00".chars() {
            f.type_char(c);
        }
        let mut w = b.list[0].clone();
        f.apply(&mut w);
        assert_eq!((w.pos[0], w.symbol.as_str(), w.custom), (-123, "AB", Some(0x00FF00)));
        let mut ms = MapScreen { page: Page::List { scroll: 0 }, searching: true, ..MapScreen::default() };
        assert!(ms.typing());
        ms.page = Page::Map;
        assert!(!ms.typing());
        assert_eq!(converted([80, 64, -17], OVERWORLD, NETHER), Some([10, 64, -3]));
        assert_eq!(converted([10, 64, -3], NETHER, OVERWORLD), Some([80, 64, -24]));
        assert_eq!(converted([1, 2, 3], OVERWORLD, "minecraft:the_end"), None);
        assert!(b.list[0].place_in(NETHER, true).is_some());
        assert!(b.list[0].place_in(NETHER, false).is_none());
        assert_eq!(distance_text(12.4, 1), "12 blocks");
        assert_eq!(distance_text(12.4, 0), "12 m");
        assert!(marker_scale(5.0) > marker_scale(500.0));
        // An identity camera: x right, y up, w = 1.
        let id: [f32; 16] = std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 });
        let (p, front) = project(&id, &id, [0.5, 0.0, 0.0], 200.0, 100.0);
        assert!(front && (p[0] - 150.0).abs() < 1e-3 && (p[1] - 50.0).abs() < 1e-3);
        let (q, _, _) = to_edge([1000.0, 50.0], 200.0, 100.0, 10.0);
        assert_eq!(q, [190.0, 50.0]);
        let (r, _, pushed) = to_edge([100.0, 50.0], 200.0, 100.0, 10.0);
        assert!(!pushed && r == [100.0, 50.0]);
        assert!(fade(0.0) > 90, "a waypoint you stand on still shows");
        assert_eq!(fade(50.0), 255);
        assert!(fade(5000.0) < 100 && fade(5000.0) > 50);
    }

    #[test]
    fn chat_coordinates_and_sharing() {
        assert_eq!(chat_coords("meet at 120 64 -300 ok"), Some([120, 64, -300]));
        assert_eq!(chat_coords("x: 12, y: 70, z: -5"), Some([12, 70, -5]));
        assert_eq!(chat_coords("XYZ: 1.5 / 64.0 / -2.25"), Some([1, 64, -2]));
        assert_eq!(chat_coords("I have 3 apples"), None);
        assert_eq!(chat_coords("player123 said 5 6"), None);
        let mut b = Book::default();
        b.add([1, 2, 3], NETHER, 0);
        assert_eq!(share_text(&b.list[0]), "Waypoint 1: 1 2 3 (The Nether)");
        assert_eq!(chat_coords(&share_text(&b.list[0])), Some([1, 2, 3]));
    }

    #[test]
    fn sub_worlds_follow_the_spawn() {
        let mut s = SubWorlds::from_text("");
        assert_eq!(s.current_name(), "world-1");
        assert!(!s.follow_spawn([0, 0]), "the first world takes the first spawn");
        assert!(!s.follow_spawn([10, -20]), "near enough is the same");
        assert!(s.follow_spawn([5000, 5000]));
        assert_eq!(s.current_name(), "world-2");
        assert!(s.follow_spawn([3, 3]));
        assert_eq!(s.current_name(), "world-1");
        s.add(None);
        assert_eq!(s.current_name(), "world-3");
        let back = SubWorlds::from_text(&s.to_text());
        assert_eq!(back, s);
        s.next();
        assert_eq!(s.current_name(), "world-1");
    }

    #[test]
    fn names() {
        assert_eq!(format_distance(12.4), "12 m");
        assert_eq!(format_distance(1450.0), "1.4 km");
        assert_eq!(folder_name("mc.example.com:25565"), "mc.example.com_25565");
        assert_eq!(folder_name(".."), "_");
        assert_eq!(pretty_id("minecraft:dark_forest"), "Dark Forest");
        assert_eq!(list_rows(240.0), 5);
    }

    #[test]
    fn waypoints_flat_vanishing_and_reached_deaths() {
        let mut b = Book::default();
        let i = b.add([10, 64, 10], OVERWORLD, 0);
        b.list[i].two_d = true;
        b.list[i].vanish = true;
        let back = Book::from_text(&b.to_text());
        assert!(back.list[0].two_d && back.list[0].vanish);
        assert_eq!(b.list[i].place_at(OVERWORLD, false, 100.0), Some([10.5, 100.0, 10.5]));
        b.add_death([0, 64, 0], OVERWORLD, 3, 100);
        b.add([50, 64, 50], OVERWORLD, 0);
        // Far above the 2D one: still reached (flat). The new death point is too young.
        assert_eq!(b.remove_reached([11.0, 120.0, 10.0], OVERWORLD, true, 105, 10), vec!["Waypoint 1".to_owned()]);
        assert!(b.remove_reached([0.5, 64.0, 0.5], OVERWORLD, true, 105, 10).is_empty());
        assert!(b.remove_reached([0.5, 64.0, 0.5], OVERWORLD, false, 200, 10).is_empty(), "setting off");
        assert_eq!(b.remove_reached([0.5, 64.0, 0.5], OVERWORLD, true, 200, 10), vec![DEATH_NAME.to_owned()]);
        assert_eq!(b.list.len(), 1, "others stay");
        // Render all sets shows a hidden set's waypoints for now.
        b.sets[0].shown = false;
        assert!(!b.visible(&b.list[0]));
        b.all_sets = true;
        assert!(b.visible(&b.list[0]));
    }

    #[test]
    fn entities_fade_and_mark_by_height() {
        let d = Settings::default();
        assert_eq!(d.depth_look(3.0), Some((255, None)));
        assert_eq!(d.depth_look(9.0), Some((95, Some(true))), "Auto fades past 8 blocks");
        assert_eq!(d.depth_look(-40.0), Some((95, Some(false))));
        let s = Settings { depth_fade: 1, height_limit: 1, show_depth: false, ..d };
        assert_eq!(s.depth_look(5.0), Some((95, None)));
        assert_eq!(s.depth_look(-17.0), None, "past the height limit: not drawn");
        assert_eq!(DOT_LIMITS[d.dot_limit], 400);
    }

    #[test]
    fn overlay_runs_and_lines() {
        assert_eq!(runs(0), vec![]);
        assert_eq!(runs(0xFFFF), vec![(0, 16)]);
        assert_eq!(runs(0b1000_0000_0000_0110), vec![(1, 2), (15, 1)]);
        let q = line_quad([0.0, 0.0], [10.0, 0.0], 2.0);
        assert_eq!(q, [[0.0, 1.0], [10.0, 1.0], [10.0, -1.0], [0.0, -1.0]]);
        assert_eq!(LIGHT_MAXES[Settings::default().light_max], 7);
    }

    #[test]
    fn the_region_cache_keeps_what_is_near_and_unsaved() {
        let near = regions_near(&[(0.0, 0.0), (5000.0, -10.0)], 1);
        assert_eq!(near.len(), 18);
        assert!(near.contains(&(-1, -1)) && near.contains(&(10, 0)) && !near.contains(&(5, 0)));
        let mut s = Store::default();
        s.put((0, 0), tile(1, 64, 1));
        s.put((100, 0), tile(1, 64, 1));
        s.put((200, 0), tile(1, 64, 1));
        s.dirty.clear();
        s.put((200, 1), tile(2, 64, 1)); // region (6, 0) changed since saved
        let epoch = s.epoch;
        assert!(s.let_go(|r| near.contains(&r)));
        assert!(s.tiles.contains_key(&(0, 0)), "near: kept");
        assert!(!s.tiles.contains_key(&(100, 0)), "far and saved: let go");
        assert!(s.tiles.contains_key(&(200, 0)), "far but not saved yet: kept");
        assert!(s.epoch > epoch);
        assert!(!s.let_go(|r| near.contains(&r)));
    }

    #[test]
    fn the_map_menu_and_footsteps() {
        let spot = Menu { at: [5.0, 5.0], block: [10, 64, -3], waypoint: None };
        assert_eq!(menu_items(&spot, false, false), vec![MenuItem::WaypointHere, MenuItem::Close]);
        assert_eq!(menu_items(&spot, true, false), vec![MenuItem::WaypointHere, MenuItem::TeleportHere, MenuItem::Close]);
        let wp = Menu { waypoint: Some(2), ..spot.clone() };
        assert!(!menu_items(&wp, true, false).contains(&MenuItem::TeleportTo), "not across dimensions");
        assert!(menu_items(&wp, true, true).contains(&MenuItem::TeleportTo));
        assert!(!menu_items(&wp, false, true).contains(&MenuItem::TeleportTo), "no teleport without cheats");
        let r = menu_rects([390.0, 290.0], 3, 400.0, 300.0);
        assert!(r[0][0] + r[0][2] <= 400.0 && r[2][1] + r[2][3] <= 300.0, "kept on screen");
        assert_eq!(tp_command([-3, 70, 8]), "tp @s -2.5 70 8.5");
        // Its Y: the floor there, else the highest known nearby, else where you are.
        let floor = |x: i32, z: i32| match (x, z) {
            (0, 0) => Some(63),
            (5, -3) => Some(80),
            (-10, 10) => Some(72),
            (100, 100) => Some(40),
            _ => None,
        };
        assert_eq!(spot_y(floor, 0, 0, 12), 64, "known: on its floor");
        assert_eq!(spot_y(floor, 2, 2, 12), 81, "unknown: over the highest known nearby");
        assert_eq!(spot_y(floor, 500, 500, 12), 12, "nothing known near: your own Y");
        assert_eq!(spot_y(floor, 100 + SPOT_NEAR + 1, 100, 12), 12, "too far to count");
        let mut f = Footsteps::default();
        assert!(f.step(0.5, 0.5));
        assert!(!f.step(1.4, 0.9), "less than 2 blocks on");
        assert!(f.step(2.5, 0.5));
        for i in 0..2000 {
            f.step(f64::from(i) * 3.0, 0.0);
        }
        assert_eq!(f.points.len(), FOOTSTEPS);
        assert_eq!(Footsteps::from_text(&f.to_text()), f);
    }

    #[test]
    fn cave_layers_save_with_their_band_and_old_files_still_read() {
        assert_eq!((band_of(24), band_of(16), band_of(15), band_of(-1), band_of(-16), band_of(-17)), (1, 1, 0, -1, -1, -2));
        assert_eq!((cave_folder(Some(-2)).as_str(), cave_folder(None).as_str()), ("caves/-2", "caves/full"));
        let mut cave = Store { band: Some(-2), ..Store::default() };
        let mut t = tile(5, -30, 0xABCDEF);
        t.kind[7] = WALL;
        t.kind[8] = DEEP;
        assert!(cave.put((-40, 3), t.clone()));
        let bytes = cave.encode_region((-2, 0));
        assert_eq!(&bytes[..4], b"LMR3");
        assert_eq!(region_band(&bytes), Some(-2));
        assert_eq!(decode_region((-2, 0), &bytes).unwrap(), vec![((-40, 3), t)]);
        assert_eq!(decode_region((-2, 0), &bytes[..bytes.len() - 1]), None);
        // The surface keeps writing LMR2, which 3.0 reads; its columns are floors.
        let mut surface = Store::default();
        surface.put((0, 0), tile(1, 64, 1));
        let old = surface.encode_region((0, 0));
        assert_eq!((&old[..4], region_band(&old)), (&b"LMR2"[..], None));
        assert!(decode_region((0, 0), &old).unwrap()[0].1.kind.iter().all(|k| *k == FLOOR));
        // Cave map types per dimension, the Nether's Full by default.
        let mut types = CaveTypes::default();
        assert_eq!(types.get(NETHER, true), CaveType::Full);
        assert_eq!(types.get(OVERWORLD, false), CaveType::Off);
        types.set(OVERWORLD, CaveType::Layered);
        let back = CaveTypes::from_text(&types.to_text());
        assert_eq!(back.get(OVERWORLD, false), CaveType::Layered);
        assert_eq!(back.get(NETHER, true), CaveType::Full, "changing one leaves the other");
        assert_eq!(CaveTypes::from_text("x\tNope\n"), CaveTypes::default());
    }

    #[test]
    fn caves_show_rock_drops_and_darker_deep_floors() {
        let palette = [0u32, 0x808080, 0, 0, 0xFF0000];
        let look = Look { textured: true, shading: false, legible: false, ..Look::default() };
        let cave = Some(CaveShade { start: 40, depth: 30 });
        // Columns: x < 0 rock, x == 0 a floor at 40, x == 1 floor 70 below (past the depth),
        // x == 2 lava deep down, x >= 3 a drop.
        let col = |x: i32, _z: i32| -> Option<Col> {
            let c = |color, height, kind| Some(Col { color, rgb: 0, height, depth: 0, kind });
            match x {
                ..0 => c(1, 40, WALL),
                0 => c(1, 40, FLOOR),
                1 => c(1, -30, FLOOR),
                2 => c(FIRE, 10, FLOOR),
                _ => c(BLACK, 10, DEEP),
            }
        };
        let mut out = [0u8; 1024];
        compose_cell(col, &palette, look, cave, CellKey { source: 1, lod: 0, x: -1, z: 0 }, &mut out);
        let at = |out: &[u8; 1024], i: usize| [out[i * 4], out[i * 4 + 1], out[i * 4 + 2]];
        // Cell -1 covers x -16..-1: rock, with the lighter edge next to the floor at x 0.
        assert_eq!(at(&out, 0), [0x2A, 0x2A, 0x2E]);
        assert_eq!(at(&out, 15), [0x4A, 0x4A, 0x50]);
        compose_cell(col, &palette, look, cave, CellKey { source: 1, lod: 0, x: 0, z: 0 }, &mut out);
        assert_eq!(at(&out, 0), [128, 128, 128], "the floor at your feet keeps its colour");
        assert_eq!(at(&out, 1), [42, 42, 42], "past the depth: about half (and shaded: it drops away west)");
        assert_eq!(at(&out, 2), [255, 0, 0], "lava stays bright");
        assert_eq!(at(&out, 3), [0x10, 0x10, 0x14], "a drop is darker than rock");
        // Legible caves darken by 30% at most.
        compose_cell(col, &palette, Look { legible: true, ..look }, cave, CellKey { source: 1, lod: 0, x: 0, z: 0 }, &mut out);
        assert_eq!(at(&out, 1), [65, 65, 65]);
        assert!((depth_factor(15, 30, false) - 0.725).abs() < 1e-6);
        assert_eq!(depth_factor(-3, 30, false), 1.0, "floors over the start are not darkened");
        // The cave zoom steps the minimap in only while the cave view is on.
        let s = Settings { zoom: 4, cave_zoom: 2, ..Settings::default() };
        assert_eq!((s.zoom_now(false), s.zoom_now(true)), (4, 2));
        assert_eq!(Settings { zoom: 1, ..s }.zoom_now(true), 0);
        // The enlarged minimap zooms out by its own setting on top.
        assert_eq!(s.bpp_now(false, false), 1.0);
        assert_eq!(s.bpp_now(true, true), 1.0, "x2 in the cave, x2 out enlarged");
        assert_eq!(Settings { enlarge_zoom: 2, ..s }.bpp_now(false, true), 4.0);
    }

    #[test]
    fn cave_settings_read_3_0_files_and_keep_a_start_y() {
        // 3.0's "Always" (index 1) is Manual now; the new settings start at their defaults.
        let old = Settings::read(|k| (k == "cave").then(|| "1".into()));
        assert_eq!(CAVE_MODES[old.cave], "Manual");
        assert_eq!((old.roof_side(), old.depth_blocks(), old.above_blocks(), old.cave_start), (3, 30, 0, None));
        let mut s = Settings { cave_start: Some(-23), ..Settings::default() };
        let stored = s.stored();
        assert!(stored.iter().any(|(k, v)| *k == "cave_start" && v == "-23"));
        let back = Settings::read(|k| stored.iter().find(|p| p.0 == k).map(|p| p.1.clone()));
        assert_eq!(back.cave_start, Some(-23));
        s.cave_start = None;
        let stored = s.stored();
        assert!(stored.iter().any(|(k, v)| *k == "cave_start" && v == "auto"));
        assert_eq!(Settings::read(|k| (k == "cave_start").then(|| "junk".into())).cave_start, None);
        // The button steps through the presets and back to "where you are".
        assert_eq!(next_start(None, false), Some(96));
        assert_eq!(next_start(Some(96), true), None);
        assert_eq!(next_start(None, true), Some(-48));
        assert_eq!(next_start(Some(-48), false), None);
        assert_eq!(next_start(Some(50), false), Some(48), "a Y set with the key: the preset under it");
        assert_eq!(next_start(Some(50), true), Some(64));
        assert_eq!(next_start(Some(-60), true), Some(-48));
        assert_eq!(next_start(Some(-60), false), None);
    }

    #[test]
    fn the_cave_view_needs_a_whole_roof_held_a_second() {
        // 3x3: every column needs a roof (at any height); one open column is not a cave.
        let mut nine = vec![Some(70); 9];
        nine[4] = Some(66);
        assert!(roofed(&nine));
        nine[8] = None;
        assert!(!roofed(&nine), "an overhang over part of the square doesn't count");
        assert!(!roofed(&[]));
        assert!(roofed(&[Some(80)]));
        // The switch follows only after 20 ticks of the new state.
        let mut sw = CaveSwitch::default();
        assert!(!sw.step(true, 100));
        assert!(!sw.step(true, 119));
        assert!(sw.step(true, 120));
        // Flicker at a cave mouth (in and out every few ticks) never switches it back.
        for t in 0..40 {
            assert!(sw.step(t % 6 < 3, 121 + t));
        }
        // Out for a full second: off.
        assert!(sw.step(true, 170));
        assert!(sw.step(false, 200));
        assert!(!sw.step(false, 220));
        // Which slice: the head's block, the depth and above settings; Manual's own Y; the
        // Nether roof's top from above it.
        let d = Settings::default();
        assert_eq!(cave_view(&d, 20, false, None), None);
        assert_eq!(cave_view(&d, 20, true, None), Some(CaveView::Slice { start: 20, depth: 30, above: 0 }));
        assert_eq!(cave_view(&d, 40, false, Some(127)), Some(CaveView::Slice { start: 40, depth: 30, above: 0 }));
        assert_eq!(cave_view(&d, 130, true, Some(127)), Some(CaveView::RoofTop));
        let manual = Settings { cave: 1, cave_start: Some(-10), cave_depth: 0, cave_above: 2, ..d };
        assert_eq!(cave_view(&manual, 70, false, None), Some(CaveView::Slice { start: -10, depth: 8, above: 4 }));
        assert_eq!(cave_view(&Settings { cave_start: None, ..manual }, 70, false, None), Some(CaveView::Slice { start: 70, depth: 8, above: 4 }));
        assert_eq!(cave_view(&Settings { cave: 2, ..d }, 20, true, Some(127)), None);
        assert_eq!(cave_view(&Settings { fair_play: true, cave: 1, ..d }, 20, true, None), None, "fair play: no caves");
    }

    #[test]
    fn new_layers_never_reuse_an_epoch() {
        // A cave layer made after another (cave view off, then on) gets an epoch none had.
        let a = fresh_epoch();
        let b = fresh_epoch();
        assert_ne!(a, b);
        assert!(b > a);
        // The cache's epoch for a layer and its band's saved layer: pairs that add up the same
        // still differ, and a new layer's first epoch is never an old one's.
        assert_ne!(epoch_mix(&[1, 5, 0]), epoch_mix(&[1, 3, 2]));
        assert_ne!(epoch_mix(&[1, a, 0]), epoch_mix(&[1, b, 0]));
        assert_ne!(epoch_mix(&[1, 2, 0]), epoch_mix(&[2, 2, 0]), "a new cell generation draws again");
        assert_eq!(epoch_mix(&[7, 8, 9]), epoch_mix(&[7, 8, 9]));
        let mut c = CellCache::default();
        let key = CellKey { source: 1, lod: 0, x: 0, z: 0 };
        let old = epoch_mix(&[0, a, 0]);
        assert!(c.get(key, old).is_none());
        let slot = c.place(key, old, false);
        assert!(slot.is_some());
        // The new layer's epoch: the old layer's cell is drawn again before it shows as current.
        let new = epoch_mix(&[0, b, 0]);
        c.next_frame();
        assert!(c.get(key, new).is_some(), "the same layer's stale cell shows until redrawn");
        assert!(c.take_wanted(16).contains(&key), "the old layer's cell is redrawn for the new one");
        // A new layer forgets the old one's cells: nothing shows until drawn for it.
        let other = CellKey { source: 0, ..key };
        c.place(other, 1, false);
        c.forget(1);
        c.next_frame();
        assert!(c.get(key, new).is_none());
        assert!(c.get(other, 1).is_some(), "other sources keep theirs");
    }

    #[test]
    fn minimap_look_extras() {
        // Every extra is off by default: the look stays as it was.
        let d = Settings::default();
        assert!(!d.auto_size && !d.depth_shade && !d.night && !d.palette_biomes && !d.glass);
        assert_eq!(FRAME_COLOURS[d.frame_colour].1, 0x0C_0C0E, "the rim it always had");
        let back = Settings::read(|k| d.stored().into_iter().find(|(n, _)| *n == k).map(|(_, v)| v));
        assert_eq!(back, d);
        // Terrain depth: none at or over you, darker below, capped.
        assert_eq!(surface_depth_factor(-5), 1.0);
        assert_eq!(surface_depth_factor(0), 1.0);
        assert!(surface_depth_factor(24) < 1.0 && surface_depth_factor(24) > surface_depth_factor(48));
        assert_eq!(surface_depth_factor(48), surface_depth_factor(200));
        assert_eq!(depth_step(64.9), 64);
        assert_eq!(depth_step(67.0), 64);
        assert_eq!(depth_step(-1.0), -4);
        let palette = [0u32; 64];
        let col = Col { color: 1, rgb: 0x80_8080, height: 40, depth: 0, kind: FLOOR };
        let flat = Look { textured: true, ..Look::default() };
        let mut one = |_x: i32, _z: i32| Some(col);
        let lit = shaded(&mut one, &palette, flat, None, 0, 0).unwrap();
        assert_eq!(lit[0], 0x80);
        let deep = shaded(&mut one, &palette, Look { depth_from: Some(88), ..flat }, None, 0, 0).unwrap();
        assert!(deep[0] < 0x80, "48 blocks under you: darker");
        let level = shaded(&mut one, &palette, Look { depth_from: Some(40), ..flat }, None, 0, 0).unwrap();
        assert_eq!(level, lit, "at your height: unchanged");
        // Palette biomes: the palette's brightness in the column's hue; off, the palette.
        let mut pal = [0u32; 64];
        pal[1] = 0x7F_B238;
        let swamp = Col { color: 1, rgb: 0x4C_763C, height: 64, depth: 0, kind: FLOOR };
        let mut sw = |_x: i32, _z: i32| Some(swamp);
        let plain = Look::default();
        let off = shaded(&mut sw, &pal, plain, None, 0, 0).unwrap();
        assert_eq!(off, [0x7F, 0xB2, 0x38, 255]);
        let on = shaded(&mut sw, &pal, Look { biomes: true, ..plain }, None, 0, 0).unwrap();
        assert_ne!(on, off);
        let lum = |c: [u8; 4]| 0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2]);
        assert!((lum(on) - lum(off)).abs() < 2.0, "as bright as the palette colour");
        let stone = Col { color: 11, ..swamp };
        let mut st = |_x: i32, _z: i32| Some(stone);
        assert_eq!(shaded(&mut st, &pal, Look { biomes: true, ..plain }, None, 0, 0), shaded(&mut st, &pal, plain, None, 0, 0), "untinted colours stay");
        // Night: full at noon, darkest at midnight, rain darker.
        let noon = sky_brightness(6000, 0.0, 0.0);
        let midnight = sky_brightness(18000, 0.0, 0.0);
        assert!((noon - 1.0).abs() < 1e-3, "{noon}");
        assert!(midnight < 0.5 && midnight >= 0.4, "{midnight}");
        assert!(sky_brightness(6000, 1.0, 0.0) < noon);
        assert!(sky_brightness(13000, 0.0, 0.0) < noon && sky_brightness(13000, 0.0, 0.0) > midnight, "dusk between");
        // Auto size: a quarter of the short side, in the sizes offered.
        assert_eq!(auto_size(640.0, 360.0), 96.0, "1080p at GUI scale 3: the default size");
        assert_eq!(auto_size(4000.0, 2000.0), SIZES[SIZES.len() - 1]);
        assert_eq!(auto_size(200.0, 120.0), SIZES[0]);
        // Glass: the ground shows, tinted.
        let g = glass_over(0x00_0000, 0xFF_0000);
        assert!(g >> 16 > 0 && g >> 16 < 0xFF);
    }
}
