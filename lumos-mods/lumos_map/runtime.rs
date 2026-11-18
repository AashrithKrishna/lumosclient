// Lumos Map: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_map`; `glue.rs` reads the game
// and calls in here. It names nothing outside `std`, so it also compiles alone:
// `rustc --edition 2024 --test runtime.rs` runs the tests below.
//
// What is here: the settings, waypoints (and their file), the explored map's tiles (one per
// chunk: each column's map colour, height and water depth) with their region files, the world
// map's view (pan, zoom, the image it composes) and where things go on the minimap.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

pub const ID: &str = "lumos_map";
pub const NAME: &str = "Map";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "A minimap in a corner with nearby players and mobs, coordinates \
and biome; waypoints (B adds one, a death point is kept for you) shown in the world and on \
both maps; and a world map (M) of everywhere you have been, saved per world and server.";

/// The mod's own screen id is the game's for every mod screen.
pub const SCREEN: &str = "mod";

// ---------------------------------------------------------------- settings

pub const CORNERS: [&str; 4] = ["Top left", "Top right", "Bottom left", "Bottom right"];
pub const SIZES: [f32; 5] = [64.0, 80.0, 96.0, 128.0, 160.0];
/// Blocks per texel of the minimap's 128-texel image.
pub const ZOOMS: [f32; 5] = [0.5, 1.0, 1.5, 2.0, 4.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub minimap: bool,
    /// Index in `CORNERS`.
    pub corner: usize,
    /// Index in `SIZES`.
    pub size: usize,
    /// Index in `ZOOMS`.
    pub zoom: usize,
    pub round: bool,
    /// Turns with the player (forward is up), or north is up.
    pub rotate: bool,
    /// Coordinates and biome under the minimap.
    pub info: bool,
    pub players: bool,
    pub hostile: bool,
    pub passive: bool,
    /// Waypoint names and distances in the world.
    pub markers: bool,
    /// A beam over each waypoint in the world.
    pub beams: bool,
    /// A waypoint where you die.
    pub death: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            minimap: true,
            corner: 1,
            size: 2,
            zoom: 1,
            round: false,
            rotate: true,
            info: true,
            players: true,
            hostile: true,
            passive: true,
            markers: true,
            beams: true,
            death: true,
        }
    }
}

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
        Self {
            minimap: flag("minimap", d.minimap),
            corner: index("corner", d.corner, CORNERS.len()),
            size: index("size", d.size, SIZES.len()),
            zoom: index("zoom", d.zoom, ZOOMS.len()),
            round: flag("round", d.round),
            rotate: flag("rotate", d.rotate),
            info: flag("info", d.info),
            players: flag("players", d.players),
            hostile: flag("hostile", d.hostile),
            passive: flag("passive", d.passive),
            markers: flag("markers", d.markers),
            beams: flag("beams", d.beams),
            death: flag("death", d.death),
        }
    }

    pub fn labels(&self) -> Vec<String> {
        let on = |b: bool| if b { "ON" } else { "OFF" };
        vec![
            format!("Minimap: {}", on(self.minimap)),
            format!("Corner: {}", CORNERS[self.corner]),
            format!("Size: {}", SIZES[self.size] as u32),
            format!("Zoom: {} block(s) per pixel", ZOOMS[self.zoom]),
            format!("Shape: {}", if self.round { "Round" } else { "Square" }),
            format!(
                "Rotation: {}",
                if self.rotate { "Turns with you" } else { "North up" }
            ),
            format!("Coordinates and biome: {}", on(self.info)),
            format!("Players: {}", on(self.players)),
            format!("Hostile mobs: {}", on(self.hostile)),
            format!("Passive mobs: {}", on(self.passive)),
            format!("Waypoints in the world: {}", on(self.markers)),
            format!("Waypoint beams: {}", on(self.beams)),
            format!("Death waypoint: {}", on(self.death)),
        ]
    }

    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.minimap = !self.minimap,
            1 => self.corner = (self.corner + 1) % CORNERS.len(),
            2 => self.size = (self.size + 1) % SIZES.len(),
            3 => self.zoom = (self.zoom + 1) % ZOOMS.len(),
            4 => self.round = !self.round,
            5 => self.rotate = !self.rotate,
            6 => self.info = !self.info,
            7 => self.players = !self.players,
            8 => self.hostile = !self.hostile,
            9 => self.passive = !self.passive,
            10 => self.markers = !self.markers,
            11 => self.beams = !self.beams,
            12 => self.death = !self.death,
            _ => {}
        }
    }

    pub fn stored(&self) -> Vec<(&'static str, String)> {
        vec![
            ("minimap", self.minimap.to_string()),
            ("corner", self.corner.to_string()),
            ("size", self.size.to_string()),
            ("zoom", self.zoom.to_string()),
            ("round", self.round.to_string()),
            ("rotate", self.rotate.to_string()),
            ("info", self.info.to_string()),
            ("players", self.players.to_string()),
            ("hostile", self.hostile.to_string()),
            ("passive", self.passive.to_string()),
            ("markers", self.markers.to_string()),
            ("beams", self.beams.to_string()),
            ("death", self.death.to_string()),
        ]
    }

    /// Whether a mob of this category (`entity-types.json`'s, `monster`, `creature`...) shows.
    pub fn shows(&self, kind: DotKind) -> bool {
        match kind {
            DotKind::Player => self.players,
            DotKind::Hostile => self.hostile,
            DotKind::Passive => self.passive,
            DotKind::Other => false,
        }
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

// ---------------------------------------------------------------- minimap geometry

/// The minimap image's side in texels (the engine minimap's).
pub const MINIMAP_TEXELS: f32 = 128.0;

/// Where a world offset `dx, dz` (blocks from the player) is on the minimap, in texels from its
/// centre (x right, y down), for the map turned with `yaw` or north up, at `bpp` blocks per
/// texel. The inverse of the engine minimap's texel-to-world mapping.
pub fn minimap_point(dx: f64, dz: f64, yaw: f32, rotate: bool, bpp: f32) -> (f32, f32) {
    let (r, f) = if rotate {
        let (s, c) = f64::from(yaw).to_radians().sin_cos();
        ([-c, -s], [-s, c])
    } else {
        ([1.0, 0.0], [0.0, -1.0])
    };
    let bpp = f64::from(bpp);
    let u = (dx * r[0] + dz * r[1]) / bpp;
    let v = -(dx * f[0] + dz * f[1]) / bpp;
    (u as f32, v as f32)
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

#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub name: String,
    pub pos: [i32; 3],
    /// `minecraft:overworld`.
    pub dim: String,
    /// Index in `COLOURS`.
    pub colour: usize,
    pub death: bool,
    pub shown: bool,
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

/// One line per waypoint: name, x, y, z, dimension, colour, death, shown, tab separated.
pub fn waypoints_to_text(list: &[Waypoint]) -> String {
    let mut out = String::from("# Lumos Map waypoints: name, x, y, z, dimension, colour, death, shown\n");
    for w in list {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            clean_name(&w.name),
            w.pos[0],
            w.pos[1],
            w.pos[2],
            w.dim,
            w.colour,
            w.death,
            w.shown
        ));
    }
    out
}

/// The waypoints of a file; lines that don't read are skipped.
pub fn waypoints_from_text(text: &str) -> Vec<Waypoint> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 8 {
                return None;
            }
            let n = |s: &str| s.trim().parse::<i32>().ok();
            Some(Waypoint {
                name: clean_name(f[0]),
                pos: [n(f[1])?, n(f[2])?, n(f[3])?],
                dim: f[4].trim().to_owned(),
                colour: f[5].trim().parse::<usize>().ok()?.min(COLOURS.len() - 1),
                death: f[6].trim() == "true",
                shown: f[7].trim() != "false",
            })
        })
        .collect()
}

/// "Waypoint N", the first N not taken.
pub fn next_name(list: &[Waypoint]) -> String {
    (1..)
        .map(|n| format!("Waypoint {n}"))
        .find(|name| !list.iter().any(|w| &w.name == name))
        .unwrap_or_default()
}

/// The colour for the next waypoint: each new one takes the next.
pub fn next_colour(list: &[Waypoint]) -> usize {
    list.iter().filter(|w| !w.death).count() % (COLOURS.len() - 2)
}

/// A waypoint added at `pos` in `dim`; returns its index.
pub fn add_waypoint(list: &mut Vec<Waypoint>, pos: [i32; 3], dim: &str) -> usize {
    let w = Waypoint {
        name: next_name(list),
        pos,
        dim: dim.to_owned(),
        colour: next_colour(list),
        death: false,
        shown: true,
    };
    list.push(w);
    list.len() - 1
}

/// The point where you died: it replaces the last one.
pub fn add_death(list: &mut Vec<Waypoint>, pos: [i32; 3], dim: &str) {
    list.retain(|w| !w.death);
    list.push(Waypoint {
        name: DEATH_NAME.to_owned(),
        pos,
        dim: dim.to_owned(),
        colour: 0,
        death: true,
        shown: true,
    });
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

// ---------------------------------------------------------------- explored tiles

/// One chunk of the map: per column (`z * 16 + x`) its map colour id (0: nothing known), the
/// height of that block and the water over it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub color: [u8; 256],
    pub height: [i16; 256],
    pub depth: [u8; 256],
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            color: [0; 256],
            height: [0; 256],
            depth: [0; 256],
        }
    }
}

/// A region file holds 32 x 32 chunks.
pub const REGION: i32 = 32;
const MAGIC: &[u8; 4] = b"LMR1";

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
    pub fn column(&self, x: i32, z: i32) -> Option<(u8, i16, u8)> {
        let t = self.tiles.get(&(x >> 4, z >> 4))?;
        let i = ((z & 15) * 16 + (x & 15)) as usize;
        (t.color[i] != 0).then(|| (t.color[i], t.height[i], t.depth[i]))
    }

    /// Region `region` as its file's bytes.
    pub fn encode_region(&self, region: (i32, i32)) -> Vec<u8> {
        let mut keys: Vec<&(i32, i32)> = self
            .tiles
            .keys()
            .filter(|k| region_of(**k) == region)
            .collect();
        keys.sort();
        let mut out = Vec::with_capacity(8 + keys.len() * 1026);
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
        }
        out
    }
}

/// The tiles of a region file; None when it isn't one.
pub fn decode_region(region: (i32, i32), bytes: &[u8]) -> Option<Vec<((i32, i32), Tile)>> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    const ONE: usize = 2 + 256 + 512 + 256;
    if count > (REGION * REGION) as usize || bytes.len() != 8 + count * ONE {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for c in bytes[8..].chunks_exact(ONE) {
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
        out.push(((region.0 * REGION + lx, region.1 * REGION + lz), t));
    }
    Some(out)
}

// ---------------------------------------------------------------- the world map

/// Map colour ids with a meaning of their own (`MapColor`).
pub const WATER: u8 = 12;

/// A column's colour: its map colour (`palette[id]`, 0xRRGGBB), lighter where the land rises
/// from the north and darker where it falls, water darker with depth.
pub fn shade(palette: &[u32], color: u8, height: i16, north: Option<i16>, depth: u8) -> [u8; 4] {
    let base = palette.get(usize::from(color)).copied().unwrap_or(0);
    let factor = if color == WATER {
        1.0 - f32::from(depth.min(12)) * 0.035
    } else {
        let diff = f32::from(height - north.unwrap_or(height));
        (1.0 + diff * 0.09).clamp(0.72, 1.18)
    };
    let ch = |s: u32| ((((base >> s) & 255) as f32 * factor).round().clamp(0.0, 255.0)) as u8;
    [ch(16), ch(8), ch(0), 255]
}

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
}

/// The largest side of the map image (the mod image slot's).
pub const IMAGE_MAX: u32 = 1024;

/// The image for a map `rect` of `w` x `h` GUI pixels: its size in texels and the GUI pixels per
/// texel (1 unless the map is wider or taller than the image can be).
pub fn image_size(w: f32, h: f32) -> (u32, u32, f32) {
    let scale = (w / IMAGE_MAX as f32).max(h / IMAGE_MAX as f32).max(1.0);
    (
        ((w / scale).floor() as u32).clamp(1, IMAGE_MAX),
        ((h / scale).floor() as u32).clamp(1, IMAGE_MAX),
        scale,
    )
}

/// Composes the world map's `iw` x `ih` RGBA image of `store` for `view`, each texel
/// `gui_per_texel` GUI pixels.
pub fn compose(
    store: &Store,
    palette: &[u32],
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
    let mut cached: Option<((i32, i32), Option<&Tile>)> = None;
    let mut column = |x: i32, z: i32| -> Option<(u8, i16, u8)> {
        let key = (x >> 4, z >> 4);
        let tile = match cached {
            Some((k, t)) if k == key => t,
            _ => {
                let t = store.tiles.get(&key);
                cached = Some((key, t));
                t
            }
        }?;
        let i = ((z & 15) * 16 + (x & 15)) as usize;
        (tile.color[i] != 0).then(|| (tile.color[i], tile.height[i], tile.depth[i]))
    };
    for j in 0..ih {
        let wz = (view.z + (f64::from(j) + 0.5 - hh) * bpp).floor() as i32;
        for i in 0..iw {
            let wx = (view.x + (f64::from(i) + 0.5 - hw) * bpp).floor() as i32;
            let o = ((j * iw + i) * 4) as usize;
            let rgba = match column(wx, wz) {
                Some((color, height, depth)) => {
                    let north = column(wx, wz - 1).map(|c| c.1);
                    shade(palette, color, height, north, depth)
                }
                None => UNKNOWN,
            };
            out[o..o + 4].copy_from_slice(&rgba);
        }
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
}

/// The map screen's state between frames.
#[derive(Clone, Debug, Default)]
pub struct MapScreen {
    pub view: MapView,
    pub page: Page,
    /// A left press on the map: where, and whether it has moved enough to be a drag.
    pub press: Option<([f32; 2], bool)>,
}

/// How far a press may move and still be a click, in GUI pixels.
pub const CLICK_SLOP: f32 = 3.0;

impl MapScreen {
    /// A drag of `delta` while pressed: pans once it is past the slop.
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
        self.view.pan(delta);
    }

    /// The press let go: whether it was a click (not a drag).
    pub fn release(&mut self) -> bool {
        matches!(self.press.take(), Some((_, false)))
    }
}

/// Rows of the waypoint list a page shows.
pub fn list_rows(height: f32) -> usize {
    (((height - 70.0) / 22.0).floor() as usize).max(1)
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
        for b in 0..13 {
            s.click(b);
        }
        assert_eq!(s.labels().len(), 13);
        let stored = s.stored();
        let read = Settings::read(|k| stored.iter().find(|p| p.0 == k).map(|p| p.1.clone()));
        assert_eq!(read, s);
        assert!(!s.minimap && s.corner == 2 && s.size == 3 && s.zoom == 2 && s.round);
        assert_eq!(Settings::read(|k| (k == "corner").then(|| "9".into())).corner, 1);
    }

    #[test]
    fn mob_kinds() {
        assert_eq!(dot_kind("minecraft:player", "misc"), DotKind::Player);
        assert_eq!(dot_kind("minecraft:zombie", "monster"), DotKind::Hostile);
        assert_eq!(dot_kind("minecraft:cow", "creature"), DotKind::Passive);
        assert_eq!(dot_kind("minecraft:cod", "water_ambient"), DotKind::Passive);
        assert_eq!(dot_kind("minecraft:armor_stand", "misc"), DotKind::Other);
        let s = Settings { passive: false, ..Settings::default() };
        assert!(!s.shows(DotKind::Passive) && s.shows(DotKind::Hostile) && !s.shows(DotKind::Other));
    }

    /// The engine minimap's texel-to-world mapping, to check `minimap_point` is its inverse.
    fn engine_world(u: f64, v: f64, yaw: f32, rotate: bool, bpp: f64) -> (f64, f64) {
        let (r, f) = if rotate {
            let (s, c) = f64::from(yaw).to_radians().sin_cos();
            ([-c, -s], [-s, c])
        } else {
            ([1.0, 0.0], [0.0, -1.0])
        };
        (bpp * (u * r[0] - v * f[0]), bpp * (u * r[1] - v * f[1]))
    }

    #[test]
    fn minimap_points_invert_the_engine() {
        for (yaw, rotate) in [(0.0, false), (37.0, true), (-150.0, true), (90.0, false)] {
            for (u, v) in [(10.0, -4.0), (-30.0, 22.5), (0.0, 0.0)] {
                let (dx, dz) = engine_world(u, v, yaw, rotate, 2.0);
                let (pu, pv) = minimap_point(dx, dz, yaw, rotate, 2.0);
                assert!((f64::from(pu) - u).abs() < 1e-3 && (f64::from(pv) - v).abs() < 1e-3);
            }
        }
        // North up: east is right, south is down.
        assert_eq!(minimap_point(4.0, 0.0, 0.0, false, 1.0), (4.0, 0.0));
        assert_eq!(minimap_point(0.0, 4.0, 0.0, false, 1.0), (0.0, 4.0));
        // Turned, facing south (yaw 0): what is ahead (+z) is up.
        let (u, v) = minimap_point(0.0, 4.0, 0.0, true, 1.0);
        assert!(u.abs() < 1e-5 && (v + 4.0).abs() < 1e-5);
    }

    #[test]
    fn clamping_keeps_points_on_the_map() {
        assert_eq!(clamp_to_map((3.0, 4.0), 10.0, true), ((3.0, 4.0), false));
        let ((x, y), moved) = clamp_to_map((30.0, 40.0), 10.0, true);
        assert!(moved && (x - 6.0).abs() < 1e-5 && (y - 8.0).abs() < 1e-5);
        let ((x, y), moved) = clamp_to_map((20.0, 5.0), 10.0, false);
        assert!(moved && x == 10.0 && y == 2.5);
    }

    #[test]
    fn waypoints_survive_their_file() {
        let mut list = Vec::new();
        assert_eq!(add_waypoint(&mut list, [1, 64, -3], "minecraft:overworld"), 0);
        add_waypoint(&mut list, [100, 70, 5], "minecraft:the_nether");
        list[1].name = "Base\twith\ttabs and a very long name that keeps on going".into();
        list[1].shown = false;
        add_death(&mut list, [5, 6, 7], "minecraft:overworld");
        add_death(&mut list, [8, 9, 10], "minecraft:overworld");
        assert_eq!(list.iter().filter(|w| w.death).count(), 1);
        let back = waypoints_from_text(&waypoints_to_text(&list));
        assert_eq!(back.len(), 3);
        assert_eq!(back[0], list[0]);
        assert_eq!(back[1].name, clean_name(&list[1].name));
        assert!(back[1].name.len() <= NAME_MAX && !back[1].name.contains('\t'));
        assert!(!back[1].shown);
        assert_eq!(back[2].pos, [8, 9, 10]);
        assert!(back[2].death);
        assert!(waypoints_from_text("garbage\nx\t1\t2").is_empty());
    }

    #[test]
    fn names_and_colours_move_on() {
        let mut list = Vec::new();
        add_waypoint(&mut list, [0, 0, 0], "d");
        add_waypoint(&mut list, [0, 0, 0], "d");
        assert_eq!(list[0].name, "Waypoint 1");
        assert_eq!(list[1].name, "Waypoint 2");
        assert_ne!(list[0].colour, list[1].colour);
        list.remove(0);
        assert_eq!(next_name(&list), "Waypoint 1");
    }

    #[test]
    fn distances_and_names() {
        assert_eq!(format_distance(12.4), "12 m");
        assert_eq!(format_distance(1450.0), "1.4 km");
        assert_eq!(distance([0.0, 0.0, 0.0], [3.0, 4.0, 0.0]), 5.0);
        assert_eq!(folder_name("mc.example.com:25565"), "mc.example.com_25565");
        assert_eq!(folder_name("../.."), ".._..");
        assert_eq!(folder_name(""), "_");
        assert_eq!(folder_name(".."), "_");
        assert_eq!(pretty_id("minecraft:dark_forest"), "Dark Forest");
        assert_eq!(pretty_id("the_nether"), "The Nether");
    }

    fn tile(c: u8, h: i16) -> Tile {
        Tile {
            color: [c; 256],
            height: [h; 256],
            depth: [1; 256],
        }
    }

    #[test]
    fn regions_round_trip() {
        let mut s = Store::default();
        assert!(s.put((-1, 2), tile(3, 70)));
        assert!(!s.put((-1, 2), tile(3, 70)), "the same tile is no change");
        assert!(!s.put((5, 5), tile(0, 0)), "an empty tile is not kept");
        assert!(s.put((-32, 33), tile(7, -10)));
        assert!(s.put((40, 0), tile(1, 1)));
        assert_eq!(region_of((-1, 2)), (-1, 0));
        assert_eq!(region_of((-32, 33)), (-1, 1));
        assert_eq!(s.dirty.len(), 3);
        let bytes = s.encode_region((-1, 0));
        let back = decode_region((-1, 0), &bytes).unwrap();
        assert_eq!(back, vec![((-1, 2), tile(3, 70))]);
        assert_eq!(decode_region((0, 0), &bytes[..bytes.len() - 1]), None);
        assert_eq!(decode_region((0, 0), b"nope"), None);
        assert_eq!(region_from_file(&region_file((-3, 12))), Some((-3, 12)));
        assert_eq!(region_from_file("r.a.b.lmr"), None);
        // Loaded tiles don't replace the session's own.
        let mut t = Store::default();
        t.put((-1, 2), tile(9, 9));
        t.merge_loaded(back);
        assert_eq!(t.tiles[&(-1, 2)].color[0], 9);
        assert_eq!(t.column(-16, 32), Some((9, 9, 1)));
        assert_eq!(t.column(0, 0), None);
    }

    #[test]
    fn shading() {
        let palette = [0, 0x808080, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x4040FF];
        assert_eq!(shade(&palette, 1, 64, Some(64), 0), [128, 128, 128, 255]);
        assert!(shade(&palette, 1, 66, Some(64), 0)[0] > 128);
        assert!(shade(&palette, 1, 62, Some(64), 0)[0] < 128);
        assert!(shade(&palette, WATER, 60, None, 10)[2] < shade(&palette, WATER, 60, None, 1)[2]);
        assert_eq!(shade(&palette, 200, 0, None, 0), [0, 0, 0, 255]);
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
        // Zooming keeps the point under the pointer.
        let under = v.world_at(rect, [50.0, 60.0]);
        v.zoom_at(rect, [50.0, 60.0], 1);
        assert_eq!(v.zoom, 2);
        let after = v.world_at(rect, [50.0, 60.0]);
        assert!((under.0 - after.0).abs() < 1e-9 && (under.1 - after.1).abs() < 1e-9);
        v.zoom_at(rect, [0.0, 0.0], -100);
        assert_eq!(v.zoom, MAP_ZOOMS.len() - 1);
    }

    #[test]
    fn a_press_is_a_click_until_it_moves() {
        let mut s = MapScreen::default();
        s.press = Some(([10.0, 10.0], false));
        s.drag([11.0, 11.0], [1.0, 1.0]);
        assert_eq!((s.view.x, s.view.z), (0.0, 0.0));
        assert!(s.release());
        s.press = Some(([10.0, 10.0], false));
        s.drag([20.0, 10.0], [10.0, 0.0]);
        assert_eq!(s.view.x, -10.0);
        assert!(!s.release());
        assert!(!s.release());
    }

    #[test]
    fn images_fit_the_slot() {
        assert_eq!(image_size(400.0, 200.0), (400, 200, 1.0));
        let (w, h, s) = image_size(2048.0, 1000.0);
        assert!(w <= IMAGE_MAX && h <= IMAGE_MAX && s == 2.0);
        assert_eq!((w, h), (1024, 500));
    }

    #[test]
    fn composing_draws_known_ground_and_leaves_the_rest_dark() {
        let palette = [0u32, 0xFF0000];
        let mut s = Store::default();
        s.put((0, 0), tile(1, 64));
        let view = MapView { x: 0.0, z: 0.0, zoom: 3 };
        let mut out = Vec::new();
        compose(&s, &palette, &view, 64, 64, 1.0, &mut out);
        assert_eq!(out.len(), 64 * 64 * 4);
        // Texel (32, 32) is block (0, 0); (0, 0) is block (-32, -32), unexplored.
        let at = |i: usize, j: usize| &out[(j * 64 + i) * 4..(j * 64 + i) * 4 + 4];
        assert_eq!(at(32, 32), &[255, 0, 0, 255]);
        assert_eq!(at(47, 47), &[255, 0, 0, 255]);
        assert_eq!(at(0, 0), &UNKNOWN);
        assert_eq!(at(48, 32), &UNKNOWN);
    }

    #[test]
    fn list_pages() {
        assert_eq!(list_rows(240.0), 7);
        assert_eq!(list_rows(10.0), 1);
    }
}
