// Lumos World Map: between Horizon's `Game` and the rules in `lumos_world_map`.
//
// Mapping: the chunks the client has loaded are scanned a few a frame (the block seen from
// above, its height, the water over the ground, and for this mod's side files the block, the
// biome and the light), into tiles kept per world or server, sub-world, dimension and cave
// layer, in Lumos Map's folders and region files, so Lumos Map's minimap shows what this map
// explored and this map shows what the minimap explored. Region files are written by one
// worker thread, each merged with what is on disk.
//
// Drawing: 128 x 128 textures in Horizon's map slots (`game.maps.slots`, the framed maps'
// atlas slots, under ids no map has), composed in the frame hook within a time budget and
// drawn by the screen hook. Near zooms come from the tiles of the regions kept in memory (at
// most the "Regions kept in memory" setting), far ones from the per-region overviews, made off
// the frame from the region files.
//
// Lumos Map works with this mod through files and settings only (neither needs the other to
// build): when this mod is on, Lumos Map opens this map for its M and enlarge keys, stops
// writing explored regions (this mod writes them) and reads them back for its minimap, and
// follows the sub-world this mod chose.
use crate::game::lumos_world_map as m;
use crate::game::lumos_world_map_keys as keys;
use crate::game::{Game, Screen};
use crate::mod_api::{NativeMod, NativeScreenView, NativeWorldId};
use horizon_client::blocks::{Fluid, MAP_COLORS};
use horizon_client::chunk::Chunk;
use horizon_client::hud::{HudBatch, rgba};
use horizon_client::mesher::TintSource;
use horizon_client::text::Text;
use horizon_client::ui;
use horizon_world::registry::BlockStateId;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

/// How often the loaded chunks are looked over for new ones.
const REFILL: Duration = Duration::from_secs(1);
/// How often the chunks round the player are scanned again after blocks changed.
const NEAR_RESCAN: Duration = Duration::from_secs(2);
/// How often changed regions are written.
const SAVE_EVERY: Duration = Duration::from_secs(30);
/// How often the region cache is looked over.
const CACHE_EVERY: Duration = Duration::from_millis(500);
/// How often the waypoint files and the minimap's footsteps are looked at for changes.
const FILES_EVERY: Duration = Duration::from_secs(1);
/// How often the roof over the player is tested.
const ROOF_EVERY: Duration = Duration::from_millis(200);
/// Time a frame may spend composing textures, with the map open.
const TEX_BUDGET: Duration = Duration::from_micros(4000);
/// Cave layers' scanning a frame.
const CAVE_BUDGET: Duration = Duration::from_micros(500);
/// The most layers (of any dimension) kept open at once.
const STORES_KEPT: usize = 4;
/// Textures not shown for this many frames are let go.
const TEX_FORGET: u64 = 600;

/// The screen's bars and tool column, GUI pixels.
const TOP: f32 = 16.0;
const BOTTOM: f32 = 24.0;
const TOOL_W: f32 = 100.0;
const LIST_W: f32 = 210.0;

// Column flags of the scan's palette.
const FLUID: u8 = 3;
const AIR: u8 = 4;
const ROOF: u8 = 8;
const GLASS: u8 = 16;
const FLOWER: u8 = 32;

const FLOWERS: [&str; 20] = [
    "dandelion", "poppy", "blue_orchid", "allium", "azure_bluet", "oxeye_daisy", "cornflower", "lily_of_the_valley",
    "wither_rose", "torchflower", "sunflower", "lilac", "rose_bush", "peony", "pink_petals", "pitcher_plant",
    "open_eyeblossom", "closed_eyeblossom", "wildflowers", "spore_blossom",
];

/// Per block state: its map colour, flags, colour from above and name.
struct Palette {
    color: Vec<u8>,
    flags: Vec<u8>,
    look: Vec<m::BlockColour>,
    /// The block's name in `State::names`.
    name: Vec<u16>,
}

impl Palette {
    fn new(game: &Game<'_>, look: Vec<m::BlockColour>, names: &mut m::Names) -> Self {
        let facts = game.res.facts();
        let n = facts.states.len();
        let (mut color, mut flags, mut name) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        for (i, f) in facts.states.iter().enumerate() {
            color.push(f.map_color);
            let mut b = match f.fluid {
                Fluid::None => 0,
                Fluid::Water => 1,
                Fluid::Lava => 2,
            };
            if facts.is_air(BlockStateId(i as u32)) {
                b |= AIR;
            } else if b == 0 && f.map_color != 0 && !f.skylight_down && !f.has_class("LeavesBlock") {
                b |= ROOF;
            }
            let short = f.name.strip_prefix("minecraft:").unwrap_or(&f.name);
            if f.map_color != 0 && (short.ends_with("stained_glass") || short.ends_with("stained_glass_pane")) {
                b |= GLASS;
            }
            if f.has_class("FlowerBlock") || f.has_class("TallFlowerBlock") || short.ends_with("_tulip") || FLOWERS.contains(&short) {
                b |= FLOWER;
            }
            flags.push(b);
            name.push(names.intern(&f.name));
        }
        Self { color, flags, look, name }
    }

    #[inline]
    fn get(&self, id: u32) -> (u8, u8) {
        let i = id as usize;
        (self.color.get(i).copied().unwrap_or(0), self.flags.get(i).copied().unwrap_or(AIR))
    }
}

/// Every block state's colour from above, made off the frame from the resources loaded now.
fn colour_table(game: &Game<'_>) -> Receiver<(u64, Vec<m::BlockColour>)> {
    let (tx, rx) = mpsc::channel();
    let states = game.native_block_states();
    let res = game.native_client_resources();
    let version = game.native_block_sprites_version();
    let _ = std::thread::Builder::new().name("lumos-world-map-colours".into()).spawn(move || {
        let mut averages: HashMap<String, Option<u32>> = HashMap::new();
        let mut out = vec![m::BlockColour::default(); states.len()];
        for (id, facts) in states.iter() {
            if facts.air {
                continue;
            }
            let Some(sprites) = states.sprites(id) else {
                continue;
            };
            let (sprite, tint) = match sprites.sides[1].first() {
                Some(face) => (Some(face.sprite), face.tint),
                None => (sprites.particle, plant_tint(&facts.name)),
            };
            let Some(name) = sprite.and_then(|s| states.texture(s)) else {
                continue;
            };
            let avg = *averages
                .entry(name.to_owned())
                .or_insert_with(|| res.texture(name).and_then(|t| m::average(t.width, t.height, &t.rgba)));
            let Some(rgb) = avg else {
                continue;
            };
            out[id.0 as usize] = m::BlockColour {
                rgb,
                tint: match tint {
                    None => m::Tint::None,
                    Some(TintSource::Grass) => m::Tint::Grass,
                    Some(TintSource::Foliage) => m::Tint::Foliage,
                    Some(TintSource::DryFoliage) => m::Tint::DryFoliage,
                    Some(TintSource::Water) => m::Tint::Water,
                    Some(TintSource::Fixed(c)) => m::Tint::Fixed(c),
                },
                known: true,
            };
        }
        let _ = tx.send((version, out));
    });
    rx
}

/// The tint of a plant whose model is only crossed planes.
fn plant_tint(name: &str) -> Option<TintSource> {
    let n = name.rsplit(':').next().unwrap_or(name);
    if n.ends_with("grass") || n.ends_with("fern") || n == "bush" || n == "sugar_cane" {
        Some(TintSource::Grass)
    } else if n == "vine" {
        Some(TintSource::Foliage)
    } else {
        None
    }
}

/// Where a scan looks down from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scan {
    Top,
    /// Under a roofed dimension's roof (its top block).
    UnderRoof(i32),
    /// Under the ground: the full cave map of an open dimension.
    UnderGround,
    /// A cave layer: rock at `start` is a wall, else the first floor down to `depth` under it.
    Cave { start: i32, depth: i32 },
}

/// How the scans colour what they find (the look settings that are baked into tiles).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Baked {
    glass: bool,
    flowers: bool,
    water_depth: bool,
    biomes: bool,
    blend: bool,
}

impl Baked {
    fn of(s: &m::Settings) -> Self {
        Self { glass: s.glass, flowers: s.flowers, water_depth: s.water_depth, biomes: s.biomes, blend: s.blend }
    }
}

#[derive(Clone, Copy, Default)]
struct Hit {
    id: u32,
    color: u8,
    y: i32,
    depth: u8,
    ground: Option<(u32, u8)>,
    glass: Option<(u32, u8)>,
    kind: u8,
    found: bool,
}

/// Plains' tints, for the map with biome tints off.
const PLAINS: m::BiomeTints = m::BiomeTints { grass: 0x91BD59, foliage: 0x77AB2F, dry_foliage: 0xA0A069, water: 0x3F76E4 };

/// One chunk's columns as `mode` sees them, and what each is (block, biome, light).
#[allow(clippy::too_many_arguments)]
fn scan(
    world: &horizon_client::world::World,
    chunk: &Chunk,
    palette: &Palette,
    air: u32,
    mode: Scan,
    baked: Baked,
    tints: &mut dyn FnMut(u16, i32, i32) -> m::BiomeTints,
    biome_name: &mut dyn FnMut(u16) -> u16,
) -> (m::Tile, m::Extra) {
    let min = chunk.min_y();
    let top = (0..chunk.sections.len()).rev().find(|&s| !chunk.sections[s].is_uniform(air)).map_or(min, |s| min + s as i32 * 16 + 15);
    let (cx, cz) = (chunk.x * 16, chunk.z * 16);
    let mut hits = [Hit::default(); 256];
    for z in 0..16 {
        for x in 0..16 {
            let mut y = top;
            let mut wall = false;
            let mut lowest = min + 1;
            let mut deep = false;
            match mode {
                Scan::Top => {}
                Scan::UnderRoof(roof) => {
                    y = y.min(roof);
                    while y > min && palette.get(chunk.get(x, y, z)).1 & AIR == 0 {
                        y -= 1;
                    }
                }
                Scan::UnderGround => {
                    while y > min && palette.get(chunk.get(x, y, z)).1 & AIR != 0 {
                        y -= 1;
                    }
                    while y > min && palette.get(chunk.get(x, y, z)).1 & AIR == 0 {
                        y -= 1;
                    }
                }
                Scan::Cave { start, depth } => {
                    lowest = (start - depth).max(min + 1);
                    deep = true;
                    let (color, flags) = palette.get(chunk.get(x, start, z));
                    if flags & AIR == 0 && color != 0 && flags & FLUID == 0 {
                        wall = true;
                        y = start;
                    } else {
                        y = y.min(start);
                    }
                }
            }
            let i = (z * 16 + x) as usize;
            if y < lowest && deep {
                hits[i] = Hit { color: m::BLACK, y: lowest, kind: m::DEEP, found: true, ..Hit::default() };
                continue;
            }
            let mut glass: Option<Hit> = None;
            while y >= lowest {
                let id = chunk.get(x, y, z);
                let (color, flags) = palette.get(id);
                if color == 0 || (!baked.flowers && flags & FLOWER != 0 && !wall) {
                    y -= 1;
                    continue;
                }
                let kind = if wall { m::WALL } else { m::FLOOR };
                if baked.glass && flags & GLASS != 0 && !wall {
                    glass.get_or_insert(Hit { id, color, y, kind, found: true, ..Hit::default() });
                    y -= 1;
                    continue;
                }
                let mut hit = Hit { id, color, y, kind, found: true, glass: glass.map(|g| (g.id, g.color)), ..Hit::default() };
                if flags & FLUID == 1 {
                    let mut below = y - 1;
                    while below > min {
                        let gid = chunk.get(x, below, z);
                        let (gc, gf) = palette.get(gid);
                        if gf & FLUID == 1 {
                            hit.depth = hit.depth.saturating_add(1);
                        } else if gc != 0 {
                            hit.ground = Some((gid, gc));
                            break;
                        }
                        below -= 1;
                    }
                }
                hits[i] = hit;
                glass = None;
                break;
            }
            if let Some(g) = glass {
                hits[i] = g;
            }
            if deep && !hits[i].found {
                hits[i] = Hit { color: m::BLACK, y: lowest, kind: m::DEEP, found: true, ..Hit::default() };
            }
        }
    }
    let mut grid: m::TintGrid = [m::BiomeTints::default(); 36];
    let mut biomes = [0u16; 256];
    for qz in 0..6i32 {
        for qx in 0..6i32 {
            let (lx, lz) = ((qx - 1) * 4 + 1, (qz - 1) * 4 + 1);
            let own = hits[(lz.clamp(0, 15) * 16 + lx.clamp(0, 15)) as usize];
            let y = if own.found { own.y } else { 64 };
            let (dx, dz) = (lx.div_euclid(16), lz.div_euclid(16));
            let other = if dx == 0 && dz == 0 { None } else { world.chunks.get(&(chunk.x + dx, chunk.z + dz)) };
            let biome = match other {
                Some(c) => c.biome(lx.rem_euclid(16), y, lz.rem_euclid(16)),
                None => chunk.biome(lx.clamp(0, 15), y, lz.clamp(0, 15)),
            };
            grid[(qz * 6 + qx) as usize] = if baked.biomes { tints(biome, cx + lx, cz + lz) } else { PLAINS };
        }
    }
    for z in 0..16 {
        for x in 0..16 {
            let h = hits[(z * 16 + x) as usize];
            biomes[(z * 16 + x) as usize] = chunk.biome(x, if h.found { h.y } else { 64 }, z);
        }
    }
    let colour = |id: u32, t: &m::BiomeTints| {
        let look = palette.look.get(id as usize).copied().unwrap_or_default();
        look.known.then(|| m::block_rgb(&look, t))
    };
    let map_rgb = |c: u8| MAP_COLORS.get(usize::from(c)).copied().unwrap_or(0);
    let mut tile = m::Tile::default();
    let mut extra = m::Extra::default();
    let mut names: HashMap<u16, u16> = HashMap::new();
    for z in 0..16 {
        for x in 0..16 {
            let i = (z * 16 + x) as usize;
            let h = hits[i];
            if !h.found {
                continue;
            }
            tile.height[i] = h.y.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            if h.kind == m::DEEP {
                tile.color[i] = h.color;
                tile.kind[i] = m::DEEP;
                continue;
            }
            let t = if baked.blend { m::blend_tints(&grid, x, z) } else { grid[((z / 4 + 1) * 6 + x / 4 + 1) as usize] };
            let mut rgb = colour(h.id, &t).unwrap_or(0);
            if let Some((gid, gc)) = h.ground
                && rgb != 0
                && baked.water_depth
            {
                let ground = colour(gid, &t).unwrap_or_else(|| map_rgb(gc));
                rgb = m::water_over(ground, rgb, h.depth).max(1);
            }
            if let Some((gid, gc)) = h.glass {
                let base = if rgb != 0 { rgb } else { map_rgb(h.color) };
                let tint = colour(gid, &t).unwrap_or_else(|| map_rgb(gc));
                rgb = m::glass_over(base, tint).max(1);
            }
            tile.color[i] = h.color;
            tile.rgb[i] = rgb;
            tile.depth[i] = if baked.water_depth { h.depth } else { 0 };
            tile.kind[i] = h.kind;
            extra.block[i] = palette.name.get(h.id as usize).copied().unwrap_or(m::NO_NAME);
            let b = biomes[i];
            extra.biome[i] = *names.entry(b).or_insert_with(|| biome_name(b));
            extra.light[i] = world.light(cx + x, h.y + 1, cz + z) & 15;
        }
    }
    (tile, extra)
}

/// The Y of the first roof block over column `x, z` above `head`, None under open sky.
fn roof_over(world: &horizon_client::world::World, palette: &Palette, x: i32, head: i32, z: i32) -> Option<i32> {
    if world.light(x, head, z) >> 4 == 15 {
        return None;
    }
    let chunk = world.chunks.get(&(x >> 4, z >> 4))?;
    let air = world.air.0;
    let mut y = head + 1;
    while y < chunk.max_y() {
        let section = chunk.section_index(y)?;
        if chunk.sections[section].is_uniform(air) {
            y = (y & !15) + 16;
            continue;
        }
        if palette.get(chunk.get(x & 15, y, z & 15)).1 & ROOF != 0 {
            return Some(y);
        }
        y += 1;
    }
    None
}

// ---------------------------------------------------------------- saving

/// A region's files to write: Lumos Map's `.lmr` and this mod's `.lwx`, each merged with
/// what is on disk.
struct Job {
    dir: PathBuf,
    region: (i32, i32),
    lmr: Vec<u8>,
    lwx: Option<Vec<u8>>,
}

/// The one thread that writes region files, so two saves of a region never race.
fn writer() -> Sender<Job> {
    thread_local! {
        static TX: RefCell<Option<Sender<Job>>> = const { RefCell::new(None) };
    }
    TX.with_borrow_mut(|tx| {
        tx.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            let _ = std::thread::Builder::new().name("lumos-world-map-save".into()).spawn(move || {
                let mut n = 0u64;
                while let Ok(job) = rx.recv() {
                    if std::fs::create_dir_all(&job.dir).is_err() {
                        continue;
                    }
                    let mut write = |name: String, bytes: Vec<u8>| {
                        n += 1;
                        let path = job.dir.join(name);
                        let tmp = path.with_extension(format!("{n}.wmtmp"));
                        if std::fs::write(&tmp, &bytes).is_ok() {
                            let _ = std::fs::rename(&tmp, &path);
                        }
                    };
                    let lmr_name = m::region_file(job.region);
                    let old = std::fs::read(job.dir.join(&lmr_name)).ok();
                    write(lmr_name, m::merge_region(job.region, old.as_deref(), &job.lmr));
                    if let Some(lwx) = job.lwx {
                        let name = extras_file(job.region);
                        let old = std::fs::read(job.dir.join(&name)).ok();
                        write(name, m::merge_extras(job.region, old.as_deref(), &lwx));
                    }
                }
            });
            tx
        })
        .clone()
    })
}

fn extras_file(region: (i32, i32)) -> String {
    format!("r.{}.{}.lwx", region.0, region.1)
}

/// Regions read off the frame: tiles, and side files with their own names.
struct Loaded {
    region: (i32, i32),
    tiles: Vec<((i32, i32), m::Tile)>,
    extras: Vec<((i32, i32), m::Extra)>,
    names: m::Names,
}

fn load_region(dir: PathBuf, band: Option<i32>, region: (i32, i32)) -> Receiver<Loaded> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new().name("lumos-world-map-load".into()).spawn(move || {
        let mut names = m::Names::default();
        let tiles = std::fs::read(dir.join(m::region_file(region)))
            .ok()
            .filter(|b| m::region_band(b) == band)
            .and_then(|b| m::decode_region(region, &b))
            .unwrap_or_default();
        let extras = std::fs::read(dir.join(extras_file(region)))
            .ok()
            .and_then(|b| m::decode_extras(region, &b, &mut names))
            .unwrap_or_default();
        let _ = tx.send(Loaded { region, tiles, extras, names });
    });
    rx
}

/// Every region's overview in `dir`, made off the frame and sent as each is done.
fn index_overviews(dir: PathBuf, band: Option<i32>, look: m::Look, cave: Option<m::CaveShade>) -> Receiver<((i32, i32), m::Overview)> {
    let (tx, rx) = mpsc::channel();
    let palette: Vec<u32> = MAP_COLORS.to_vec();
    let _ = std::thread::Builder::new().name("lumos-world-map-overviews".into()).spawn(move || {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(region) = name.to_str().and_then(m::region_from_file) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(e.path()) else {
                continue;
            };
            if m::region_band(&bytes) != band {
                continue;
            }
            let Some(tiles) = m::decode_region(region, &bytes) else {
                continue;
            };
            let tiles: HashMap<(i32, i32), m::Tile> = tiles.into_iter().collect();
            let mut column = |x: i32, z: i32| tiles.get(&(x >> 4, z >> 4)).and_then(|t| t.col(x, z)).map(|c| (c, 0u8));
            let ov = m::overview_of(&mut column, &palette, look, cave, region);
            if tx.send((region, ov)).is_err() {
                return;
            }
        }
    });
    rx
}

// ---------------------------------------------------------------- one layer of one dimension

/// A dimension's layer (its surface, a cave band, its full cave map): the tiles of the regions
/// in memory, every region's overview, and (for the dimension played) its scanning.
struct Store {
    layer: m::Layer,
    dir: PathBuf,
    source: u32,
    store: m::Store,
    extras: HashMap<(i32, i32), m::Extra>,
    /// Regions read (or being read) from disk, and when each was last wanted.
    loaded: HashMap<(i32, i32), u64>,
    loading: Vec<Receiver<Loaded>>,
    pending: HashSet<(i32, i32)>,
    overviews: HashMap<(i32, i32), m::Overview>,
    indexing: Option<Receiver<((i32, i32), m::Overview)>>,
    /// The look the overviews were made in.
    overview_look: Option<m::Look>,
    /// Regions whose overview is made again from memory (their tiles changed).
    overview_dirty: HashSet<(i32, i32)>,
    seen: HashSet<(i32, i32)>,
    queue: VecDeque<(i32, i32)>,
    refilled: Option<Instant>,
    near: (Option<Instant>, u64),
    saved: Instant,
    cache_at: Option<Instant>,
    used: Instant,
}

impl Store {
    fn open(world: &Path, dim: &str, layer: m::Layer, source: u32) -> Self {
        let mut dir = world.join(m::folder_name(dim));
        if let Some(f) = layer.folder() {
            dir = dir.join(f);
        }
        Self {
            layer,
            dir,
            source,
            store: m::Store { band: layer.band(), ..m::Store::default() },
            extras: HashMap::new(),
            loaded: HashMap::new(),
            loading: Vec::new(),
            pending: HashSet::new(),
            overviews: HashMap::new(),
            indexing: None,
            overview_look: None,
            overview_dirty: HashSet::new(),
            seen: HashSet::new(),
            queue: VecDeque::new(),
            refilled: None,
            near: (None, 0),
            saved: Instant::now(),
            cache_at: None,
            used: Instant::now(),
        }
    }

    /// A cave layer's depth shading.
    fn cave(&self, depth: i32) -> Option<m::CaveShade> {
        match self.layer {
            m::Layer::Band(b) => Some(m::CaveShade { start: m::band_top(b), depth }),
            _ => None,
        }
    }

    fn put(&mut self, chunk: (i32, i32), tile: m::Tile, extra: m::Extra) {
        let region = m::region_of(chunk);
        let extra_changed = self.extras.get(&chunk) != Some(&extra);
        if extra_changed {
            self.extras.insert(chunk, extra);
        }
        if self.store.put(chunk, tile) || extra_changed {
            self.store.dirty.insert(region);
            self.overview_dirty.insert(region);
        }
    }

    fn column(&self, x: i32, z: i32) -> m::Sample {
        let c = self.store.column(x, z)?;
        let light = self.extras.get(&(x >> 4, z >> 4)).map_or(0, |e| e.light[((z & 15) * 16 + (x & 15)) as usize]);
        Some((c, light))
    }

    /// Whether region `r` is in memory and read whole.
    fn ready(&self, r: (i32, i32)) -> bool {
        self.loaded.contains_key(&r) && !self.pending.contains(&r)
    }

    /// Writes the changed regions (merged with disk by the writer).
    fn save(&mut self, names: &m::Names) {
        self.saved = Instant::now();
        let dirty = std::mem::take(&mut self.store.dirty);
        let tx = writer();
        for r in dirty {
            let lmr = self.store.encode_region(r);
            let mut chunks: Vec<((i32, i32), &m::Extra)> = self.extras.iter().filter(|(k, _)| m::region_of(**k) == r).map(|(k, e)| (*k, e)).collect();
            chunks.sort_by_key(|c| c.0);
            let lwx = (!chunks.is_empty()).then(|| m::encode_extras(&chunks, names));
            let _ = tx.send(Job { dir: self.dir.clone(), region: r, lmr, lwx });
        }
    }

    /// Takes in regions read from disk (the session's own tiles stay over them); the regions
    /// that came in.
    fn follow_loading(&mut self, names: &mut m::Names) -> Vec<(i32, i32)> {
        let mut done = Vec::new();
        self.loading.retain(|rx| match rx.try_recv() {
            Ok(l) => {
                done.push(l);
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        let mut regions = Vec::new();
        for l in done {
            let map: Vec<u16> = l.names.list.iter().map(|n| names.intern(n)).collect();
            let remap = |v: u16| if v == m::NO_NAME { v } else { map.get(usize::from(v)).copied().unwrap_or(m::NO_NAME) };
            for (k, mut e) in l.extras {
                e.block = e.block.map(remap);
                e.biome = e.biome.map(remap);
                self.extras.entry(k).or_insert(e);
            }
            let keys: Vec<(i32, i32)> = l.tiles.iter().map(|(k, _)| *k).collect();
            self.store.merge_loaded(l.tiles);
            self.store.changed.extend(keys);
            self.pending.remove(&l.region);
            self.overview_dirty.insert(l.region);
            regions.push(l.region);
        }
        regions
    }

    /// The region cache: `wanted` regions read, the least recently wanted ones let go past
    /// `limit` (saved first).
    fn follow_cache(&mut self, wanted: &HashSet<(i32, i32)>, limit: usize, tick: u64, names: &m::Names) {
        for r in wanted {
            match self.loaded.get_mut(r) {
                Some(t) => *t = tick,
                None => {
                    self.loaded.insert(*r, tick);
                    self.pending.insert(*r);
                    self.loading.push(load_region(self.dir.clone(), self.layer.band(), *r));
                }
            }
        }
        if self.cache_at.is_some_and(|t| t.elapsed() < CACHE_EVERY) {
            return;
        }
        self.cache_at = Some(Instant::now());
        let pending = &self.pending;
        let loaded: HashMap<(i32, i32), u64> = self.loaded.iter().filter(|(r, _)| !pending.contains(*r)).map(|(r, t)| (*r, *t)).collect();
        // Wanted by any caller this frame or the last (the scan round you and the view both ask).
        let recent: HashSet<(i32, i32)> = self.loaded.iter().filter(|(_, t)| **t + 2 >= tick).map(|(r, _)| *r).collect();
        let gone = m::to_evict(&loaded, &recent, limit);
        if gone.is_empty() {
            return;
        }
        if gone.iter().any(|r| self.store.dirty.contains(r)) {
            self.save(names);
        }
        let gone: HashSet<(i32, i32)> = gone.into_iter().collect();
        self.store.tiles.retain(|k, _| !gone.contains(&m::region_of(*k)));
        self.extras.retain(|k, _| !gone.contains(&m::region_of(*k)));
        self.loaded.retain(|r, _| !gone.contains(r));
    }

    /// Overviews: all of the dimension's made off the frame once per look, those of changed
    /// regions in memory made again here (a few a call); the regions whose overview changed.
    fn follow_overviews(&mut self, look: m::Look, depth: i32, budget: Duration) -> Vec<(i32, i32)> {
        let day = m::Look { night: m::NIGHT_STEPS, ..look };
        let cave = self.cave(depth);
        let mut changed = Vec::new();
        if self.overview_look != Some(day) {
            self.overview_look = Some(day);
            self.overviews.clear();
            self.indexing = Some(index_overviews(self.dir.clone(), self.layer.band(), day, cave));
            self.overview_dirty.extend(self.loaded.keys().copied());
        }
        if let Some(rx) = &self.indexing {
            let started = Instant::now();
            loop {
                match rx.try_recv() {
                    Ok((r, ov)) => {
                        // A region in memory is made from memory (it may be newer).
                        if !self.loaded.contains_key(&r) {
                            self.overviews.insert(r, ov);
                            changed.push(r);
                        }
                        if started.elapsed() > budget / 4 {
                            break;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.indexing = None;
                        break;
                    }
                }
            }
        }
        let started = Instant::now();
        let ready: Vec<(i32, i32)> = self.overview_dirty.iter().filter(|r| self.ready(**r)).copied().collect();
        for r in ready {
            if started.elapsed() > budget {
                break;
            }
            self.overview_dirty.remove(&r);
            let mut column = |x: i32, z: i32| self.column(x, z);
            let ov = m::overview_of(&mut column, &MAP_COLORS, day, cave, r);
            self.overviews.insert(r, ov);
            changed.push(r);
        }
        changed
    }
}

// ---------------------------------------------------------------- textures

/// A texture in a map slot.
struct Tex {
    id: i32,
    slot: Option<usize>,
    /// The epoch it was composed in (None: not composed since it was claimed).
    epoch: Option<u64>,
    dirty: bool,
    last: u64,
}

#[derive(Default)]
struct Textures {
    map: HashMap<m::TexKey, Tex>,
    by_id: HashMap<i32, m::TexKey>,
    ids: m::TexIds,
    /// Goes up when every texture must be composed again (the look, the colours).
    epoch: u64,
    frame: u64,
    /// The textures the screen wanted last frame, in the order it drew them.
    wanted: Vec<m::TexKey>,
}

impl Textures {
    /// The slot to draw `key` from, if it is composed.
    fn ready(&self, key: m::TexKey) -> Option<usize> {
        let t = self.map.get(&key)?;
        (t.epoch.is_some()).then_some(t.slot).flatten()
    }

    fn mark(&mut self, key: m::TexKey) {
        if let Some(t) = self.map.get_mut(&key) {
            t.dirty = true;
        }
    }

    /// Chunk `c` of `source` changed: its near textures composed again.
    fn chunk_changed(&mut self, source: u32, c: (i32, i32)) {
        for lod in 0..m::BASE_LOD {
            self.mark(m::TexKey::of_chunk(source, lod, c.0, c.1));
        }
    }

    /// Region `r` of `source` changed (came in, or its overview did).
    fn region_changed(&mut self, source: u32, r: (i32, i32), near: bool) {
        let lods = if near { 0..m::BASE_LOD } else { m::BASE_LOD..m::MAX_LOD + 1 };
        for lod in lods {
            let (x0, z0) = (f64::from(r.0) * 512.0, f64::from(r.1) * 512.0);
            for k in m::TexKey::covering(source, lod, x0, z0, x0 + 511.0, z0 + 511.0) {
                self.mark(k);
            }
        }
    }
}

// ---------------------------------------------------------------- state

#[derive(Default)]
struct State {
    /// `<instance>/lumos-map/<world>[/<sub-world>]`.
    world_dir: Option<PathBuf>,
    /// The played dimension's id when the world was opened.
    world_dim: String,
    stores: HashMap<(String, m::Layer), Store>,
    sources: HashMap<(String, m::Layer), u32>,
    names: m::Names,
    palette: Option<Palette>,
    colours: Option<Receiver<(u64, Vec<m::BlockColour>)>>,
    colours_version: Option<u64>,
    tints: HashMap<u16, m::BiomeTints>,
    biome_names: HashMap<u16, u16>,
    baked: Option<Baked>,
    world_asked: Option<Instant>,
    /// A server's folder and its sub-worlds, and when the spawn was last looked at.
    subworlds: Option<(PathBuf, m::SubWorlds)>,
    spawn_checked: Option<Instant>,
    /// Asking which map this is: the suggestion and the spawn; mapping waits for the answer.
    asking: Option<(m::Choice, [i32; 2])>,
    book: m::Book,
    book_seen: (Option<SystemTime>, Option<SystemTime>),
    global_path: Option<PathBuf>,
    files_at: Option<Instant>,
    steps: m::Footsteps,
    steps_dirty: bool,
    steps_for: String,
    screen: m::MapScreen,
    /// The map was set up for this opening.
    opened: bool,
    last_frame: Option<Instant>,
    tex: Textures,
    tick: u64,
    look: Option<m::Look>,
    cave_switch: m::CaveSwitch,
    roof_at: Option<Instant>,
    clock: Option<Instant>,
    was_dead: bool,
    notice: Option<(String, Instant)>,
    pending_command: Option<String>,
    pending_chat: Option<String>,
    pending_export: bool,
    /// The map closes on the next frame (its key pressed on it).
    pending_close: bool,
    /// The page shown on the mod's page in Options > Mods.
    options_page: usize,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with_borrow_mut(f)
}

fn settings(game: &Game<'_>) -> m::Settings {
    let own = m::settings(|key| game.native_setting(m::ID, key));
    if minimap_on(game) { own.with_minimap_caves(|k| game.native_setting(m::MINIMAP_ID, k)) } else { own }
}

/// Keeps `after` and writes the settings that differ from `before`.
fn store_settings(game: &Game<'_>, before: &m::Settings, after: m::Settings) {
    m::set_settings(after);
    let current = m::SETTINGS_VERSION.to_string();
    let all = game.native_setting(m::ID, m::SETTINGS_VERSION_KEY).is_none_or(|v| v.trim() != current);
    for ((key, value), (_, old)) in after.stored().into_iter().zip(before.stored()) {
        if all || value != old {
            game.native_set_setting(m::ID, key, &value);
        }
    }
}

/// Whether native mod `id` is built into this game and on (Lumos's switches allow it, and
/// it isn't turned off in Options > Mods).
fn mod_on(game: &Game<'_>, id: &str) -> bool {
    option_env!("HORIZON_MIXIN_MODS").is_some_and(|l| l.split(',').any(|m| m.trim() == id))
        && ::horizon_core::lumos_mods::on(id)
        && game.native_on(id)
}

/// Whether Lumos Map is here and on.
fn minimap_on(game: &Game<'_>) -> bool {
    mod_on(game, m::MINIMAP_ID)
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// `<instance>/lumos-map/`: Lumos Map's folder, shared.
fn base(game: &Game<'_>) -> PathBuf {
    game.native_instance_dir().join("lumos-map")
}

fn world_folder(game: &Game<'_>) -> Option<String> {
    match game.native_client_state()?.world? {
        NativeWorldId::Singleplayer(folder) => Some(format!("sp-{}", m::folder_name(&folder))),
        NativeWorldId::Server(address) => Some(format!("mp-{}", m::folder_name(&address))),
    }
}

fn player_pos(game: &Game<'_>) -> Option<[f64; 3]> {
    let p = game.player.as_ref()?;
    Some([p.position.x, p.position.y, p.position.z])
}

fn head_block(game: &Game<'_>) -> Option<i32> {
    let p = game.player.as_ref()?;
    Some((p.position.y + f64::from(p.dimensions.eye_height)).floor() as i32)
}

fn roof_top(game: &Game<'_>) -> Option<i32> {
    let shape = game.world.shape;
    shape.has_ceiling.then(|| shape.min_y + shape.logical_height - 1)
}

fn notice(s: &mut State, text: String) {
    s.notice = Some((text, Instant::now() + Duration::from_secs(3)));
}

fn map_open(game: &Game<'_>) -> bool {
    game.native_mod_screen() == Some(m::ID)
}

fn may_teleport(game: &Game<'_>) -> bool {
    game.ui.registries.permission >= 2
}

/// A server's sub-world folder, as Lumos Map keeps it: the first is the server's own.
fn sub_dir(server: &Path, name: &str) -> PathBuf {
    if name == "world-1" { server.to_owned() } else { server.join(name) }
}

fn save_subworlds(s: &State) {
    if let Some((dir, subs)) = &s.subworlds {
        let _ = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join("subworlds.txt"), subs.to_text()));
    }
}

/// Every open layer saved, and let go.
fn close_world(s: &mut State) {
    let names = std::mem::take(&mut s.names);
    for (_, mut st) in s.stores.drain() {
        st.save(&names);
    }
    s.names = names;
    save_steps(s);
    s.world_dir = None;
}

fn save_steps(s: &mut State) {
    if !std::mem::take(&mut s.steps_dirty) {
        return;
    }
    let Some(dir) = &s.world_dir else {
        return;
    };
    let dir = dir.join(m::folder_name(&s.steps_for));
    let text = s.steps.to_text();
    let _ = std::thread::Builder::new().name("lumos-world-map-save".into()).spawn(move || {
        let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join("footsteps.txt"), text));
    });
}

/// Opens world (or sub-world) folder `dir`: its waypoints; its layers open as they are wanted.
fn open_world(s: &mut State, dir: PathBuf) {
    if s.world_dir.as_ref() == Some(&dir) {
        return;
    }
    close_world(s);
    s.world_dir = Some(dir);
    s.book_seen = (None, None);
    load_book(s);
    s.screen.menu = None;
    s.screen.page = m::Page::Map;
    s.screen.layer = None;
    s.screen.dim = None;
    s.steps_for.clear();
    s.tex.epoch += 1;
}

fn load_book(s: &mut State) {
    let Some(dir) = &s.world_dir else {
        return;
    };
    let path = dir.join("waypoints.txt");
    let all_sets = s.book.all_sets;
    s.book = std::fs::read_to_string(&path).map(|t| m::Book::from_text(&t)).unwrap_or_default();
    s.book.all_sets = all_sets;
    let mut g_time = None;
    if let Some(g) = &s.global_path {
        g_time = mtime(g);
        if let Ok(t) = std::fs::read_to_string(g) {
            s.book.merge_global(m::Book::from_text(&t));
        }
    }
    s.book_seen = (mtime(&path), g_time);
}

/// Writes the waypoints as Lumos Map does (the world's file and the every-world file).
fn save_book(s: &mut State) {
    let Some(dir) = s.world_dir.clone() else {
        return;
    };
    let (own, all) = s.book.split_global();
    let path = dir.join("waypoints.txt");
    let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, own.to_text()));
    if let Some(g) = &s.global_path {
        let _ = std::fs::write(g, all.to_text());
    }
    s.book_seen = (mtime(&path), s.global_path.as_ref().and_then(|g| mtime(g)));
}

/// Waypoint files changed by Lumos Map are read again; the minimap's footsteps too.
fn follow_files(game: &Game<'_>, s: &mut State) {
    if s.files_at.is_some_and(|t| t.elapsed() < FILES_EVERY) {
        return;
    }
    s.files_at = Some(Instant::now());
    let Some(dir) = &s.world_dir else {
        return;
    };
    let now = (mtime(&dir.join("waypoints.txt")), s.global_path.as_ref().and_then(|g| mtime(g)));
    if now != s.book_seen {
        load_book(s);
        s.screen.menu = None;
    }
    // Footsteps: the dimension played's; Lumos Map writes them when it is on.
    let dim = game.dim.id();
    if s.steps_for != dim || (minimap_on(game) && s.tick % 10 == 0) {
        if s.steps_for != dim {
            save_steps(s);
        }
        let Some(dir) = &s.world_dir else {
            return;
        };
        s.steps = m::Footsteps::from_text(&std::fs::read_to_string(dir.join(m::folder_name(dim)).join("footsteps.txt")).unwrap_or_default());
        s.steps_for = dim.to_owned();
    }
}

/// Follows the world played: its folder (a server's sub-world, by the map mode).
fn follow_world(game: &Game<'_>, s: &mut State, set: &m::Settings) {
    if s.world_dir.is_some() && s.world_dim == game.dim.id() && s.spawn_checked.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
        return;
    }
    if s.world_asked.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) && s.world_dir.is_none() {
        return;
    }
    s.world_asked = Some(Instant::now());
    s.spawn_checked = Some(Instant::now());
    s.world_dim = game.dim.id().to_owned();
    let Some(world) = world_folder(game) else {
        return;
    };
    let server = base(game).join(&world);
    s.global_path = Some(base(game).join("global-waypoints.txt"));
    if !world.starts_with("mp-") {
        s.subworlds = None;
        s.asking = None;
        open_world(s, server);
        return;
    }
    let mut subs = match s.subworlds.take() {
        Some((d, subs)) if d == server => subs,
        _ => m::SubWorlds::from_text(&std::fs::read_to_string(server.join("subworlds.txt")).unwrap_or_default()),
    };
    if set.map_mode != 2
        && let Some(spawn) = game.spawn
        && s.asking.is_none()
    {
        let spawn = [spawn.x.floor() as i32, spawn.z.floor() as i32];
        let (choice, ask) = m::suggestion(&subs, spawn, set.map_mode == 0);
        if ask {
            s.asking = Some((choice, spawn));
            notice(s, "Which map is this world? Open the map to choose".into());
        } else if choice != m::Choice::Map(subs.current) || subs.list[subs.current].1.is_none() {
            let before = subs.clone();
            m::choose(&mut subs, choice, Some(spawn));
            if before != subs {
                s.subworlds = Some((server.clone(), subs.clone()));
                save_subworlds(s);
            }
        }
    }
    let dir = sub_dir(&server, subs.current_name());
    s.subworlds = Some((server, subs));
    open_world(s, dir);
}

/// The map-selection answer: the sub-world chosen becomes the one mapped (Lumos Map follows
/// `subworlds.txt`).
fn answer(s: &mut State, choice: m::Choice) {
    let spawn = s.asking.take().map(|(_, sp)| sp);
    let Some((server, subs)) = s.subworlds.as_mut() else {
        return;
    };
    m::choose(subs, choice, spawn);
    let dir = sub_dir(server, subs.current_name());
    let name = subs.current_name().to_owned();
    save_subworlds(s);
    open_world(s, dir);
    s.spawn_checked = Some(Instant::now() + Duration::from_secs(60 * 60 * 24));
    notice(s, format!("Map: {name}"));
}

/// The colour table: asked for when the resources change, taken in when made.
fn follow_colours(game: &Game<'_>, s: &mut State) {
    let version = game.native_block_sprites_version();
    if let Some(rx) = &s.colours
        && let Ok((v, table)) = rx.try_recv()
    {
        s.colours = None;
        s.palette = Some(Palette::new(game, table, &mut s.names));
        s.colours_version = Some(v);
        s.tints.clear();
        rescan(s);
    }
    if s.colours.is_none() && s.colours_version != Some(version) {
        s.colours = Some(colour_table(game));
        if s.palette.is_none() {
            s.palette = Some(Palette::new(game, Vec::new(), &mut s.names));
        }
    }
}

/// Every loaded chunk mapped again (new colours, or a look baked into tiles changed).
fn rescan(s: &mut State) {
    for st in s.stores.values_mut() {
        st.seen.clear();
        st.queue.clear();
        st.refilled = None;
    }
    s.tex.epoch += 1;
}

fn tints_of(game: &Game<'_>, cache: &mut HashMap<u16, m::BiomeTints>, biome: u16, x: i32, z: i32) -> m::BiomeTints {
    *cache.entry(biome).or_insert_with(|| {
        game.native_biome_tint(biome, x, z).map_or_else(Default::default, |c| m::BiomeTints {
            grass: c.grass,
            foliage: c.foliage,
            dry_foliage: c.dry_foliage,
            water: c.water,
        })
    })
}

/// The source id of a dimension's layer, for its textures.
fn source_of(s: &mut State, dim: &str, layer: m::Layer) -> u32 {
    let n = s.sources.len() as u32 + 1;
    *s.sources.entry((dim.to_owned(), layer)).or_insert(n)
}

/// Layer `layer` of `dim`, opened if it isn't (the least used other one closed past
/// `STORES_KEPT`).
fn store<'a>(s: &'a mut State, dim: &str, layer: m::Layer) -> Option<&'a mut Store> {
    let dir = s.world_dir.clone()?;
    let key = (dim.to_owned(), layer);
    if !s.stores.contains_key(&key) {
        if s.stores.len() >= STORES_KEPT {
            let played = s.world_dim.clone();
            let oldest = s
                .stores
                .iter()
                .filter(|(k, _)| !(k.0 == played && k.1 == m::Layer::Surface))
                .min_by_key(|(_, st)| st.used)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest
                && let Some(mut old) = s.stores.remove(&k)
            {
                old.save(&s.names);
            }
        }
        let source = source_of(s, dim, layer);
        s.stores.insert(key.clone(), Store::open(&dir, dim, layer, source));
    }
    let st = s.stores.get_mut(&key)?;
    st.used = Instant::now();
    Some(st)
}

/// How a layer of the dimension played is scanned.
fn scan_mode(game: &Game<'_>, layer: m::Layer, depth: i32) -> Scan {
    match layer {
        m::Layer::Surface => roof_top(game).map_or(Scan::Top, Scan::UnderRoof),
        m::Layer::Full => roof_top(game).map_or(Scan::UnderGround, Scan::UnderRoof),
        m::Layer::Band(b) => Scan::Cave { start: m::band_top(b), depth },
    }
}

/// Scans the loaded chunks into layer `layer` of the dimension played within `budget`: new
/// ones nearest first, then those round the player again after blocks changed.
fn explore(game: &Game<'_>, s: &mut State, layer: m::Layer, set: &m::Settings, budget: Duration) {
    let started = Instant::now();
    let Some(p) = player_pos(game) else {
        return;
    };
    let baked = Baked::of(set);
    let dim = game.dim.id();
    let mode = scan_mode(game, layer, set.depth_blocks());
    let tick = s.tick;
    let limit = set.cache_regions();
    let centre = ((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4);
    if store(s, dim, layer).is_none() {
        return;
    }
    let State { stores, palette, tints, biome_names, names, tex, .. } = s;
    let (Some(st), Some(palette)) = (stores.get_mut(&(dim.to_owned(), layer)), palette.as_ref()) else {
        return;
    };
    // The regions round you stay in memory (scanned tiles join what is on disk).
    let mut wanted: HashSet<(i32, i32)> = HashSet::new();
    let r = m::region_of(centre);
    for dz in -1..=1 {
        for dx in -1..=1 {
            wanted.insert((r.0 + dx, r.1 + dz));
        }
    }
    st.follow_cache(&wanted, limit, tick, names);
    for r in st.follow_loading(names) {
        tex.region_changed(st.source, r, true);
    }
    let world = &game.world;
    if st.queue.is_empty() && st.refilled.is_none_or(|t| t.elapsed() >= REFILL) {
        st.refilled = Some(Instant::now());
        st.seen.retain(|k| world.chunks.contains_key(k));
        let mut fresh: Vec<(i32, i32)> = world
            .chunks
            .keys()
            .filter(|k| !st.seen.contains(*k))
            .filter(|k| if st.store.tiles.contains_key(*k) { set.map_update } else { set.map_new })
            .copied()
            .collect();
        if set.map_update && world.revision != st.near.1 && st.near.0.is_none_or(|t| t.elapsed() >= NEAR_RESCAN) {
            st.near = (Some(Instant::now()), world.revision);
            for dz in -2..=2 {
                for dx in -2..=2 {
                    let k = (centre.0 + dx, centre.1 + dz);
                    if st.seen.contains(&k) {
                        fresh.push(k);
                    }
                }
            }
        }
        fresh.sort_by_key(|k| (k.0 - centre.0).abs().max((k.1 - centre.1).abs()));
        st.queue.extend(fresh);
    }
    let air = world.air.0;
    let mut tint = |b: u16, x: i32, z: i32| tints_of(game, tints, b, x, z);
    let mut biome_name = |b: u16| {
        *biome_names.entry(b).or_insert_with(|| {
            let name = world_biomes!(game).get(b).name.clone();
            names.intern(&name)
        })
    };
    while started.elapsed() < budget {
        let Some(k) = st.queue.pop_front() else {
            break;
        };
        let Some(chunk) = world.chunks.get(&k) else {
            continue;
        };
        let (tile, extra) = scan(world, chunk, palette, air, mode, baked, &mut tint, &mut biome_name);
        if tile.color.iter().any(|c| *c != 0) {
            st.put(k, tile, extra);
        }
        st.seen.insert(k);
    }
    for c in std::mem::take(&mut st.store.changed) {
        tex.chunk_changed(st.source, c);
    }
    if st.saved.elapsed() >= SAVE_EVERY {
        st.save(names);
    }
}

/// The roof test a few times a second: a roof over every column of the 3 x 3 square round
/// the player (open dimensions only), fed to the switch.
fn follow_roof(game: &Game<'_>, s: &mut State) {
    if s.roof_at.is_some_and(|t| t.elapsed() < ROOF_EVERY) {
        return;
    }
    s.roof_at = Some(Instant::now());
    let (Some(p), Some(head), Some(palette)) = (player_pos(game), head_block(game), s.palette.as_ref()) else {
        return;
    };
    let roofed = roof_top(game).is_none() && {
        let (bx, bz) = (p[0].floor() as i32, p[2].floor() as i32);
        let mut columns = Vec::with_capacity(9);
        'all: for dz in -1..=1 {
            for dx in -1..=1 {
                let r = roof_over(&game.world, palette, bx + dx, head, bz + dz);
                columns.push(r);
                if r.is_none() {
                    break 'all;
                }
            }
        }
        m::roofed(&columns)
    };
    let tick = (s.clock.get_or_insert_with(Instant::now).elapsed().as_millis() / 50) as u64;
    s.cave_switch.step(roofed, tick);
}

/// The layer the map shows now (in the dimension it shows).
fn shown_layer(game: &Game<'_>, s: &State, set: &m::Settings) -> m::Layer {
    let played = s.screen.dim.is_none();
    let auto = set.cave_mode == 0 && played && roof_top(game).is_none();
    m::shown_layer(s.screen.layer, auto, s.cave_switch.on, head_block(game).unwrap_or(64))
}

fn shown_dim(game: &Game<'_>, s: &State) -> String {
    s.screen.dim.clone().unwrap_or_else(|| game.dim.id().to_owned())
}

/// The night step for lighting now (the Overworld's sky; other dimensions keep theirs).
fn night_now(game: &Game<'_>, dim: &str) -> u8 {
    if dim != m::OVERWORLD {
        return m::NIGHT_STEPS;
    }
    let w = &game.world.weather;
    m::night_step(m::sky_light(game.world.day_time, w.rain_level.1, w.thunder_level.1))
}

/// Writes `rgba` (128 x 128) into map slot `slot`.
fn upload(game: &mut Game<'_>, slot: usize, rgba: Vec<u8>) -> bool {
    let Some(name) = horizon_client::hud::map_slot_sprite(slot) else {
        return false;
    };
    let Some(rect) = game.gui.atlas.sprites.get(name).copied() else {
        return false;
    };
    if rect.w < m::TEX || rect.h < m::TEX || rgba.len() != (m::TEX * m::TEX * 4) as usize {
        return false;
    }
    let (w, h) = (game.gui.atlas.width as f32, game.gui.atlas.height as f32);
    game.map_patches.push(horizon_client::font::Patch {
        x: (rect.u0 * w).round() as u32,
        y: (rect.v0 * h).round() as u32,
        width: m::TEX,
        height: m::TEX,
        rgba,
    });
    true
}

/// The textures the screen asked for: slots kept (or claimed), and the stale ones composed and
/// uploaded within the budget, the far ones first.
fn follow_textures(game: &mut Game<'_>, set: &m::Settings) {
    let started = Instant::now();
    let wanted = with(|s| {
        s.tex.frame += 1;
        let frame = s.tex.frame;
        s.tex.map.retain(|_, t| frame.saturating_sub(t.last) < TEX_FORGET);
        let ids: HashSet<i32> = s.tex.map.values().map(|t| t.id).collect();
        s.tex.by_id.retain(|id, _| ids.contains(id));
        std::mem::take(&mut s.tex.wanted)
    });
    let mut out = Vec::new();
    for key in wanted {
        // Keep (or find) its slot.
        let (id, had) = with(|s| {
            let id = s.tex.ids.id(key);
            s.tex.by_id.insert(id, key);
            let frame = s.tex.frame;
            let t = s.tex.map.entry(key).or_insert(Tex { id, slot: None, epoch: None, dirty: true, last: frame });
            t.last = frame;
            (id, t.slot)
        });
        let slot = match game.maps.slots.show(&id) {
            Some(slot) => Some(slot),
            None => match game.maps.slots.claim(id) {
                Some((slot, old)) => {
                    with(|s| {
                        if let Some(old) = old
                            && let Some(k) = s.tex.by_id.get(&old).copied()
                            && let Some(t) = s.tex.map.get_mut(&k)
                        {
                            t.slot = None;
                            t.epoch = None;
                        }
                        if let Some(t) = s.tex.map.get_mut(&key) {
                            t.epoch = None;
                        }
                    });
                    Some(slot)
                }
                None => None,
            },
        };
        let Some(slot) = slot else {
            with(|s| {
                if let Some(t) = s.tex.map.get_mut(&key) {
                    t.slot = None;
                }
            });
            continue;
        };
        if had != Some(slot) {
            with(|s| {
                if let Some(t) = s.tex.map.get_mut(&key) {
                    t.slot = Some(slot);
                    t.epoch = None;
                }
            });
        }
        let stale = with(|s| s.tex.map.get(&key).is_none_or(|t| t.dirty || t.epoch != Some(s.tex.epoch)));
        if !stale || started.elapsed() >= TEX_BUDGET {
            continue;
        }
        let composed = with(|s| compose_texture(game, s, set, key, &mut out));
        if composed {
            if upload(game, slot, std::mem::take(&mut out)) {
                with(|s| {
                    let epoch = s.tex.epoch;
                    if let Some(t) = s.tex.map.get_mut(&key) {
                        t.epoch = Some(epoch);
                        t.dirty = false;
                    }
                });
            }
        }
    }
}

/// Composes texture `key` into `out`; false when it can't be yet (its regions are still being
/// read, or its layer isn't open).
fn compose_texture(game: &Game<'_>, s: &mut State, set: &m::Settings, key: m::TexKey, out: &mut Vec<u8>) -> bool {
    let Some(((dim, _), _)) = s.sources.iter().find(|(_, v)| **v == key.source) else {
        return false;
    };
    let dim = dim.clone();
    let Some(st) = s.stores.values().find(|st| st.source == key.source) else {
        return false;
    };
    let night = if set.lighting { night_now(game, &dim) } else { m::NIGHT_STEPS };
    if key.from_overviews() {
        let get = |r: (i32, i32)| st.overviews.get(&r);
        m::compose_far(&get, key, night, out);
        return true;
    }
    let (ox, oz) = key.origin();
    let span = key.span() as f64;
    let regions = m::regions_in(ox as f64, oz as f64, ox as f64 + span - 1.0, oz as f64 + span - 1.0);
    if regions.iter().any(|r| !st.ready(*r)) {
        return false;
    }
    let look = s.look.unwrap_or_else(|| set.look(night));
    let mut column = |x: i32, z: i32| st.column(x, z);
    m::compose(&mut column, &MAP_COLORS, look, st.cave(set.depth_blocks()), ox as i32, oz as i32, 1 << key.lod, m::TEX as usize, out);
    true
}

/// Keeps the layer the map shows read (the regions in view, within the memory limit) and its
/// overviews made; whether the view's near detail fits in memory.
fn follow_shown(game: &Game<'_>, s: &mut State, set: &m::Settings, rect: [f32; 4]) -> bool {
    let dim = shown_dim(game, s);
    let layer = shown_layer(game, s, set);
    let tick = s.tick;
    let view = s.screen.view;
    let (x0, z0, x1, z1) = view.bounds(rect);
    let regions = m::regions_in(x0, z0, x1, z1);
    let limit = set.cache_regions();
    let near = m::fine_lod(view.bpp) < m::BASE_LOD || !set.overviews;
    let fits = regions.len() + 9 <= limit;
    let look = set.look(night_now(game, &dim));
    let depth = set.depth_blocks();
    let played = dim == game.dim.id();
    let centre = player_pos(game).map(|p| m::region_of(((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4)));
    if store(s, &dim, layer).is_none() {
        return false;
    }
    let State { stores, names, tex, .. } = s;
    let Some(st) = stores.get_mut(&(dim.clone(), layer)) else {
        return false;
    };
    let mut wanted: HashSet<(i32, i32)> = HashSet::new();
    if near && fits {
        wanted.extend(regions.iter().copied());
    }
    if played && let Some(r) = centre {
        for dz in -1..=1 {
            for dx in -1..=1 {
                wanted.insert((r.0 + dx, r.1 + dz));
            }
        }
    }
    st.follow_cache(&wanted, limit, tick, names);
    for r in st.follow_loading(names) {
        tex.region_changed(st.source, r, true);
    }
    for c in std::mem::take(&mut st.store.changed) {
        tex.chunk_changed(st.source, c);
    }
    if set.overviews {
        for r in st.follow_overviews(look, depth, Duration::from_micros(1500)) {
            tex.region_changed(st.source, r, false);
        }
    }
    near && fits
}

/// The world map's area of a `w` x `h` GUI.
fn map_rect(w: f32, h: f32, hide: bool, list: bool) -> [f32; 4] {
    if hide {
        return [0.0, 0.0, w, h];
    }
    let right = TOOL_W + if list { LIST_W } else { 0.0 };
    [0.0, TOP, (w - right).max(1.0), (h - TOP - BOTTOM).max(1.0)]
}

fn rect_now(game: &Game<'_>, s: &State) -> [f32; 4] {
    let (w, h) = game.gui_size();
    map_rect(w, h, s.screen.hide_ui, matches!(s.screen.page, m::Page::List { .. }))
}

/// Sets the map up as it opens: centred on you, at your layer, the map selection first when a
/// world is waiting for an answer.
fn on_open(game: &Game<'_>, s: &mut State, set: &m::Settings) {
    s.opened = true;
    let p = player_pos(game).unwrap_or_default();
    s.screen.view.centre_on(p[0], p[2]);
    let (lo, hi) = set.zooms();
    s.screen.view.bpp = s.screen.view.bpp.clamp(lo, hi);
    s.screen.view.target = s.screen.view.bpp;
    s.screen.follow = set.follow;
    s.screen.press = None;
    s.screen.menu = None;
    s.screen.layer = None;
    s.screen.dim = None;
    s.screen.page = match s.asking {
        Some((choice, _)) => m::Page::Maps { choice },
        None => m::Page::Map,
    };
}

/// `native_frame`: the world followed, chunks mapped, a death marked, the map's textures kept.
pub(super) fn on_frame(game: &mut Game<'_>) {
    if !game.native_on(m::ID) || game.player.is_none() {
        return;
    }
    let open = map_open(game);
    let set = settings(game);
    if let Some(text) = with(|s| s.pending_chat.take()) {
        game.native_mod_screen_close(m::ID);
        let _ = game.native_open_chat(&text);
        return;
    }
    if let Some(command) = with(|s| s.pending_command.take()) {
        game.native_mod_screen_close(m::ID);
        game.send_command(&command);
        return;
    }
    if with(|s| std::mem::take(&mut s.pending_close)) {
        game.native_mod_screen_close(m::ID);
        return;
    }
    let dead = game.screen == Screen::Dead;
    let minimap = minimap_on(game);
    let mut said = None;
    with(|s| {
        s.tick += 1;
        if open && !s.opened {
            on_open(game, s, &set);
        }
        if !open {
            s.opened = false;
        }
        follow_colours(game, s);
        let baked = Baked::of(&set);
        if s.baked.is_some_and(|b| b != baked) {
            rescan(s);
        }
        s.baked = Some(baked);
        follow_world(game, s, &set);
        follow_files(game, s);
        if set.cave_mode == 0 {
            follow_roof(game, s);
        }
        // Mapping (not while asking which map this is).
        if s.asking.is_none() && s.world_dir.is_some() {
            let budget = Duration::from_micros(set.budget_us(open));
            explore(game, s, m::Layer::Surface, &set, budget);
            let full = roof_top(game).is_none() && s.screen.layer == Some(m::Layer::Full) && open && s.screen.dim.is_none();
            if full {
                explore(game, s, m::Layer::Full, &set, CAVE_BUDGET);
            }
            let cave = if open && s.screen.dim.is_none() {
                match shown_layer(game, s, &set) {
                    m::Layer::Band(b) => Some(b),
                    _ => None,
                }
            } else if set.cave_scan && set.cave_mode == 0 && s.cave_switch.on && roof_top(game).is_none() {
                head_block(game).map(m::band_of)
            } else {
                None
            };
            if let Some(b) = cave {
                explore(game, s, m::Layer::Band(b), &set, CAVE_BUDGET);
            }
        }
        // Footsteps (Lumos Map records them when it is on).
        if set.footsteps
            && !minimap
            && let Some(p) = player_pos(game)
            && s.steps_for == game.dim.id()
            && s.steps.step(p[0], p[2])
        {
            s.steps_dirty = true;
        }
        if s.tick % 1800 == 0 {
            save_steps(s);
        }
        // Death points, when the minimap (which makes them) isn't on.
        if dead && !s.was_dead && set.deaths && !minimap && s.world_dir.is_some()
            && let Some(p) = player_pos(game)
        {
            s.book.add_death(m::block_at(p), game.dim.id(), m::DEATHS[set.deaths_kept.min(3)], now_secs());
            save_book(s);
        }
        s.was_dead = dead;
        if open {
            let rect = rect_now(game, s);
            let dt = s.last_frame.map_or(0.0, |t| t.elapsed().as_secs_f64());
            s.screen.view.step(rect, dt);
            if s.screen.follow
                && s.screen.dim.is_none()
                && let Some(p) = player_pos(game)
            {
                s.screen.view.centre_on(p[0], p[2]);
            }
            let dim = shown_dim(game, s);
            let look = set.look(if set.lighting { night_now(game, &dim) } else { m::NIGHT_STEPS });
            if s.look != Some(look) {
                s.look = Some(look);
                s.tex.epoch += 1;
            }
            follow_shown(game, s, &set, rect);
            if std::mem::take(&mut s.pending_export) {
                said = Some(export_png(game, s, &set));
            }
        }
        s.last_frame = Some(Instant::now());
    });
    if let Some(text) = said {
        with(|s| notice(s, text));
    }
    if open {
        follow_textures(game, &set);
    }
}

/// The client left its level: every layer saved and let go.
pub(super) fn on_level_unload(game: &mut Game<'_>) {
    let minimap = minimap_on(game);
    with(|s| {
        if !minimap && s.book.drop_temporary() {
            save_book(s);
        }
        close_world(s);
        s.subworlds = None;
        s.asking = None;
        s.world_asked = None;
        s.spawn_checked = None;
        s.was_dead = false;
        s.cave_switch = m::CaveSwitch::default();
        s.tex.epoch += 1;
    });
}

/// `native_key`: M opens the map (and Lumos Map's enlarge key, when set), the quick confirm
/// key answers which map this is.
pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || !game.native_on(m::ID) || game.player.is_none() {
        return;
    }
    if map_open(game) {
        return;
    }
    if !game.native_playing() {
        return;
    }
    // (Lumos Map's enlarge key opens this map from Lumos Map's own key hook, when set.)
    if keys::pressed(game, keys::MAP, key, true) {
        game.native_mod_screen_open(m::ID);
    } else if keys::pressed(game, keys::CONFIRM, key, true) {
        let said = with(|s| match s.asking {
            Some((choice, _)) => {
                answer(s, choice);
                s.subworlds.as_ref().map(|(_, subs)| format!("World map: {}", subs.current_name()))
            }
            None => None,
        });
        if let Some(text) = said {
            game.native_client_action_bar(Text::literal(text), false);
        }
    }
}

/// The map saved as a PNG: the view at a block a pixel, or the whole map from its overviews.
fn export_png(game: &Game<'_>, s: &mut State, set: &m::Settings) -> String {
    let dim = shown_dim(game, s);
    let layer = shown_layer(game, s, set);
    let rect = rect_now(game, s);
    let view = s.screen.view;
    let depth = set.depth_blocks();
    let look = set.look(m::NIGHT_STEPS);
    let Some(st) = s.stores.get(&(dim.clone(), layer)) else {
        return "Nothing to save yet".into();
    };
    let (w, h, image) = if set.export == 0 {
        let (x0, z0, x1, z1) = view.bounds(rect);
        let (w, h) = (((x1 - x0) as u32).clamp(1, 2048), ((z1 - z0) as u32).clamp(1, 2048));
        // Composed 256 blocks a side at a time (a whole view at once would need a sample grid
        // of hundreds of megabytes).
        const PIECE: u32 = 256;
        let (ox, oz) = (x0.floor() as i32, z0.floor() as i32);
        let mut img = vec![0u8; (w * h * 4) as usize];
        let mut out = Vec::new();
        let mut column = |x: i32, z: i32| st.column(x, z);
        for pz in (0..h).step_by(PIECE as usize) {
            for px in (0..w).step_by(PIECE as usize) {
                m::compose(&mut column, &MAP_COLORS, look, st.cave(depth), ox + px as i32, oz + pz as i32, 1, PIECE as usize, &mut out);
                let cols = PIECE.min(w - px) as usize;
                for j in 0..PIECE.min(h - pz) as usize {
                    let src = j * PIECE as usize * 4;
                    let dst = (((pz as usize + j) * w as usize) + px as usize) * 4;
                    img[dst..dst + cols * 4].copy_from_slice(&out[src..src + cols * 4]);
                }
            }
        }
        (w, h, img)
    } else {
        if st.overviews.is_empty() {
            return "No overview yet (switch overviews on, or wait)".into();
        }
        let (rx0, rz0) = st.overviews.keys().fold((i32::MAX, i32::MAX), |a, r| (a.0.min(r.0), a.1.min(r.1)));
        let (rx1, rz1) = st.overviews.keys().fold((i32::MIN, i32::MIN), |a, r| (a.0.max(r.0), a.1.max(r.1)));
        let (cols, rows) = (((rx1 - rx0 + 1) as usize).min(128), ((rz1 - rz0 + 1) as usize).min(128));
        let (w, h) = (cols * m::OVERVIEW, rows * m::OVERVIEW);
        let mut img = vec![0u8; w * h * 4];
        for (r, ov) in &st.overviews {
            let (cx, cz) = ((r.0 - rx0) as usize, (r.1 - rz0) as usize);
            if cx >= cols || cz >= rows {
                continue;
            }
            for j in 0..m::OVERVIEW {
                let dst = ((cz * m::OVERVIEW + j) * w + cx * m::OVERVIEW) * 4;
                let src = j * m::OVERVIEW * 4;
                img[dst..dst + m::OVERVIEW * 4].copy_from_slice(&ov.rgba[src..src + m::OVERVIEW * 4]);
            }
        }
        (w as u32, h as u32, img)
    };
    let dir = base(game).join("exports");
    let world = s
        .world_dir
        .as_ref()
        .and_then(|d| d.strip_prefix(base(game)).ok())
        .map_or_else(|| "map".to_owned(), |p| m::folder_name(&p.to_string_lossy()));
    let name = format!("world-map-{world}-{}-{}.png", m::folder_name(&dim), now_secs());
    let path = dir.join(&name);
    let _ = std::thread::Builder::new().name("lumos-world-map-export".into()).spawn(move || {
        let png = m::png(w, h, &image);
        let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, png));
    });
    format!("Saved lumos-map/exports/{name} ({w} x {h})")
}

// ---------------------------------------------------------------- the screen

fn ours(game: &Game<'_>, id: &str) -> bool {
    id == m::SCREEN && map_open(game) && game.native_on(m::ID)
}

fn inside(r: [f32; 4], p: [f32; 2]) -> bool {
    p[0] >= r[0] && p[0] < r[0] + r[2] && p[1] >= r[1] && p[1] < r[1] + r[3]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Dimension,
    Caves,
    LayerDown,
    LayerUp,
    Waypoints,
    Maps,
    Centre,
    Follow,
    Grid,
    ZoomIn,
    ZoomOut,
    Png,
    Settings,
    Controls,
}

/// The tool column's buttons (the map selection only on a server).
fn tools(w: f32, mp: bool) -> Vec<(Tool, [f32; 4])> {
    let x = w - TOOL_W + 4.0;
    let full = TOOL_W - 8.0;
    let half = (full - 4.0) / 2.0;
    let mut out = Vec::new();
    let mut y = TOP + 4.0;
    let mut row = |a: Tool, b: Option<Tool>, out: &mut Vec<(Tool, [f32; 4])>| {
        match b {
            Some(b) => {
                out.push((a, [x, y, half, 18.0]));
                out.push((b, [x + half + 4.0, y, half, 18.0]));
            }
            None => out.push((a, [x, y, full, 18.0])),
        }
        y += 21.0;
    };
    row(Tool::Dimension, None, &mut out);
    row(Tool::Caves, None, &mut out);
    row(Tool::LayerDown, Some(Tool::LayerUp), &mut out);
    row(Tool::Waypoints, None, &mut out);
    if mp {
        row(Tool::Maps, None, &mut out);
    }
    row(Tool::Centre, Some(Tool::Follow), &mut out);
    row(Tool::ZoomIn, Some(Tool::ZoomOut), &mut out);
    row(Tool::Grid, None, &mut out);
    row(Tool::Png, None, &mut out);
    row(Tool::Settings, None, &mut out);
    row(Tool::Controls, None, &mut out);
    out
}

fn tool_label(game: &Game<'_>, t: Tool, s: &State, set: &m::Settings) -> String {
    let on = |b: bool| if b { "on" } else { "off" };
    match t {
        Tool::Dimension => m::pretty_id(&shown_dim(game, s)),
        Tool::Caves => format!("Caves: {}", m::CAVE_MODES[set.cave_mode.min(1)]),
        Tool::LayerDown => "Layer -".into(),
        Tool::LayerUp => "Layer +".into(),
        Tool::Waypoints => "Waypoints".into(),
        Tool::Maps => format!("Map: {}", s.subworlds.as_ref().map_or("-", |(_, w)| w.current_name())),
        Tool::Centre => "On me".into(),
        Tool::Follow => format!("Follow {}", on(s.screen.follow)),
        Tool::Grid => format!("Grid: {}", on(set.grid)),
        Tool::ZoomIn => "Zoom +".into(),
        Tool::ZoomOut => "Zoom -".into(),
        Tool::Png => "Export PNG".into(),
        Tool::Settings => "Settings".into(),
        Tool::Controls => "Controls".into(),
    }
}

fn button(game: &Game<'_>, batch: &mut HudBatch, r: [f32; 4], label: &str, mouse: [f32; 2]) {
    ui::button(batch, &game.gui.atlas, r[0], r[1], r[2], r[3], &Text::literal(label), true, inside(r, mouse));
}

fn text_box(game: &Game<'_>, batch: &mut HudBatch, field: [f32; 4], text: &str, focused: bool) {
    game.native_fill(batch, field, if focused { 0xFFFF_FFFF } else { 0xFFA0_A0A0 });
    game.native_fill(batch, [field[0] + 1.0, field[1] + 1.0, field[2] - 2.0, field[3] - 2.0], 0xFF00_0000);
    let caret = if focused && (game.started.elapsed().as_millis() / 500) % 2 == 0 { "_" } else { "" };
    game.native_text(batch, &format!("{text}{caret}"), [field[0] + 4.0, field[1] + 5.0], 0xFFFFFF, false);
}

/// The layer's name for the title (and its Y range, when set to show).
fn layer_name(layer: m::Layer, set: &m::Settings) -> String {
    match layer {
        m::Layer::Surface => "Surface".into(),
        m::Layer::Full => "Full cave map".into(),
        m::Layer::Band(b) if set.layer_y => format!("Caves Y {}..{}", m::band_top(b), m::band_top(b) - set.depth_blocks()),
        m::Layer::Band(_) => "Caves".into(),
    }
}

/// Draws the shown layer's textures over the map's `rect`: far ones from overviews under the
/// near ones; the textures not ready are asked for.
fn draw_terrain(game: &Game<'_>, batch: &mut HudBatch, s: &mut State, set: &m::Settings, rect: [f32; 4], near_ok: bool) {
    let dim = shown_dim(game, s);
    let layer = shown_layer(game, s, set);
    let Some(&source) = s.sources.get(&(dim, layer)) else {
        return;
    };
    let view = s.screen.view;
    let (x0, z0, x1, z1) = view.bounds(rect);
    let lod = m::fine_lod(view.bpp);
    let mut layers = Vec::new();
    if set.overviews {
        layers.push(lod.max(m::BASE_LOD));
    }
    if lod < m::BASE_LOD && near_ok {
        layers.push(lod);
    } else if !set.overviews && near_ok {
        layers.push(lod.min(m::BASE_LOD - 1));
    }
    for l in layers {
        for key in m::TexKey::covering(source, l, x0, z0, x1, z1) {
            s.tex.wanted.push(key);
            let Some(slot) = s.tex.ready(key) else {
                continue;
            };
            let Some(sprite) = horizon_client::hud::map_slot_sprite(slot).and_then(|n| game.gui.atlas.sprites.get(n).copied()) else {
                continue;
            };
            let (ox, oz) = key.origin();
            let span = key.span() as f64;
            let a = view.screen_of(rect, ox as f64, oz as f64);
            let b = view.screen_of(rect, ox as f64 + span, oz as f64 + span);
            let (l0, t0) = (a[0].max(rect[0]), a[1].max(rect[1]));
            let (r0, b0) = (b[0].min(rect[0] + rect[2]), b[1].min(rect[1] + rect[3]));
            if r0 <= l0 || b0 <= t0 {
                continue;
            }
            let (sw, sh) = (b[0] - a[0], b[1] - a[1]);
            let du = (sprite.u1 - sprite.u0) / sprite.w as f32 * m::TEX as f32;
            let dv = (sprite.v1 - sprite.v0) / sprite.h as f32 * m::TEX as f32;
            let eps_u = du / m::TEX as f32 * 0.01;
            let eps_v = dv / m::TEX as f32 * 0.01;
            let u = |x: f32| sprite.u0 + ((x - a[0]) / sw * du).clamp(eps_u, du - eps_u);
            let v = |y: f32| sprite.v0 + ((y - a[1]) / sh * dv).clamp(eps_v, dv - eps_v);
            batch.quad(
                [[l0, t0], [r0, t0], [r0, b0], [l0, b0]],
                [[u(l0), v(t0)], [u(r0), v(t0)], [u(r0), v(b0)], [u(l0), v(b0)]],
                rgba(255, 255, 255, 255),
            );
        }
    }
}

/// Whether waypoint `w` shows on the map now.
fn on_map(s: &State, set: &m::Settings, w: &m::Waypoint) -> bool {
    set.waypoints && w.on_map && m::passes(set.filter, w, s.book.current_set(), s.book.visible(w))
}

/// The waypoint drawn under GUI point `mouse`.
fn waypoint_at(s: &State, set: &m::Settings, dim: &str, rect: [f32; 4], mouse: [f32; 2]) -> Option<usize> {
    let view = s.screen.view;
    s.book.list.iter().position(|wp| {
        let Some(c) = wp.place_in(dim, set.convert) else {
            return false;
        };
        let [x, y] = view.screen_of(rect, c[0], c[2]);
        on_map(s, set, wp) && (mouse[0] - x).abs() <= 6.0 && (mouse[1] - y).abs() <= 6.0
    })
}

/// The block to stand on at GUI point `mouse`.
fn spot_at(game: &Game<'_>, s: &State, set: &m::Settings, rect: [f32; 4], mouse: [f32; 2]) -> [i32; 3] {
    let (x, z) = s.screen.view.world_at(rect, mouse);
    let (x, z) = (x.floor() as i32, z.floor() as i32);
    let fallback = player_pos(game).map_or(64, |p| p[1].floor() as i32);
    let key = (shown_dim(game, s), shown_layer(game, s, set));
    let st = s.stores.get(&key);
    let floor = |x: i32, z: i32| st.and_then(|st| st.store.column(x, z)).filter(|c| c.kind == m::FLOOR).map(|c| i32::from(c.height));
    [x, m::spot_y(floor, x, z, fallback), z]
}

/// What the cursor is over: its Y, block and biome (saved, else the live world's).
fn cursor_info(game: &Game<'_>, s: &State, set: &m::Settings, x: i32, z: i32) -> (Option<i32>, Option<String>, Option<String>) {
    let key = (shown_dim(game, s), shown_layer(game, s, set));
    let st = s.stores.get(&key);
    let col = st.and_then(|st| st.store.column(x, z)).filter(|c| c.kind == m::FLOOR);
    let y = col.map(|c| i32::from(c.height));
    let extra = st.and_then(|st| st.extras.get(&(x >> 4, z >> 4)));
    let i = ((z & 15) * 16 + (x & 15)) as usize;
    let mut block = extra.and_then(|e| s.names.get(e.block[i])).map(m::pretty_id);
    let mut biome = extra.and_then(|e| s.names.get(e.biome[i])).map(m::pretty_id);
    if key.0 == game.dim.id()
        && let Some(y) = y
    {
        if block.is_none()
            && let Some(chunk) = game.world.chunks.get(&(x >> 4, z >> 4))
        {
            let id = chunk.get(x & 15, y, z & 15);
            block = game.res.facts().states.get(id as usize).map(|f| m::pretty_id(&f.name));
        }
        if biome.is_none() {
            biome = game.native_biome([x, y, z]).map(|b| m::pretty_id(&b));
        }
    }
    (y, block, biome)
}

fn draw_dot(game: &Game<'_>, batch: &mut HudBatch, at: [f32; 2], r: f32, argb: u32) {
    let oct = |r: f32| -> [[f32; 2]; 8] {
        std::array::from_fn(|i| {
            let a = (i as f32 + 0.5) * std::f32::consts::FRAC_PI_4;
            [at[0] + r * a.cos(), at[1] + r * a.sin()]
        })
    };
    for (r, c) in [(r + 0.8, 0xFF00_0000), (r, argb)] {
        let o = oct(r);
        game.native_quad(batch, [o[0], o[1], o[2], o[3]], c);
        game.native_quad(batch, [o[0], o[3], o[4], o[7]], c);
        game.native_quad(batch, [o[4], o[5], o[6], o[7]], c);
    }
}

/// A waypoint's mark: its colour with its letter or symbol (a death point dark with an X).
fn draw_mark(game: &Game<'_>, batch: &mut HudBatch, at: [f32; 2], wp: &m::Waypoint, alpha: u32) {
    let text = if wp.death { "X".to_owned() } else { wp.mark() };
    let iw = game.native_text_width(&text).max(6.0);
    let [x, y] = at;
    let fill = if wp.death { 0x0040_1010 } else { wp.rgb() };
    game.native_fill(batch, [x - iw / 2.0 - 2.0, y - 6.0, iw + 4.0, 11.0], alpha << 24);
    game.native_fill(batch, [x - iw / 2.0 - 1.0, y - 5.0, iw + 2.0, 9.0], alpha << 24 | fill);
    let ink = if wp.death { 0xFF5555 } else { 0x000000 };
    game.native_text(batch, &text, [x - game.native_text_width(&text) / 2.0, y - 4.0], ink, false);
}

/// `native_screen_render_post`: the map and whatever is over it.
pub(super) fn on_render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if !ours(game, screen.id) {
        return;
    }
    let (w, h) = (batch.width, batch.height);
    let mouse = screen.mouse;
    let set = settings(game);
    game.native_fill(batch, [0.0, 0.0, w, h], 0xFF10_1014);
    with(|s| {
        if !s.opened {
            return;
        }
        draw_map(game, batch, s, &set, mouse);
        match s.screen.page.clone() {
            m::Page::Map => {}
            m::Page::List { scroll } => draw_list(game, batch, s, &set, scroll, mouse),
            m::Page::Edit { index, form, draft } => {
                let wp = draft.as_deref().cloned().or_else(|| s.book.list.get(index).cloned());
                if let Some(wp) = wp {
                    draw_edit(game, batch, s, &wp, draft.is_some(), &form, mouse);
                }
            }
            m::Page::Confirm { index } => {
                let name = s.book.list.get(index).map_or_else(String::new, |w| w.name.clone());
                draw_dialog(game, batch, &format!("Delete {name}?"), "Delete", mouse);
            }
            m::Page::Maps { choice } => draw_maps(game, batch, s, choice, mouse),
            m::Page::Settings { tab } => draw_settings(game, batch, &set, tab, mouse),
            m::Page::Controls => draw_controls(game, batch, mouse),
        }
    });
}

fn draw_map(game: &Game<'_>, batch: &mut HudBatch, s: &mut State, set: &m::Settings, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let rect = rect_now(game, s);
    let view = s.screen.view;
    let [ur, ug, ub, _] = m::UNKNOWN;
    game.native_fill(batch, rect, 0xFF00_0000 | u32::from(ur) << 16 | u32::from(ug) << 8 | u32::from(ub));
    let dim = shown_dim(game, s);
    let layer = shown_layer(game, s, set);
    let (x0, z0, x1, z1) = view.bounds(rect);
    let near_ok = {
        let regions = m::regions_in(x0, z0, x1, z1).len();
        regions + 9 <= set.cache_regions()
    };
    draw_terrain(game, batch, s, set, rect, near_ok);
    let here = game.dim.id();
    let played = dim == here;
    // Ground not loaded now, dimmed.
    if set.dim_unloaded && played && layer == m::Layer::Surface
        && let Some(p) = player_pos(game)
    {
        let rd = f64::from(game.settings.render_distance.max(2));
        let (pcx, pcz) = ((p[0] / 16.0).floor(), (p[2] / 16.0).floor());
        let a = view.screen_of(rect, (pcx - rd) * 16.0, (pcz - rd) * 16.0);
        let b = view.screen_of(rect, (pcx + rd + 1.0) * 16.0, (pcz + rd + 1.0) * 16.0);
        let (l, t) = (a[0].clamp(rect[0], rect[0] + rect[2]), a[1].clamp(rect[1], rect[1] + rect[3]));
        let (r, bt) = (b[0].clamp(rect[0], rect[0] + rect[2]), b[1].clamp(rect[1], rect[1] + rect[3]));
        let shade = 0x5000_0000;
        game.native_fill(batch, [rect[0], rect[1], rect[2], t - rect[1]], shade);
        game.native_fill(batch, [rect[0], bt, rect[2], rect[1] + rect[3] - bt], shade);
        game.native_fill(batch, [rect[0], t, l - rect[0], bt - t], shade);
        game.native_fill(batch, [r, t, rect[0] + rect[2] - r, bt - t], shade);
    }
    // Chunk grid and region lines.
    let lines = |batch: &mut HudBatch, step: f64, argb: u32| {
        let mut x = (x0 / step).floor() * step;
        while x <= x1 {
            let [sx, _] = view.screen_of(rect, x, z0);
            game.native_fill(batch, [sx.round(), rect[1], 1.0, rect[3]], argb);
            x += step;
        }
        let mut z = (z0 / step).floor() * step;
        while z <= z1 {
            let [_, sy] = view.screen_of(rect, x0, z);
            game.native_fill(batch, [rect[0], sy.round(), rect[2], 1.0], argb);
            z += step;
        }
    };
    if set.grid && view.bpp <= 4.0 {
        lines(batch, 16.0, 0x40FF_FFFF);
    }
    if set.regions && view.bpp <= 64.0 {
        lines(batch, 512.0, 0x80FF_D060);
    }
    // Your recent path.
    if set.footsteps && played && s.steps_for == here {
        let every = (view.bpp / 2.0).ceil().max(1.0) as usize;
        for p in s.steps.points.iter().step_by(every) {
            let [x, y] = view.screen_of(rect, f64::from(p[0]) + 0.5, f64::from(p[1]) + 0.5);
            if inside(rect, [x, y]) {
                game.native_fill(batch, [x - 1.0, y - 1.0, 2.0, 2.0], 0xB0E8_D890);
            }
        }
    }
    // Waypoints.
    let mut hovered_name: Option<(String, [f32; 2], u32)> = None;
    for wp in s.book.list.iter().filter(|w| on_map(s, set, w)) {
        let Some(c) = wp.place_in(&dim, set.convert) else {
            continue;
        };
        let [x, y] = view.screen_of(rect, c[0], c[2]);
        if !inside(rect, [x, y]) {
            continue;
        }
        let alpha = if wp.shown { 0xFF } else { 0x80 };
        draw_mark(game, batch, [x, y], wp, alpha);
        let hover = (mouse[0] - x).abs() <= 6.0 && (mouse[1] - y).abs() <= 6.0;
        if m::shows_name(set.wp_names, hover, view.bpp) {
            let tw = game.native_text_width(&wp.name);
            if hover {
                hovered_name = Some((wp.name.clone(), [x - tw / 2.0, y - 16.0], wp.rgb()));
            } else {
                game.native_text(batch, &wp.name, [x - tw / 2.0, y - 16.0], wp.rgb(), true);
            }
        }
    }
    // Other players.
    if set.players && played {
        let t = game.clock.fraction();
        for mob in game.mobs.iter() {
            if mob.removed || mob.kind.id != "minecraft:player" {
                continue;
            }
            let at = mob.previous.lerp(mob.position, t);
            let [x, y] = view.screen_of(rect, at.x, at.z);
            if !inside(rect, [x, y]) {
                continue;
            }
            draw_dot(game, batch, [x, y], 2.5, 0xFF55_FFFF);
            let hover = (mouse[0] - x).abs() <= 5.0 && (mouse[1] - y).abs() <= 5.0;
            if (set.player_names || hover)
                && let Some(name) = game.remote_player_name(mob.id)
            {
                let tw = game.native_text_width(name);
                game.native_text(batch, name, [x - tw / 2.0, y - 12.0], 0x55FFFF, true);
            }
        }
    }
    // You (in the dimension you are in; in the other of Overworld/Nether at your place there).
    if set.arrow
        && let Some(p) = player_pos(game)
    {
        let at = if played {
            Some((p[0], p[2]))
        } else if set.convert {
            match (here, dim.as_str()) {
                (m::OVERWORLD, m::NETHER) | (m::NETHER, m::OVERWORLD) => Some(m::convert_view(p[0], p[2], here, &dim, true)),
                _ => None,
            }
        } else {
            None
        };
        if let Some((px, pz)) = at {
            let [x, y] = view.screen_of(rect, px, pz);
            if inside(rect, [x, y]) {
                let colour = m::ARROW_COLOURS[set.arrow_colour.min(3)].1;
                let alpha = if played { 0xFF00_0000 } else { 0x9000_0000 };
                game.native_quad(batch, m::arrow([x, y], game.yaw, 7.0), alpha);
                game.native_quad(batch, m::arrow([x, y], game.yaw, 5.0), alpha | colour);
            }
        }
    }
    if let Some((name, at, rgb)) = hovered_name {
        game.native_text(batch, &name, at, rgb, true);
    }
    if s.screen.hide_ui {
        return;
    }
    // The bars and the tools.
    game.native_fill(batch, [0.0, 0.0, w, TOP], 0xE000_0000);
    game.native_fill(batch, [0.0, h - BOTTOM, w, BOTTOM], 0xE000_0000);
    game.native_fill(batch, [w - TOOL_W, TOP, TOOL_W, h - TOP - BOTTOM], 0xE018_181C);
    let world = s.subworlds.as_ref().map_or_else(String::new, |(_, subs)| format!(" ({})", subs.current_name()));
    let title = format!("World Map{world} - {} - {}   {}", m::pretty_id(&dim), layer_name(layer, set), m::zoom_text(view.bpp));
    game.native_text(batch, &title, [6.0, 4.0], 0xFFFFFF, true);
    let mp = s.subworlds.is_some();
    for (tool, r) in tools(w, mp) {
        button(game, batch, r, &tool_label(game, tool, s, set), mouse);
    }
    let lines: Vec<(String, u32)> = match &s.notice {
        Some((text, until)) if Instant::now() < *until => vec![(text.clone(), 0xFFFF80)],
        _ if inside(rect, mouse) && set.cursor => {
            let (x, z) = view.world_at(rect, mouse);
            let (bx, bz) = (x.floor() as i32, z.floor() as i32);
            let (y, block, biome) = cursor_info(game, s, set, bx, bz);
            let mut first = m::cursor_line(bx, bz, y);
            if let Some(o) = m::other_dim_line(&dim, bx, bz) {
                first.push_str(&format!("   {o}"));
            }
            let mut out = vec![(first, 0xE0E0E0)];
            if set.cursor_biome {
                let mut second = String::new();
                if let Some(b) = biome {
                    second.push_str(&format!("Biome: {b}"));
                }
                if let Some(b) = block {
                    second.push_str(&format!("   Block: {b}"));
                }
                if !second.is_empty() {
                    out.push((second, 0xA0C0A0));
                }
            }
            out
        }
        _ if s.asking.is_some() => vec![("This world's map isn't chosen yet: Map: ... to choose".into(), 0xFFFF80)],
        _ => vec![("Drag to move, wheel to zoom, right-click for the menu; Controls lists the keys".into(), 0xA0A0A0)],
    };
    for (i, (text, rgb)) in lines.iter().enumerate() {
        game.native_text(batch, text, [6.0, h - BOTTOM + 3.0 + i as f32 * 10.0], *rgb, false);
    }
    // The right-click menu, over everything.
    if let Some(menu) = s.screen.menu.clone() {
        let items = menu_items(game, s, set, &menu);
        let rects = m::menu_rects(menu.at, items.len(), w, h);
        if let (Some(first), Some(last)) = (rects.first(), rects.last()) {
            game.native_fill(batch, [first[0] - 1.0, first[1] - 1.0, first[2] + 2.0, last[1] + last[3] - first[1] + 2.0], 0xFF60_6068);
        }
        let shown = menu.waypoint.and_then(|i| s.book.list.get(i)).is_none_or(|w| w.shown);
        for (item, r) in items.iter().zip(&rects) {
            game.native_fill(batch, *r, if inside(*r, mouse) { 0xF040_4870 } else { 0xF018_181C });
            game.native_text(batch, item.label(shown), [r[0] + 4.0, r[1] + 3.0], 0xFFFFFF, false);
        }
        let b = menu.block;
        let head = match menu.waypoint.and_then(|i| s.book.list.get(i)) {
            Some(wp) => wp.name.clone(),
            None => format!("{} {} {}", b[0], b[1], b[2]),
        };
        if let Some(r) = rects.first() {
            game.native_text(batch, &head, [r[0], r[1] - 10.0], 0xC0C0C0, true);
        }
    }
}

fn menu_items(game: &Game<'_>, s: &State, set: &m::Settings, menu: &m::Menu) -> Vec<m::MenuItem> {
    let dim = shown_dim(game, s);
    let same = match menu.waypoint.and_then(|i| s.book.list.get(i)) {
        Some(w) => w.dim == game.dim.id() || (set.convert && w.place_in(game.dim.id(), true).is_some() && dim == game.dim.id()),
        None => dim == game.dim.id(),
    };
    m::menu_items(menu.waypoint.is_some(), set.teleport && may_teleport(game), same, set.share)
}

/// A new waypoint at `block` of the dimension shown, in the editor before it is made.
fn new_waypoint(game: &Game<'_>, s: &mut State, set: &m::Settings, block: [i32; 3]) {
    let dim = shown_dim(game, s);
    let mut wp = s.book.draft(block, &dim, now_secs());
    m::style_new(&mut wp, set.wp_colour, 0);
    let form = m::Form::of_new(&wp);
    s.screen.page = m::Page::Edit { index: 0, form, draft: Some(Box::new(wp)) };
}

fn menu_pick(game: &Game<'_>, s: &mut State, set: &m::Settings, menu: &m::Menu, item: m::MenuItem) {
    match (item, menu.waypoint) {
        (m::MenuItem::WaypointHere, _) => new_waypoint(game, s, set, menu.block),
        (m::MenuItem::TeleportHere, _) => s.pending_command = Some(m::tp_command(menu.block)),
        (m::MenuItem::CentreHere, _) => {
            s.screen.follow = false;
            s.screen.view.centre_on(f64::from(menu.block[0]) + 0.5, f64::from(menu.block[2]) + 0.5);
        }
        (m::MenuItem::Edit, Some(i)) => edit(s, i),
        (m::MenuItem::Toggle, Some(i)) if i < s.book.list.len() => {
            s.book.list[i].shown = !s.book.list[i].shown;
            save_book(s);
        }
        (m::MenuItem::Delete, Some(i)) if i < s.book.list.len() => s.screen.page = m::Page::Confirm { index: i },
        (m::MenuItem::TeleportTo, Some(i)) if i < s.book.list.len() => {
            let w = &s.book.list[i];
            let pos = if w.dim == game.dim.id() { Some(w.pos) } else { m::converted(w.pos, &w.dim, game.dim.id()) };
            if let Some(p) = pos {
                s.pending_command = Some(m::tp_command(p));
            }
        }
        (m::MenuItem::Share, Some(i)) if i < s.book.list.len() => s.pending_chat = Some(m::share_text(&s.book.list[i])),
        _ => {}
    }
}

fn edit(s: &mut State, i: usize) {
    if let Some(w) = s.book.list.get(i) {
        s.screen.page = m::Page::Edit { index: i, form: m::Form::of(w), draft: None };
    }
}

// The list beside the map.

fn list_panel(w: f32, h: f32) -> [f32; 4] {
    [w - TOOL_W - LIST_W, TOP, LIST_W, h - TOP - BOTTOM]
}

/// The list's header buttons: filter, set, all sets shown; and its rows.
fn list_head(p: [f32; 4]) -> [[f32; 4]; 3] {
    let bw = (p[2] - 16.0) / 3.0;
    [
        [p[0] + 4.0, p[1] + 4.0, bw, 16.0],
        [p[0] + 8.0 + bw, p[1] + 4.0, bw, 16.0],
        [p[0] + 12.0 + bw * 2.0, p[1] + 4.0, bw, 16.0],
    ]
}

fn list_row(p: [f32; 4], i: usize) -> [f32; 4] {
    [p[0] + 4.0, p[1] + 26.0 + i as f32 * 16.0, p[2] - 8.0, 15.0]
}

/// The waypoints the list shows, nearest first.
fn listed(game: &Game<'_>, s: &State, set: &m::Settings) -> Vec<usize> {
    let from = player_pos(game);
    s.book
        .order(1, from)
        .into_iter()
        .filter(|&i| {
            let w = &s.book.list[i];
            m::passes(set.filter, w, s.book.current_set(), s.book.visible(w))
        })
        .collect()
}

fn draw_list(game: &Game<'_>, batch: &mut HudBatch, s: &State, set: &m::Settings, scroll: usize, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let p = list_panel(w, h);
    game.native_fill(batch, p, 0xF010_1014);
    let [filter, set_b, all] = list_head(p);
    button(game, batch, filter, m::FILTERS[set.filter.min(3)], mouse);
    button(game, batch, set_b, s.book.current_set(), mouse);
    button(game, batch, all, if s.book.all_sets { "All sets" } else { "Set only" }, mouse);
    let shown = listed(game, s, set);
    let rows = m::list_rows(h);
    let here = player_pos(game);
    for (k, &i) in shown.iter().skip(scroll).take(rows).enumerate() {
        let wp = &s.book.list[i];
        let r = list_row(p, k);
        if inside(r, mouse) {
            game.native_fill(batch, r, 0x60FF_FFFF);
        }
        draw_mark(game, batch, [r[0] + 7.0, r[1] + 7.0], wp, if wp.shown { 0xFF } else { 0x80 });
        let ink = if wp.shown { 0xFFFFFF } else { 0x808080 };
        game.native_text(batch, &wp.name, [r[0] + 18.0, r[1] + 4.0], ink, false);
        let d = match here {
            Some(p) if wp.dim == game.dim.id() => m::format_distance(m::distance(p, wp.centre())),
            _ => m::pretty_id(&wp.dim),
        };
        let dw = game.native_text_width(&d);
        game.native_text(batch, &d, [r[0] + r[2] - dw - 2.0, r[1] + 4.0], 0xA0A0A0, false);
    }
    if shown.is_empty() {
        game.native_text(batch, "No waypoints here", [p[0] + 8.0, p[1] + 30.0], 0xA0A0A0, false);
    }
}

// The editor.

struct EditLayout {
    fields: [[f32; 4]; 4],
    colour: [f32; 4],
    set: [f32; 4],
    shown: [f32; 4],
    save: [f32; 4],
    delete: [f32; 4],
    cancel: [f32; 4],
}

fn edit_layout(w: f32, h: f32) -> EditLayout {
    let (pw, ph) = (240.0, 190.0);
    let (x, y) = ((w - pw) / 2.0, (h - ph) / 2.0);
    let f = |i: f32| [x + 70.0, y + 24.0 + i * 22.0, pw - 80.0, 18.0];
    let b = |i: f32| [x + 10.0 + i * 76.0, y + ph - 26.0, 70.0, 18.0];
    EditLayout {
        fields: [f(0.0), f(1.0), f(2.0), f(3.0)],
        colour: [x + 10.0, y + 116.0, 70.0, 18.0],
        set: [x + 85.0, y + 116.0, 70.0, 18.0],
        shown: [x + 160.0, y + 116.0, 70.0, 18.0],
        save: b(0.0),
        delete: b(1.0),
        cancel: b(2.0),
    }
}

/// The editor's box `i` (name, x, y, z) as `Form` numbers them.
const EDIT_FIELDS: [usize; 4] = [m::F_NAME, m::F_X, m::F_X + 1, m::F_X + 2];

fn draw_edit(game: &Game<'_>, batch: &mut HudBatch, s: &State, wp: &m::Waypoint, new: bool, form: &m::Form, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let l = edit_layout(w, h);
    let panel = [l.fields[0][0] - 70.0, l.fields[0][1] - 24.0, 240.0, 190.0];
    game.native_fill(batch, panel, 0xF018_181C);
    game.native_text(batch, if new { "New waypoint" } else { "Edit waypoint" }, [panel[0] + 8.0, panel[1] + 6.0], 0xFFFFFF, true);
    for (k, (r, label)) in l.fields.iter().zip(["Name", "X", "Y", "Z"]).enumerate() {
        game.native_text(batch, label, [panel[0] + 10.0, r[1] + 5.0], 0xC0C0C0, false);
        let f = EDIT_FIELDS[k];
        let text = if f == m::F_NAME && form.fields[f].is_empty() && form.focus != Some(f) { wp.name.clone() } else { form.fields[f].clone() };
        text_box(game, batch, *r, &text, form.focus == Some(f));
    }
    game.native_fill(batch, [l.colour[0] - 1.0, l.colour[1] - 5.0, l.colour[2] + 2.0, 3.0], 0xFF00_0000 | wp.rgb());
    button(game, batch, l.colour, m::COLOURS[wp.colour.min(m::COLOURS.len() - 1)].0, mouse);
    button(game, batch, l.set, &wp.set, mouse);
    button(game, batch, l.shown, if wp.shown { "Enabled" } else { "Disabled" }, mouse);
    button(game, batch, l.save, if new { "Create" } else { "Save" }, mouse);
    if !new {
        button(game, batch, l.delete, "Delete", mouse);
    }
    button(game, batch, l.cancel, "Cancel", mouse);
    let _ = s;
}

/// The editor's waypoint with its boxes applied.
fn edited(s: &State) -> Option<m::Waypoint> {
    let m::Page::Edit { index, form, draft } = &s.screen.page else {
        return None;
    };
    let mut wp = draft.as_deref().cloned().or_else(|| s.book.list.get(*index).cloned())?;
    form.apply(&mut wp);
    Some(wp)
}

/// The editor closes: kept (`save`) or not.
fn finish_edit(s: &mut State, save: bool) {
    if save
        && let Some(wp) = edited(s)
        && let m::Page::Edit { index, draft, .. } = &s.screen.page
    {
        if draft.is_some() {
            s.book.insert(wp);
        } else if let Some(w) = s.book.list.get_mut(*index) {
            *w = wp;
        }
        save_book(s);
    }
    s.screen.page = m::Page::Map;
}

/// The editor's waypoint changed in place (its colour, set, shown), the boxes kept.
fn edit_waypoint(s: &mut State, f: impl FnOnce(&mut m::Waypoint, &m::Book)) {
    let book = s.book.clone();
    if let m::Page::Edit { index, draft, .. } = &mut s.screen.page {
        match draft {
            Some(d) => f(d, &book),
            None => {
                if let Some(w) = s.book.list.get_mut(*index) {
                    f(w, &book);
                }
            }
        }
    }
}

fn dialog_layout(w: f32, h: f32) -> ([f32; 4], [[f32; 4]; 2]) {
    let p = [(w - 220.0) / 2.0, (h - 60.0) / 2.0, 220.0, 60.0];
    (p, [[p[0] + 10.0, p[1] + 34.0, 95.0, 18.0], [p[0] + 115.0, p[1] + 34.0, 95.0, 18.0]])
}

fn draw_dialog(game: &Game<'_>, batch: &mut HudBatch, question: &str, yes: &str, mouse: [f32; 2]) {
    let (p, [a, b]) = dialog_layout(batch.width, batch.height);
    game.native_fill(batch, p, 0xF018_181C);
    game.native_text(batch, question, [p[0] + 10.0, p[1] + 10.0], 0xFFFFFF, true);
    button(game, batch, a, yes, mouse);
    button(game, batch, b, "Cancel", mouse);
}

// The map selection.

fn maps_layout(w: f32, h: f32, n: usize) -> ([f32; 4], Vec<[f32; 4]>, [[f32; 4]; 3]) {
    let rows = n.min(10);
    let ph = 60.0 + 18.0 * rows as f32 + 30.0;
    let p = [(w - 260.0) / 2.0, (h - ph) / 2.0, 260.0, ph];
    let list = (0..rows).map(|i| [p[0] + 10.0, p[1] + 40.0 + i as f32 * 18.0, 240.0, 16.0]).collect();
    let by = p[1] + ph - 26.0;
    (p, list, [[p[0] + 10.0, by, 76.0, 18.0], [p[0] + 92.0, by, 76.0, 18.0], [p[0] + 174.0, by, 76.0, 18.0]])
}

fn draw_maps(game: &Game<'_>, batch: &mut HudBatch, s: &State, choice: m::Choice, mouse: [f32; 2]) {
    let Some((_, subs)) = &s.subworlds else {
        return;
    };
    let (p, rows, [use_b, new_b, cancel]) = maps_layout(batch.width, batch.height, subs.list.len());
    game.native_fill(batch, p, 0xF018_181C);
    let title = if s.asking.is_some() { "Which map is this world?" } else { "Maps of this server" };
    game.native_text(batch, title, [p[0] + 10.0, p[1] + 8.0], 0xFFFFFF, true);
    game.native_text(batch, "Worlds on one address are told apart by spawn", [p[0] + 10.0, p[1] + 22.0], 0x909090, false);
    for (i, r) in rows.iter().enumerate() {
        let (name, spawn) = &subs.list[i];
        let chosen = choice == m::Choice::Map(i);
        game.native_fill(batch, *r, if chosen { 0xC040_4870 } else if inside(*r, mouse) { 0x60FF_FFFF } else { 0x40FF_FFFF });
        let at = spawn.map_or_else(|| "no spawn yet".to_owned(), |sp| format!("spawn {} {}", sp[0], sp[1]));
        let cur = if i == subs.current { "  (in use)" } else { "" };
        game.native_text(batch, &format!("{name}{cur}"), [r[0] + 4.0, r[1] + 4.0], 0xFFFFFF, false);
        let aw = game.native_text_width(&at);
        game.native_text(batch, &at, [r[0] + r[2] - aw - 4.0, r[1] + 4.0], 0xA0A0A0, false);
    }
    button(game, batch, use_b, if choice == m::Choice::New { "Use new" } else { "Use this" }, mouse);
    button(game, batch, new_b, "New map", mouse);
    button(game, batch, cancel, if s.asking.is_some() { "Later" } else { "Back" }, mouse);
}

// Settings and controls.

fn settings_layout(w: f32, h: f32, n: usize) -> (Vec<[f32; 4]>, Vec<[f32; 4]>, [f32; 4]) {
    let tabs_w = 62.0;
    let tabs = (0..m::PAGES.len()).map(|i| [6.0 + i as f32 * (tabs_w + 2.0), 22.0, tabs_w, 16.0]).collect();
    let col_w = ((w - 30.0) / 2.0).min(220.0);
    let left = (w - col_w * 2.0 - 6.0) / 2.0;
    let per_col = ((h - 90.0) / 20.0).floor().max(1.0) as usize;
    let rows = (0..n)
        .map(|i| {
            let (c, r) = (i / per_col, i % per_col);
            [left + c as f32 * (col_w + 6.0), 46.0 + r as f32 * 20.0, col_w, 18.0]
        })
        .collect();
    (tabs, rows, [(w - 100.0) / 2.0, h - 26.0, 100.0, 18.0])
}

fn draw_settings(game: &Game<'_>, batch: &mut HudBatch, set: &m::Settings, tab: usize, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    game.native_fill(batch, [0.0, 0.0, w, h], 0xF010_1014);
    game.native_text(batch, "World Map settings (right-click: back a step)", [6.0, 6.0], 0xFFFFFF, true);
    let rows: Vec<String> = if tab == m::KEYS_PAGE {
        keys::labels()
            .into_iter()
            .map(|(name, label)| format!("{label}: {}", horizon_client::keybinds::display_name(&keys::bound(game, name)).plain()))
            .collect()
    } else {
        set.page(tab).into_iter().map(|(_, l)| l).collect()
    };
    let (tabs, rects, back) = settings_layout(w, h, rows.len());
    for (i, r) in tabs.iter().enumerate() {
        if i == tab {
            game.native_fill(batch, [r[0], r[1] + r[3], r[2], 2.0], 0xFFFF_FFFF);
        }
        button(game, batch, *r, m::PAGES[i], mouse);
    }
    for (r, label) in rects.iter().zip(&rows) {
        button(game, batch, *r, label, mouse);
    }
    if tab == m::KEYS_PAGE {
        game.native_text(batch, "Rebind these in Options > Mods > World Map (or Controls)", [6.0, h - 40.0], 0xA0A0A0, false);
    }
    button(game, batch, back, "Back", mouse);
}

fn draw_controls(game: &Game<'_>, batch: &mut HudBatch, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    game.native_fill(batch, [0.0, 0.0, w, h], 0xF010_1014);
    game.native_text(batch, "World Map controls", [10.0, 8.0], 0xFFFFFF, true);
    let mut lines: Vec<String> = vec![
        "Drag: move the map   Wheel: zoom about the pointer".into(),
        "Arrows or WASD: move   Right-click: the menu (waypoints, teleport, centre)".into(),
        "Click a waypoint: edit it   Escape: back, or close".into(),
    ];
    for (name, label) in keys::labels() {
        let b = horizon_client::keybinds::display_name(&keys::bound(game, name)).plain();
        lines.push(format!("{label}: {b}"));
    }
    for (i, l) in lines.iter().enumerate() {
        game.native_text(batch, l, [10.0, 26.0 + i as f32 * 11.0], 0xD0D0D0, false);
    }
    button(game, batch, [(w - 100.0) / 2.0, h - 26.0, 100.0, 18.0], "Back", mouse);
}

// ---------------------------------------------------------------- input

/// A layer step on the map (down or up), from your feet.
fn step_layer(game: &Game<'_>, s: &mut State, set: &m::Settings, down: bool) {
    let now = shown_layer(game, s, set);
    let feet = player_pos(game).map_or(64, |p| p[1].floor() as i32);
    let shape = game.world.shape;
    let next = m::layer_step(now, feet, down, shape.min_y, shape.max_y(), roof_top(game).is_none());
    s.screen.layer = Some(next);
}

/// The next dimension's map, the view moved to the same place there.
fn next_dim(game: &Game<'_>, s: &mut State, set: &m::Settings) {
    let now = shown_dim(game, s);
    let saved: Vec<String> = s
        .world_dir
        .as_ref()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().to_str().and_then(m::dimension_of_folder)).collect())
        .unwrap_or_default();
    let next = m::next_dimension(&now, &saved);
    let (x, z) = m::convert_view(s.screen.view.x, s.screen.view.z, &now, &next, set.convert);
    s.screen.view.centre_on(x, z);
    s.screen.follow = false;
    s.screen.layer = None;
    s.screen.dim = (next != game.dim.id()).then_some(next);
}

/// `native_screen_mouse_button_pressed_pre`.
pub(super) fn on_press(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    if !ours(game, screen.id) || *canceled {
        return;
    }
    *canceled = true;
    let (w, h) = game.gui_size();
    let mouse = screen.mouse;
    let before = settings(game);
    let mut set = before;
    with(|s| {
        let rect = rect_now(game, s);
        if let Some(menu) = s.screen.menu.take() {
            if button == 0 {
                let items = menu_items(game, s, &set, &menu);
                let rects = m::menu_rects(menu.at, items.len(), w, h);
                if let Some(k) = rects.iter().position(|r| inside(*r, mouse)) {
                    menu_pick(game, s, &set, &menu, items[k]);
                }
            }
            return;
        }
        match s.screen.page.clone() {
            m::Page::Settings { tab } => {
                let n = if tab == m::KEYS_PAGE { keys::labels().len() } else { set.page(tab).len() };
                let (tabs, rows, back) = settings_layout(w, h, n);
                if let Some(t) = tabs.iter().position(|r| inside(*r, mouse)) {
                    s.screen.page = m::Page::Settings { tab: t };
                } else if inside(back, mouse) {
                    s.screen.page = m::Page::Map;
                } else if tab != m::KEYS_PAGE
                    && let Some(k) = rows.iter().position(|r| inside(*r, mouse))
                    && let Some(&(field, _)) = set.page(tab).get(k)
                {
                    set.click_dir(field, button == 1);
                }
            }
            m::Page::Controls => s.screen.page = m::Page::Map,
            m::Page::Confirm { index } => {
                let (_, [yes, no]) = dialog_layout(w, h);
                if inside(yes, mouse) {
                    s.book.remove(&[index]);
                    save_book(s);
                    s.screen.page = m::Page::Map;
                } else if inside(no, mouse) {
                    s.screen.page = m::Page::Map;
                }
            }
            m::Page::Maps { choice } => {
                let n = s.subworlds.as_ref().map_or(0, |(_, subs)| subs.list.len());
                let (_, rows, [use_b, new_b, cancel]) = maps_layout(w, h, n);
                if let Some(i) = rows.iter().position(|r| inside(*r, mouse)) {
                    s.screen.page = m::Page::Maps { choice: m::Choice::Map(i) };
                } else if inside(use_b, mouse) {
                    answer(s, choice);
                    s.screen.page = m::Page::Map;
                } else if inside(new_b, mouse) {
                    answer(s, m::Choice::New);
                    s.screen.page = m::Page::Map;
                } else if inside(cancel, mouse) {
                    s.screen.page = m::Page::Map;
                }
            }
            m::Page::Edit { draft, .. } => {
                let l = edit_layout(w, h);
                if let Some(k) = l.fields.iter().position(|r| inside(*r, mouse)) {
                    if let m::Page::Edit { form, .. } = &mut s.screen.page {
                        form.focus = Some(EDIT_FIELDS[k]);
                    }
                } else if inside(l.colour, mouse) {
                    edit_waypoint(s, |wp, _| {
                        wp.custom = None;
                        wp.colour = (wp.colour + if button == 1 { m::COLOURS.len() - 1 } else { 1 }) % m::COLOURS.len();
                    });
                } else if inside(l.set, mouse) {
                    edit_waypoint(s, |wp, book| {
                        let i = book.set_index(&wp.set).map_or(0, |i| (i + 1) % book.sets.len());
                        wp.set = book.sets[i].name.clone();
                    });
                } else if inside(l.shown, mouse) {
                    edit_waypoint(s, |wp, _| wp.shown = !wp.shown);
                } else if inside(l.save, mouse) {
                    finish_edit(s, true);
                } else if inside(l.delete, mouse) && draft.is_none() {
                    if let m::Page::Edit { index, .. } = s.screen.page {
                        s.screen.page = m::Page::Confirm { index };
                    }
                } else if inside(l.cancel, mouse) {
                    finish_edit(s, false);
                } else if let m::Page::Edit { form, .. } = &mut s.screen.page {
                    form.focus = None;
                }
            }
            page => {
                // The map, with the list beside it.
                if !s.screen.hide_ui
                    && let Some((tool, _)) = tools(w, s.subworlds.is_some()).into_iter().find(|(_, r)| inside(*r, mouse))
                {
                    let back = button == 1;
                    match tool {
                        Tool::Dimension => next_dim(game, s, &set),
                        Tool::Caves => {
                            set.cave_mode = (set.cave_mode + 1) % m::CAVE_MODES.len();
                            s.screen.layer = None;
                        }
                        Tool::LayerDown => step_layer(game, s, &set, true),
                        Tool::LayerUp => step_layer(game, s, &set, false),
                        Tool::Waypoints => {
                            s.screen.page = if matches!(page, m::Page::List { .. }) { m::Page::Map } else { m::Page::List { scroll: 0 } };
                        }
                        Tool::Maps => {
                            let choice = s.asking.map_or_else(|| m::Choice::Map(s.subworlds.as_ref().map_or(0, |(_, w)| w.current)), |(c, _)| c);
                            s.screen.page = m::Page::Maps { choice };
                        }
                        Tool::Centre => {
                            if let Some(p) = player_pos(game) {
                                s.screen.dim = None;
                                s.screen.view.centre_on(p[0], p[2]);
                            }
                        }
                        Tool::Follow => s.screen.follow = !s.screen.follow,
                        Tool::Grid => set.grid = !set.grid,
                        Tool::ZoomIn | Tool::ZoomOut => {
                            let (lo, hi) = set.zooms();
                            let c = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
                            let n = if (tool == Tool::ZoomIn) != back { 1.0 } else { -1.0 };
                            s.screen.view.zoom_at(rect, c, n * 2.0, lo, hi, set.smooth);
                        }
                        Tool::Png => s.pending_export = true,
                        Tool::Settings => s.screen.page = m::Page::Settings { tab: 0 },
                        Tool::Controls => s.screen.page = m::Page::Controls,
                    }
                    return;
                }
                if let m::Page::List { scroll } = page {
                    let p = list_panel(w, h);
                    if inside(p, mouse) {
                        let [filter, set_b, all] = list_head(p);
                        let shown = listed(game, s, &set);
                        let row = (0..m::list_rows(h)).find(|&k| inside(list_row(p, k), mouse)).and_then(|k| shown.get(scroll + k).copied());
                        if inside(filter, mouse) {
                            set.filter = (set.filter + if button == 1 { m::FILTERS.len() - 1 } else { 1 }) % m::FILTERS.len();
                        } else if inside(set_b, mouse) {
                            s.book.next_set();
                            save_book(s);
                        } else if inside(all, mouse) {
                            s.book.all_sets = !s.book.all_sets;
                        } else if let Some(i) = row {
                            let wp = &s.book.list[i];
                            if button == 1 {
                                s.screen.menu = Some(m::Menu { at: mouse, block: wp.pos, waypoint: Some(i) });
                            } else if let Some(c) = wp.place_in(&shown_dim(game, s), set.convert) {
                                s.screen.follow = false;
                                s.screen.view.centre_on(c[0], c[2]);
                            } else {
                                edit(s, i);
                            }
                        }
                        return;
                    }
                }
                if !inside(rect, mouse) {
                    return;
                }
                if button == 1 {
                    let dim = shown_dim(game, s);
                    let waypoint = waypoint_at(s, &set, &dim, rect, mouse);
                    let block = spot_at(game, s, &set, rect, mouse);
                    s.screen.menu = Some(m::Menu { at: mouse, block, waypoint });
                } else if button == 0 {
                    s.screen.press = Some((mouse, false));
                }
            }
        }
    });
    if set != before {
        store_settings(game, &before, set);
    }
}

/// `native_screen_mouse_button_released_pre`: a click on a waypoint opens it.
pub(super) fn on_release(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    if !ours(game, screen.id) || button != 0 {
        return;
    }
    *canceled = true;
    let set = settings(game);
    let mouse = screen.mouse;
    with(|s| {
        if !s.screen.release() || !matches!(s.screen.page, m::Page::Map | m::Page::List { .. }) {
            return;
        }
        let rect = rect_now(game, s);
        if !inside(rect, mouse) {
            return;
        }
        let dim = shown_dim(game, s);
        if let Some(i) = waypoint_at(s, &set, &dim, rect, mouse) {
            edit(s, i);
        }
    });
}

/// `native_screen_mouse_dragged_pre_v2`: a drag moves the map.
pub(super) fn on_drag(game: &mut Game<'_>, screen: &str, mouse: [f32; 2], delta: [f32; 2], canceled: &mut bool) {
    if !ours(game, screen) {
        return;
    }
    *canceled = true;
    with(|s| s.screen.drag(mouse, delta));
}

/// `native_screen_mouse_scrolled_pre_v2`: the wheel zooms about the pointer, or scrolls the list.
pub(super) fn on_scroll(game: &mut Game<'_>, screen: &str, mouse: [f32; 2], scroll: [f64; 2], canceled: &mut bool) {
    if !ours(game, screen) || scroll[1] == 0.0 {
        return;
    }
    *canceled = true;
    let (w, h) = game.gui_size();
    let set = settings(game);
    let (lo, hi) = set.zooms();
    let notches = scroll[1].clamp(-3.0, 3.0);
    let shift = game.native_has_shift_down();
    with(|s| {
        let rect = rect_now(game, s);
        let list = matches!(s.screen.page, m::Page::List { .. }) && inside(list_panel(w, h), mouse);
        if list {
            let max = listed(game, s, &set).len().saturating_sub(m::list_rows(h));
            if let m::Page::List { scroll } = &mut s.screen.page {
                *scroll = (*scroll as i64 - notches.signum() as i64).clamp(0, max as i64) as usize;
            }
        } else if matches!(s.screen.page, m::Page::Map | m::Page::List { .. }) && inside(rect, mouse) {
            // Shift: whole steps (twice a notch's).
            let n = if shift { notches.signum() * 2.0 } else { notches };
            s.screen.view.zoom_at(rect, mouse, n, lo, hi, set.smooth);
        }
    });
}

/// `native_screen_key_pressed_pre`: typing; the map's keys; Escape steps back a page.
pub(super) fn on_key_pressed(game: &Game<'_>, screen: &NativeScreenView<'_>, key: &str, canceled: &mut bool) {
    if !ours(game, screen.id) || *canceled {
        return;
    }
    let before = settings(game);
    let mut set = before;
    let mouse = screen.mouse;
    with(|s| {
        let typing = s.screen.page.typing();
        let escape = key == "key.keyboard.escape";
        let enter = key == "key.keyboard.enter" || key == "key.keyboard.keypad.enter";
        if typing {
            *canceled = true;
            match key {
                "key.keyboard.backspace" => {
                    if let m::Page::Edit { form, .. } = &mut s.screen.page {
                        form.backspace();
                    }
                }
                "key.keyboard.tab" => {
                    if let m::Page::Edit { form, .. } = &mut s.screen.page {
                        let k = EDIT_FIELDS.iter().position(|f| Some(*f) == form.focus).map_or(0, |k| (k + 1) % EDIT_FIELDS.len());
                        form.focus = Some(EDIT_FIELDS[k]);
                    }
                }
                _ if enter => finish_edit(s, true),
                _ if escape => finish_edit(s, false),
                _ => {}
            }
            return;
        }
        if escape {
            if s.screen.menu.take().is_some() {
                *canceled = true;
                return;
            }
            if s.screen.page != m::Page::Map {
                if matches!(s.screen.page, m::Page::Edit { .. }) {
                    finish_edit(s, false);
                }
                s.screen.page = m::Page::Map;
                *canceled = true;
            }
            return;
        }
        if matches!(s.screen.page, m::Page::Edit { .. }) && enter {
            finish_edit(s, true);
            *canceled = true;
            return;
        }
        if !matches!(s.screen.page, m::Page::Map | m::Page::List { .. }) {
            return;
        }
        let rect = rect_now(game, s);
        let pan = match key {
            "key.keyboard.w" | "key.keyboard.up" => Some((0, -1)),
            "key.keyboard.s" | "key.keyboard.down" => Some((0, 1)),
            "key.keyboard.a" | "key.keyboard.left" => Some((-1, 0)),
            "key.keyboard.d" | "key.keyboard.right" => Some((1, 0)),
            _ => None,
        };
        let (lo, hi) = set.zooms();
        let centre = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
        let acted = if let Some(dir) = pan {
            s.screen.follow = false;
            m::key_pan(&mut s.screen.view, rect, dir);
            true
        } else if keys::on_screen(game, keys::ZOOM_IN, key) {
            s.screen.view.zoom_at(rect, centre, 2.0, lo, hi, set.smooth);
            true
        } else if keys::on_screen(game, keys::ZOOM_OUT, key) {
            s.screen.view.zoom_at(rect, centre, -2.0, lo, hi, set.smooth);
            true
        } else if keys::on_screen(game, keys::CAVE, key) {
            set.cave_mode = (set.cave_mode + 1) % m::CAVE_MODES.len();
            s.screen.layer = None;
            notice(s, format!("Cave mode: {}", m::CAVE_MODES[set.cave_mode]));
            true
        } else if keys::on_screen(game, keys::LAYER_DOWN, key) {
            step_layer(game, s, &set, true);
            true
        } else if keys::on_screen(game, keys::LAYER_UP, key) {
            step_layer(game, s, &set, false);
            true
        } else if keys::on_screen(game, keys::DIMENSION, key) {
            next_dim(game, s, &set);
            true
        } else if keys::on_screen(game, keys::CENTRE, key) {
            if let Some(p) = player_pos(game) {
                s.screen.dim = None;
                s.screen.view.centre_on(p[0], p[2]);
            }
            true
        } else if keys::on_screen(game, keys::FOLLOW, key) {
            s.screen.follow = !s.screen.follow;
            true
        } else if keys::on_screen(game, keys::WAYPOINT, key) {
            let block = spot_at(game, s, &set, rect, if inside(rect, mouse) { mouse } else { centre });
            new_waypoint(game, s, &set, block);
            true
        } else if keys::on_screen(game, keys::WAYPOINTS, key) {
            s.screen.page = if matches!(s.screen.page, m::Page::List { .. }) { m::Page::Map } else { m::Page::List { scroll: 0 } };
            true
        } else if keys::on_screen(game, keys::HIDE_UI, key) {
            s.screen.hide_ui = !s.screen.hide_ui;
            true
        } else if keys::on_screen(game, keys::EXPORT, key) {
            s.pending_export = true;
            true
        } else if keys::on_screen(game, keys::MAP, key) {
            s.pending_close = true;
            true
        } else {
            // Other keys don't reach the game (the inventory key doesn't close the map).
            key.starts_with("key.keyboard.") && !key.starts_with("key.keyboard.f")
        };
        if acted {
            *canceled = true;
        }
    });
    if set != before {
        store_settings(game, &before, set);
    }
}

/// `native_screen_char_typed_pre`: a character of a name or a number.
pub(super) fn on_char(game: &Game<'_>, screen: &NativeScreenView<'_>, c: char, canceled: &mut bool) {
    if !ours(game, screen.id) {
        return;
    }
    with(|s| {
        if let m::Page::Edit { form, .. } = &mut s.screen.page {
            if form.focus.is_some() {
                form.type_char(c);
            }
            *canceled = true;
        }
    });
}

pub(super) fn on_list(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: m::ID.into(),
        title: format!("{} {}", m::NAME, m::VERSION),
        description: m::DESCRIPTION.into(),
        keys: keys::listed(),
    });
}

pub(super) fn on_settings(game: &mut Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id != m::ID {
        return;
    }
    let page = with(|st| st.options_page);
    buttons.push(format!("Page: {} (click for the next)", m::PAGES[page]));
    if page == m::KEYS_PAGE {
        for (name, label) in keys::labels() {
            let bound = keys::bound(game, name);
            buttons.push(format!("{label}: {}", horizon_client::keybinds::display_name(&bound).plain()));
        }
    } else {
        buttons.extend(settings(game).page(page).into_iter().map(|(_, l)| l));
    }
}

pub(super) fn on_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != m::ID {
        return;
    }
    if button == 0 {
        with(|st| st.options_page = (st.options_page + 1) % m::PAGES.len());
        return;
    }
    let page = with(|st| st.options_page);
    if page == m::KEYS_PAGE {
        return;
    }
    let before = settings(game);
    let mut s = before;
    let Some(&(field, _)) = s.page(page).get(button - 1) else {
        return;
    };
    s.click(field);
    store_settings(game, &before, s);
}
