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
pub const VERSION: &str = "2.0.0";
pub const DESCRIPTION: &str = "A minimap in the game's own colours with nearby players and \
mobs, cave view and info lines; waypoints in sets, with death points, shown in the world and \
on both maps; and a world map (M) of everywhere you have been, saved per world and server.";

/// The mod's own screen id is the game's for every mod screen.
pub const SCREEN: &str = "mod";

// ---------------------------------------------------------------- settings

pub const CORNERS: [&str; 4] = ["Top left", "Top right", "Bottom left", "Bottom right"];
pub const SIZES: [f32; 6] = [64.0, 80.0, 96.0, 112.0, 128.0, 160.0];
/// Blocks per texel of the minimap (two texels a GUI pixel).
pub const ZOOMS: [f32; 7] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];
pub const OPACITY: [u8; 5] = [255, 217, 178, 140, 102];
pub const DEATHS: [usize; 4] = [1, 3, 5, 10];
pub const SORTS: [&str; 3] = ["Name", "Distance", "Newest"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub minimap: bool,
    /// Index in `CORNERS`.
    pub corner: usize,
    /// Index in `SIZES`.
    pub size: usize,
    /// Index in `ZOOMS`.
    pub zoom: usize,
    /// Index in `OPACITY`.
    pub opacity: usize,
    pub round: bool,
    /// Turns with the player (forward is up), or north is up.
    pub rotate: bool,
    /// N, E, S, W on the rim.
    pub compass: bool,
    /// The game's colours (textures and biome tints), or the flat map palette.
    pub textured: bool,
    /// Slopes lit and shaded.
    pub shading: bool,
    /// Underground, in the Nether and under a roof: the floor around you.
    pub cave: bool,
    pub coords: bool,
    pub biome: bool,
    pub light: bool,
    pub time: bool,
    pub players: bool,
    pub names: bool,
    pub hostile: bool,
    pub passive: bool,
    /// Waypoint names and distances in the world.
    pub markers: bool,
    /// A beam over each waypoint in the world.
    pub beams: bool,
    /// Markers fade with distance.
    pub fade: bool,
    /// Overworld waypoints shown in the Nether at their Nether place, and the other way.
    pub convert: bool,
    pub death: bool,
    /// Index in `DEATHS`, how many death points are kept.
    pub deaths: usize,
    /// Index in `SORTS`, the waypoint list's order.
    pub sort: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            minimap: true,
            corner: 1,
            size: 2,
            zoom: 3,
            opacity: 0,
            round: false,
            rotate: true,
            compass: true,
            textured: true,
            shading: true,
            cave: true,
            coords: true,
            biome: true,
            light: false,
            time: false,
            players: true,
            names: true,
            hostile: true,
            passive: true,
            markers: true,
            beams: true,
            fade: true,
            convert: true,
            death: true,
            deaths: 1,
            sort: 1,
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
impl ShowEntry for usize {
    fn show(&self) -> String {
        format!("{self}")
    }
}

macro_rules! settings_table {
    ($( $field:ident: $kind:ident $(($table:ident))?, $label:expr; )*) => {
        impl Settings {
            pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
                let d = Self::default();
                let flag = |key: &str, default: bool| get(key).map_or(default, |v| v.trim() == "true");
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
                vec![ $( (stringify!($field), self.$field.to_string()), )* ]
            }
            pub fn labels(&self) -> Vec<String> {
                let on = |b: bool| if b { "ON".to_owned() } else { "OFF".to_owned() };
                let _ = &on;
                vec![ $( format!("{}: {}", $label, settings_table!(@show $kind, on, self.$field $(, $table)?)), )* ]
            }
            pub fn click(&mut self, button: usize) {
                let names = [$(stringify!($field)),*];
                let Some(name) = names.get(button) else { return };
                $(
                    if *name == stringify!($field) {
                        settings_table!(@click $kind, self.$field $(, $table)?);
                    }
                )*
            }
            pub const COUNT: usize = [$(stringify!($field)),*].len();
        }
    };
    (@read flag, $flag:ident, $index:ident, $key:expr, $d:expr) => { $flag($key, $d) };
    (@read index, $flag:ident, $index:ident, $key:expr, $d:expr, $table:ident) => { $index($key, $d, $table.len()) };
    (@show flag, $on:ident, $v:expr) => { $on($v) };
    (@show index, $on:ident, $v:expr, $table:ident) => { ShowEntry::show(&$table[$v]) };
    (@click flag, $v:expr) => { $v = !$v };
    (@click index, $v:expr, $table:ident) => { $v = ($v + 1) % $table.len() };
}

settings_table! {
    minimap: flag, "Minimap";
    corner: index(CORNERS), "Corner";
    size: index(SIZES), "Size";
    zoom: index(ZOOMS), "Zoom (blocks per pixel)";
    opacity: index(OPACITY), "Opacity";
    round: flag, "Round";
    rotate: flag, "Turns with you";
    compass: flag, "N E S W on the rim";
    textured: flag, "Game colours (off: map palette)";
    shading: flag, "Terrain shading";
    cave: flag, "Cave view underground";
    coords: flag, "Coordinates";
    biome: flag, "Biome";
    light: flag, "Light level";
    time: flag, "Time of day";
    players: flag, "Players";
    names: flag, "Player names";
    hostile: flag, "Hostile mobs";
    passive: flag, "Passive mobs";
    markers: flag, "Waypoints in the world";
    beams: flag, "Waypoint beams";
    fade: flag, "Waypoints fade with distance";
    convert: flag, "Overworld/Nether waypoints converted";
    death: flag, "Death waypoints";
    deaths: index(DEATHS), "Deaths kept";
    sort: index(SORTS), "Waypoint list order";
}

impl Settings {
    pub fn shows(&self, kind: DotKind) -> bool {
        match kind {
            DotKind::Player => self.players,
            DotKind::Hostile => self.hostile,
            DotKind::Passive => self.passive,
            DotKind::Other => false,
        }
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

pub fn dot_colour(kind: DotKind) -> u32 {
    match kind {
        DotKind::Player => 0xFFFF_FFFF,
        DotKind::Hostile => 0xFFE0_3C3C,
        DotKind::Passive => 0xFF5C_D65C,
        DotKind::Other => 0xFFA0_A0A0,
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

/// One chunk of the map: per column (`z * 16 + x`) its map colour id (0: nothing known), its
/// colour in the game's look (0xRRGGBB, 0: not known, use the map colour), the height of that
/// block and the water over it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub color: [u8; 256],
    pub rgb: [u32; 256],
    pub height: [i16; 256],
    pub depth: [u8; 256],
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            color: [0; 256],
            rgb: [0; 256],
            height: [0; 256],
            depth: [0; 256],
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
        })
    }
}

/// A region file holds 32 x 32 chunks.
pub const REGION: i32 = 32;
const MAGIC_V1: &[u8; 4] = b"LMR1";
const MAGIC: &[u8; 4] = b"LMR2";

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
        }
    }

    /// The column at block `x, z`.
    #[inline]
    pub fn column(&self, x: i32, z: i32) -> Option<Col> {
        self.tiles.get(&(x >> 4, z >> 4))?.col(x, z)
    }

    /// Region `region` as its file's bytes.
    pub fn encode_region(&self, region: (i32, i32)) -> Vec<u8> {
        let mut keys: Vec<&(i32, i32)> = self
            .tiles
            .keys()
            .filter(|k| region_of(**k) == region)
            .collect();
        keys.sort();
        let mut out = Vec::with_capacity(8 + keys.len() * ONE_V2);
        out.extend_from_slice(MAGIC);
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
        }
        out
    }
}

const ONE_V1: usize = 2 + 256 + 512 + 256;
const ONE_V2: usize = ONE_V1 + 768;

/// The tiles of a region file (this version's, or the first's without colours); None when it
/// isn't one.
pub fn decode_region(region: (i32, i32), bytes: &[u8]) -> Option<Vec<((i32, i32), Tile)>> {
    if bytes.len() < 8 {
        return None;
    }
    let one = match &bytes[..4] {
        m if m == MAGIC => ONE_V2,
        m if m == MAGIC_V1 => ONE_V1,
        _ => return None,
    };
    let count = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    if count > (REGION * REGION) as usize || bytes.len() != 8 + count * one {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for c in bytes[8..].chunks_exact(one) {
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
        if one == ONE_V2 {
            for (i, p) in c[1026..1794].chunks_exact(3).enumerate() {
                t.rgb[i] = u32::from(p[0]) | u32::from(p[1]) << 8 | u32::from(p[2]) << 16;
            }
        }
        out.push(((region.0 * REGION + lx, region.1 * REGION + lz), t));
    }
    Some(out)
}

// ---------------------------------------------------------------- shading

/// How a column is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub textured: bool,
    pub shading: bool,
}

/// A column's colour as an image shows it: its own colour (or its map colour), lit where the
/// ground rises towards the north-west and shaded where it falls away (the light comes from
/// there), water darker with depth.
pub fn shade(
    palette: &[u32],
    look: Look,
    c: Col,
    north: Option<i16>,
    west: Option<i16>,
) -> [u8; 4] {
    let base = if look.textured && c.rgb != 0 {
        c.rgb
    } else {
        palette.get(usize::from(c.color)).copied().unwrap_or(0)
    };
    let factor = if c.color == WATER || c.depth > 0 {
        1.0 - f32::from(c.depth.min(14)) * 0.03
    } else if look.shading {
        let d = |n: Option<i16>| f32::from(c.height - n.unwrap_or(c.height));
        (1.0 + (d(north) + d(west)) * 0.06).clamp(0.62, 1.22)
    } else {
        1.0
    };
    let ch = |s: u32| ((((base >> s) & 255) as f32 * factor).round().clamp(0.0, 255.0)) as u8;
    [ch(16), ch(8), ch(0), 255]
}

/// A column with its north and west neighbours' heights for its shading.
fn shaded(
    column: &mut impl FnMut(i32, i32) -> Option<Col>,
    palette: &[u32],
    look: Look,
    x: i32,
    z: i32,
) -> Option<[u8; 4]> {
    let c = column(x, z)?;
    if !look.shading || c.color == WATER || c.depth > 0 {
        return Some(shade(palette, look, c, None, None));
    }
    let n = column(x, z - 1).map(|c| c.height);
    let w = column(x - 1, z).map(|c| c.height);
    Some(shade(palette, look, c, n, w))
}

// ---------------------------------------------------------------- the minimap

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

/// What the minimap shows: where, turned how, how far, and in which shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MiniView {
    pub x: f64,
    pub z: f64,
    pub yaw: f32,
    pub rotate: bool,
    /// Blocks per texel.
    pub bpp: f32,
    pub round: bool,
    pub look: Look,
}

impl MiniView {
    /// Whether the image for `self` would look different from `other`'s.
    pub fn differs(&self, other: &MiniView) -> bool {
        let moved = ((self.x - other.x).abs().max((self.z - other.z).abs()) as f32) >= self.bpp / 4.0;
        let turned = self.rotate && (self.yaw - other.yaw).abs() >= 0.5;
        moved
            || turned
            || self.rotate != other.rotate
            || self.round != other.round
            || self.bpp != other.bpp
            || self.look != other.look
    }
}

const RIM: [u8; 4] = [16, 16, 16, 255];
const UNSEEN: [u8; 4] = [24, 24, 28, 230];

/// The minimap's `side` x `side` RGBA image of `column` for `view`.
pub fn compose_minimap(
    mut column: impl FnMut(i32, i32) -> Option<Col>,
    palette: &[u32],
    view: &MiniView,
    side: usize,
    out: &mut Vec<u8>,
) {
    out.clear();
    out.resize(side * side * 4, 0);
    let (r, f) = axes(view.yaw, view.rotate);
    let bpp = f64::from(view.bpp);
    let half = side as f64 / 2.0;
    for j in 0..side {
        for i in 0..side {
            let (u, v) = (i as f64 + 0.5 - half, j as f64 + 0.5 - half);
            let o = (j * side + i) * 4;
            let edge = if view.round {
                let d = (u * u + v * v).sqrt();
                if d > half {
                    continue;
                }
                d > half - 1.5
            } else {
                i == 0 || j == 0 || i == side - 1 || j == side - 1
            };
            if edge {
                out[o..o + 4].copy_from_slice(&RIM);
                continue;
            }
            let wx = view.x + bpp * (u * r[0] - v * f[0]);
            let wz = view.z + bpp * (u * r[1] - v * f[1]);
            let rgba = shaded(&mut column, palette, view.look, wx.floor() as i32, wz.floor() as i32)
                .unwrap_or(UNSEEN);
            out[o..o + 4].copy_from_slice(&rgba);
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

/// The game's time of day (`day_time`, ticks) as "hh:mm".
pub fn clock(day_time: i64) -> String {
    let t = (day_time + 6000).rem_euclid(24000);
    format!("{:02}:{:02}", t / 1000, (t % 1000) * 60 / 1000)
}

// ---------------------------------------------------------------- the world map

/// Unexplored ground on the world map.
pub const UNKNOWN: [u8; 4] = [18, 18, 22, 255];

/// Blocks per GUI pixel the world map steps through.
pub const MAP_ZOOMS: [f64; 9] = [0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0];

/// What the world map shows: its centre in blocks and its zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapView {
    pub x: f64,
    pub z: f64,
    /// Index in `MAP_ZOOMS`.
    pub zoom: usize,
}

impl Default for MapView {
    fn default() -> Self {
        Self { x: 0.0, z: 0.0, zoom: 3 }
    }
}

impl MapView {
    pub fn bpp(&self) -> f64 {
        MAP_ZOOMS[self.zoom.min(MAP_ZOOMS.len() - 1)]
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

    /// One zoom step in (`steps` > 0) or out, keeping the point under `p` where it is.
    pub fn zoom_at(&mut self, rect: [f32; 4], p: [f32; 2], steps: i32) {
        let before = self.world_at(rect, p);
        let zoom = (self.zoom as i32 - steps).clamp(0, MAP_ZOOMS.len() as i32 - 1) as usize;
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

/// The mod image slot (1024 square) is shared: the world map's image takes its top
/// `MAP_IMAGE_H` rows, the minimap's its bottom left corner.
pub const IMAGE_W: u32 = 1024;
pub const MAP_IMAGE_H: u32 = 768;
pub const MINI_AT: [u32; 2] = [0, 768];
pub const MINI_MAX: u32 = 256;

/// The image for a map `rect` of `w` x `h` GUI pixels: its size in texels and the GUI pixels per
/// texel (1 unless the map is wider or taller than its part of the slot).
pub fn image_size(w: f32, h: f32) -> (u32, u32, f32) {
    let scale = (w / IMAGE_W as f32).max(h / MAP_IMAGE_H as f32).max(1.0);
    (
        ((w / scale).floor() as u32).clamp(1, IMAGE_W),
        ((h / scale).floor() as u32).clamp(1, MAP_IMAGE_H),
        scale,
    )
}

/// The minimap image's side in texels for a minimap `size` GUI pixels across: two texels a GUI
/// pixel, at most `MINI_MAX`.
pub fn mini_side(size: f32) -> usize {
    ((size * 2.0) as usize).clamp(32, MINI_MAX as usize)
}

/// Composes the world map's `iw` x `ih` RGBA image of `column` for `view`, each texel
/// `gui_per_texel` GUI pixels. Ground the client holds no chunk of is dimmed when `loaded` is
/// given.
#[allow(clippy::too_many_arguments)]
pub fn compose(
    mut column: impl FnMut(i32, i32) -> Option<Col>,
    palette: &[u32],
    look: Look,
    view: &MapView,
    iw: u32,
    ih: u32,
    gui_per_texel: f32,
    loaded: Option<&dyn Fn(i32, i32) -> bool>,
    out: &mut Vec<u8>,
) {
    out.clear();
    out.resize((iw * ih * 4) as usize, 0);
    let bpp = view.bpp() * f64::from(gui_per_texel);
    let (hw, hh) = (f64::from(iw) / 2.0, f64::from(ih) / 2.0);
    let mut chunk_loaded: Option<((i32, i32), bool)> = None;
    for j in 0..ih {
        let wz = (view.z + (f64::from(j) + 0.5 - hh) * bpp).floor() as i32;
        for i in 0..iw {
            let wx = (view.x + (f64::from(i) + 0.5 - hw) * bpp).floor() as i32;
            let o = ((j * iw + i) * 4) as usize;
            let mut rgba = shaded(&mut column, palette, look, wx, wz).unwrap_or(UNKNOWN);
            if let Some(is_loaded) = loaded
                && rgba != UNKNOWN
            {
                let key = (wx >> 4, wz >> 4);
                let held = match chunk_loaded {
                    Some((k, v)) if k == key => v,
                    _ => {
                        let v = is_loaded(key.0, key.1);
                        chunk_loaded = Some((key, v));
                        v
                    }
                };
                if !held {
                    for c in &mut rgba[..3] {
                        *c = (u16::from(*c) * 3 / 5) as u8;
                    }
                }
            }
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
}

impl Waypoint {
    pub fn rgb(&self) -> u32 {
        COLOURS[self.colour.min(COLOURS.len() - 1)].1
    }
    /// The block centre at its foot.
    pub fn centre(&self) -> [f64; 3] {
        [
            f64::from(self.pos[0]) + 0.5,
            f64::from(self.pos[1]),
            f64::from(self.pos[2]) + 0.5,
        ]
    }
    /// One or two letters for its marker: its words' first letters.
    pub fn initials(&self) -> String {
        let mut s: String = self
            .name
            .split_whitespace()
            .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
            .take(2)
            .flat_map(char::to_uppercase)
            .collect();
        if s.is_empty() {
            s.push('?');
        }
        s
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
}

impl Default for Book {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            sets: vec![WaySet { name: DEFAULT_SET.into(), shown: true }],
            current: 0,
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
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                clean_name(&w.name),
                w.pos[0],
                w.pos[1],
                w.pos[2],
                w.dim,
                w.colour,
                w.death,
                w.shown,
                clean_name(&w.set),
                w.created
            ));
        }
        out
    }

    /// A file of this version or the first (whose lines have no set or time); lines that don't
    /// read are skipped.
    pub fn from_text(text: &str) -> Self {
        let mut book = Book { list: Vec::new(), sets: Vec::new(), current: 0 };
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
            book.list.push(Waypoint {
                name: clean_name(f[0]),
                pos: [x, y, z],
                dim: f[4].trim().to_owned(),
                colour: colour.min(COLOURS.len() - 1),
                death: f[6].trim() == "true",
                shown: f[7].trim() != "false",
                set: set.unwrap_or_else(|| DEFAULT_SET.to_owned()),
                created: f.get(9).and_then(|s| s.trim().parse().ok()).unwrap_or(0),
            });
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
        w.shown && self.set_index(&w.set).is_none_or(|i| self.sets[i].shown)
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

    /// A waypoint added at `pos` in `dim` to the current set; returns its index.
    pub fn add(&mut self, pos: [i32; 3], dim: &str, now: u64) -> usize {
        let colour = self.list.iter().filter(|w| !w.death).count() % (COLOURS.len() - 2);
        let w = Waypoint {
            name: self.next_name(),
            pos,
            dim: dim.to_owned(),
            colour,
            death: false,
            shown: true,
            set: self.current_set().to_owned(),
            created: now,
        };
        self.list.push(w);
        self.list.len() - 1
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

/// A marker's opacity at `d` blocks: clear right beside it, full from 8 blocks, fading again
/// far away to a third at 2000 blocks.
pub fn fade(d: f64) -> u8 {
    let near = ((d - 2.0) / 6.0).clamp(0.0, 1.0);
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
    /// Editing waypoint `index`; `name` is the text box.
    Edit {
        index: usize,
        name: String,
    },
    /// Naming a new set.
    NewSet {
        name: String,
    },
}

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
}

/// How far a press may move and still be a click, in GUI pixels.
pub const CLICK_SLOP: f32 = 3.0;

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
    (((height - 92.0) / 22.0).floor() as usize).max(1)
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
        assert!(!s.minimap && s.corner == 2 && s.size == 3 && s.zoom == 4 && s.round && !s.textured);
        assert_eq!(Settings::read(|k| (k == "corner").then(|| "9".into())).corner, 1);
        assert_eq!(Settings::default().labels()[4], "Opacity: 100%");
        let mut z = Settings::default();
        assert!(z.zoom_step(true));
        assert_eq!(z.zoom, 2);
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
        let a = arrow([0.0, 0.0], 0.0, 4.0);
        assert!(a[0][1] > 3.9 && a[0][0].abs() < 1e-4, "yaw 0 points down");
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
        assert_eq!(clock(0), "06:00");
        assert_eq!(clock(18000), "00:00");
        assert_eq!(clock(6500), "12:30");
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
        Tile { color: [c; 256], rgb: [rgb; 256], height: [h; 256], depth: [0; 256] }
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
        let look = Look { textured: true, shading: true };
        let col = |h: i16, rgb: u32| Col { color: 1, rgb, height: h, depth: 0 };
        assert_eq!(shade(&palette, look, col(64, 0), Some(64), Some(64)), [128, 128, 128, 255]);
        assert!(shade(&palette, look, col(66, 0), Some(64), Some(64))[0] > 128);
        assert!(shade(&palette, look, col(62, 0), Some(64), Some(64))[0] < 128);
        assert_eq!(shade(&palette, look, col(64, 0x00FF00), None, None), [0, 255, 0, 255]);
        let flat = Look { textured: false, shading: false };
        assert_eq!(shade(&palette, flat, col(70, 0x00FF00), Some(10), None), [128, 128, 128, 255]);
        let water = |d: u8| Col { color: WATER, rgb: 0, height: 60, depth: d };
        assert!(shade(&palette, look, water(10), None, None)[2] < shade(&palette, look, water(1), None, None)[2]);
    }

    #[test]
    fn the_view_pans_zooms_and_maps_points_both_ways() {
        let rect = [0.0, 20.0, 400.0, 200.0];
        let mut v = MapView { x: 100.0, z: -50.0, zoom: 3 };
        assert_eq!(v.world_at(rect, [200.0, 120.0]), (100.0, -50.0));
        let p = v.screen_of(rect, 110.0, -45.0);
        assert_eq!(v.world_at(rect, p), (110.0, -45.0));
        v.pan([10.0, -4.0]);
        assert_eq!((v.x, v.z), (90.0, -46.0));
        let under = v.world_at(rect, [50.0, 60.0]);
        v.zoom_at(rect, [50.0, 60.0], 1);
        assert_eq!(v.zoom, 2);
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
    fn images_fit_their_parts_of_the_slot() {
        assert_eq!(image_size(400.0, 200.0), (400, 200, 1.0));
        let (w, h, s) = image_size(2048.0, 1000.0);
        assert!(w <= IMAGE_W && h <= MAP_IMAGE_H && s == 2.0);
        let (_, h, _) = image_size(800.0, 1536.0);
        assert_eq!(h, MAP_IMAGE_H);
        assert_eq!(mini_side(96.0), 192);
        assert_eq!(mini_side(400.0), 256);
        assert!(MINI_AT[1] + MINI_MAX <= 1024 && MAP_IMAGE_H <= MINI_AT[1]);
    }

    #[test]
    fn composing_draws_known_ground_and_dims_unloaded() {
        let palette = [0u32, 0xFF0000];
        let mut s = Store::default();
        s.put((0, 0), tile(1, 64, 0));
        s.put((1, 0), tile(1, 64, 0));
        let look = Look { textured: true, shading: true };
        let view = MapView { x: 0.0, z: 0.0, zoom: 3 };
        let mut out = Vec::new();
        let only_first = |x: i32, _z: i32| x == 0;
        compose(|x, z| s.column(x, z), &palette, look, &view, 64, 64, 1.0, Some(&only_first), &mut out);
        let at = |o: &Vec<u8>, i: usize, j: usize| o[(j * 64 + i) * 4..(j * 64 + i) * 4 + 4].to_vec();
        assert_eq!(at(&out, 32, 32), vec![255, 0, 0, 255]);
        assert_eq!(at(&out, 50, 32), vec![153, 0, 0, 255], "explored but not loaded: dimmed");
        assert_eq!(at(&out, 0, 0), UNKNOWN.to_vec());
        let mv = MiniView { x: 8.0, z: 8.0, yaw: 0.0, rotate: false, bpp: 1.0, round: true, look };
        compose_minimap(|x, z| s.column(x, z), &palette, &mv, 32, &mut out);
        assert_eq!(out.len(), 32 * 32 * 4);
        assert_eq!(out[3], 0, "round leaves the corners clear");
        assert_eq!(&out[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4], &[255, 0, 0, 255]);
        assert!(mv.differs(&MiniView { x: 9.0, ..mv }));
        assert!(!mv.differs(&MiniView { x: 8.1, ..mv }));
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
        assert_eq!(b.list[1].initials(), "AB");
        assert_eq!(converted([80, 64, -17], OVERWORLD, NETHER), Some([10, 64, -3]));
        assert_eq!(converted([10, 64, -3], NETHER, OVERWORLD), Some([80, 64, -24]));
        assert_eq!(converted([1, 2, 3], OVERWORLD, "minecraft:the_end"), None);
        assert!(b.list[0].place_in(NETHER, true).is_some());
        assert!(b.list[0].place_in(NETHER, false).is_none());
        assert_eq!(fade(0.0), 0);
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
        assert_eq!(list_rows(240.0), 6);
    }
}
