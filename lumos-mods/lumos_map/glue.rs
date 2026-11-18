// Lumos Map: between Horizon's `Game` and the rules in `lumos_map`.
//
// Terrain is coloured as the game draws it: each block state's top texture averaged once (off the
// frame, again when the resources change), tinted by its biome's grass, foliage or water colour.
// The explored map keeps a tile per chunk, scanned a few at a time from the chunks the client has
// loaded and saved in region files per world or server. The minimap and the world map are drawn
// into parts of the mod image slot, and only when what they show has changed.
use crate::game::lumos_map as m;
use crate::game::lumos_map_keys as keys;
use crate::game::{Game, Screen};
use crate::mod_api::{NativeMod, NativeScreenView, NativeWorldId};
use horizon_client::blocks::{Fluid, MAP_COLORS};
use horizon_client::chunk::Chunk;
use horizon_client::extras::Corner;
use horizon_client::extras_hud::Corners;
use horizon_client::hud::{HudBatch, MOD_IMAGE_SPRITE, rgba};
use horizon_client::mesher::TintSource;
use horizon_client::text::Text;
use horizon_client::ui;
use horizon_client::world_primitives::WorldPrimitives;
use horizon_world::registry::BlockStateId;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// Scanning time a frame may take, with the map closed and open.
const BUDGET: Duration = Duration::from_micros(700);
const BUDGET_OPEN: Duration = Duration::from_micros(2500);
/// The cave layer's scanning time a frame.
const CAVE_BUDGET: Duration = Duration::from_micros(500);
/// How often the loaded chunks are looked over for new ones.
const REFILL: Duration = Duration::from_secs(1);
/// How often the chunks around the player are scanned again after blocks changed.
const NEAR_RESCAN: Duration = Duration::from_secs(2);
/// How often changed regions are written.
const SAVE_EVERY: Duration = Duration::from_secs(30);
/// Time a frame may spend drawing terrain cells, with the map closed and open.
const CELL_BUDGET: Duration = Duration::from_micros(800);
const CELL_BUDGET_OPEN: Duration = Duration::from_micros(3000);

/// The map screen's bars, in GUI pixels.
const TOP: f32 = 22.0;
const BOTTOM: f32 = 14.0;

// Column flags of the scan's palette.
const FLUID: u8 = 3;
const AIR: u8 = 4;
/// Makes a roof for the cave view: not air or a fluid, has a map colour, and the map doesn't
/// see through it (leaves, glass, panes and bars let the sky down).
const ROOF: u8 = 8;
/// How often the roof over the player is tested.
const ROOF_EVERY: Duration = Duration::from_millis(200);

/// Per block state: its map colour, flags and colour from above.
struct Palette {
    color: Vec<u8>,
    flags: Vec<u8>,
    look: Vec<m::BlockColour>,
    /// Biome colours blended across borders (the setting, kept here for the scans).
    blend: bool,
}

impl Palette {
    fn new(game: &Game<'_>, look: Vec<m::BlockColour>) -> Self {
        let facts = game.res.facts();
        let mut color = Vec::with_capacity(facts.states.len());
        let mut flags = Vec::with_capacity(facts.states.len());
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
            flags.push(b);
        }
        Self { color, flags, look, blend: true }
    }

    #[inline]
    fn get(&self, id: u32) -> (u8, u8) {
        let i = id as usize;
        (
            self.color.get(i).copied().unwrap_or(0),
            self.flags.get(i).copied().unwrap_or(AIR),
        )
    }
}

/// Every block state's colour from above, made off the frame from the resources loaded now.
fn colour_table(game: &Game<'_>) -> Receiver<(u64, Vec<m::BlockColour>)> {
    let (tx, rx) = mpsc::channel();
    let states = game.native_block_states();
    let res = game.native_client_resources();
    let version = game.native_block_sprites_version();
    let _ = std::thread::Builder::new()
        .name("lumos-map-colours".into())
        .spawn(move || {
            let mut averages: HashMap<String, Option<u32>> = HashMap::new();
            let mut out = vec![m::BlockColour::default(); states.len()];
            for (id, facts) in states.iter() {
                if facts.air {
                    continue;
                }
                let Some(sprites) = states.sprites(id) else {
                    continue;
                };
                // The top face; for a plant drawn as crossed planes, its whole picture.
                let (sprite, tint) = match sprites.sides[1].first() {
                    Some(face) => (Some(face.sprite), face.tint),
                    None => (sprites.particle, plant_tint(&facts.name)),
                };
                let Some(name) = sprite.and_then(|s| states.texture(s)) else {
                    continue;
                };
                let avg = *averages.entry(name.to_owned()).or_insert_with(|| {
                    res.texture(name)
                        .and_then(|t| m::average(t.width, t.height, &t.rgba))
                });
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

/// The tint of a plant whose model is only crossed planes (they keep no face to read it from).
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

/// What one column's scan found: the block seen from above, its height, the water over the
/// ground and the ground under that water.
#[derive(Clone, Copy, Default)]
struct Hit {
    id: u32,
    color: u8,
    y: i32,
    depth: u8,
    /// Under water: the first block with a colour below it.
    ground: Option<(u32, u8)>,
    /// `m::FLOOR`, `m::WALL` or `m::DEEP`.
    kind: u8,
    found: bool,
}

/// Where a scan looks down from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scan {
    /// From the top of the chunk: the surface.
    Top,
    /// From the first open space under a roofed dimension's roof (its top block is `.0`).
    UnderRoof(i32),
    /// From the first open space under the ground (the full cave map of an open dimension).
    UnderGround,
    /// A cave slice: a block at `start` is a wall, else the first floor down to `depth` blocks
    /// under it (nothing there: a drop); floors up to `above` blocks over it, with open space on
    /// them, show too.
    Cave { start: i32, depth: i32, above: i32 },
}

impl Scan {
    /// The same slice for the maps: a cave slice moved by less than two blocks is still it.
    fn same(self, other: Scan) -> bool {
        match (self, other) {
            (Scan::Cave { start: a, depth: d, above: u }, Scan::Cave { start: b, depth: e, above: v }) => {
                d == e && u == v && (a - b).abs() < 2
            }
            _ => self == other,
        }
    }
}

/// The Y of the first roof block over column `x, z` above `head`, or None under open sky (sky
/// light at the head is the quick answer; air sections are passed over whole).
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

/// One chunk's columns as `mode` sees them: the first block with a map colour going down, and
/// the water over the ground. Colours take the biome tints blended across the quarts around
/// each block (no seams between biomes), and water shows the ground under it.
fn scan(
    world: &horizon_client::world::World,
    chunk: &Chunk,
    palette: &Palette,
    air: u32,
    mode: Scan,
    tints: &mut dyn FnMut(u16, i32, i32) -> m::BiomeTints,
) -> m::Tile {
    let min = chunk.min_y();
    let top = (0..chunk.sections.len())
        .rev()
        .find(|&s| !chunk.sections[s].is_uniform(air))
        .map_or(min, |s| min + s as i32 * 16 + 15);
    let (cx, cz) = (chunk.x * 16, chunk.z * 16);
    let mut hits = [Hit::default(); 256];
    for z in 0..16 {
        for x in 0..16 {
            let mut y = top;
            let mut wall = false;
            // The lowest Y looked at, and whether finding nothing there is a drop.
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
                Scan::Cave { start, depth, above } => {
                    lowest = (start - depth).max(min + 1);
                    deep = true;
                    // A floor over the start with open space on it (a lava lake or a cave over
                    // you); the rock you stand under is passed through.
                    let mut over = None;
                    let mut k = start + above;
                    while k > start {
                        let (c, f) = palette.get(chunk.get(x, k, z));
                        if c != 0 && f & AIR == 0 && palette.get(chunk.get(x, k + 1, z)).1 & AIR != 0 {
                            over = Some(k);
                            break;
                        }
                        k -= 1;
                    }
                    let (color, flags) = palette.get(chunk.get(x, start, z));
                    if let Some(k) = over {
                        y = k;
                    } else if flags & AIR == 0 && color != 0 && flags & FLUID == 0 {
                        wall = true;
                        y = start;
                    } else {
                        y = y.min(start);
                    }
                }
            }
            if y < lowest && deep {
                // An empty chunk part over the slice: open down past the depth.
                hits[(z * 16 + x) as usize] = Hit { color: m::BLACK, y: lowest, kind: m::DEEP, found: true, ..Hit::default() };
                continue;
            }
            while y >= lowest {
                let id = chunk.get(x, y, z);
                let (color, flags) = palette.get(id);
                if color == 0 {
                    y -= 1;
                    continue;
                }
                let kind = if wall { m::WALL } else { m::FLOOR };
                let mut hit = Hit { id, color, y, kind, found: true, ..Hit::default() };
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
                hits[(z * 16 + x) as usize] = hit;
                break;
            }
            if deep && !hits[(z * 16 + x) as usize].found {
                hits[(z * 16 + x) as usize] = Hit { color: m::BLACK, y: lowest, kind: m::DEEP, found: true, ..Hit::default() };
            }
        }
    }
    // The biome tints of this chunk's quarts and the ring of quarts around it (from the chunks
    // beside it where they are loaded), each at its column's height.
    let mut grid: m::TintGrid = [m::BiomeTints::default(); 36];
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
            grid[(qz * 6 + qx) as usize] = tints(biome, cx + lx, cz + lz);
        }
    }
    let colour = |id: u32, t: &m::BiomeTints| {
        let look = palette.look.get(id as usize).copied().unwrap_or_default();
        look.known.then(|| m::block_rgb(&look, t))
    };
    let map_rgb = |c: u8| MAP_COLORS.get(usize::from(c)).copied().unwrap_or(0);
    let mut tile = m::Tile::default();
    for z in 0..16 {
        for x in 0..16 {
            let i = (z * 16 + x) as usize;
            let h = hits[i];
            if !h.found {
                continue;
            }
            if h.kind == m::DEEP {
                tile.color[i] = h.color;
                tile.height[i] = h.y.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
                tile.kind[i] = m::DEEP;
                continue;
            }
            let t = if palette.blend { m::blend_tints(&grid, x, z) } else { grid[((z / 4 + 1) * 6 + x / 4 + 1) as usize] };
            let mut rgb = colour(h.id, &t).unwrap_or(0);
            if let Some((gid, gc)) = h.ground
                && rgb != 0
            {
                let ground = colour(gid, &t).unwrap_or_else(|| map_rgb(gc));
                rgb = m::water_over(ground, rgb, h.depth).max(1);
            }
            if h.kind == m::WALL {
                // Walls cut at the layer: darker, so the floor stands out.
                let base = if rgb != 0 { rgb } else { map_rgb(h.color) };
                rgb = m::tinted(base, 0x707070).max(1);
            }
            tile.color[i] = h.color;
            tile.rgb[i] = rgb;
            tile.height[i] = h.y.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            tile.depth[i] = h.depth;
            tile.kind[i] = h.kind;
        }
    }
    tile
}

/// The explored map of the world (or server) and dimension being played.
struct Explored {
    dim: &'static str,
    /// `<instance>/lumos-map/<world>/<dimension>/`.
    dir: PathBuf,
    store: m::Store,
    /// Chunks scanned since they were loaded (with the colours of this table).
    seen: HashSet<(i32, i32)>,
    queue: VecDeque<(i32, i32)>,
    refilled: Option<Instant>,
    /// The near chunks' last rescan, and the world's revision then.
    near: (Option<Instant>, u64),
    saved: Instant,
    /// Region files being read (which, and their tiles to come), and the regions read or
    /// being read: the region cache keeps those near you and the map's view.
    loading: Vec<(HashSet<(i32, i32)>, Receiver<Vec<((i32, i32), m::Tile)>>)>,
    cached: HashSet<(i32, i32)>,
    cache_at: Option<Instant>,
    /// Saved cave layers by band (None: the full cave map), under `<dim>/caves/`.
    caves: HashMap<Option<i32>, Saved>,
    /// Your recent path here (`<dim>/footsteps.txt`), and whether it changed since saved.
    steps: m::Footsteps,
    steps_dirty: bool,
}

impl Explored {
    fn open(world: &Path, dim: &'static str) -> Self {
        let dir = world.join(m::folder_name(dim));
        let steps = m::Footsteps::from_text(&std::fs::read_to_string(dir.join("footsteps.txt")).unwrap_or_default());
        Self {
            dim,
            dir,
            store: m::Store::default(),
            seen: HashSet::new(),
            queue: VecDeque::new(),
            refilled: None,
            near: (None, 0),
            saved: Instant::now(),
            loading: Vec::new(),
            cached: HashSet::new(),
            cache_at: None,
            caves: HashMap::new(),
            steps,
            steps_dirty: false,
        }
    }

    /// Writes the regions changed since the last save (its cave layers' too), off the frame.
    fn save(&mut self) {
        self.saved = Instant::now();
        // A region still being read is written once its saved tiles are in (else the file
        // would lose them).
        let reading: HashSet<(i32, i32)> = self.loading.iter().flat_map(|(r, _)| r.iter().copied()).collect();
        let later: Vec<(i32, i32)> = self.store.dirty.iter().filter(|r| reading.contains(r)).copied().collect();
        for r in &later {
            self.store.dirty.remove(r);
        }
        write_regions(&mut self.store, &self.dir);
        self.store.dirty.extend(later);
        for c in self.caves.values_mut() {
            write_regions(&mut c.store, &c.dir);
        }
        if std::mem::take(&mut self.steps_dirty) {
            let (dir, text) = (self.dir.clone(), self.steps.to_text());
            let _ = std::thread::Builder::new().name("lumos-map-save".into()).spawn(move || {
                let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join("footsteps.txt"), text));
            });
        }
    }

    /// The saved cave layer of `band` (None: the full cave map), its regions loading off the
    /// frame the first time it is asked for.
    fn cave(&mut self, band: Option<i32>) -> &mut Saved {
        let dir = self.dir.join(m::cave_folder(band));
        self.caves.entry(band).or_insert_with(|| Saved {
            store: m::Store { band, ..m::Store::default() },
            loading: Some(load_regions(dir.clone(), band, None)),
            dir,
        })
    }

    /// Takes in regions that finished loading.
    fn follow_loading(&mut self) {
        let mut done = Vec::new();
        for (i, (_, rx)) in self.loading.iter().enumerate() {
            if let Ok(tiles) = rx.try_recv() {
                done.push((i, tiles));
            }
        }
        for (i, tiles) in done.into_iter().rev() {
            self.store.merge_loaded(tiles);
            self.loading.remove(i);
        }
        for c in self.caves.values_mut() {
            if let Some(rx) = &c.loading
                && let Ok(tiles) = rx.try_recv()
            {
                c.store.merge_loaded(tiles);
                c.loading = None;
            }
        }
    }
}

impl Explored {
    /// The region cache, once a second: the saved regions within `radius` of you and of the
    /// map's view (`view`, with its own reach) read, the ones past a region more let go once
    /// saved.
    fn follow_cache(&mut self, you: (f64, f64), view: Option<((f64, f64), i32)>, radius: i32) {
        if self.cache_at.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return;
        }
        self.cache_at = Some(Instant::now());
        let mut want = m::regions_near(&[you], radius);
        let mut keep = m::regions_near(&[you], radius + 1);
        if let Some((centre, reach)) = view {
            want.extend(m::regions_near(&[centre], reach));
            keep.extend(m::regions_near(&[centre], reach + 1));
        }
        let new: HashSet<(i32, i32)> = want.difference(&self.cached).copied().collect();
        if !new.is_empty() {
            self.cached.extend(new.iter().copied());
            self.loading.push((new.clone(), load_regions(self.dir.clone(), None, Some(new))));
        }
        let reading: HashSet<(i32, i32)> = self.loading.iter().flat_map(|(r, _)| r.iter().copied()).collect();
        self.store.let_go(|r| keep.contains(&r) || reading.contains(&r));
        let dirty = &self.store.dirty;
        self.cached.retain(|r| keep.contains(r) || reading.contains(r) || dirty.contains(r));
    }
}

/// Another dimension's saved surface map, read for the world map (not scanned: you aren't there).
struct Viewed {
    dim: String,
    store: m::Store,
    loading: Option<Receiver<Vec<((i32, i32), m::Tile)>>>,
}

/// A saved cave layer: a band's tiles (or the full cave map's), kept as the surface's are.
struct Saved {
    store: m::Store,
    dir: PathBuf,
    loading: Option<Receiver<Vec<((i32, i32), m::Tile)>>>,
}

/// The region files of `dir` (of cave layer `band`, None for the surface), read off the frame:
/// all of them, or `only` these regions.
fn load_regions(dir: PathBuf, band: Option<i32>, only: Option<HashSet<(i32, i32)>>) -> Receiver<Vec<((i32, i32), m::Tile)>> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("lumos-map-load".into())
        .spawn(move || {
            let mut all = Vec::new();
            if let Some(only) = only {
                for region in only {
                    if let Ok(bytes) = std::fs::read(dir.join(m::region_file(region)))
                        && m::region_band(&bytes) == band
                        && let Some(tiles) = m::decode_region(region, &bytes)
                    {
                        all.extend(tiles);
                    }
                }
            } else if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let name = e.file_name();
                    let Some(region) = name.to_str().and_then(m::region_from_file) else {
                        continue;
                    };
                    if let Ok(bytes) = std::fs::read(e.path())
                        && m::region_band(&bytes) == band
                        && let Some(tiles) = m::decode_region(region, &bytes)
                    {
                        all.extend(tiles);
                    }
                }
            }
            let _ = tx.send(all);
        });
    rx
}

/// Writes `store`'s regions changed since the last save into `dir`, off the frame.
fn write_regions(store: &mut m::Store, dir: &Path) {
    if store.dirty.is_empty() {
        return;
    }
    let regions: Vec<(i32, i32)> = store.dirty.drain().collect();
    let files: Vec<(PathBuf, Vec<u8>)> = regions
        .into_iter()
        .map(|r| (dir.join(m::region_file(r)), store.encode_region(r)))
        .collect();
    let dir = dir.to_owned();
    let _ = std::thread::Builder::new()
        .name("lumos-map-save".into())
        .spawn(move || {
            if std::fs::create_dir_all(&dir).is_err() {
                return;
            }
            for (path, bytes) in files {
                let tmp = path.with_extension("tmp");
                if std::fs::write(&tmp, &bytes).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
        });
}

/// Tiles of one layer (under the player's head, or the world map's slice), scanned live from the
/// loaded chunks; a cave slice's are also kept in its band's saved layer.
#[derive(Default)]
struct Layer {
    /// What it was scanned as (None: nothing yet).
    mode: Option<Scan>,
    tiles: HashMap<(i32, i32), (m::Tile, u64)>,
    queue: VecDeque<(i32, i32)>,
    revision: u64,
    /// Chunks scanned since the maps last looked, and a count that goes up when it starts over.
    changed: Vec<(i32, i32)>,
    epoch: u64,
}

impl Layer {
    /// A cave slice's depth shading (none for a roof's top).
    fn shade(&self) -> Option<m::CaveShade> {
        match self.mode? {
            Scan::Cave { start, depth, .. } => Some(m::CaveShade { start, depth }),
            _ => None,
        }
    }

    fn column(&self, x: i32, z: i32) -> Option<m::Col> {
        self.tiles.get(&(x >> 4, z >> 4))?.0.col(x, z)
    }

    /// A cave slice's band (its saved layer).
    fn band(&self) -> Option<i32> {
        match self.mode? {
            Scan::Cave { start, .. } => Some(m::band_of(start)),
            _ => None,
        }
    }
}

#[derive(Default)]
struct State {
    explored: Option<Explored>,
    /// `<instance>/lumos-map/<world>`, and the waypoints of it.
    world_dir: Option<PathBuf>,
    book: m::Book,
    palette: Option<Palette>,
    /// The colour table being made, and the resources' version it is for.
    colours: Option<Receiver<(u64, Vec<m::BlockColour>)>>,
    colours_version: Option<u64>,
    /// Biome tints by biome id (the client's table).
    tints: HashMap<u16, m::BiomeTints>,
    /// The last look for the world's id (it is asked at most once a second).
    world_asked: Option<Instant>,
    was_dead: bool,
    screen: m::MapScreen,
    /// The terrain cells in the mod image slot, shared by the minimap and the world map.
    cells: m::CellCache,
    /// The slot's place in the atlas when the cells were drawn (a rebuilt atlas loses them),
    /// and the look they were drawn in.
    cells_for: Option<([f32; 4], m::Look)>,
    /// Goes up when every cell must be drawn again (new colours, another world).
    cells_gen: u64,
    /// The cave layer under the player's head.
    cave: Option<Layer>,
    /// The roof test's switch, when it last looked, and the clock its ticks count on.
    cave_switch: m::CaveSwitch,
    roof_checked: Option<Instant>,
    clock: Option<Instant>,
    /// When reached waypoints were last looked for.
    reached_at: Option<Instant>,
    /// Another dimension's saved map, shown on the world map instead of this one's.
    viewed: Option<Viewed>,
    /// The light overlay: per chunk near you, a bit per column of each row marked dark; when it
    /// was last made.
    light_mask: HashMap<(i32, i32), [u16; 16]>,
    light_at: Option<Instant>,
    /// The minimap is enlarged now (key held, or toggled on).
    big: bool,
    big_toggled: bool,
    /// What the minimap's cave view shows this frame (None: the surface).
    cave_view: Option<m::CaveView>,
    /// The world map's cave map type per dimension (`<world>/cave-maps.txt`).
    cave_types: m::CaveTypes,
    /// The world map's layered view keeps its top Y at the player's head.
    slice_follow: bool,
    /// The dimension the world map shows and whether it is roofed (for its tool labels).
    map_dim: String,
    map_roofed: bool,
    /// Entity faces in the slot's icon row.
    icons: Icons,
    /// The page shown on the mod's page in Options > Mods.
    options_page: usize,
    /// The biome at the player's block, by block.
    biome: Option<([i32; 3], String)>,
    /// The world map's layer (`MapScreen::slice`), scanned live.
    slice: Option<Layer>,
    /// A server's folder and its sub-worlds; when the spawn was last looked at.
    subworlds: Option<(PathBuf, m::SubWorlds)>,
    spawn_checked: Option<Instant>,
    /// Coordinates seen in chat lately, newest last: who said them, and where.
    chat: VecDeque<(String, [i32; 3])>,
    /// Text to open the chat with once the map is closed (sharing a waypoint).
    pending_chat: Option<String>,
    /// A command sent once the map is closed (teleporting to a waypoint).
    pending_command: Option<String>,
    /// Text put on the clipboard on the next frame (a waypoint's coordinates).
    pending_clipboard: Option<String>,
    /// `<instance>/lumos-map/global-waypoints.txt`: waypoints kept for every world.
    global_path: Option<PathBuf>,
    /// The world map saved as a PNG on the next frame.
    pending_export: bool,
    /// A notice on the map's bar, until when.
    notice: Option<(String, Instant)>,
    /// The map closed on the next frame (the "New waypoint" box opened with the key is done),
    /// with what to say on the action bar: the waypoint made, and its colour.
    pending_close: Option<Option<(String, u32)>>,
    /// The character of the key that just opened the "New waypoint" box, and when: the game
    /// hands it to the screen the key opened, and it mustn't land in the name box.
    swallow: Option<(char, Instant)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with_borrow_mut(f)
}

fn settings(game: &Game<'_>) -> m::Settings {
    m::settings(|key| game.native_setting(m::ID, key))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `<instance>/lumos-map/`.
fn base(game: &Game<'_>) -> PathBuf {
    game.native_instance_dir().join("lumos-map")
}

/// The folder of the world being played: `sp-<save folder>` or `mp-<address>`.
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

fn block_of(p: [f64; 3]) -> [i32; 3] {
    m::block_at(p)
}

fn save_book(s: &State) {
    let Some(dir) = &s.world_dir else {
        return;
    };
    let (own, all) = s.book.split_global();
    let _ = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join("waypoints.txt"), own.to_text()));
    if let Some(g) = &s.global_path {
        let _ = std::fs::write(g, all.to_text());
    }
}

/// Writes `rgba` (`w` x `h`) into the mod image slot at `at`.
fn upload(game: &mut Game<'_>, at: [u32; 2], w: u32, h: u32, rgba: Vec<u8>) -> bool {
    let Some(rect) = game.gui.atlas.sprites.get(MOD_IMAGE_SPRITE).copied() else {
        return false;
    };
    if at[0] + w > rect.w || at[1] + h > rect.h || rgba.len() != (w * h * 4) as usize {
        return false;
    }
    let (aw, ah) = (game.gui.atlas.width as f32, game.gui.atlas.height as f32);
    game.map_patches.push(horizon_client::font::Patch {
        x: (rect.u0 * aw).round() as u32 + at[0],
        y: (rect.v0 * ah).round() as u32 + at[1],
        width: w,
        height: h,
        rgba,
    });
    true
}

/// Follows the world and dimension played: the explored map and the waypoints of each.
fn follow_world(game: &Game<'_>, s: &mut State) {
    let dim = game.dim.id();
    if s.explored.as_ref().is_some_and(|e| e.dim == dim) {
        return;
    }
    if s.world_asked.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
        return;
    }
    s.world_asked = Some(Instant::now());
    let Some(world) = world_folder(game) else {
        return;
    };
    let dir = base(game).join(&world);
    s.global_path = Some(base(game).join("global-waypoints.txt"));
    let dir = if world.starts_with("mp-") {
        // A server: one of its sub-worlds.
        let subs = match s.subworlds.take() {
            Some((d, subs)) if d == dir => subs,
            _ => m::SubWorlds::from_text(&std::fs::read_to_string(dir.join("subworlds.txt")).unwrap_or_default()),
        };
        let sub = sub_dir(&dir, subs.current_name());
        s.subworlds = Some((dir, subs));
        s.spawn_checked = None;
        sub
    } else {
        s.subworlds = None;
        dir
    };
    open_world(s, dir, dim);
}

/// A server's sub-world folder: the first is the server's own (where the first version kept
/// it), the others beside it.
fn sub_dir(server: &Path, name: &str) -> PathBuf {
    if name == "world-1" {
        server.to_owned()
    } else {
        server.join(name)
    }
}

fn save_subworlds(s: &State) {
    if let Some((dir, subs)) = &s.subworlds {
        let _ = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join("subworlds.txt"), subs.to_text()));
    }
}

/// On a server, the world's spawn tells its sub-worlds apart: a spawn of another one switches
/// to it (or to a new one).
fn follow_spawn(game: &Game<'_>, s: &mut State) {
    if s.spawn_checked.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
        return;
    }
    s.spawn_checked = Some(Instant::now());
    // "Server maps: Single": one map, whatever the spawn.
    if settings(game).map_mode == 1 {
        return;
    }
    let Some(spawn) = game.spawn else {
        return;
    };
    let dim = game.dim.id();
    let Some((dir, subs)) = s.subworlds.as_mut() else {
        return;
    };
    let before = subs.clone();
    let changed = subs.follow_spawn([spawn.x.floor() as i32, spawn.z.floor() as i32]);
    let sub = sub_dir(dir, subs.current_name());
    if *subs != before {
        save_subworlds(s);
    }
    if changed {
        open_world(s, sub, dim);
        notice(s, "Another world on this server: its own map and waypoints".into());
    }
}

/// Switches a server's sub-world: to the next, or a new one.
fn switch_subworld(game: &Game<'_>, s: &mut State, new: bool) {
    let dim = game.dim.id();
    let Some((dir, subs)) = s.subworlds.as_mut() else {
        return;
    };
    if new {
        subs.add(None);
    } else {
        subs.next();
    }
    let sub = sub_dir(dir, subs.current_name());
    let name = subs.current_name().to_owned();
    save_subworlds(s);
    open_world(s, sub, dim);
    // Chosen by hand: the spawn doesn't switch it back while in this level.
    s.spawn_checked = Some(Instant::now() + Duration::from_secs(60 * 60 * 24));
    notice(s, format!("Map world: {name}"));
}

fn notice(s: &mut State, text: String) {
    s.notice = Some((text, Instant::now() + Duration::from_secs(3)));
}

/// Opens the explored map of `dir` (a world, or a server's sub-world) in `dim`.
fn open_world(s: &mut State, dir: PathBuf, dim: &'static str) {
    if let Some(mut old) = s.explored.take() {
        old.save();
    }
    if s.world_dir.as_ref() != Some(&dir) {
        s.book = std::fs::read_to_string(dir.join("waypoints.txt"))
            .map(|t| m::Book::from_text(&t))
            .unwrap_or_default();
        if let Some(g) = &s.global_path
            && let Ok(t) = std::fs::read_to_string(g)
        {
            s.book.merge_global(m::Book::from_text(&t));
        }
        s.screen.selected.clear();
        s.cave_types = m::CaveTypes::from_text(&std::fs::read_to_string(dir.join("cave-maps.txt")).unwrap_or_default());
        s.world_dir = Some(dir.clone());
    }
    s.explored = Some(Explored::open(&dir, dim));
    s.cave = None;
    s.slice = None;
    s.cells.clear();
    s.cells_gen += 1;
}

/// The colour table: asked for when the resources change, taken in when it is made.
fn follow_colours(game: &Game<'_>, s: &mut State) {
    let version = game.native_block_sprites_version();
    if let Some(rx) = &s.colours
        && let Ok((v, table)) = rx.try_recv()
    {
        s.colours = None;
        s.palette = Some(Palette::new(game, table));
        s.colours_version = Some(v);
        s.tints.clear();
        s.cells_gen += 1;
        // Every loaded chunk again, in these colours.
        if let Some(e) = s.explored.as_mut() {
            e.seen.clear();
            e.queue.clear();
            e.refilled = None;
        }
        s.cave = None;
    }
    if s.colours.is_none() && s.colours_version != Some(version) {
        s.colours = Some(colour_table(game));
        // Until it is made, the map palette.
        if s.palette.is_none() {
            s.palette = Some(Palette::new(game, Vec::new()));
        }
    }
}

/// A biome's tints, asked of the game once per biome.
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

/// Scans chunks for the world map within `budget`: new ones nearest first, then the ones
/// around the player again after blocks changed.
fn explore(game: &Game<'_>, s: &mut State, budget: Duration) {
    let started = Instant::now();
    let Some(p) = player_pos(game) else {
        return;
    };
    let full = roof_top(game).is_none() && cave_type(game, s) == m::CaveType::Full;
    // The map's view and how many regions it reaches across (at most twice the cache's).
    let view = map_open(game).then(|| {
        let (w, h) = game.gui_size();
        let r = map_rect(w, h);
        let v = s.screen.view;
        ((v.x, v.z), ((f64::from(r[2].max(r[3])) / 2.0 * v.bpp() / 512.0).ceil() as i32).max(1))
    });
    let set = settings(game);
    let (footsteps, update, new) = (set.footsteps, set.map_update, set.map_new);
    let State { explored, palette, tints, .. } = s;
    let (Some(e), Some(palette)) = (explored.as_mut(), palette.as_ref()) else {
        return;
    };
    let radius = m::CACHE_RADII[set.map_cache.min(m::CACHE_RADII.len() - 1)] as i32;
    e.follow_cache((p[0], p[2]), view.map(|(c, reach)| (c, reach.min(radius * 2))), radius);
    e.follow_loading();
    if footsteps && e.steps.step(p[0], p[2]) {
        e.steps_dirty = true;
    }
    let world = &game.world;
    let centre = ((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4);
    if e.queue.is_empty() && e.refilled.is_none_or(|t| t.elapsed() >= REFILL) {
        e.refilled = Some(Instant::now());
        e.seen.retain(|k| world.chunks.contains_key(k));
        let mut fresh: Vec<(i32, i32)> = world
            .chunks
            .keys()
            .filter(|k| !e.seen.contains(*k))
            // "Map new chunks" off: only ones already on the map; "Update changed chunks" off:
            // only ones not on it yet.
            .filter(|k| if e.store.tiles.contains_key(*k) { update } else { new })
            .copied()
            .collect();
        if update && world.revision != e.near.1 && e.near.0.is_none_or(|t| t.elapsed() >= NEAR_RESCAN) {
            e.near = (Some(Instant::now()), world.revision);
            for dz in -2..=2 {
                for dx in -2..=2 {
                    let k = (centre.0 + dx, centre.1 + dz);
                    if e.seen.contains(&k) {
                        fresh.push(k);
                    }
                }
            }
        }
        fresh.sort_by_key(|k| (k.0 - centre.0).abs().max((k.1 - centre.1).abs()));
        e.queue.extend(fresh);
    }
    let air = world.air.0;
    let mode = roof_top(game).map_or(Scan::Top, Scan::UnderRoof);
    let mut tint = |b: u16, x: i32, z: i32| tints_of(game, tints, b, x, z);
    while started.elapsed() < budget {
        let Some(k) = e.queue.pop_front() else {
            break;
        };
        let Some(chunk) = world.chunks.get(&k) else {
            continue;
        };
        e.store.put(k, scan(world, chunk, palette, air, mode, &mut tint));
        if full {
            // The full cave map, chosen for this dimension: kept as the surface is.
            let tile = scan(world, chunk, palette, air, Scan::UnderGround, &mut tint);
            e.cave(None).store.put(k, tile);
        }
        e.seen.insert(k);
    }
    if e.saved.elapsed() >= SAVE_EVERY {
        e.save();
    }
}

/// A roofed dimension's roof: the top block of its logical height (the Nether's bedrock roof),
/// None in a dimension with open sky.
fn roof_top(game: &Game<'_>) -> Option<i32> {
    let shape = game.world.shape;
    shape.has_ceiling.then(|| shape.min_y + shape.logical_height - 1)
}

/// The world map's cave map type for the dimension played.
fn cave_type(game: &Game<'_>, s: &State) -> m::CaveType {
    s.cave_types.get(game.dim.id(), roof_top(game).is_some())
}

/// Chooses the world map's cave map type for the dimension played, saved with the world; the
/// view follows (a layered one starts at the head, a full one maps the loaded chunks again).
fn set_cave_type(game: &Game<'_>, s: &mut State, t: m::CaveType) {
    s.cave_types.set(game.dim.id(), t);
    if let Some(dir) = &s.world_dir {
        let _ = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join("cave-maps.txt"), s.cave_types.to_text()));
    }
    match t {
        m::CaveType::Layered => {
            if s.screen.slice.is_none() {
                s.screen.slice = head_block(game);
            }
        }
        _ => s.screen.slice = None,
    }
    if t == m::CaveType::Full
        && roof_top(game).is_none()
        && let Some(e) = s.explored.as_mut()
    {
        e.cave(None);
        e.seen.clear();
        e.queue.clear();
        e.refilled = None;
    }
}

/// The world map shows the next dimension's saved map (the three of the game, then back to
/// the one you are in).
fn view_next_dimension(game: &Game<'_>, s: &mut State) {
    let here = game.dim.id();
    let all = dimensions(here);
    let now = s.viewed.as_ref().map_or(here, |v| v.dim.as_str());
    let at = all.iter().position(|d| d == now).unwrap_or(0);
    let next = all[(at + 1) % all.len()].clone();
    s.screen.menu = None;
    if next == here {
        s.viewed = None;
    } else if let Some(world) = &s.world_dir {
        let dir = world.join(m::folder_name(&next));
        s.viewed = Some(Viewed { loading: Some(load_regions(dir, None, None)), dim: next, store: m::Store::default() });
    }
    // The cells of the old view go (they show another place).
    s.cells.clear();
    s.cells_gen += 1;
}

/// What the world map draws from: the surface (0), its live slice over the band's saved layer
/// (2: a layered view, or a roof's top) or the full cave map (3; a roofed dimension's surface
/// map is its full one).
fn map_source(game: &Game<'_>, s: &State) -> u8 {
    if s.viewed.is_some() {
        return 4;
    }
    let roofed = roof_top(game).is_some();
    if m::settings(|_| None).fair_play {
        return 0;
    }
    match cave_type(game, s) {
        m::CaveType::Layered if s.screen.slice.is_some() => 2,
        m::CaveType::Off if roofed => 2,
        m::CaveType::Full if !roofed => 3,
        _ => 0,
    }
}

/// The column the world map shows at `x, z`.
fn map_column(s: &State, source: u8, x: i32, z: i32) -> Option<m::Col> {
    match source {
        4 => s.viewed.as_ref()?.store.column(x, z),
        2 => s.slice.as_ref().and_then(|l| {
            l.column(x, z).or_else(|| saved(s, Some(l.band()?))?.store.column(x, z))
        }),
        3 => saved(s, None)?.store.column(x, z),
        _ => s.explored.as_ref()?.store.column(x, z),
    }
}

/// The world map's title for what it shows.
fn map_layer_name(game: &Game<'_>, s: &State, settings: &m::Settings) -> String {
    match (map_source(game, s), s.screen.slice) {
        (2, Some(y)) => format!("Caves Y {y}..{}", y - settings.depth_blocks()),
        (2, None) => "Roof top".into(),
        (3, _) => "Caves (full)".into(),
        _ if roof_top(game).is_some() => "Under the roof".into(),
        _ => "Surface".into(),
    }
}

/// The block the player's head is in.
fn head_block(game: &Game<'_>) -> Option<i32> {
    let p = game.player.as_ref()?;
    Some((p.position.y + f64::from(p.dimensions.eye_height)).floor() as i32)
}

/// The roof test, a few times a second: a roof over every column of the square round the
/// player (Auto only; a roofed dimension is always under its roof), fed to the switch.
fn follow_roof(game: &Game<'_>, s: &mut State, settings: &m::Settings) {
    if settings.cave != 0 || s.roof_checked.is_some_and(|t| t.elapsed() < ROOF_EVERY) {
        return;
    }
    s.roof_checked = Some(Instant::now());
    let (Some(p), Some(head), Some(palette)) = (player_pos(game), head_block(game), s.palette.as_ref()) else {
        return;
    };
    let roofed = roof_top(game).is_some() || {
        let r = settings.roof_side() / 2;
        let (bx, bz) = (p[0].floor() as i32, p[2].floor() as i32);
        let mut columns = Vec::with_capacity(25);
        'all: for dz in -r..=r {
            for dx in -r..=r {
                let roof = roof_over(&game.world, palette, bx + dx, head, bz + dz);
                columns.push(roof);
                if roof.is_none() {
                    break 'all;
                }
            }
        }
        m::roofed(&columns)
    };
    let tick = (s.clock.get_or_insert_with(Instant::now).elapsed().as_millis() / 50) as u64;
    s.cave_switch.step(roofed, tick);
}

/// Keeps a layer scanned as `mode` within `radius` chunks of `centre`, within `budget`: the
/// cave layer, or the world map's slice.
#[allow(clippy::too_many_arguments)]
fn follow_layer(
    game: &Game<'_>,
    s: &mut State,
    slice: bool,
    mode: Scan,
    centre: (i32, i32),
    radius: i32,
    budget: Duration,
) {
    let started = Instant::now();
    let State { cave, slice: sliced, palette, tints, explored, .. } = s;
    let Some(palette) = palette.as_ref() else {
        return;
    };
    let layer = if slice { sliced } else { cave }.get_or_insert_with(Layer::default);
    let world = &game.world;
    // Another slice (a new height by more than a step) or changed blocks: scanned again,
    // nearest first.
    if layer.mode.is_none_or(|m| !m.same(mode)) || layer.tiles.is_empty() {
        if !layer.tiles.is_empty() || layer.mode != Some(mode) {
            layer.epoch += 1;
        }
        layer.mode = Some(mode);
        layer.queue.clear();
        layer.tiles.retain(|_, _| false);
        layer.changed.clear();
    }
    if layer.queue.is_empty() {
        let mut keys = Vec::new();
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let k = (centre.0 + dx, centre.1 + dz);
                let stale = layer.tiles.get(&k).is_none_or(|t| t.1 != world.revision && dx.abs() <= 2 && dz.abs() <= 2);
                if stale && world.chunks.contains_key(&k) {
                    keys.push(k);
                }
            }
        }
        keys.sort_by_key(|k| (k.0 - centre.0).abs().max((k.1 - centre.1).abs()));
        layer.queue.extend(keys);
        layer.tiles.retain(|k, _| (k.0 - centre.0).abs() <= radius + 2 && (k.1 - centre.1).abs() <= radius + 2);
    }
    // The band's saved layer, loading now if it wasn't.
    if let (Some(band), Some(e)) = (layer.band(), explored.as_mut()) {
        e.cave(Some(band));
    }
    let air = world.air.0;
    let mode = layer.mode.unwrap_or(mode);
    let mut tint = |b: u16, x: i32, z: i32| tints_of(game, tints, b, x, z);
    while started.elapsed() < budget {
        let Some(k) = layer.queue.pop_front() else {
            break;
        };
        let Some(chunk) = world.chunks.get(&k) else {
            continue;
        };
        let tile = scan(world, chunk, palette, air, mode, &mut tint);
        if let (Some(band), Some(e)) = (layer.band(), explored.as_mut()) {
            e.cave(Some(band)).store.put(k, tile.clone());
        }
        layer.tiles.insert(k, (tile, world.revision));
        layer.revision += 1;
        layer.changed.push(k);
    }
}

/// The world map's area of a `w` x `h` GUI.
fn map_rect(w: f32, h: f32) -> [f32; 4] {
    [0.0, TOP, (w - TOOL_W).max(1.0), (h - TOP - BOTTOM).max(1.0)]
}

fn map_open(game: &Game<'_>) -> bool {
    game.native_mod_screen() == Some(m::ID)
}

fn look(settings: &m::Settings) -> m::Look {
    m::Look {
        textured: settings.textured,
        shading: settings.shading,
        legible: settings.cave_legible,
    }
}

/// The world map follows the player (when set) and keeps its layer scanned.
fn follow_map(game: &Game<'_>, s: &mut State, settings: &m::Settings) {
    if let Some(v) = s.viewed.as_mut()
        && let Some(rx) = &v.loading
        && let Ok(tiles) = rx.try_recv()
    {
        v.store.merge_loaded(tiles);
        v.loading = None;
    }
    if s.map_dim != game.dim.id() {
        s.map_dim = game.dim.id().to_owned();
    }
    s.map_roofed = roof_top(game).is_some();
    if s.screen.follow
        && let Some(p) = player_pos(game)
    {
        s.screen.view.x = p[0];
        s.screen.view.z = p[2];
    }
    if s.slice_follow
        && s.screen.slice.is_some()
        && let Some(head) = head_block(game)
    {
        s.screen.slice = Some(head);
    }
    let mode = match (map_source(game, s), s.screen.slice) {
        (2, Some(y)) => Scan::Cave { start: y, depth: settings.depth_blocks(), above: 0 },
        (2, None) => Scan::Top,
        _ => {
            s.slice = None;
            return;
        }
    };
    let view = s.screen.view;
    let centre = ((view.x.floor() as i32) >> 4, (view.z.floor() as i32) >> 4);
    let radius = game.settings.render_distance.max(2) + 1;
    follow_layer(game, s, true, mode, centre, radius, CAVE_BUDGET * 2);
}

/// The explored map around the world map's centre, a block a pixel (at most 2048 across), saved
/// as a PNG in `<instance>/lumos-map/exports/`.
fn export_png(game: &Game<'_>, s: &mut State, settings: &m::Settings) {
    let Some(e) = s.explored.as_ref() else {
        return;
    };
    let view = s.screen.view;
    // The explored tiles' extent, cut to 2048 blocks around the view.
    let (mut x0, mut z0, mut x1, mut z1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    let (cx, cz) = ((view.x.floor() as i32) >> 4, (view.z.floor() as i32) >> 4);
    for &(x, z) in e.store.tiles.keys() {
        if (x - cx).abs() <= 64 && (z - cz).abs() <= 64 {
            x0 = x0.min(x);
            z0 = z0.min(z);
            x1 = x1.max(x);
            z1 = z1.max(z);
        }
    }
    if x0 > x1 {
        notice(s, "Nothing explored here to save".into());
        return;
    }
    let (w, h) = (((x1 - x0 + 1) * 16) as u32, ((z1 - z0 + 1) * 16) as u32);
    let at = m::MapView {
        x: f64::from(x0 * 16) + f64::from(w) / 2.0,
        z: f64::from(z0 * 16) + f64::from(h) / 2.0,
        zoom: m::MAP_ZOOM_ONE,
    };
    let mut image = Vec::new();
    m::compose(|x, z| e.store.column(x, z), &MAP_COLORS, look(settings), &at, w, h, 1.0, &mut image);
    let dir = base(game).join("exports");
    let world = s
        .world_dir
        .as_ref()
        .and_then(|d| d.strip_prefix(base(game)).ok())
        .map_or_else(|| "map".to_owned(), |p| m::folder_name(&p.to_string_lossy()));
    let name = format!("{world}-{}-{}.png", m::folder_name(e.dim), now_secs());
    let path = dir.join(&name);
    let _ = std::thread::Builder::new()
        .name("lumos-map-export".into())
        .spawn(move || {
            let png = m::png(w, h, &image);
            let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, png));
        });
    notice(s, format!("Saved lumos-map/exports/{name} ({w} x {h})"));
}

/// The minimap's blocks per GUI pixel.
fn mini_bpp(settings: &m::Settings, cave: bool) -> f32 {
    settings.bpp_now(cave, with(|s| s.big))
}

/// The minimap's side in GUI pixels: its size, or most of the screen while enlarged.
fn mini_size(game: &Game<'_>, settings: &m::Settings, big: bool) -> f32 {
    if big {
        let (w, h) = game.gui_size();
        (w.min(h) * 0.8).floor().max(m::SIZES[settings.size])
    } else {
        m::SIZES[settings.size]
    }
}

/// Underground: the cave layer under the head, kept scanned around the player.
fn follow_cave(game: &Game<'_>, s: &mut State, settings: &m::Settings) {
    let (Some(p), Some(head)) = (player_pos(game), head_block(game)) else {
        return;
    };
    follow_roof(game, s, settings);
    s.cave_view = m::cave_view(settings, head, s.cave_switch.on, roof_top(game));
    let mode = match s.cave_view {
        Some(m::CaveView::Slice { start, depth, above }) => Scan::Cave { start, depth, above },
        Some(m::CaveView::RoofTop) => Scan::Top,
        None => {
            s.cave = None;
            return;
        }
    };
    let size = mini_size(game, settings, s.big);
    let reach = size / 2.0 * settings.bpp_now(true, s.big) * std::f32::consts::SQRT_2;
    let radius = ((reach / 16.0).ceil() as i32 + 1).min(game.settings.render_distance.max(2));
    let centre = ((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4);
    follow_layer(game, s, false, mode, centre, radius, CAVE_BUDGET);
}

/// A saved cave layer of the dimension shown (`band` None: the full cave map).
fn saved(s: &State, band: Option<i32>) -> Option<&Saved> {
    s.explored.as_ref()?.caves.get(&band)
}

/// How far round you (chunks) the light overlay looks, and how often it looks again.
const LIGHT_RADIUS: i32 = 2;
const LIGHT_EVERY: Duration = Duration::from_millis(500);

/// The light overlay's marks, twice a second: the floors near you (the cave view's in a cave)
/// whose block light over them is at most the setting's, where mobs can spawn. Water, rock and
/// drops are left out.
fn follow_light(game: &Game<'_>, s: &mut State, settings: &m::Settings) {
    if !settings.light_overlay {
        s.light_mask.clear();
        return;
    }
    if s.light_at.is_some_and(|t| t.elapsed() < LIGHT_EVERY) {
        return;
    }
    s.light_at = Some(Instant::now());
    let Some(p) = player_pos(game) else {
        return;
    };
    let max = m::LIGHT_MAXES[settings.light_max.min(m::LIGHT_MAXES.len() - 1)] as u8;
    let centre = ((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4);
    let cave = s.cave.as_ref().filter(|_| s.cave_view.is_some());
    let mut mask = HashMap::new();
    for dz in -LIGHT_RADIUS..=LIGHT_RADIUS {
        for dx in -LIGHT_RADIUS..=LIGHT_RADIUS {
            let (cx, cz) = (centre.0 + dx, centre.1 + dz);
            if !game.world.chunks.contains_key(&(cx, cz)) {
                continue;
            }
            let mut rows = [0u16; 16];
            for (lz, row) in rows.iter_mut().enumerate() {
                for lx in 0..16 {
                    let (x, z) = (cx * 16 + lx, cz * 16 + lz as i32);
                    let col = match cave {
                        Some(l) => l.column(x, z),
                        None => s.explored.as_ref().and_then(|e| e.store.column(x, z)),
                    };
                    let Some(c) = col.filter(|c| c.kind == m::FLOOR && c.depth == 0 && c.color != m::WATER) else {
                        continue;
                    };
                    if game.world.light(x, i32::from(c.height) + 1, z) & 15 <= max {
                        *row |= 1 << lx;
                    }
                }
            }
            if rows.iter().any(|r| *r != 0) {
                mask.insert((cx, cz), rows);
            }
        }
    }
    s.light_mask = mask;
}

/// The minimap's overlays over its terrain: dark floors (the light overlay), slime chunks and
/// the chunk grid, each turned with the map and cut to its outline.
fn draw_overlays(
    game: &Game<'_>,
    batch: &mut HudBatch,
    s: &State,
    settings: &m::Settings,
    area: [f64; 4],
    to_screen: &dyn Fn(f64, f64) -> [f32; 2],
    shape: &m::Shape,
) {
    let outline = shape.polygon();
    let mut fill = |corners: [[f32; 2]; 4], argb: u32| {
        let poly = corners.map(|c| [c[0], c[1], 0.0, 0.0]);
        for q in shape.cut(&poly, &outline) {
            game.native_quad(batch, q.map(|v| [v[0], v[1]]), argb);
        }
    };
    let (cx0, cz0) = ((area[0] / 16.0).floor() as i32, (area[1] / 16.0).floor() as i32);
    let (cx1, cz1) = ((area[2] / 16.0).floor() as i32, (area[3] / 16.0).floor() as i32);
    let rect = |x0: f64, z0: f64, x1: f64, z1: f64| [to_screen(x0, z0), to_screen(x1, z0), to_screen(x1, z1), to_screen(x0, z1)];
    if settings.light_overlay {
        for (&(cx, cz), rows) in &s.light_mask {
            for (lz, row) in rows.iter().enumerate() {
                for (x, n) in m::runs(*row) {
                    let (x0, z0) = (f64::from(cx * 16 + x), f64::from(cz * 16 + lz as i32));
                    fill(rect(x0, z0, x0 + f64::from(n), z0 + 1.0), 0x70FF_3030);
                }
            }
        }
    }
    if settings.mini_slime
        && let Some(seed) = game.native_world_seed()
    {
        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                if m::slime_chunk(seed, cx, cz) {
                    let (x0, z0) = (f64::from(cx * 16), f64::from(cz * 16));
                    fill(rect(x0, z0, x0 + 16.0, z0 + 16.0), 0x5030_E040);
                }
            }
        }
    }
    if settings.mini_grid {
        let (z0, z1) = (f64::from(cz0 * 16), f64::from((cz1 + 1) * 16));
        let (x0, x1) = (f64::from(cx0 * 16), f64::from((cx1 + 1) * 16));
        for cx in cx0..=cx1 + 1 {
            let x = f64::from(cx * 16);
            fill(m::line_quad(to_screen(x, z0), to_screen(x, z1), 1.0), 0x50FF_FFFF);
        }
        for cz in cz0..=cz1 + 1 {
            let z = f64::from(cz * 16);
            fill(m::line_quad(to_screen(x0, z), to_screen(x1, z), 1.0), 0x50FF_FFFF);
        }
    }
}

/// A source's epoch for its cells: they are drawn again when it changes (a live layer's, with
/// its band's saved layer under it).
fn epoch_of(s: &State, source: u8) -> u64 {
    let layer = |l: &Layer| l.epoch + l.band().and_then(|b| saved(s, Some(b))).map_or(0, |c| c.store.epoch);
    let own = match source {
        0 => s.explored.as_ref().map_or(0, |e| e.store.epoch),
        1 => s.cave.as_ref().map_or(0, layer),
        2 => s.slice.as_ref().map_or(0, layer),
        4 => s.viewed.as_ref().map_or(0, |v| v.store.epoch),
        _ => saved(s, None).map_or(0, |c| c.store.epoch),
    };
    s.cells_gen << 32 | own
}

/// Cell upkeep once a frame: cells of changed chunks marked, the cells the maps asked for last
/// frame drawn and uploaded within `budget`, and a new frame begun.
fn draw_wanted_cells(game: &mut Game<'_>, s: &mut State, settings: &m::Settings, budget: Duration) {
    let started = Instant::now();
    // A rebuilt atlas (resources reloaded) or another look: every cell again.
    let rect = game.gui.atlas.sprites.get(MOD_IMAGE_SPRITE).map(|r| [r.u0, r.v0, r.u1, r.v1]);
    let Some(rect) = rect else {
        return;
    };
    let now = (rect, look(settings));
    if s.cells_for != Some(now) {
        s.cells_for = Some(now);
        s.cells.clear();
        s.cells_gen += 1;
    }
    if let Some(e) = s.explored.as_mut() {
        for k in std::mem::take(&mut e.store.changed) {
            s.cells.chunk_changed(0, k);
        }
        for (band, c) in e.caves.iter_mut() {
            for k in std::mem::take(&mut c.store.changed) {
                if band.is_some() {
                    s.cells.chunk_changed(1, k);
                    s.cells.chunk_changed(2, k);
                } else {
                    s.cells.chunk_changed(3, k);
                }
            }
        }
    }
    if let Some(l) = s.cave.as_mut() {
        for k in std::mem::take(&mut l.changed) {
            s.cells.chunk_changed(1, k);
        }
    }
    if let Some(l) = s.slice.as_mut() {
        for k in std::mem::take(&mut l.changed) {
            s.cells.chunk_changed(2, k);
        }
    }
    let todo = s.cells.take_wanted(1024);
    let look = look(settings);
    let mut px = [0u8; 1024];
    for key in todo {
        if started.elapsed() >= budget {
            break;
        }
        let empty = match key.source {
            0 => match s.explored.as_ref() {
                Some(e) => m::compose_cell(|x, z| e.store.column(x, z), &MAP_COLORS, look, None, key, &mut px),
                None => continue,
            },
            1 | 2 => match if key.source == 1 { s.cave.as_ref() } else { s.slice.as_ref() } {
                Some(l) => {
                    // Live tiles over the band's saved ones (caves explored before, unloaded).
                    let kept = l.band().and_then(|b| saved(s, Some(b)));
                    let column = |x, z| l.column(x, z).or_else(|| kept.and_then(|c| c.store.column(x, z)));
                    m::compose_cell(column, &MAP_COLORS, look, l.shade(), key, &mut px)
                }
                None => continue,
            },
            4 => match s.viewed.as_ref() {
                Some(v) => m::compose_cell(|x, z| v.store.column(x, z), &MAP_COLORS, look, None, key, &mut px),
                None => continue,
            },
            _ => match saved(s, None) {
                Some(c) => m::compose_cell(|x, z| c.store.column(x, z), &MAP_COLORS, look, None, key, &mut px),
                None => continue,
            },
        };
        let epoch = epoch_of(s, key.source);
        let Some(slot) = s.cells.place(key, epoch, empty) else {
            break;
        };
        if !empty {
            upload(game, m::slot_origin(slot), m::CELL, m::CELL, px.to_vec());
        }
    }
    s.cells.next_frame();
}

/// `native_frame`: the world followed, chunks explored, a death marked, the cells kept.
pub(super) fn on_frame(game: &mut Game<'_>) {
    if !game.native_on(m::ID) || game.player.is_none() {
        return;
    }
    let open = map_open(game);
    let dead = game.screen == Screen::Dead;
    let settings = settings(game);
    // A waypoint shared from the map: the map closes and the chat opens with it typed.
    if let Some(text) = with(|s| s.pending_chat.take()) {
        game.native_mod_screen_close(m::ID);
        let _ = game.native_open_chat(&text);
        return;
    }
    if let Some(text) = with(|s| s.pending_clipboard.take()) {
        use horizon_client::clipboard::Clipboard;
        game.clipboard.set(&text);
        game.native_client_action_bar(Text::literal(format!("Copied {text}")), false);
    }
    if let Some(said) = with(|s| s.pending_close.take()) {
        game.native_mod_screen_close(m::ID);
        if let Some((text, rgb)) = said {
            game.native_client_action_bar(Text::colored(text, rgb), false);
        }
        return;
    }
    if let Some(command) = with(|s| s.pending_command.take()) {
        game.native_mod_screen_close(m::ID);
        game.send_command(&command);
        return;
    }
    let held = keys::held(game, keys::ENLARGE);
    let mut reached = None;
    with(|s| {
        s.big = settings.minimap && if settings.enlarge_toggle { s.big_toggled } else { held && game.native_playing() };
        follow_colours(game, s);
        if let Some(p) = s.palette.as_mut() {
            p.blend = settings.blend;
        }
        follow_world(game, s);
        follow_spawn(game, s);
        explore(game, s, if open { BUDGET_OPEN } else { BUDGET });
        if std::mem::take(&mut s.pending_export) {
            export_png(game, s, &settings);
        }
        if dead && !s.was_dead && settings.death && s.world_dir.is_some() {
            if let Some(p) = player_pos(game) {
                s.book.add_death(block_of(p), game.dim.id(), m::DEATHS[settings.deaths], now_secs());
                save_book(s);
            }
        }
        s.was_dead = dead;
        // Waypoints that go when reached (and death points, walked back to).
        if !dead
            && s.reached_at.is_none_or(|t| t.elapsed() >= Duration::from_millis(500))
            && let Some(p) = player_pos(game)
        {
            s.reached_at = Some(Instant::now());
            let gone = s.book.remove_reached(p, game.dim.id(), settings.death_reached, now_secs(), 10);
            if !gone.is_empty() {
                s.screen.selected.clear();
                save_book(s);
                reached = Some(format!("Reached {}", gone.join(", ")));
            }
        }
        if open {
            if s.screen.page == m::Page::Map {
                follow_map(game, s, &settings);
            }
        } else if settings.minimap && !game.ui.hide_gui {
            follow_cave(game, s, &settings);
            follow_light(game, s, &settings);
        }
        draw_wanted_cells(game, s, &settings, if open { CELL_BUDGET_OPEN } else { CELL_BUDGET });
        s.icons.follow(game);
    });
    if let Some(text) = reached {
        game.native_client_action_bar(Text::literal(text), false);
    }
}

/// The client left its level: the explored map is saved and let go.
pub(super) fn on_level_unload(game: &mut Game<'_>) {
    with(|s| {
        // Temporary waypoints go with the world.
        if s.book.drop_temporary() {
            save_book(s);
        }
        if let Some(mut e) = s.explored.take() {
            e.save();
        }
        s.cave = None;
        s.cave_view = None;
        s.viewed = None;
        s.cave_switch = m::CaveSwitch::default();
        s.cells.clear();
        s.cells_gen += 1;
        s.world_asked = None;
        s.biome = None;
        s.was_dead = false;
        s.slice = None;
        s.spawn_checked = None;
    });
    let _ = game;
}

/// Where the cells of `source` at `lod` covering world `x0..x1, z0..z1` land on screen
/// (`to_screen` maps a world point), cut to the convex `clip`, drawn `alpha` opaque. Cells not
/// drawn yet are asked for.
#[allow(clippy::too_many_arguments)]
fn draw_cells(
    game: &Game<'_>,
    s: &mut State,
    batch: &mut HudBatch,
    source: u8,
    lod: u8,
    area: [f64; 4],
    to_screen: &dyn Fn(f64, f64) -> [f32; 2],
    shape: &m::Shape,
    alpha: u8,
) {
    let clip = shape.polygon();
    let Some(r) = game.gui.atlas.sprites.get(MOD_IMAGE_SPRITE).copied() else {
        return;
    };
    let (du, dv) = ((r.u1 - r.u0) / r.w as f32, (r.v1 - r.v0) / r.h as f32);
    let span = f64::from(16i32 << lod);
    let cell = |v: f64| (v / span).floor() as i32;
    let epoch = epoch_of(s, source);
    let colour = rgba(255, 255, 255, alpha);
    // A hair inside each cell, so nearest sampling never reads the next one.
    let eps = 0.01;
    for cz in cell(area[1])..=cell(area[3]) {
        for cx in cell(area[0])..=cell(area[2]) {
            let key = m::CellKey { source, lod, x: cx, z: cz };
            let Some((slot, empty)) = s.cells.get(key, epoch) else {
                continue;
            };
            if empty {
                continue;
            }
            let [ox, oy] = m::slot_origin(slot);
            let (u0, v0) = (r.u0 + (ox as f32 + eps) * du, r.v0 + (oy as f32 + eps) * dv);
            let (u1, v1) = (r.u0 + (ox as f32 + 16.0 - eps) * du, r.v0 + (oy as f32 + 16.0 - eps) * dv);
            let (x0, z0) = (f64::from(cx) * span, f64::from(cz) * span);
            let p = [to_screen(x0, z0), to_screen(x0 + span, z0), to_screen(x0 + span, z0 + span), to_screen(x0, z0 + span)];
            let poly = [
                [p[0][0], p[0][1], u0, v0],
                [p[1][0], p[1][1], u1, v0],
                [p[2][0], p[2][1], u1, v1],
                [p[3][0], p[3][1], u0, v1],
            ];
            for q in shape.cut(&poly, &clip) {
                batch.quad(q.map(|v| [v[0], v[1]]), q.map(|v| [v[2], v[3]]), colour);
            }
        }
    }
}

/// A convex polygon filled with `argb`.
fn fill_polygon(game: &Game<'_>, batch: &mut HudBatch, poly: &[[f32; 2]], argb: u32) {
    let verts: Vec<m::Vert> = poly.iter().map(|p| [p[0], p[1], 0.0, 0.0]).collect();
    for q in m::fan_quads(&verts) {
        game.native_quad(batch, q.map(|v| [v[0], v[1]]), argb);
    }
}

/// `native_key`: M opens (or closes) the world map, B opens the "New waypoint" box (or, with
/// "B adds instantly", adds one where you stand).
pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || !game.native_on(m::ID) || game.player.is_none() {
        return;
    }
    if map_open(game) {
        let typing = with(|s| s.screen.typing());
        if !typing && keys::pressed(game, keys::MAP, key, true) {
            game.native_mod_screen_close(m::ID);
        }
        return;
    }
    if !game.native_playing() {
        return;
    }
    let page = if keys::pressed(game, keys::MAP, key, true) {
        Some(m::Page::Map)
    } else if keys::pressed(game, keys::WAYPOINT_LIST, key, true) {
        Some(m::Page::List { scroll: 0 })
    } else if keys::pressed(game, keys::SETTINGS, key, true) {
        Some(m::Page::Settings { tab: 0 })
    } else {
        None
    };
    if let Some(page) = page {
        let Some(p) = player_pos(game) else {
            return;
        };
        let set = settings(game);
        if game.native_mod_screen_open(m::ID) {
            with(|s| {
                s.screen.view.x = p[0];
                s.screen.view.z = p[2];
                s.screen.page = page;
                s.screen.press = None;
                s.screen.follow = set.map_follow;
                s.screen.grid = set.map_grid;
                s.viewed = None;
                // A layered cave map opens at your head.
                s.screen.slice = (cave_type(game, s) == m::CaveType::Layered).then(|| head_block(game)).flatten();
                let (lo, hi) = set.map_zooms();
                s.screen.view.zoom = s.screen.view.zoom.clamp(lo, hi);
            });
        }
    } else if keys::pressed(game, keys::WAYPOINT, key, true) {
        let Some(p) = player_pos(game) else {
            return;
        };
        let dim = game.dim.id();
        let set = settings(game);
        if !set.instant_add {
            // The box, prefilled with the block you stand in, the next name, the current set.
            let draft = with(|s| {
                s.world_dir.as_ref()?;
                let mut wp = s.book.draft(block_of(p), dim, now_secs());
                m::style_new(&mut wp, set.wp_colour, set.wp_symbol);
                Some(wp)
            });
            let Some(wp) = draft else {
                return;
            };
            if game.native_mod_screen_open(m::ID) {
                let typed = key.strip_prefix("key.keyboard.").and_then(|k| {
                    let mut c = k.chars();
                    c.next().filter(|_| c.next().is_none())
                });
                with(|s| {
                    s.screen.view.x = p[0];
                    s.screen.view.z = p[2];
                    s.screen.press = None;
                    s.screen.searching = false;
                    s.screen.page = m::Page::new_waypoint(wp, true);
                    s.swallow = typed.map(|c| (c, Instant::now()));
                });
            }
            return;
        }
        let added = with(|s| {
            s.world_dir.as_ref()?;
            let i = s.book.add(block_of(p), dim, now_secs());
            s.book.style_new(i, set.wp_colour, set.wp_symbol);
            save_book(s);
            let w = &s.book.list[i];
            Some((w.name.clone(), w.pos, w.rgb()))
        });
        if let Some((name, pos, rgb)) = added {
            let text = format!("Added {name} at {} {} {}", pos[0], pos[1], pos[2]);
            game.native_client_action_bar(Text::colored(text, rgb), false);
        }
    } else {
        // The minimap's own keys: zoom, cave view, shown.
        let mut s = settings(game);
        let said = if keys::pressed(game, keys::ZOOM_IN, key, true) {
            s.zoom_step(true).then(|| format!("Minimap zoom: {} block(s) a pixel", m::ZOOMS[s.zoom]))
        } else if keys::pressed(game, keys::ZOOM_OUT, key, true) {
            s.zoom_step(false).then(|| format!("Minimap zoom: {} block(s) a pixel", m::ZOOMS[s.zoom]))
        } else if s.fair_play && (keys::pressed(game, keys::CAVE, key, true) || keys::pressed(game, keys::CAVE_HERE, key, true)) {
            Some("Fair play is on: no cave view".to_owned())
        } else if keys::pressed(game, keys::CAVE, key, true) {
            s.cave = (s.cave + 1) % m::CAVE_MODES.len();
            // Manual starts where you are.
            if s.cave == 1 {
                s.cave_start = head_block(game);
            }
            Some(match (s.cave, s.cave_start) {
                (1, Some(y)) => format!("Cave view: Manual from Y {y}"),
                _ => format!("Cave view: {}", m::CAVE_MODES[s.cave]),
            })
        } else if keys::pressed(game, keys::CAVE_HERE, key, true) {
            s.cave = 1;
            s.cave_start = head_block(game);
            s.cave_start.map(|y| format!("Cave view: Manual from Y {y}"))
        } else if keys::pressed(game, keys::NEXT_SET, key, true) {
            let name = with(|st| {
                st.book.next_set();
                save_book(st);
                st.book.current_set().to_owned()
            });
            game.native_client_action_bar(Text::literal(format!("Waypoint set: {name}")), false);
            return;
        } else if keys::pressed(game, keys::ALL_SETS, key, true) {
            let all = with(|st| {
                st.book.all_sets = !st.book.all_sets;
                st.book.all_sets
            });
            let text = if all { "Waypoints: every set shown" } else { "Waypoints: each set as set" };
            game.native_client_action_bar(Text::literal(text), false);
            return;
        } else if keys::pressed(game, keys::RADAR, key, true) {
            s.radar = !s.radar;
            Some(format!("Entities on the minimap: {}", if s.radar { "Shown" } else { "Hidden" }))
        } else if keys::pressed(game, keys::GRID, key, true) {
            s.mini_grid = !s.mini_grid;
            Some(format!("Minimap chunk grid: {}", if s.mini_grid { "On" } else { "Off" }))
        } else if keys::pressed(game, keys::SLIME, key, true) {
            s.mini_slime = !s.mini_slime;
            Some(format!("Minimap slime chunks: {}", if s.mini_slime { "On" } else { "Off" }))
        } else if keys::pressed(game, keys::LIGHT, key, true) {
            s.light_overlay = !s.light_overlay;
            Some(format!("Light overlay: {}", if s.light_overlay { "On" } else { "Off" }))
        } else if keys::pressed(game, keys::ENLARGE, key, true) {
            if s.enlarge_toggle {
                with(|st| st.big_toggled = !st.big_toggled);
            }
            None
        } else if keys::pressed(game, keys::WAYPOINTS, key, true) {
            s.waypoints = !s.waypoints;
            Some(format!("Waypoints: {}", if s.waypoints { "Shown" } else { "Hidden" }))
        } else if keys::pressed(game, keys::MINIMAP, key, true) {
            s.minimap = !s.minimap;
            Some(format!("Minimap: {}", if s.minimap { "Shown" } else { "Hidden" }))
        } else {
            None
        };
        if let Some(text) = said {
            m::set_settings(s);
            for (k, v) in s.stored() {
                game.native_set_setting(m::ID, k, &v);
            }
            game.native_client_action_bar(Text::literal(text), false);
        }
    }
}

/// `native_client_chat_received`: coordinates in chat are kept for the waypoint list's "From
/// chat" (the chat's own text can't be clicked from a mod).
pub(super) fn on_chat(game: &Game<'_>, overlay: bool, message: &Text) {
    if overlay || !game.native_on(m::ID) {
        return;
    }
    let line = message.plain();
    if let Some(at) = m::chat_coords(&line) {
        let short: String = line.chars().take(40).collect();
        with(|s| {
            s.chat.push_back((short, at));
            while s.chat.len() > 5 {
                s.chat.pop_front();
            }
        });
    }
}

fn corner(i: usize) -> Corner {
    match i {
        0 => Corner::TopLeft,
        1 => Corner::TopRight,
        2 => Corner::BottomLeft,
        _ => Corner::BottomRight,
    }
}

/// `native_hud`: the minimap, with the mobs and players near you and your waypoints over it,
/// and its info lines under it.
pub(super) fn on_hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    if !game.native_on(m::ID) || map_open(game) {
        return;
    }
    let s = settings(game);
    let Some(p) = player_pos(game) else {
        return;
    };
    draw_waypoint_markers(game, batch, &s);
    if !s.minimap {
        return;
    }
    let at = corner(s.corner);
    if !with(|st| st.big) && !game.native_corner_shown(at) {
        return;
    }
    let big = with(|st| st.big);
    // Enlarged: centred, most of the screen, north up unless it keeps turning.
    let mut s = s;
    if big {
        s.rotate &= s.enlarge_rotate;
    }
    let s = s;
    let size = mini_size(game, &s, big);
    let bpp = mini_bpp(&s, with(|st| st.cave_view.is_some()));
    let block = block_of(p);
    let mut lines: Vec<String> = Vec::new();
    for letter in s.info_letters() {
        match letter {
            'C' if s.coords && !s.hide_coords => lines.push(format!("{} {} {}", block[0], block[1], block[2])),
            'B' if s.biome => {
                let biome = with(|st| match &st.biome {
                    Some((b, name)) if *b == block => Some(name.clone()),
                    _ => None,
                })
                .or_else(|| {
                    let name = m::pretty_id(&game.native_biome(block)?);
                    with(|st| st.biome = Some((block, name.clone())));
                    Some(name)
                });
                if let Some(b) = biome {
                    lines.push(b);
                }
            }
            'F' if s.facing => lines.push(m::facing(game.yaw)),
            'L' if s.light => {
                let l = game.world.light(block[0], block[1], block[2]);
                lines.push(format!("Light {} (sky {})", l & 15, l >> 4));
            }
            'T' if s.time => lines.push(m::game_clock(game.world.day_time, s.clock_12h)),
            'K' if s.chunk && !s.hide_coords => lines.push(m::chunk_line(block)),
            'O' if s.other_coords && !s.hide_coords => {
                let other = match game.dim.id() {
                    m::OVERWORLD => m::converted(block, m::OVERWORLD, m::NETHER).map(|b| ("Nether", b)),
                    m::NETHER => m::converted(block, m::NETHER, m::OVERWORLD).map(|b| ("Overworld", b)),
                    _ => None,
                };
                if let Some((name, b)) = other {
                    lines.push(format!("{name} {} {} {}", b[0], b[1], b[2]));
                }
            }
            'A' if s.angles => lines.push(format!("Yaw {:.1} Pitch {:.1}", m::wrap_degrees(game.yaw), game.pitch)),
            'W' if s.weather => lines.push(m::weather(game.world.weather.rain_level.1, game.world.weather.thunder_level.1).to_owned()),
            'R' if s.real_time => {
                use chrono::Timelike;
                let now = chrono::Local::now();
                lines.push(m::hm(now.hour(), now.minute(), s.clock_12h));
            }
            'D' if s.dimension => lines.push(m::pretty_id(game.dim.id())),
            _ => {}
        }
    }
    // Room for the frame round the map.
    let pad = 3.0;
    let info_h = lines.len() as f32 * 10.0;
    let (ox, oy) = (m::OFFSETS[s.offset_x.min(m::OFFSETS.len() - 1)], m::OFFSETS[s.offset_y.min(m::OFFSETS.len() - 1)]);
    let (x, y0, top) = if big {
        let (w, h) = (batch.width, batch.height);
        ((w - size) / 2.0, ((h - size - info_h) / 2.0 - pad).max(0.0), true)
    } else {
        let (x, y0) = corners.place(at, size + pad * 2.0 + ox, size + pad * 2.0 + oy + info_h + if lines.is_empty() { 0.0 } else { 1.0 });
        let left = matches!(at, Corner::TopLeft | Corner::BottomLeft);
        let x = x + pad + if left { ox } else { 0.0 };
        let y0 = y0 + if matches!(at, Corner::TopLeft | Corner::TopRight) { oy } else { 0.0 };
        (x, y0, matches!(at, Corner::TopLeft | Corner::TopRight))
    };
    let (map_y, text_y) = if top { (y0 + pad, y0 + size + pad * 2.0 + 1.0) } else { (y0 + info_h + 1.0 + pad, y0) };
    let alpha = m::OPACITY[s.opacity];
    let (cx, cy) = (x + size / 2.0, map_y + size / 2.0);
    // The map moves smoothly between ticks: the player where this frame draws them.
    let p = {
        let pl = game.player.as_ref().map(|pl| pl.position);
        match pl {
            Some(now) => {
                let at = game.previous_position.lerp(now, game.clock.camera_fraction());
                [at.x, at.y, at.z]
            }
            None => p,
        }
    };
    let yaw = game.yaw;
    let to_map = |wx: f64, wz: f64| m::minimap_point(wx - p[0], wz - p[2], yaw, s.rotate, bpp);
    draw_minimap_terrain(game, batch, &s, p, [cx, cy], size, alpha);
    for (i, line) in lines.iter().enumerate() {
        let w = game.native_text_width(line);
        game.native_text(batch, line, [x + (size - w) / 2.0, text_y + i as f32 * 10.0], 0xFFFFFF, true);
    }
    let radius = size / 2.0 - 2.0;
    // Items, mobs and players: their faces (or dots).
    draw_entities(game, batch, &s, p, [cx, cy], radius, &to_map);
    // Your own arrow.
    let turn = if s.rotate { 180.0 } else { yaw };
    game.native_quad(batch, m::arrow([cx, cy], turn, 5.0), 0xFF00_0000);
    game.native_quad(batch, m::arrow([cx, cy], turn, 3.8), 0xFFFF_FFFF);
    // Waypoints: on the rim when they are farther than the map reaches.
    let dim = game.dim.id();
    with(|st| {
        if !s.waypoints || !s.wp_minimap {
            return;
        }
        for w in st.book.list.iter().filter(|w| w.on_minimap && st.book.visible(w)) {
            let Some(c) = w.place_in(dim, s.convert) else {
                continue;
            };
            let ((u, v), rim) = m::clamp_to_map(to_map(c[0], c[2]), radius - 3.0, s.round);
            let (wx, wy) = ((cx + u).round(), (cy + v).round());
            let r = if rim { 2.5 } else { 3.5 };
            let dia = |r: f32| [[wx, wy - r], [wx + r, wy], [wx, wy + r], [wx - r, wy]];
            game.native_quad(batch, dia(r + 1.2), 0xE000_0000);
            game.native_quad(batch, dia(r), 0xFF00_0000 | w.rgb());
        }
    });
    if s.compass > 0 {
        let reach = radius - 3.0;
        for (letter, dx, dy) in m::compass(game.yaw, s.rotate) {
            if s.compass == 1 && letter != 'N' {
                continue;
            }
            let t = letter.to_string();
            let w = game.native_text_width(&t);
            let (tx, ty) = (cx + dx * reach - w / 2.0, cy + dy * reach - 4.0);
            let colour = if letter == 'N' { 0xFF5555 } else { 0xE0E0E0 };
            game.native_text(batch, &t, [tx.round(), ty.round()], colour, true);
        }
    }
}

/// Entity icons: each type's face (a player's skin face), cut once from the texture the game
/// draws it with and kept in the mod image slot's icon row.
#[derive(Default)]
struct Icons {
    /// The game's entity models (`entity-models.json`), read off the frame once.
    models: Option<horizon_client::mobs::EntityModelDocument>,
    loading: Option<Receiver<Option<horizon_client::mobs::EntityModelDocument>>>,
    /// Per texture and model: its slot and size, or None (no face: a dot).
    faces: HashMap<(String, String), Option<(usize, u32, u32)>>,
    next: usize,
    /// The slot's place in the atlas the icons were uploaded to.
    for_rect: Option<[f32; 4]>,
}

/// New icons cut a frame at most (each decodes a texture).
const ICONS_PER_FRAME: usize = 2;

/// Every face of `part` and its children named `name` (the first found), as runtime faces.
fn part_faces(part: &horizon_client::mobs::Part, name: &str) -> Option<Vec<m::Face>> {
    if part.name == name {
        return Some(
            part.polygons
                .iter()
                .map(|p| (p.normal, p.vertices.map(|v| [v[3], v[4]])))
                .collect(),
        );
    }
    part.children.iter().find_map(|c| part_faces(c, name))
}

fn all_faces(part: &horizon_client::mobs::Part, out: &mut Vec<m::Face>) {
    out.extend(part.polygons.iter().map(|p| (p.normal, p.vertices.map(|v| [v[3], v[4]]))));
    for c in &part.children {
        all_faces(c, out);
    }
}

impl Icons {
    /// The icon of an entity drawn with `texture` and model `model` (slot, width, height):
    /// cut now if there is budget left this frame, else asked again next frame.
    fn get(&mut self, game: &mut Game<'_>, texture: &str, model: &str, budget: &mut usize) -> Option<(usize, u32, u32)> {
        let key = (texture.to_owned(), model.to_owned());
        if let Some(found) = self.faces.get(&key) {
            return *found;
        }
        if *budget == 0 {
            return None;
        }
        let doc = self.models.as_ref()?;
        *budget -= 1;
        let made = (|| {
            let part = doc.model(&m::model_key(model))?;
            let head = part_faces(&part, "head").and_then(|f| m::front(&f));
            let rect = head.or_else(|| {
                let mut all = Vec::new();
                all_faces(&part, &mut all);
                m::front(&all)
            })?;
            let hat = part_faces(&part, "hat").and_then(|f| m::front(&f));
            let tex = game.native_client_texture(texture)?;
            m::cut_icon(tex.width, tex.height, &tex.rgba, rect, hat)
        })();
        let entry = made.and_then(|(w, h, cell)| {
            if self.next >= m::ICON_SLOTS {
                // Full: start over (rare: hundreds of kinds and skins).
                self.faces.clear();
                self.next = 0;
            }
            let slot = self.next;
            self.next += 1;
            upload(game, m::icon_origin(slot), m::ICON, m::ICON, cell).then_some((slot, w, h))
        });
        self.faces.insert(key, entry);
        entry
    }

    /// Keeps the models loading and forgets the icons when the atlas was rebuilt.
    fn follow(&mut self, game: &Game<'_>) {
        let rect = game.gui.atlas.sprites.get(MOD_IMAGE_SPRITE).map(|r| [r.u0, r.v0, r.u1, r.v1]);
        if rect != self.for_rect {
            self.for_rect = rect;
            self.faces.clear();
            self.next = 0;
        }
        if let Some(rx) = &self.loading
            && let Ok(doc) = rx.try_recv()
        {
            self.loading = None;
            self.models = doc;
        }
        if self.models.is_none() && self.loading.is_none() {
            let (tx, rx) = mpsc::channel();
            let root = game.options.resources.clone();
            let _ = std::thread::Builder::new()
                .name("lumos-map-models".into())
                .spawn(move || {
                    let _ = tx.send(horizon_client::mobs::EntityModelDocument::load(&root).ok());
                });
            self.loading = Some(rx);
        }
    }
}

/// Draws icon `icon` centred on `at`, `size` GUI pixels for its longer side, on a dark outline.
fn draw_icon(game: &Game<'_>, batch: &mut HudBatch, icon: (usize, u32, u32), at: [f32; 2], size: f32, alpha: u8) {
    let Some(r) = game.gui.atlas.sprites.get(MOD_IMAGE_SPRITE).copied() else {
        return;
    };
    let (slot, iw, ih) = icon;
    let k = size / iw.max(ih) as f32;
    let (w, h) = (iw as f32 * k, ih as f32 * k);
    let (x, y) = ((at[0] - w / 2.0).round(), (at[1] - h / 2.0).round());
    game.native_fill(batch, [x - 1.0, y - 1.0, w + 2.0, h + 2.0], u32::from(alpha / 4 * 3) << 24);
    let (du, dv) = ((r.u1 - r.u0) / r.w as f32, (r.v1 - r.v0) / r.h as f32);
    let [ox, oy] = m::icon_origin(slot);
    let mut src = r;
    src.u0 = r.u0 + (ox as f32 + 0.01) * du;
    src.v0 = r.v0 + (oy as f32 + 0.01) * dv;
    src.u1 = r.u0 + (ox as f32 + iw as f32 - 0.01) * du;
    src.v1 = r.v0 + (oy as f32 + ih as f32 - 0.01) * dv;
    batch.rect_uv(x, y, w, h, src, rgba(255, 255, 255, alpha));
}

/// A small round dot (an octagon, three quads) with a thin dark rim.
fn draw_dot(game: &Game<'_>, batch: &mut HudBatch, at: [f32; 2], r: f32, argb: u32) {
    let rim = (argb >> 24) * 3 / 4 << 24;
    let oct = |r: f32| -> [[f32; 2]; 8] {
        std::array::from_fn(|i| {
            let a = (i as f32 + 0.5) * std::f32::consts::FRAC_PI_4;
            [at[0] + r * a.cos(), at[1] + r * a.sin()]
        })
    };
    for (r, c) in [(r + 0.7, rim), (r, argb)] {
        let o = oct(r);
        game.native_quad(batch, [o[0], o[1], o[2], o[3]], c);
        game.native_quad(batch, [o[0], o[3], o[4], o[7]], c);
        game.native_quad(batch, [o[4], o[5], o[6], o[7]], c);
    }
}

/// The entities on the minimap: dropped items, then mobs, then players on top; faces, or dots.
#[allow(clippy::too_many_arguments)]
fn draw_entities(
    game: &mut Game<'_>,
    batch: &mut HudBatch,
    s: &m::Settings,
    p: [f64; 3],
    c: [f32; 2],
    radius: f32,
    to_map: &dyn Fn(f64, f64) -> (f32, f32),
) {
    if !s.radar || s.fair_play {
        return;
    }
    let size = m::ICON_SIZES[s.icon_size.min(m::ICON_SIZES.len() - 1)];
    let range = s.max_range();
    let max_dots = m::DOT_LIMITS[s.dot_limit.min(m::DOT_LIMITS.len() - 1)];
    let t = game.clock.fraction();
    let near = |x: f64, z: f64| range.is_none_or(|r| (x - p[0]).powi(2) + (z - p[2]).powi(2) <= r * r);
    // Dots, or (holding the player list key, or always) faces as faint badges: players with
    // their names, mobs without (unless "Show mob names").
    let details = s.always_details || (s.tab_details && keys::vanilla_held(game, "key.playerlist"));
    if s.items {
        let items: Vec<(String, [f64; 3])> = game
            .dropped
            .iter()
            .filter(|e| e.stack.count > 0)
            .map(|e| {
                let at = e.previous.lerp(e.position, t);
                (e.stack.item.to_string(), [at.x, at.y, at.z])
            })
            .filter(|(_, at)| near(at[0], at[2]) && s.depth_look(at[1] - p[1]).is_some())
            .take(100)
            .collect();
        for (item, at) in items {
            let (u, v) = to_map(at[0], at[2]);
            if m::clamp_to_map((u, v), radius, s.round).1 {
                continue;
            }
            let pos = [c[0] + u, c[1] + v];
            if !details {
                draw_dot(game, batch, pos, 1.5, 0xFFE0_C060);
            } else {
                let sz = (size * 0.8).max(5.0);
                game.native_item(batch, &item, [pos[0] - sz / 2.0, pos[1] - sz / 2.0], sz);
            }
        }
    }
    // (kind, texture, model, place, name)
    let mut list: Vec<(m::DotKind, String, String, [f64; 3], Option<String>)> = Vec::new();
    for mob in game.mobs.iter() {
        if list.len() >= max_dots {
            break;
        }
        if mob.removed || mob.invisible {
            continue;
        }
        let kind = m::dot_kind(&mob.kind.id, &mob.kind.category);
        if !s.shows(kind) {
            continue;
        }
        let at = mob.previous.lerp(mob.position, t);
        if !near(at.x, at.z) || s.depth_look(at.y - p[1]).is_none() {
            continue;
        }
        // Dots need no texture or name: those are only looked up for the detail view.
        if !details {
            list.push((kind, String::new(), String::new(), [at.x, at.y, at.z], None));
            continue;
        }
        let (model, texture) = horizon_client::mobs::render::body(mob);
        let name = if kind == m::DotKind::Player {
            s.names.then(|| game.remote_player_name(mob.id).map(str::to_owned)).flatten()
        } else {
            // Mobs show their faces only, unless their names are turned on.
            s.mob_names.then(|| mob.custom_name.clone().unwrap_or_else(|| m::pretty_id(&mob.kind.id)))
        };
        list.push((kind, texture.into_owned(), model.into_owned(), [at.x, at.y, at.z], name));
    }
    list.sort_by_key(|e| e.0 == m::DotKind::Player);
    let label = m::LABEL_SIZES[s.label_size.min(m::LABEL_SIZES.len() - 1)];
    let mut budget = ICONS_PER_FRAME;
    for (kind, texture, model, at, name) in list {
        let (u, v) = to_map(at[0], at[2]);
        if m::clamp_to_map((u, v), radius, s.round).1 {
            continue;
        }
        let pos = [c[0] + u, c[1] + v];
        let (a, mark) = s.depth_look(at[1] - p[1]).unwrap_or((255, None));
        let rgb = s.dot_rgb(kind);
        if !details {
            let r = if kind == m::DotKind::Player { 1.6 } else { 1.1 };
            draw_dot(game, batch, pos, r, u32::from(a) << 24 | rgb);
            if let Some(up) = mark {
                draw_depth_mark(game, batch, pos, r + 1.0, up, rgb);
            }
            continue;
        }
        let icon = with(|st| st.icons.get(game, &texture, &model, &mut budget));
        let half = size / 2.0;
        // The badge: a faint glass square in the entity's colour with a brighter rim.
        let (bx, by) = ((pos[0] - half).round() - 1.5, (pos[1] - half).round() - 1.5);
        let glow = |al: u32| (al * u32::from(a) / 255) << 24 | rgb;
        game.native_fill(batch, [bx - 1.0, by - 1.0, size + 5.0, size + 5.0], glow(0x22));
        game.native_fill(batch, [bx, by, size + 3.0, size + 3.0], glow(0x55));
        game.native_fill(batch, [bx + 1.0, by + 1.0, size + 1.0, size + 1.0], (u32::from(a) * 0x60 / 255) << 24);
        match icon {
            Some(icon) => draw_icon(game, batch, icon, pos, size, (u32::from(a) * 0xD8 / 255) as u8),
            None => draw_dot(game, batch, pos, half * 0.6, u32::from(a) << 24 | rgb),
        }
        if let Some(up) = mark {
            draw_depth_mark(game, batch, pos, half + 2.5, up, rgb);
        }
        let mut text = name.unwrap_or_default();
        if s.entity_y {
            text = format!("{text} Y {}", at[1].floor() as i32).trim().to_owned();
        }
        if !text.is_empty() {
            let w = game.native_text_width(&text) * label;
            let (tx, ty) = ((bx + size + 5.0).round(), (pos[1] - 4.0 * label).round());
            game.native_fill(batch, [tx - 1.0, ty - 1.0, w + 2.0, 9.0 * label + 1.0], 0x7000_0000);
            let start = batch.vertices.len();
            game.native_text(batch, &text, [tx, ty], 0xFFFFFF, false);
            batch.scale_since(start, tx, ty, label, label);
        }
    }
}

/// A small tick over (above you) or under (below you) an entity's dot or face.
fn draw_depth_mark(game: &Game<'_>, batch: &mut HudBatch, at: [f32; 2], gap: f32, up: bool, rgb: u32) {
    let (x, k) = (at[0], if up { -1.0 } else { 1.0 });
    let y = at[1] + k * gap;
    let tip = [x, y + k * 2.0];
    game.native_quad(batch, [tip, [x + 2.0, y], [x, y], [x - 2.0, y]], 0xFF00_0000 | rgb);
}

/// The minimap's ground: its shadow, the unexplored background, the terrain cells turned and
/// moved on the GPU (no image redrawn as you walk or turn), and its frame.
#[allow(clippy::too_many_arguments)]
fn draw_minimap_terrain(
    game: &Game<'_>,
    batch: &mut HudBatch,
    settings: &m::Settings,
    p: [f64; 3],
    c: [f32; 2],
    size: f32,
    alpha: u8,
) {
    let half = size / 2.0;
    let round = settings.round;
    let shade = |a: u32| (a * u32::from(alpha) / 255) << 24;
    // A soft shadow under it.
    if settings.frame != 0 {
    } else if round {
        for (r0, r1, a) in [(half, half + 3.0, 0x30u32), (half, half + 1.5, 0x40)] {
            for q in m::ring(c, r0, r1) {
                game.native_quad(batch, q, shade(a));
            }
        }
    } else {
        game.native_fill(batch, [c[0] - half - 2.0, c[1] - half - 1.0, size + 4.0, size + 4.0], shade(0x30));
        game.native_fill(batch, [c[0] - half - 1.0, c[1] - half, size + 2.0, size + 2.0], shade(0x40));
    }
    let outline = m::outline(c, half, round);
    let bg = m::BACKGROUNDS[settings.background.min(m::BACKGROUNDS.len() - 1)].1;
    fill_polygon(game, batch, &outline, shade(bg >> 24) | (bg & 0xFF_FFFF));
    let bpp = f64::from(mini_bpp(settings, with(|st| st.cave_view.is_some())));
    let reach = f64::from(half) * bpp * if settings.rotate { std::f64::consts::SQRT_2 } else { 1.0 } + 1.0;
    let area = [p[0] - reach, p[2] - reach, p[0] + reach, p[2] + reach];
    // A texel no smaller than about a GUI pixel, and few enough cells.
    let lod = m::lod_for(bpp, 1500, |lod| {
        let n = (2.0 * reach / f64::from(16i32 << lod)).ceil() as usize + 1;
        n * n
    });
    let yaw = game.yaw;
    let rotate = settings.rotate;
    let to_screen = |wx: f64, wz: f64| {
        let (u, v) = m::minimap_point(wx - p[0], wz - p[2], yaw, rotate, bpp as f32);
        [c[0] + u, c[1] + v]
    };
    with(|st| {
        let source = if st.cave.is_some() && st.cave_view.is_some() { 1 } else { 0 };
        let shape = if round { m::Shape::Circle(c, half) } else { m::Shape::Rect([c[0] - half, c[1] - half, size, size]) };
        draw_cells(game, st, batch, source, lod, area, &to_screen, &shape, alpha);
        draw_overlays(game, batch, st, settings, area, &to_screen, &shape);
    });
    // The frame: a dark rim with a faint light edge inside it.
    if settings.frame == 2 {
    } else if round {
        for q in m::ring(c, half - 0.5, half + 1.0) {
            game.native_quad(batch, q, shade(0xFF) | 0x00_0C0C_0E);
        }
        for q in m::ring(c, half - 1.5, half - 0.5) {
            game.native_quad(batch, q, shade(0x28) | 0x00FF_FFFF);
        }
    } else {
        let (x, y) = (c[0] - half, c[1] - half);
        let dark = shade(0xFF) | 0x00_0C0C_0E;
        game.native_fill(batch, [x - 1.0, y - 1.0, size + 2.0, 1.0], dark);
        game.native_fill(batch, [x - 1.0, y + size, size + 2.0, 1.0], dark);
        game.native_fill(batch, [x - 1.0, y, 1.0, size], dark);
        game.native_fill(batch, [x + size, y, 1.0, size], dark);
        let light = shade(0x28) | 0x00FF_FFFF;
        game.native_fill(batch, [x, y, size, 1.0], light);
        game.native_fill(batch, [x, y + 1.0, 1.0, size - 1.0], light);
    }
}

/// Waypoints on screen: a small marker in the waypoint's colour with its initials where the
/// waypoint is, its distance under it and its name over it when looked at (or always); kept at
/// the screen's edge with an arrow when it is off screen. No beams.
fn draw_waypoint_markers(game: &Game<'_>, batch: &mut HudBatch, s: &m::Settings) {
    if !s.markers || !s.waypoints {
        return;
    }
    let Some(cam) = game.native_camera() else {
        return;
    };
    let (w, h) = (batch.width, batch.height);
    let dim = game.dim.id();
    let range: Option<f64> = m::WP_RANGES.get(s.wp_range).and_then(|r| r.parse().ok());
    let eye = cam.position;
    // (distance, screen point, pushed to the edge, edge angle, mark, name, colour, always named)
    let mut shown: Vec<(f64, [f32; 2], bool, f32, String, String, u32, bool)> = Vec::new();
    with(|st| {
        for wp in st.book.list.iter().filter(|w| w.on_screen && st.book.visible(w)) {
            // A 2D waypoint sits at eye height (its marker straight ahead, its distance flat).
            let Some(c) = wp.place_at(dim, s.convert, eye[1] - 1.0) else {
                continue;
            };
            let at = [c[0], c[1] + 1.0, c[2]];
            let d = m::distance(eye, at);
            if range.is_some_and(|r| d > r) {
                continue;
            }
            let rel = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
            let (p, front) = m::project(&cam.view, &cam.projection, rel, w, h);
            let (q, angle, pushed) = m::to_edge(p, w, h, 14.0);
            if (pushed || !front) && !s.wp_edge {
                continue;
            }
            shown.push((d, q, pushed || !front, angle, wp.mark(), wp.name.clone(), wp.rgb(), wp.always_name));
        }
    });
    // Far ones first, so near ones are on top.
    shown.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let centre = [w / 2.0, h / 2.0];
    // The one nearest the crosshair (within reach) shows its name when names show "looked at".
    let looked = shown
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.2)
        .map(|(i, e)| (i, (e.1[0] - centre[0]).hypot(e.1[1] - centre[1])))
        .filter(|&(_, r)| r < 40.0)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i);
    for (i, (d, q, edge, angle, initials, name, rgb, always_name)) in shown.into_iter().enumerate() {
        let alpha = if s.fade { u32::from(m::fade(d)).max(if edge { 120 } else { 0 }) } else { 255 };
        if alpha == 0 {
            continue;
        }
        let k = if s.wp_scale { m::marker_scale(d) } else { 1.0 };
        let r = 6.0 * k;
        let [x, y] = [q[0].round(), q[1].round()];
        let a = alpha << 24;
        // The marker: a dark rim, the colour, a lighter top half for a little depth.
        if s.wp_shape == 0 {
            let dia = |r: f32| [[x, y - r], [x + r, y], [x, y + r], [x - r, y]];
            game.native_quad(batch, dia(r + 1.5), a / 255 * 200 & 0xFF00_0000);
            game.native_quad(batch, dia(r), a | rgb);
            game.native_quad(batch, [[x, y - r], [x + r, y], [x, y], [x - r, y]], (alpha * 50 / 255) << 24 | 0xFF_FFFF);
        } else {
            game.native_fill(batch, [x - r - 1.0, y - r - 1.0, r * 2.0 + 2.0, r * 2.0 + 2.0], a / 255 * 200 & 0xFF00_0000);
            game.native_fill(batch, [x - r, y - r, r * 2.0, r * 2.0], a | rgb);
            game.native_fill(batch, [x - r, y - r, r * 2.0, r], (alpha * 50 / 255) << 24 | 0xFF_FFFF);
        }
        // Its letter (or symbol), dark on a light colour and light on a dark one.
        let lum = ((rgb >> 16) & 255) * 3 + ((rgb >> 8) & 255) * 6 + (rgb & 255);
        let ink = if lum > 1300 { 0x000000 } else { 0xFFFFFF };
        let text: String = initials.chars().take(2).collect();
        let tw = game.native_text_width(&text);
        let start = batch.vertices.len();
        game.native_text(batch, &text, [x - tw / 2.0, y - 3.5], ink, false);
        let ts = (k * 0.75).clamp(0.5, 1.0);
        batch.scale_since(start, x, y, ts, ts);
        if edge {
            // An arrow beside it pointing the way to turn.
            let (sa, ca) = angle.sin_cos();
            let tip = [x + ca * (r + 7.0), y + sa * (r + 7.0)];
            let base = [x + ca * (r + 3.0), y + sa * (r + 3.0)];
            let side = [-sa * 3.0, ca * 3.0];
            game.native_quad(
                batch,
                [tip, [base[0] + side[0], base[1] + side[1]], base, [base[0] - side[0], base[1] - side[1]]],
                a | rgb,
            );
        }
        // Distance under it; the name over it.
        let dist = m::distance_text(d, s.units);
        let dw = game.native_text_width(&dist) * 0.75;
        let start = batch.vertices.len();
        game.native_text(batch, &dist, [x - dw / 2.0, y + r + 3.0], 0xE0E0E0, true);
        batch.scale_since(start, x, y + r + 3.0, 0.75, 0.75);
        let show_name = always_name || match s.wp_names {
            1 => true,
            0 => looked == Some(i),
            _ => false,
        };
        if show_name && !name.is_empty() {
            let ls = m::WP_LABELS[s.wp_label.min(m::WP_LABELS.len() - 1)];
            let nw = game.native_text_width(&name) * ls;
            let (nx, ny) = ((x - nw / 2.0).round(), (y - r - 3.0 - 10.0 * ls).round());
            game.native_fill(batch, [nx - 2.0, ny - 1.0, nw + 4.0, 9.0 * ls + 2.0], (alpha * 0x90 / 255) << 24);
            let start = batch.vertices.len();
            game.native_text(batch, &name, [nx, ny], rgb, false);
            batch.scale_since(start, nx, ny, ls, ls);
        }
    }
}

/// `native_world_primitives`: the old style's beam over each waypoint, when it is turned on.
pub(super) fn on_world(game: &Game<'_>, camera: [f64; 3], primitives: &mut WorldPrimitives) {
    if !game.native_on(m::ID) || game.player.is_none() {
        return;
    }
    let s = settings(game);
    if !s.beams {
        return;
    }
    let dim = game.dim.id();
    let top = game.world.shape.max_y();
    with(|st| {
        for w in st.book.list.iter().filter(|w| st.book.visible(w)) {
            let Some(c) = w.place_in(dim, s.convert) else {
                continue;
            };
            let d = m::distance(camera, c);
            let alpha = if s.fade { u32::from(m::fade(d)) } else { 255 };
            if alpha == 0 {
                continue;
            }
            let height = (top - c[1] as i32).clamp(1, 2048) as u32;
            primitives.beam(c, height, alpha << 24 | w.rgb());
        }
    });
}

// ---------------------------------------------------------------- the map screen

fn ours(game: &Game<'_>, id: &str) -> bool {
    id == m::SCREEN && map_open(game) && game.native_on(m::ID)
}

fn inside(r: [f32; 4], p: [f32; 2]) -> bool {
    p[0] >= r[0] && p[0] < r[0] + r[2] && p[1] >= r[1] && p[1] < r[1] + r[3]
}

/// The tool column's width at the map's right.
const TOOL_W: f32 = 96.0;
/// How far the layer buttons step.
const LAYER_STEP: i32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Waypoints,
    Settings,
    OnMe,
    Follow,
    Grid,
    Slime,
    LayerDown,
    LayerUp,
    Caves,
    FollowY,
    Dimension,
    Filter,
    Png,
    World,
    NewWorld,
}

/// The tool column's buttons; slime chunks only where the seed is known, sub-worlds only on a
/// server.
fn tools(w: f32, mp: bool, slime: bool) -> Vec<(Tool, [f32; 4])> {
    let x = w - TOOL_W + 3.0;
    let full = TOOL_W - 6.0;
    let half = (full - 2.0) / 2.0;
    let mut y = TOP + 4.0;
    let mut out = vec![(Tool::Waypoints, [x, y, half, 16.0]), (Tool::Settings, [x + half + 2.0, y, half, 16.0])];
    y += 18.0;
    out.push((Tool::OnMe, [x, y, half, 16.0]));
    out.push((Tool::Follow, [x + half + 2.0, y, half, 16.0]));
    y += 18.0;
    out.push((Tool::Grid, [x, y, half, 16.0]));
    if slime {
        out.push((Tool::Slime, [x + half + 2.0, y, half, 16.0]));
    }
    y += 30.0;
    out.push((Tool::LayerDown, [x, y, half, 16.0]));
    out.push((Tool::LayerUp, [x + half + 2.0, y, half, 16.0]));
    y += 18.0;
    out.push((Tool::Caves, [x, y, full, 16.0]));
    y += 18.0;
    out.push((Tool::FollowY, [x, y, full, 16.0]));
    y += 18.0;
    out.push((Tool::Dimension, [x, y, full, 16.0]));
    y += 18.0;
    out.push((Tool::Filter, [x, y, full, 16.0]));
    y += 18.0;
    out.push((Tool::Png, [x, y, full, 16.0]));
    if mp {
        y += 18.0;
        out.push((Tool::World, [x, y, full, 16.0]));
        y += 18.0;
        out.push((Tool::NewWorld, [x, y, full, 16.0]));
    }
    out
}

fn tool_label(t: Tool, s: &State) -> String {
    let on = |b: bool| if b { "ON" } else { "OFF" };
    match t {
        Tool::Waypoints => "Waypoints".into(),
        Tool::Settings => "Settings".into(),
        Tool::OnMe => "On me".into(),
        Tool::Follow => format!("Follow {}", on(s.screen.follow)),
        Tool::Grid => format!("Grid {}", on(s.screen.grid)),
        Tool::Slime => format!("Slime {}", on(s.screen.slime)),
        Tool::LayerDown => "Down".into(),
        Tool::LayerUp => "Up".into(),
        Tool::Caves => format!("Caves: {}", m::CAVE_TYPES[s.cave_types.get(&s.map_dim, s.map_roofed).index()]),
        Tool::FollowY => format!("Y follows me {}", on(s.slice_follow)),
        Tool::Dimension => format!("View: {}", m::pretty_id(s.viewed.as_ref().map_or(&s.map_dim, |v| &v.dim))),
        Tool::Filter => format!("Show: {}", m::MAP_FILTERS[s.screen.filter]),
        Tool::Png => "Save as PNG".into(),
        Tool::World => s
            .subworlds
            .as_ref()
            .map_or_else(String::new, |(_, w)| format!("Map: {}", w.current_name())),
        Tool::NewWorld => "New map world".into(),
    }
}

/// The world map's layer a step down or up: stepping down from the surface or the full map
/// starts the layered view at the feet (kept as this dimension's cave map), stepping up past
/// the top leaves it.
fn step_layer(game: &Game<'_>, s: &mut State, feet: i32, top: i32, down: bool) {
    let was = s.screen.slice;
    match layer_step(was, feet, down, top) {
        Some(y) => {
            s.screen.slice = Some(y);
            if cave_type(game, s) != m::CaveType::Layered {
                set_cave_type(game, s, m::CaveType::Layered);
            }
            // Moved by hand: no longer following the head.
            s.slice_follow &= was.is_none();
        }
        None if was.is_some() => set_cave_type(game, s, m::CaveType::Off),
        None => {}
    }
}

/// The layer one step down or up from `now` (`None`: the surface), from the player's feet.
fn layer_step(now: Option<i32>, feet: i32, down: bool, top: i32) -> Option<i32> {
    match (now, down) {
        (None, true) => Some(feet),
        (None, false) => None,
        (Some(y), true) => Some(y - LAYER_STEP),
        (Some(y), false) if y + LAYER_STEP >= top => None,
        (Some(y), false) => Some(y + LAYER_STEP),
    }
}

/// The list's panel, its header buttons (set, set shown, new set, delete set) and its bottom
/// ones (back, order, from chat, add here).
/// The waypoint list's parts.
struct ListLayout {
    panel: [f32; 4],
    rows: usize,
    search: [f32; 4],
    /// Set, set shown, new set, rename set, delete set.
    head: [[f32; 4]; 5],
    /// Delete, hide, show, move to set, view.
    bulk: [[f32; 4]; 5],
    /// Back, order, from chat, new.
    foot: [[f32; 4]; 4],
}

fn list_layout(w: f32, h: f32) -> ListLayout {
    let pw = 400.0f32.min(w - 12.0);
    let panel = [(w - pw) / 2.0, 10.0, pw, h - 20.0];
    let x = panel[0] + 6.0;
    let inner = pw - 12.0;
    // `n` buttons of the given widths (summing to 1) across a row at `y`.
    let row = |y: f32, parts: &[f32]| -> Vec<[f32; 4]> {
        let gap = 2.0;
        let usable = inner - gap * (parts.len() as f32 - 1.0);
        let mut at = x;
        parts
            .iter()
            .map(|p| {
                let r = [at, y, usable * p, 18.0];
                at += usable * p + gap;
                r
            })
            .collect()
    };
    let head = row(panel[1] + 22.0, &[0.3, 0.17, 0.15, 0.18, 0.2]);
    let bulk = row(h - 56.0, &[0.18, 0.14, 0.14, 0.24, 0.3]);
    let foot = row(h - 34.0, &[0.25, 0.25, 0.25, 0.25]);
    ListLayout {
        panel,
        rows: m::list_rows(h),
        search: [panel[0] + pw - 6.0 - 140.0, panel[1] + 4.0, 140.0, 14.0],
        head: [head[0], head[1], head[2], head[3], head[4]],
        bulk: [bulk[0], bulk[1], bulk[2], bulk[3], bulk[4]],
        foot: [foot[0], foot[1], foot[2], foot[3]],
    }
}

fn list_row(panel: [f32; 4], i: usize) -> [f32; 4] {
    [panel[0] + 6.0, panel[1] + 44.0 + i as f32 * 22.0, panel[2] - 12.0, 20.0]
}

/// A row's tick box.
fn row_tick(r: [f32; 4]) -> [f32; 4] {
    [r[0] + 3.0, r[1] + 5.0, 10.0, 10.0]
}

/// The waypoints the list shows now, in order.
fn listed(s: &State, settings: &m::Settings, here: Option<[f64; 3]>) -> Vec<usize> {
    let set = (s.screen.list_view == 1).then(|| s.book.current_set().to_owned());
    let mut out = s.book.listed(settings.sort, here, &s.screen.search, set.as_deref());
    if s.screen.list_view == 2 {
        out.retain(|&i| s.book.list[i].death);
    }
    out
}

/// A waypoint's marker as the list draws it: a diamond in its colour with its mark.
fn draw_mark(game: &Game<'_>, batch: &mut HudBatch, at: [f32; 2], wp: &m::Waypoint, faded: bool) {
    let a: u32 = if faded { 0x80 } else { 0xFF };
    let r = 6.5;
    let [x, y] = at;
    let dia = |r: f32| [[x, y - r], [x + r, y], [x, y + r], [x - r, y]];
    game.native_quad(batch, dia(r + 1.0), (a * 200 / 255) << 24);
    game.native_quad(batch, dia(r), a << 24 | wp.rgb());
    let rgb = wp.rgb();
    let lum = ((rgb >> 16) & 255) * 3 + ((rgb >> 8) & 255) * 6 + (rgb & 255);
    let ink = if lum > 1300 { 0x000000 } else { 0xFFFFFF };
    let text: String = wp.mark().chars().take(2).collect();
    let tw = game.native_text_width(&text);
    let start = batch.vertices.len();
    game.native_text(batch, &text, [x - tw / 2.0, y - 3.5], ink, false);
    batch.scale_since(start, x, y, 0.75, 0.75);
}

/// The waypoint editor's parts.
struct EditLayout {
    /// The text boxes: name, symbol, x, y, z, hex (`m::Form` order).
    fields: [[f32; 4]; 6],
    swatches: [[f32; 4]; 10],
    use_pos: [f32; 4],
    dim: [f32; 4],
    set: [f32; 4],
    visible: [f32; 4],
    temporary: [f32; 4],
    scope: [f32; 4],
    always_name: [f32; 4],
    two_d: [f32; 4],
    vanish: [f32; 4],
    on_screen: [f32; 4],
    on_minimap: [f32; 4],
    on_map: [f32; 4],
    teleport: [f32; 4],
    copy: [f32; 4],
    share: [f32; 4],
    duplicate: [f32; 4],
    delete: [f32; 4],
    done: [f32; 4],
    panel: [f32; 4],
    /// The "New waypoint" box's buttons and its (shorter) panel.
    create: [f32; 4],
    cancel: [f32; 4],
    new_panel: [f32; 4],
}

fn edit_layout(w: f32, h: f32) -> EditLayout {
    let pw = 300.0f32.min(w - 16.0);
    let x = (w - pw) / 2.0;
    let y = (h / 2.0 - 112.0).max(4.0);
    let half = |i: f32, k: usize| {
        let cw = (pw - 2.0) / 2.0;
        [x + k as f32 * (cw + 2.0), y + 76.0 + i * 20.0, cw, 18.0]
    };
    let third = |i: f32, k: usize| {
        let cw = (pw - 4.0) / 3.0;
        [x + k as f32 * (cw + 2.0), y + 76.0 + i * 20.0, cw, 18.0]
    };
    let fw = (pw - 110.0 - 6.0) / 3.0;
    let xyz = |k: usize| [x + k as f32 * (fw + 2.0), y + 54.0, fw, 16.0];
    EditLayout {
        fields: [
            [x, y + 14.0, pw, 16.0],
            [x, y + 34.0, 30.0, 16.0],
            xyz(0),
            xyz(1),
            xyz(2),
            [x + 192.0, y + 34.0, pw - 192.0, 16.0],
        ],
        swatches: std::array::from_fn(|i| [x + 36.0 + i as f32 * 15.0, y + 35.0, 13.0, 14.0]),
        use_pos: [x + pw - 110.0, y + 54.0, 110.0, 16.0],
        dim: half(0.0, 0),
        set: half(0.0, 1),
        visible: half(1.0, 0),
        temporary: half(1.0, 1),
        scope: half(2.0, 0),
        always_name: half(2.0, 1),
        two_d: half(6.0, 0),
        vanish: half(6.0, 1),
        on_screen: third(3.0, 0),
        on_minimap: third(3.0, 1),
        on_map: third(3.0, 2),
        teleport: third(4.0, 0),
        copy: third(4.0, 1),
        share: third(4.0, 2),
        duplicate: third(5.0, 0),
        delete: third(5.0, 1),
        done: third(5.0, 2),
        panel: [x - 8.0, y - 6.0, pw + 16.0, 76.0 + 7.0 * 20.0 + 10.0],
        create: half(1.0, 0),
        cancel: half(1.0, 1),
        new_panel: [x - 8.0, y - 6.0, pw + 16.0, 76.0 + 2.0 * 20.0 + 10.0],
    }
}

/// Whether the server lets this player use commands such as /tp (operator level 2 or more).
fn may_teleport(game: &Game<'_>) -> bool {
    game.ui.registries.permission >= 2
}

/// The dimensions a waypoint can be moved to: the three of the game and the one played now.
fn dimensions(now: &str) -> Vec<String> {
    let mut d: Vec<String> = ["minecraft:overworld", "minecraft:the_nether", "minecraft:the_end"].map(str::to_owned).to_vec();
    if !d.iter().any(|x| x == now) {
        d.push(now.to_owned());
    }
    d
}

/// A one-line box (a confirm or a name) centred, with two buttons under it.
fn dialog_layout(w: f32, h: f32) -> ([f32; 4], [[f32; 4]; 2]) {
    let pw = 220.0;
    let x = (w - pw) / 2.0;
    let y = h / 2.0 - 30.0;
    ([x, y, pw, 18.0], [[x, y + 24.0, 109.0, 18.0], [x + 111.0, y + 24.0, 109.0, 18.0]])
}

/// The new set's name box and its buttons: create, cancel.
fn new_set_layout(w: f32, h: f32) -> ([f32; 4], [[f32; 4]; 2]) {
    let pw = 200.0;
    let x = (w - pw) / 2.0;
    let y = h / 2.0 - 30.0;
    ([x, y, pw, 18.0], [[x, y + 24.0, 98.0, 18.0], [x + 102.0, y + 24.0, 98.0, 18.0]])
}

fn button(game: &Game<'_>, batch: &mut HudBatch, r: [f32; 4], label: &str, mouse: [f32; 2]) {
    let hover = inside(r, mouse);
    ui::button(
        batch,
        &game.gui.atlas,
        r[0],
        r[1],
        r[2],
        r[3],
        &Text::literal(label),
        true,
        hover,
    );
}

fn text_box(game: &Game<'_>, batch: &mut HudBatch, field: [f32; 4], text: &str) {
    game.native_fill(batch, field, 0xFFA0_A0A0);
    game.native_fill(batch, [field[0] + 1.0, field[1] + 1.0, field[2] - 2.0, field[3] - 2.0], 0xFF00_0000);
    let caret = if (game.started.elapsed().as_millis() / 500) % 2 == 0 { "_" } else { "" };
    game.native_text(batch, &format!("{text}{caret}"), [field[0] + 4.0, field[1] + 5.0], 0xFFFFFF, false);
}

/// Whether a waypoint shows on the world map with its filter.
fn on_map(s: &State, w: &m::Waypoint) -> bool {
    let set = m::settings(|_| None);
    set.waypoints
        && set.wp_worldmap
        && w.on_map
        && s.book.visible(w)
        && match s.screen.filter {
            0 => true,
            1 => w.set == s.book.current_set(),
            _ => false,
        }
}

/// `native_screen_render_post`: the map, the list, the editor or the new set's name.
pub(super) fn on_render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if !ours(game, screen.id) {
        return;
    }
    let (w, h) = (batch.width, batch.height);
    let mouse = screen.mouse;
    // Opened with the key from the world, the "New waypoint" box lets the world show behind it.
    let over_world = with(|s| matches!(&s.screen.page, m::Page::Edit { draft: Some(d), .. } if d.close));
    game.native_fill(batch, [0.0, 0.0, w, h], if over_world { 0x9010_1014 } else { 0xFF10_1014 });
    let settings = settings(game);
    with(|s| match s.screen.page.clone() {
        m::Page::Map => draw_map(game, batch, s, &settings, mouse),
        m::Page::List { scroll } => draw_list(game, batch, s, &settings, scroll, mouse),
        m::Page::Edit { form, draft: Some(d), .. } => draw_edit(game, batch, &d.wp, true, &form, mouse),
        m::Page::Edit { index, form, draft: None } => {
            if let Some(wp) = s.book.list.get(index) {
                draw_edit(game, batch, wp, false, &form, mouse);
            }
        }
        m::Page::Confirm { delete } => {
            let q = format!("Delete {} waypoint{}?", delete.len(), if delete.len() == 1 { "" } else { "s" });
            draw_dialog(game, batch, &q, None, "Delete", mouse);
        }
        m::Page::RenameSet { name } => draw_dialog(game, batch, "The set's new name", Some(&name), "Rename", mouse),
        m::Page::Settings { tab } => draw_settings(game, batch, &settings, tab, mouse),
        m::Page::NewSet { name } => {
            let (field, [create, cancel]) = new_set_layout(w, h);
            game.native_fill(batch, [field[0] - 8.0, field[1] - 20.0, field[2] + 16.0, 70.0], 0xC000_0000);
            game.native_text(batch, "New set's name", [field[0], field[1] - 12.0], 0xFFFFFF, true);
            text_box(game, batch, field, &name);
            button(game, batch, create, "Create", mouse);
            button(game, batch, cancel, "Cancel", mouse);
        }
    });
}

fn draw_map(game: &Game<'_>, batch: &mut HudBatch, s: &mut State, settings: &m::Settings, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let rect = map_rect(w, h);
    let view = s.screen.view;
    let [ur, ug, ub, _] = m::UNKNOWN;
    game.native_fill(batch, rect, 0xFF00_0000 | u32::from(ur) << 16 | u32::from(ug) << 8 | u32::from(ub));
    let (x0, z0, x1, z1) = view.bounds(rect);
    {
        let bpp = view.bpp();
        let (bw, bh) = (x1 - x0, z1 - z0);
        // A texel about a GUI pixel; coarser when the cells would not fit the slot.
        let lod = m::lod_for(bpp, m::CELLS * 9 / 10, |lod| {
            let span = f64::from(16i32 << lod);
            ((bw / span).ceil() as usize + 1) * ((bh / span).ceil() as usize + 1)
        });
        let source = map_source(game, s);
        let to_screen = |x: f64, z: f64| view.screen_of(rect, x, z);
        draw_cells(game, s, batch, source, lod, [x0, z0, x1, z1], &to_screen, &m::Shape::Rect(rect), 255);
    }
    // Explored ground outside the chunks loaded now, dimmed (the surface view only).
    if settings.map_shade && map_source(game, s) == 0
        && let Some(p) = player_pos(game)
    {
        let rd = f64::from(game.settings.render_distance.max(2));
        let (pcx, pcz) = ((p[0] / 16.0).floor(), (p[2] / 16.0).floor());
        let a = view.screen_of(rect, (pcx - rd) * 16.0, (pcz - rd) * 16.0);
        let b = view.screen_of(rect, (pcx + rd + 1.0) * 16.0, (pcz + rd + 1.0) * 16.0);
        let (l, t) = (a[0].clamp(rect[0], rect[0] + rect[2]), a[1].clamp(rect[1], rect[1] + rect[3]));
        let (r, bt) = (b[0].clamp(rect[0], rect[0] + rect[2]), b[1].clamp(rect[1], rect[1] + rect[3]));
        let dim = 0x5000_0000;
        game.native_fill(batch, [rect[0], rect[1], rect[2], t - rect[1]], dim);
        game.native_fill(batch, [rect[0], bt, rect[2], rect[1] + rect[3] - bt], dim);
        game.native_fill(batch, [rect[0], t, l - rect[0], bt - t], dim);
        game.native_fill(batch, [r, t, rect[0] + rect[2] - r, bt - t], dim);
    }
    // The chunk grid, when chunks are at least 4 pixels across.
    if s.screen.grid && view.bpp() <= 4.0 {
        let mut cx = (x0 / 16.0).floor() as i64 * 16;
        while (cx as f64) <= x1 {
            let [sx, _] = view.screen_of(rect, cx as f64, z0);
            game.native_fill(batch, [sx.round(), rect[1], 1.0, rect[3]], 0x40FF_FFFF);
            cx += 16;
        }
        let mut cz = (z0 / 16.0).floor() as i64 * 16;
        while (cz as f64) <= z1 {
            let [_, sy] = view.screen_of(rect, x0, cz as f64);
            game.native_fill(batch, [rect[0], sy.round(), rect[2], 1.0], 0x40FF_FFFF);
            cz += 16;
        }
    }
    // Slime chunks, where the seed is known (single player).
    if s.screen.slime
        && view.bpp() <= 4.0
        && let Some(seed) = game.native_world_seed()
    {
        let (cx0, cz0) = ((x0 / 16.0).floor() as i32, (z0 / 16.0).floor() as i32);
        let (cx1, cz1) = ((x1 / 16.0).floor() as i32, (z1 / 16.0).floor() as i32);
        let side = (16.0 / view.bpp()) as f32;
        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                if m::slime_chunk(seed, cx, cz) {
                    let [sx, sy] = view.screen_of(rect, f64::from(cx * 16), f64::from(cz * 16));
                    let left = sx.max(rect[0]);
                    let right = (sx + side).min(rect[0] + rect[2]);
                    let up = sy.max(rect[1]);
                    let down = (sy + side).min(rect[1] + rect[3]);
                    if right > left && down > up {
                        game.native_fill(batch, [left, up, right - left, down - up], 0x5030_E040);
                    }
                }
            }
        }
    }
    let here = game.dim.id();
    let dim = s.viewed.as_ref().map_or(here, |v| v.dim.as_str()).to_owned();
    let dim = dim.as_str();
    // Waypoints with their letter or symbol; names shown when zoomed in or pointed at.
    for wp in s.book.list.iter().filter(|w| on_map(s, w)) {
        let Some(c) = wp.place_in(dim, settings.convert) else {
            continue;
        };
        let [x, y] = view.screen_of(rect, c[0], c[2]);
        if !inside(rect, [x, y]) {
            continue;
        }
        let initials = wp.mark();
        let iw = game.native_text_width(&initials).max(6.0);
        game.native_fill(batch, [x - iw / 2.0 - 2.0, y - 6.0, iw + 4.0, 11.0], 0xFF00_0000);
        game.native_fill(batch, [x - iw / 2.0 - 1.0, y - 5.0, iw + 2.0, 9.0], 0xFF00_0000 | wp.rgb());
        game.native_text(batch, &initials, [x - game.native_text_width(&initials) / 2.0, y - 4.0], 0x000000, false);
        let hover = (mouse[0] - x).abs() <= iw / 2.0 + 2.0 && (mouse[1] - y).abs() <= 6.0;
        if hover || view.bpp() <= 2.0 {
            let tw = game.native_text_width(&wp.name);
            game.native_text(batch, &wp.name, [x - tw / 2.0, y - 16.0], wp.rgb(), true);
        }
    }
    // Your recent path, as small dots (fewer when zoomed far out).
    if settings.footsteps
        && s.viewed.is_none()
        && let Some(e) = s.explored.as_ref()
    {
        let every = (view.bpp() / 2.0).ceil().max(1.0) as usize;
        for p in e.steps.points.iter().step_by(every) {
            let [x, y] = view.screen_of(rect, f64::from(p[0]) + 0.5, f64::from(p[1]) + 0.5);
            if inside(rect, [x, y]) {
                game.native_fill(batch, [x - 1.0, y - 1.0, 2.0, 2.0], 0xB0E8_D890);
            }
        }
    }
    // Other players: dots in their colour, their names when pointed at or zoomed in.
    if settings.map_players && !settings.fair_play && s.viewed.is_none() {
        let t = game.clock.fraction();
        let rgb = settings.dot_rgb(m::DotKind::Player);
        for mob in game.mobs.iter() {
            if mob.removed || m::dot_kind(&mob.kind.id, &mob.kind.category) != m::DotKind::Player {
                continue;
            }
            let at = mob.previous.lerp(mob.position, t);
            let [x, y] = view.screen_of(rect, at.x, at.z);
            if !inside(rect, [x, y]) {
                continue;
            }
            draw_dot(game, batch, [x, y], 2.5, 0xFF00_0000 | rgb);
            let hover = (mouse[0] - x).abs() <= 5.0 && (mouse[1] - y).abs() <= 5.0;
            if (hover || view.bpp() <= 1.0)
                && let Some(name) = game.remote_player_name(mob.id)
            {
                let tw = game.native_text_width(name);
                game.native_text(batch, name, [x - tw / 2.0, y - 13.0], rgb, true);
            }
        }
    }
    // You: an arrow the way you face (in the dimension you are in).
    if dim == here
        && let Some(p) = player_pos(game)
    {
        let [x, y] = view.screen_of(rect, p[0], p[2]);
        if inside(rect, [x, y]) {
            game.native_quad(batch, m::arrow([x, y], game.yaw, 7.0), 0xFF00_0000);
            game.native_quad(batch, m::arrow([x, y], game.yaw, 5.0), 0xFFFF_FFFF);
        }
    }
    // The bars and the tools.
    game.native_fill(batch, [0.0, 0.0, w, TOP], 0xE000_0000);
    game.native_fill(batch, [0.0, h - BOTTOM, w, BOTTOM], 0xE000_0000);
    game.native_fill(batch, [w - TOOL_W, TOP, TOOL_W, h - TOP - BOTTOM], 0xE018_181C);
    let layer = map_layer_name(game, s, settings);
    let title = format!("World Map - {} - {layer}", m::pretty_id(dim));
    game.native_text(batch, &title, [6.0, 7.0], 0xFFFFFF, true);
    let slime = game.native_world_seed().is_some();
    for (tool, r) in tools(w, s.subworlds.is_some(), slime) {
        if tool == Tool::LayerDown {
            game.native_text(batch, &layer, [r[0], r[1] - 10.0], 0xC0C0C0, false);
        }
        button(game, batch, r, &tool_label(tool, s), mouse);
    }
    let hint = "Drag or WASD to move, wheel to zoom, [ ] cave layers, click: waypoint, right-click: menu";
    let bottom = match &s.notice {
        Some((text, until)) if Instant::now() < *until => text.clone(),
        _ if inside(rect, mouse) && settings.map_cursor => {
            let (x, z) = view.world_at(rect, mouse);
            let (bx, bz) = (x.floor() as i32, z.floor() as i32);
            let y = map_column(s, map_source(game, s), bx, bz)
                .filter(|c| c.kind == m::FLOOR)
                .map_or_else(String::new, |c| format!(" Y {}", c.height));
            let other = match dim {
                m::OVERWORLD => format!("   Nether {} {}", bx.div_euclid(8), bz.div_euclid(8)),
                m::NETHER => format!("   Overworld {} {}", bx * 8, bz * 8),
                _ => String::new(),
            };
            if settings.hide_coords {
                format!("(coordinates hidden)   {} block(s) a pixel", view.bpp())
            } else {
                format!("X {bx}{y} Z {bz}{other}   {} block(s) a pixel", view.bpp())
            }
        }
        _ => hint.to_owned(),
    };
    game.native_text(batch, &bottom, [6.0, h - BOTTOM + 3.0], 0xC0C0C0, false);
    // The right-click menu, over everything.
    if let Some(menu) = s.screen.menu.clone() {
        let items = menu_items(game, s, &menu);
        let rects = m::menu_rects(menu.at, items.len(), w, h);
        if let (Some(first), Some(last)) = (rects.first(), rects.last()) {
            game.native_fill(batch, [first[0] - 1.0, first[1] - 1.0, first[2] + 2.0, last[1] + last[3] - first[1] + 2.0], 0xFF60_6068);
        }
        for (item, r) in items.iter().zip(&rects) {
            game.native_fill(batch, *r, if inside(*r, mouse) { 0xF040_4870 } else { 0xF018_181C });
            game.native_text(batch, item.label(), [r[0] + 4.0, r[1] + 3.0], 0xFFFFFF, false);
        }
        let b = menu.block;
        let head = format!("{} {} {}", b[0], b[1], b[2]);
        if let Some(r) = rects.first() {
            game.native_text(batch, &head, [r[0], r[1] - 10.0], 0xC0C0C0, true);
        }
    }
}

fn draw_list(game: &Game<'_>, batch: &mut HudBatch, s: &State, settings: &m::Settings, scroll: usize, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let l = list_layout(w, h);
    let [set, set_shown, new_set, rename_set, del_set] = l.head;
    let [del, hide, show, to_set, view] = l.bulk;
    let [back, order, from_chat, add] = l.foot;
    let panel = l.panel;
    game.native_fill(batch, panel, 0xC000_0000);
    let here = player_pos(game);
    let shown = listed(s, settings, here);
    let title = format!("Waypoints ({} of {})", shown.len(), s.book.list.len());
    game.native_text(batch, &title, [panel[0] + 6.0, panel[1] + 7.0], 0xFFFFFF, true);
    // The search box: click it and type.
    let sb = l.search;
    game.native_fill(batch, sb, if s.screen.searching { 0xFFC0_C0C0 } else { 0xFF60_6060 });
    game.native_fill(batch, [sb[0] + 1.0, sb[1] + 1.0, sb[2] - 2.0, sb[3] - 2.0], 0xFF00_0000);
    let caret = if s.screen.searching && (game.started.elapsed().as_millis() / 500) % 2 == 0 { "_" } else { "" };
    let (text, colour) = if s.screen.search.is_empty() && !s.screen.searching {
        ("Search...".to_owned(), 0x808080)
    } else {
        (format!("{}{caret}", s.screen.search), 0xFFFFFF)
    };
    game.native_text(batch, &text, [sb[0] + 3.0, sb[1] + 3.0], colour, false);
    let cur = &s.book.sets[s.book.current.min(s.book.sets.len() - 1)];
    button(game, batch, set, &format!("Set: {}", cur.name), mouse);
    button(game, batch, set_shown, &format!("Shown: {}", if cur.shown { "ON" } else { "OFF" }), mouse);
    button(game, batch, new_set, "New set", mouse);
    button(game, batch, rename_set, "Rename set", mouse);
    button(game, batch, del_set, "Delete set", mouse);
    let dim = game.dim.id();
    for (i, &index) in shown.iter().skip(scroll).take(l.rows).enumerate() {
        let wp = &s.book.list[index];
        let r = list_row(panel, i);
        let hover = inside(r, mouse);
        let picked = s.screen.selected.contains(&index);
        game.native_fill(batch, r, if picked { 0x7040_6090 } else if hover { 0x6050_5070 } else { 0x4030_3040 });
        let t = row_tick(r);
        game.native_fill(batch, t, 0xFFA0_A0A0);
        game.native_fill(batch, [t[0] + 1.0, t[1] + 1.0, t[2] - 2.0, t[3] - 2.0], if picked { 0xFF55_AAFF } else { 0xFF10_1010 });
        let visible = s.book.visible(wp);
        draw_mark(game, batch, [r[0] + 24.0, r[1] + 10.0], wp, !visible);
        let colour = if visible { 0xFFFFFF } else { 0x707070 };
        let mut label = if wp.set == s.book.current_set() { wp.name.clone() } else { format!("{} ({})", wp.name, wp.set) };
        if !wp.shown {
            label.push_str("  - hidden");
        }
        if wp.global {
            label.push_str("  [all worlds]");
        }
        if wp.temporary {
            label.push_str("  [temporary]");
        }
        game.native_text(batch, &label, [r[0] + 36.0, r[1] + 6.0], colour, false);
        let mut info = if settings.hide_coords {
            "hidden".to_owned()
        } else if wp.two_d {
            format!("{} ~ {}", wp.pos[0], wp.pos[2])
        } else {
            format!("{} {} {}", wp.pos[0], wp.pos[1], wp.pos[2])
        };
        if wp.dim != dim {
            info = format!("{info}  {}", m::pretty_id(&wp.dim));
        } else if let Some(p) = here
            && let Some(c) = wp.place_at(dim, false, p[1])
        {
            info = format!("{info}  {}", m::distance_text(m::distance(p, c), settings.units));
        }
        let iw = game.native_text_width(&info);
        game.native_text(batch, &info, [r[0] + r[2] - iw - 4.0, r[1] + 6.0], 0xA0A0A0, false);
    }
    if shown.is_empty() {
        let t = if s.book.list.is_empty() {
            "No waypoints yet: B in the world, a click on the map, or + New"
        } else {
            "No waypoint here matches"
        };
        let tw = game.native_text_width(t);
        game.native_text(batch, t, [(w - tw) / 2.0, panel[1] + 60.0], 0xA0A0A0, false);
    }
    let n = s.screen.selected.len();
    let which = if n > 0 { format!(" ({n})") } else { String::new() };
    let del_label = if s.screen.list_view == 2 && n == 0 { "Clear all".to_owned() } else { format!("Delete{which}") };
    button(game, batch, del, &del_label, mouse);
    button(game, batch, hide, &format!("Hide{which}"), mouse);
    button(game, batch, show, &format!("Show{which}"), mouse);
    button(game, batch, to_set, &format!("To set {}", s.book.current_set()), mouse);
    button(game, batch, view, &format!("List: {}", m::LIST_VIEWS[s.screen.list_view.min(2)]), mouse);
    button(game, batch, back, "Back to map", mouse);
    button(game, batch, order, &format!("Order: {}", m::SORTS[settings.sort]), mouse);
    let chat = match s.chat.back() {
        Some(_) => format!("From chat ({})", s.chat.len()),
        None => "From chat".to_owned(),
    };
    button(game, batch, from_chat, &chat, mouse);
    button(game, batch, add, "+ New", mouse);
    let tip = if inside(from_chat, mouse)
        && let Some((line, at)) = s.chat.back()
    {
        format!("{line} -> {} {} {}", at[0], at[1], at[2])
    } else if [del, hide, show, to_set].iter().any(|r| inside(*r, mouse)) {
        if n > 0 { "On the ticked waypoints (Shift: delete without asking)".into() } else { "On every waypoint listed (tick some to choose)".into() }
    } else {
        "Click: edit. Right-click: hide or show. The box: tick for the buttons below.".into()
    };
    game.native_text(batch, &tip, [panel[0] + 6.0, h - 68.0], 0x909090, false);
}

/// The editor for `wp`; `new`: the "New waypoint" box (the boxes, colour, place, dimension and
/// set, then Create and Cancel).
fn draw_edit(game: &Game<'_>, batch: &mut HudBatch, wp: &m::Waypoint, new: bool, form: &m::Form, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let l = edit_layout(w, h);
    game.native_fill(batch, if new { l.new_panel } else { l.panel }, 0xD000_0000);
    let kind = if new { "New waypoint" } else if wp.death { "Death point" } else { "Waypoint" };
    let title = format!("{kind} in {}", m::pretty_id(&wp.dim));
    game.native_text(batch, &title, [l.fields[0][0], l.fields[0][1] - 11.0], 0xFFFFFF, true);
    draw_mark(game, batch, [l.fields[0][0] + l.fields[0][2] - 8.0, l.fields[0][1] - 7.0], wp, false);
    let blink = (game.started.elapsed().as_millis() / 500) % 2 == 0;
    for (i, r) in l.fields.iter().enumerate() {
        let focused = form.focus == Some(i);
        game.native_fill(batch, *r, if focused { 0xFFE0_E0E0 } else { 0xFF70_7070 });
        game.native_fill(batch, [r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], 0xFF00_0000);
        let mut text = form.fields[i].clone();
        if i == m::F_HEX {
            text = format!("#{text}");
        }
        if focused && blink {
            text.push('_');
        }
        let empty = form.fields[i].is_empty() && !focused;
        let hint = match i {
            m::F_NAME if new => wp.name.as_str(),
            m::F_SYMBOL => "Aa",
            m::F_HEX => "#custom",
            _ => "",
        };
        if new && i == m::F_NAME && form.fields[i].is_empty() {
            // The name it will get, faint behind the caret.
            game.native_text(batch, hint, [r[0] + 3.0, r[1] + 4.0], 0x606060, false);
            if focused && blink {
                game.native_text(batch, "_", [r[0] + 3.0, r[1] + 4.0], 0xFFFFFF, false);
            }
        } else if empty && !hint.is_empty() {
            game.native_text(batch, hint, [r[0] + 3.0, r[1] + 4.0], 0x606060, false);
        } else {
            game.native_text(batch, &text, [r[0] + 3.0, r[1] + 4.0], 0xFFFFFF, false);
        }
    }
    for (k, label) in ["X", "Y", "Z"].iter().enumerate() {
        let r = l.fields[m::F_X + k];
        game.native_text(batch, label, [r[0] + r[2] - 8.0, r[1] + 4.0], 0x707070, false);
    }
    for (i, r) in l.swatches.iter().enumerate() {
        let chosen = wp.custom.is_none() && wp.colour == i;
        game.native_fill(batch, *r, if chosen { 0xFFFF_FFFF } else { 0xFF20_2020 });
        game.native_fill(batch, [r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], 0xFF00_0000 | m::COLOURS[i].1);
    }
    let on = |b: bool| if b { "ON" } else { "OFF" };
    button(game, batch, l.use_pos, "Use my position", mouse);
    button(game, batch, l.dim, &format!("Dim: {}", m::pretty_id(&wp.dim)), mouse);
    button(game, batch, l.set, &format!("Set: {}", wp.set), mouse);
    if new {
        button(game, batch, l.create, "Create", mouse);
        button(game, batch, l.cancel, "Cancel", mouse);
        let tip = "Enter: create. Escape: cancel. Tab: next box.";
        game.native_text(batch, tip, [l.new_panel[0] + 8.0, l.new_panel[1] + l.new_panel[3] + 3.0], 0xA0A0A0, true);
        return;
    }
    button(game, batch, l.visible, &format!("Visible: {}", on(wp.shown)), mouse);
    button(game, batch, l.temporary, &format!("Temporary: {}", on(wp.temporary)), mouse);
    button(game, batch, l.scope, if wp.global { "Kept for: all worlds" } else { "Kept for: this world" }, mouse);
    button(game, batch, l.always_name, &format!("Always named: {}", on(wp.always_name)), mouse);
    button(game, batch, l.two_d, &format!("2D (no height): {}", on(wp.two_d)), mouse);
    button(game, batch, l.vanish, &format!("Gone when reached: {}", on(wp.vanish)), mouse);
    button(game, batch, l.on_screen, &format!("Screen: {}", on(wp.on_screen)), mouse);
    button(game, batch, l.on_minimap, &format!("Minimap: {}", on(wp.on_minimap)), mouse);
    button(game, batch, l.on_map, &format!("Map: {}", on(wp.on_map)), mouse);
    if may_teleport(game) {
        button(game, batch, l.teleport, "Teleport", mouse);
    }
    button(game, batch, l.copy, "Copy X Y Z", mouse);
    button(game, batch, l.share, "Share in chat", mouse);
    button(game, batch, l.duplicate, "Duplicate", mouse);
    button(game, batch, l.delete, "Delete", mouse);
    button(game, batch, l.done, "Done", mouse);
    let other = match wp.dim.as_str() {
        m::OVERWORLD => m::converted(wp.pos, m::OVERWORLD, m::NETHER).map(|p| format!("In the Nether: {} {} {}", p[0], p[1], p[2])),
        m::NETHER => m::converted(wp.pos, m::NETHER, m::OVERWORLD).map(|p| format!("In the Overworld: {} {} {}", p[0], p[1], p[2])),
        _ => None,
    };
    if let Some(t) = other {
        game.native_text(batch, &t, [l.panel[0] + 8.0, l.panel[1] + l.panel[3] + 3.0], 0xA0A0A0, true);
    }
}

/// A question with two buttons (deleting), or a name box (a set's new name).
fn draw_dialog(game: &Game<'_>, batch: &mut HudBatch, question: &str, field: Option<&str>, yes: &str, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let (r, [ok, cancel]) = dialog_layout(w, h);
    game.native_fill(batch, [r[0] - 8.0, r[1] - 20.0, r[2] + 16.0, 70.0], 0xD000_0000);
    game.native_text(batch, question, [r[0], r[1] - 12.0], 0xFFFFFF, true);
    if let Some(text) = field {
        text_box(game, batch, r, text);
    }
    button(game, batch, ok, yes, mouse);
    button(game, batch, cancel, "Cancel", mouse);
}

/// The settings screen: its page tabs, the rows of settings (two columns) and the back button.
fn settings_layout(w: f32, h: f32, n: usize) -> (Vec<[f32; 4]>, Vec<[f32; 4]>, [f32; 4]) {
    let pw = 420.0f32.min(w - 12.0);
    let x = (w - pw) / 2.0;
    let pages = m::PAGES.len();
    let tw = (pw - (pages - 1) as f32 * 2.0) / pages as f32;
    let tabs = (0..pages).map(|i| [x + i as f32 * (tw + 2.0), 30.0, tw, 18.0]).collect();
    // Two columns, or three when two would run off the screen.
    let cols = if 56.0 + n.div_ceil(2) as f32 * 22.0 > h - 30.0 { 3 } else { 2 };
    let col = (pw - 4.0 * (cols - 1) as f32) / cols as f32;
    let rows = (0..n)
        .map(|i| [x + (i % cols) as f32 * (col + 4.0), 56.0 + (i / cols) as f32 * 22.0, col, 20.0])
        .filter(|r| r[1] + r[3] < h - 30.0)
        .collect();
    (tabs, rows, [x + pw / 2.0 - 50.0, h - 26.0, 100.0, 20.0])
}

/// The map's settings screen: a tab per page, the page's settings as buttons (click for the next
/// choice, right-click for the one before), and the keys on the Keys page.
fn draw_settings(game: &Game<'_>, batch: &mut HudBatch, settings: &m::Settings, tab: usize, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let entries = settings.page(tab);
    let (tabs, rows, back) = settings_layout(w, h, entries.len());
    let title = format!("{} {} settings", m::NAME, m::VERSION);
    let tw = game.native_text_width(&title);
    game.native_text(batch, &title, [(w - tw) / 2.0, 12.0], 0xFFFFFF, true);
    for (i, r) in tabs.iter().enumerate() {
        if i == tab {
            game.native_fill(batch, [r[0] - 1.0, r[1] - 1.0, r[2] + 2.0, r[3] + 2.0], 0xFFE0_E0E0);
        }
        button(game, batch, *r, m::PAGES[i], mouse);
    }
    for (r, (_, label)) in rows.iter().zip(&entries) {
        button(game, batch, *r, label, mouse);
    }
    if tab == m::KEYS_PAGE {
        let x = tabs[0][0];
        let mut y = 58.0;
        game.native_text(batch, "Rebind these in Options > Controls > Key Binds, or on this mod's", [x, y], 0xC0C0C0, false);
        y += 10.0;
        game.native_text(batch, "page in Options > Mods. The player list key (Tab) shows faces.", [x, y], 0xC0C0C0, false);
        y += 16.0;
        for (name, label) in keys::labels() {
            let bound = keys::bound(game, name);
            let shown = horizon_client::keybinds::display_name(&bound).plain();
            game.native_text(batch, &format!("{label}: {shown}"), [x, y], 0xFFFFFF, false);
            y += 11.0;
        }
    } else {
        game.native_text(batch, "Click: next choice. Right-click: the one before.", [tabs[0][0], h - 40.0], 0x909090, false);
    }
    button(game, batch, back, "Back to map", mouse);
}

/// What the editor's boxes say put on its waypoint (not saved yet).
fn apply_form(s: &mut State) {
    match &mut s.screen.page {
        m::Page::Edit { form, draft: Some(d), .. } => form.apply(&mut d.wp),
        m::Page::Edit { index, form, draft: None } => {
            if let Some(w) = s.book.list.get_mut(*index) {
                form.apply(w);
            }
        }
        _ => {}
    }
}

/// The "New waypoint" box done: its waypoint added and saved (`create`), or dropped. Opened
/// with the key, the map closes; from the list, back to the list.
fn finish_new(s: &mut State, create: bool) {
    apply_form(s);
    let m::Page::Edit { draft: Some(d), .. } = std::mem::take(&mut s.screen.page) else {
        return;
    };
    let m::Draft { wp, close } = *d;
    let added = create.then(|| {
        let i = s.book.insert(wp);
        save_book(s);
        let w = &s.book.list[i];
        (format!("Added {} at {} {} {}", w.name, w.pos[0], w.pos[1], w.pos[2]), w.rgb())
    });
    if close {
        s.screen.page = m::Page::Map;
        s.pending_close = Some(added);
    } else {
        s.screen.page = m::Page::List { scroll: 0 };
        if let Some((text, _)) = added {
            notice(s, text);
        }
    }
}

/// The editor's boxes put on its waypoint, saved; back to `to`.
fn finish_edit(s: &mut State, to: m::Page) {
    if matches!(s.screen.page, m::Page::Edit { draft: None, .. }) {
        apply_form(s);
        save_book(s);
    }
    s.screen.page = to;
}

fn edit(s: &mut State, index: usize) {
    let form = m::Form::of(&s.book.list[index]);
    s.screen.selected.clear();
    s.screen.page = m::Page::Edit { index, form, draft: None };
}

/// `native_screen_mouse_button_pressed_pre`: buttons, rows, tools, and a press on the map.
pub(super) fn on_press(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    if !ours(game, screen.id) || *canceled {
        return;
    }
    *canceled = true;
    let (w, h) = game.gui_size();
    let mouse = screen.mouse;
    let here = player_pos(game).map(block_of);
    let dim = game.dim.id();
    let mut settings = settings(game);
    let mut settings_changed = false;
    let fallback_y = here.map_or(64, |p| p[1]);
    with(|s| match s.screen.page.clone() {
        // The right-click menu takes the next click, on a row or anywhere (closing it).
        m::Page::Map if s.screen.menu.is_some() => {
            let Some(menu) = s.screen.menu.take() else {
                return;
            };
            if button == 0 {
                let items = menu_items(game, s, &menu);
                let rects = m::menu_rects(menu.at, items.len(), w, h);
                if let Some(k) = rects.iter().position(|r| inside(*r, mouse)) {
                    menu_pick(game, s, &menu, items[k], &settings);
                }
            }
        }
        m::Page::Map if button == 1 => {
            let rect = map_rect(w, h);
            if inside(rect, mouse) {
                let waypoint = waypoint_at(s, &shown_dim(game, s), settings.convert, rect, mouse);
                let block = spot_at(game, s, rect, mouse, fallback_y);
                s.screen.menu = Some(m::Menu { at: mouse, block, waypoint });
            }
        }
        m::Page::Map if button == 0 => {
            let slime = game.native_world_seed().is_some();
            let hit = tools(w, s.subworlds.is_some(), slime).into_iter().find(|(_, r)| inside(*r, mouse));
            if let Some((tool, _)) = hit {
                let feet = here.map_or(64, |p| p[1]);
                let top = game.world.shape.max_y();
                match tool {
                    Tool::Waypoints => s.screen.page = m::Page::List { scroll: 0 },
                    Tool::Settings => s.screen.page = m::Page::Settings { tab: 0 },
                    Tool::OnMe => {
                        if let Some(p) = player_pos(game) {
                            s.screen.view.x = p[0];
                            s.screen.view.z = p[2];
                        }
                    }
                    Tool::Follow => s.screen.follow = !s.screen.follow,
                    Tool::Grid => s.screen.grid = !s.screen.grid,
                    Tool::Slime => s.screen.slime = !s.screen.slime,
                    Tool::LayerDown => step_layer(game, s, feet, top, true),
                    Tool::LayerUp => step_layer(game, s, feet, top, false),
                    Tool::Caves => {
                        let next = m::CaveType::from_index(cave_type(game, s).index() + 1);
                        set_cave_type(game, s, next);
                        notice(s, format!("Cave map here: {}", m::CAVE_TYPES[next.index()]));
                    }
                    Tool::Dimension => view_next_dimension(game, s),
                    Tool::FollowY => {
                        s.slice_follow = !s.slice_follow;
                        if s.slice_follow && cave_type(game, s) != m::CaveType::Layered {
                            set_cave_type(game, s, m::CaveType::Layered);
                        }
                    }
                    Tool::Filter => s.screen.filter = (s.screen.filter + 1) % m::MAP_FILTERS.len(),
                    Tool::Png => s.pending_export = true,
                    Tool::World => switch_subworld(game, s, false),
                    Tool::NewWorld => switch_subworld(game, s, true),
                }
            } else if inside(map_rect(w, h), mouse) {
                s.screen.press = Some((mouse, false));
            }
        }
        m::Page::Map => {}
        m::Page::List { scroll } => {
            let l = list_layout(w, h);
            let [set, set_shown, new_set, rename_set, del_set] = l.head;
            let [del, hide, show, to_set, view] = l.bulk;
            let [back, order, from_chat, add] = l.foot;
            let shown = listed(s, &settings, player_pos(game));
            let hit = (0..l.rows).find(|&i| inside(list_row(l.panel, i), mouse));
            let row = hit.and_then(|i| shown.get(scroll + i).copied());
            s.screen.searching = button == 0 && inside(l.search, mouse);
            if button == 1 {
                // Right-click: hidden for now, or shown again.
                if let Some(i) = row {
                    s.book.list[i].shown = !s.book.list[i].shown;
                    save_book(s);
                }
                return;
            }
            if button != 0 {
                return;
            }
            // The bulk buttons act on the ticked waypoints, else on every one listed.
            let targets = if s.screen.selected.is_empty() { shown.clone() } else { s.screen.selected.clone() };
            if let (Some(i), Some(r)) = (row, hit.map(|k| list_row(l.panel, k)))
                && inside(row_tick(r), mouse)
            {
                if let Some(at) = s.screen.selected.iter().position(|&x| x == i) {
                    s.screen.selected.remove(at);
                } else {
                    s.screen.selected.push(i);
                }
            } else if inside(set, mouse) {
                s.book.next_set();
                save_book(s);
            } else if inside(set_shown, mouse) {
                let c = s.book.current;
                s.book.sets[c].shown = !s.book.sets[c].shown;
                save_book(s);
            } else if inside(new_set, mouse) {
                s.screen.page = m::Page::NewSet { name: String::new() };
            } else if inside(rename_set, mouse) {
                s.screen.page = m::Page::RenameSet { name: s.book.current_set().to_owned() };
            } else if inside(del_set, mouse) {
                s.book.remove_set();
                s.screen.selected.clear();
                save_book(s);
            } else if inside(del, mouse) {
                // Without ticks, only the death points' "Clear all" deletes the whole list.
                let delete = if s.screen.selected.is_empty() && s.screen.list_view != 2 { Vec::new() } else { targets };
                if delete.is_empty() {
                    notice(s, "Tick the waypoints to delete first".into());
                } else if game.native_has_shift_down() {
                    s.book.remove(&delete);
                    s.screen.selected.clear();
                    save_book(s);
                } else {
                    s.screen.page = m::Page::Confirm { delete };
                }
            } else if inside(hide, mouse) || inside(show, mouse) {
                if s.book.set_shown(&targets, inside(show, mouse)) {
                    save_book(s);
                }
            } else if inside(to_set, mouse) {
                let set = s.book.current_set().to_owned();
                s.book.move_to_set(&targets, &set);
                save_book(s);
            } else if inside(view, mouse) {
                s.screen.list_view = (s.screen.list_view + 1) % m::LIST_VIEWS.len();
                s.screen.selected.clear();
                s.screen.page = m::Page::List { scroll: 0 };
            } else if inside(back, mouse) {
                s.screen.page = m::Page::Map;
            } else if inside(order, mouse) {
                settings.sort = (settings.sort + 1) % m::SORTS.len();
                settings_changed = true;
            } else if inside(from_chat, mouse) {
                if let Some((_, at)) = s.chat.pop_back() {
                    let i = s.book.add(at, dim, now_secs());
                    s.book.style_new(i, settings.wp_colour, settings.wp_symbol);
                    save_book(s);
                    edit(s, i);
                }
            } else if inside(add, mouse) {
                // The "New waypoint" box, as the key opens it (Create and Cancel come back here).
                if let Some(pos) = here {
                    let mut wp = s.book.draft(pos, dim, now_secs());
                    m::style_new(&mut wp, settings.wp_colour, settings.wp_symbol);
                    s.screen.selected.clear();
                    s.screen.page = m::Page::new_waypoint(wp, false);
                }
            } else if let Some(i) = row {
                edit(s, i);
            }
        }
        m::Page::Edit { draft: Some(_), .. } if button == 0 => {
            let l = edit_layout(w, h);
            if let Some(f) = l.fields.iter().position(|r| inside(*r, mouse)) {
                if let m::Page::Edit { form, .. } = &mut s.screen.page {
                    form.focus = Some(f);
                }
                return;
            }
            if inside(l.create, mouse) || inside(l.cancel, mouse) {
                finish_new(s, inside(l.create, mouse));
                return;
            }
            apply_form(s);
            let sets: Vec<String> = s.book.sets.iter().map(|x| x.name.clone()).collect();
            if let m::Page::Edit { form, draft: Some(d), .. } = &mut s.screen.page {
                let wp = &mut d.wp;
                if let Some(c) = l.swatches.iter().position(|r| inside(*r, mouse)) {
                    wp.colour = c;
                    wp.custom = None;
                } else if inside(l.use_pos, mouse) {
                    if let Some(pos) = here {
                        wp.pos = pos;
                        wp.dim = dim.to_owned();
                    }
                } else if inside(l.dim, mouse) {
                    let all = dimensions(dim);
                    let at = all.iter().position(|x| *x == wp.dim).unwrap_or(0);
                    wp.dim = all[(at + 1) % all.len()].clone();
                } else if inside(l.set, mouse) && !sets.is_empty() {
                    let at = sets.iter().position(|x| *x == wp.set).unwrap_or(0);
                    wp.set = sets[(at + 1) % sets.len()].clone();
                }
                // The boxes show the waypoint again; the name typed so far and the focus stay.
                let (name, focus) = (std::mem::take(&mut form.fields[m::F_NAME]), form.focus);
                *form = m::Form::of(wp);
                form.fields[m::F_NAME] = name;
                form.focus = focus;
            }
        }
        m::Page::Edit { index, .. } if button == 0 => {
            let l = edit_layout(w, h);
            if index >= s.book.list.len() {
                s.screen.page = m::Page::List { scroll: 0 };
                return;
            }
            // Text boxes take the focus; anything else puts what was typed on the waypoint.
            if let Some(f) = l.fields.iter().position(|r| inside(*r, mouse)) {
                if let m::Page::Edit { form, .. } = &mut s.screen.page {
                    form.focus = Some(f);
                }
                return;
            }
            apply_form(s);
            let mut to: Option<m::Page> = None;
            if let Some(c) = l.swatches.iter().position(|r| inside(*r, mouse)) {
                s.book.list[index].colour = c;
                s.book.list[index].custom = None;
            } else if inside(l.use_pos, mouse) {
                if let Some(pos) = here {
                    s.book.list[index].pos = pos;
                    s.book.list[index].dim = dim.to_owned();
                }
            } else if inside(l.dim, mouse) {
                let all = dimensions(dim);
                let at = all.iter().position(|d| *d == s.book.list[index].dim).unwrap_or(0);
                s.book.list[index].dim = all[(at + 1) % all.len()].clone();
            } else if inside(l.set, mouse) {
                let at = s.book.set_index(&s.book.list[index].set).unwrap_or(0);
                s.book.list[index].set = s.book.sets[(at + 1) % s.book.sets.len()].name.clone();
            } else if inside(l.visible, mouse) {
                s.book.list[index].shown = !s.book.list[index].shown;
            } else if inside(l.temporary, mouse) {
                s.book.list[index].temporary = !s.book.list[index].temporary;
            } else if inside(l.scope, mouse) {
                s.book.list[index].global = !s.book.list[index].global;
            } else if inside(l.always_name, mouse) {
                s.book.list[index].always_name = !s.book.list[index].always_name;
            } else if inside(l.two_d, mouse) {
                s.book.list[index].two_d = !s.book.list[index].two_d;
            } else if inside(l.vanish, mouse) {
                s.book.list[index].vanish = !s.book.list[index].vanish;
            } else if inside(l.on_screen, mouse) {
                s.book.list[index].on_screen = !s.book.list[index].on_screen;
            } else if inside(l.on_minimap, mouse) {
                s.book.list[index].on_minimap = !s.book.list[index].on_minimap;
            } else if inside(l.on_map, mouse) {
                s.book.list[index].on_map = !s.book.list[index].on_map;
            } else if inside(l.teleport, mouse) && may_teleport(game) {
                if s.book.list[index].dim == dim {
                    s.pending_command = Some(format!("tp @s {} {} {}", s.book.list[index].pos[0], s.book.list[index].pos[1], s.book.list[index].pos[2]));
                    to = Some(m::Page::Map);
                } else {
                    notice(s, "That waypoint is in another dimension".into());
                }
            } else if inside(l.copy, mouse) {
                s.pending_clipboard = Some(format!("{} {} {}", s.book.list[index].pos[0], s.book.list[index].pos[1], s.book.list[index].pos[2]));
            } else if inside(l.share, mouse) {
                s.pending_chat = Some(m::share_text(&s.book.list[index]));
                to = Some(m::Page::Map);
            } else if inside(l.duplicate, mouse) {
                if let Some(i) = s.book.duplicate(index, now_secs()) {
                    save_book(s);
                    edit(s, i);
                    return;
                }
            } else if inside(l.delete, mouse) {
                if game.native_has_shift_down() {
                    s.book.remove(&[index]);
                    s.screen.selected.clear();
                    to = Some(m::Page::List { scroll: 0 });
                } else {
                    to = Some(m::Page::Confirm { delete: vec![index] });
                }
            } else if inside(l.done, mouse) {
                to = Some(m::Page::List { scroll: 0 });
            }
            save_book(s);
            match to {
                Some(page) => s.screen.page = page,
                None => {
                    if let m::Page::Edit { form, .. } = &mut s.screen.page {
                        *form = m::Form::of(&s.book.list[index]);
                        form.focus = None;
                    }
                }
            }
        }
        m::Page::Confirm { delete } if button == 0 => {
            let (_, [ok, cancel]) = dialog_layout(w, h);
            if inside(ok, mouse) {
                s.book.remove(&delete);
                s.screen.selected.clear();
                save_book(s);
                s.screen.page = m::Page::List { scroll: 0 };
            } else if inside(cancel, mouse) {
                s.screen.page = m::Page::List { scroll: 0 };
            }
        }
        m::Page::RenameSet { name } if button == 0 => {
            let (_, [ok, cancel]) = dialog_layout(w, h);
            if inside(ok, mouse) {
                if !s.book.rename_set(&name) {
                    notice(s, "That set name is empty or taken".into());
                }
                save_book(s);
                s.screen.page = m::Page::List { scroll: 0 };
            } else if inside(cancel, mouse) {
                s.screen.page = m::Page::List { scroll: 0 };
            }
        }
        m::Page::Settings { tab } => {
            let (tabs, rows, back) = settings_layout(w, h, settings.page(tab).len());
            if button == 0 {
                if let Some(t) = tabs.iter().position(|r| inside(*r, mouse)) {
                    s.screen.page = m::Page::Settings { tab: t };
                    return;
                }
                if inside(back, mouse) {
                    s.screen.page = m::Page::Map;
                    return;
                }
            }
            if button > 1 {
                return;
            }
            let entries = settings.page(tab);
            if let Some(i) = rows.iter().position(|r| inside(*r, mouse))
                && let Some(&(field, _)) = entries.get(i)
            {
                settings.click_dir(field, button == 1);
                settings_changed = true;
            }
        }
        m::Page::NewSet { name } if button == 0 => {
            let (_, [create, cancel]) = new_set_layout(w, h);
            if inside(create, mouse) {
                s.book.add_set(&name);
                save_book(s);
                s.screen.page = m::Page::List { scroll: 0 };
            } else if inside(cancel, mouse) {
                s.screen.page = m::Page::List { scroll: 0 };
            }
        }
        _ => {}
    });
    if settings_changed {
        m::set_settings(settings);
        for (k, v) in settings.stored() {
            game.native_set_setting(m::ID, k, &v);
        }
    }
}

/// `native_screen_mouse_button_released_pre`: a click (not a drag) on the map opens the
/// waypoint under it, or adds one there.
pub(super) fn on_release(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    if !ours(game, screen.id) || button != 0 {
        return;
    }
    *canceled = true;
    let (w, h) = game.gui_size();
    let rect = map_rect(w, h);
    let mouse = screen.mouse;
    let set = settings(game);
    let convert = set.convert;
    let fallback_y = player_pos(game).map_or(64, |p| p[1].floor() as i32);
    with(|s| {
        if s.screen.page != m::Page::Map || !s.screen.release() || !inside(rect, mouse) {
            return;
        }
        let dim = shown_dim(game, s);
        let dim = dim.as_str();
        let hit = waypoint_at(s, dim, convert, rect, mouse);
        let index = hit.unwrap_or_else(|| {
            let [x, y, z] = spot_at(game, s, rect, mouse, fallback_y);
            let i = s.book.add([x, y, z], dim, now_secs());
            s.book.style_new(i, set.wp_colour, set.wp_symbol);
            save_book(s);
            i
        });
        edit(s, index);
    });
}

/// The waypoint drawn under GUI point `mouse` of the world map.
fn waypoint_at(s: &State, dim: &str, convert: bool, rect: [f32; 4], mouse: [f32; 2]) -> Option<usize> {
    let view = s.screen.view;
    s.book.list.iter().position(|wp| {
        let Some(c) = wp.place_in(dim, convert) else {
            return false;
        };
        let [x, y] = view.screen_of(rect, c[0], c[2]);
        on_map(s, wp) && (mouse[0] - x).abs() <= 6.0 && (mouse[1] - y).abs() <= 6.0
    })
}

/// The block to stand on at GUI point `mouse` of the world map: over the floor the map shows
/// there, else at `fallback_y`.
fn spot_at(game: &Game<'_>, s: &State, rect: [f32; 4], mouse: [f32; 2], fallback_y: i32) -> [i32; 3] {
    let (x, z) = s.screen.view.world_at(rect, mouse);
    let (x, z) = (x.floor() as i32, z.floor() as i32);
    let column = map_column(s, map_source(game, s), x, z).filter(|c| c.kind == m::FLOOR);
    [x, column.map_or(fallback_y, |c| i32::from(c.height) + 1), z]
}

/// The right-click menu's rows now.
fn menu_items(game: &Game<'_>, s: &State, menu: &m::Menu) -> Vec<m::MenuItem> {
    let here = menu.waypoint.and_then(|i| s.book.list.get(i)).is_some_and(|w| w.dim == game.dim.id());
    // No teleport into another dimension's map.
    m::menu_items(menu, may_teleport(game) && s.viewed.is_none(), here)
}

/// The dimension the world map shows: another one's saved map, or the one you are in.
fn shown_dim(game: &Game<'_>, s: &State) -> String {
    s.viewed.as_ref().map_or_else(|| game.dim.id().to_owned(), |v| v.dim.clone())
}

/// A row of the right-click menu chosen.
fn menu_pick(game: &Game<'_>, s: &mut State, menu: &m::Menu, item: m::MenuItem, settings: &m::Settings) {
    let dim = shown_dim(game, s);
    let dim = dim.as_str();
    match (item, menu.waypoint) {
        (m::MenuItem::WaypointHere, _) => {
            let i = s.book.add(menu.block, dim, now_secs());
            s.book.style_new(i, settings.wp_colour, settings.wp_symbol);
            save_book(s);
            edit(s, i);
        }
        (m::MenuItem::TeleportHere, _) => s.pending_command = Some(m::tp_command(menu.block)),
        (m::MenuItem::Edit, Some(i)) if i < s.book.list.len() => edit(s, i),
        (m::MenuItem::Hide, Some(i)) if i < s.book.list.len() => {
            s.book.list[i].shown = false;
            save_book(s);
            notice(s, format!("Hid {} (the list shows it again)", s.book.list[i].name));
        }
        (m::MenuItem::Delete, Some(i)) if i < s.book.list.len() => s.screen.page = m::Page::Confirm { delete: vec![i] },
        (m::MenuItem::TeleportTo, Some(i)) if i < s.book.list.len() => {
            s.pending_command = Some(m::tp_command(s.book.list[i].pos));
        }
        _ => {}
    }
}

/// `native_screen_mouse_dragged_pre_v2`: a drag on the map moves it.
pub(super) fn on_drag(game: &mut Game<'_>, screen: &str, mouse: [f32; 2], delta: [f32; 2], canceled: &mut bool) {
    if !ours(game, screen) {
        return;
    }
    *canceled = true;
    with(|s| {
        if s.screen.page == m::Page::Map {
            s.screen.drag(mouse, delta);
        }
    });
}

/// `native_screen_mouse_scrolled_pre_v2`: the wheel zooms the map about the pointer, or scrolls
/// the list.
pub(super) fn on_scroll(game: &mut Game<'_>, screen: &str, mouse: [f32; 2], scroll: [f64; 2], canceled: &mut bool) {
    if !ours(game, screen) || scroll[1] == 0.0 {
        return;
    }
    *canceled = true;
    let (w, h) = game.gui_size();
    let step = if scroll[1] > 0.0 { 1 } else { -1 };
    let (lo, hi) = settings(game).map_zooms();
    let set = settings(game);
    let here = player_pos(game);
    with(|s| {
        let max = listed(s, &set, here).len().saturating_sub(m::list_rows(h));
        match &mut s.screen.page {
            m::Page::Map => s.screen.view.zoom_at(map_rect(w, h), mouse, step, lo, hi),
            m::Page::List { scroll } => {
                *scroll = (*scroll as i64 - i64::from(step)).clamp(0, max as i64) as usize;
            }
            _ => {}
        }
    });
}

/// `native_screen_key_pressed_pre`: typing names; WASD or the arrows move the map, [ and ] step
/// its layer; Escape and Enter step back a page instead of closing the map.
pub(super) fn on_key_pressed(game: &Game<'_>, screen: &NativeScreenView<'_>, key: &str, canceled: &mut bool) {
    if !ours(game, screen.id) || *canceled {
        return;
    }
    let (w, h) = game.gui_size();
    let feet = player_pos(game).map_or(64, |p| p[1].floor() as i32);
    let top = game.world.shape.max_y();
    with(|s| {
        let typing = s.screen.typing();
        match key {
            "key.keyboard.escape" | "key.keyboard.enter" | "key.keyboard.keypad.enter" | "key.keyboard.tab" if typing => {
                let escape = key == "key.keyboard.escape";
                match s.screen.page.clone() {
                    m::Page::NewSet { name } => {
                        if !escape {
                            s.book.add_set(&name);
                            save_book(s);
                        }
                        s.screen.page = m::Page::List { scroll: 0 };
                    }
                    m::Page::RenameSet { name } => {
                        if !escape {
                            s.book.rename_set(&name);
                            save_book(s);
                        }
                        s.screen.page = m::Page::List { scroll: 0 };
                    }
                    m::Page::Edit { .. } if key == "key.keyboard.tab" => {
                        if let m::Page::Edit { form, .. } = &mut s.screen.page {
                            form.focus = Some(form.focus.map_or(0, |f| (f + 1) % 6));
                        }
                    }
                    m::Page::Edit { draft: Some(_), .. } => finish_new(s, !escape),
                    m::Page::Edit { .. } if escape => finish_edit(s, m::Page::List { scroll: 0 }),
                    m::Page::Edit { index, .. } => {
                        apply_form(s);
                        save_book(s);
                        if let (m::Page::Edit { form, .. }, Some(w)) = (&mut s.screen.page, s.book.list.get(index)) {
                            *form = m::Form::of(w);
                            form.focus = None;
                        }
                    }
                    _ => s.screen.searching = false,
                }
                *canceled = true;
            }
            "key.keyboard.escape" if matches!(s.screen.page, m::Page::Edit { .. }) => {
                finish_edit(s, m::Page::List { scroll: 0 });
                *canceled = true;
            }
            "key.keyboard.escape" if s.screen.menu.is_some() => {
                s.screen.menu = None;
                *canceled = true;
            }
            "key.keyboard.escape" if matches!(s.screen.page, m::Page::Confirm { .. }) => {
                s.screen.page = m::Page::List { scroll: 0 };
                *canceled = true;
            }
            "key.keyboard.escape" if matches!(s.screen.page, m::Page::List { .. } | m::Page::Settings { .. }) => {
                s.screen.page = m::Page::Map;
                *canceled = true;
            }
            "key.keyboard.backspace" if typing => {
                match &mut s.screen.page {
                    m::Page::Edit { form, .. } => form.backspace(),
                    m::Page::NewSet { name } | m::Page::RenameSet { name } => {
                        name.pop();
                    }
                    _ => {
                        s.screen.search.pop();
                    }
                }
                *canceled = true;
            }
            // Letters type, they don't act (the inventory key doesn't close the map).
            _ if typing => *canceled = true,
            _ if s.screen.page == m::Page::Map => {
                let rect = map_rect(w, h);
                let pan = match key {
                    "key.keyboard.w" | "key.keyboard.up" => Some((0, -1)),
                    "key.keyboard.s" | "key.keyboard.down" => Some((0, 1)),
                    "key.keyboard.a" | "key.keyboard.left" => Some((-1, 0)),
                    "key.keyboard.d" | "key.keyboard.right" => Some((1, 0)),
                    _ => None,
                };
                if let Some(dir) = pan {
                    s.screen.key_pan(rect, dir);
                    *canceled = true;
                } else if key == "key.keyboard.left.bracket" {
                    step_layer(game, s, feet, top, true);
                    *canceled = true;
                } else if key == "key.keyboard.right.bracket" {
                    step_layer(game, s, feet, top, false);
                    *canceled = true;
                }
            }
            _ => {}
        }
    });
}

/// `native_screen_char_typed_pre`: a character of a name.
pub(super) fn on_char(game: &Game<'_>, screen: &NativeScreenView<'_>, c: char, canceled: &mut bool) {
    if !ours(game, screen.id) {
        return;
    }
    with(|s| {
        // The add-waypoint key's own character, handed to the box it just opened.
        if let Some((k, at)) = s.swallow.take()
            && at.elapsed() < Duration::from_millis(250)
            && c.to_lowercase().eq(k.to_lowercase())
        {
            *canceled = true;
            return;
        }
        let searching = s.screen.searching && matches!(s.screen.page, m::Page::List { .. });
        match &mut s.screen.page {
            m::Page::Edit { form, .. } if form.focus.is_some() => {
                form.type_char(c);
                *canceled = true;
            }
            // The "New waypoint" box keeps every character from the game.
            m::Page::Edit { draft: Some(_), .. } => *canceled = true,
            m::Page::NewSet { name } | m::Page::RenameSet { name } => {
                if !c.is_control() && name.chars().count() < m::NAME_MAX {
                    name.push(c);
                }
                *canceled = true;
            }
            m::Page::List { .. } if searching => {
                if !c.is_control() && s.screen.search.chars().count() < 24 {
                    s.screen.search.push(c);
                }
                *canceled = true;
            }
            _ => {}
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
    let s = settings(game);
    if page == m::KEYS_PAGE {
        for (name, label) in keys::labels() {
            let bound = keys::bound(game, name);
            buttons.push(format!("{label}: {}", horizon_client::keybinds::display_name(&bound).plain()));
        }
    } else {
        buttons.extend(s.page(page).into_iter().map(|(_, l)| l));
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
    let mut s = settings(game);
    let Some(&(field, _)) = s.page(page).get(button - 1) else {
        return;
    };
    s.click(field);
    m::set_settings(s);
    for (key, value) in s.stored() {
        game.native_set_setting(m::ID, key, &value);
    }
}
