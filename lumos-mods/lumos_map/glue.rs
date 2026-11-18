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
/// The most entity markers drawn.
const MAX_DOTS: usize = 400;

/// The map screen's bars, in GUI pixels.
const TOP: f32 = 22.0;
const BOTTOM: f32 = 14.0;

// Column flags of the scan's palette.
const FLUID: u8 = 3;
const AIR: u8 = 4;

/// Per block state: its map colour, flags and colour from above.
struct Palette {
    color: Vec<u8>,
    flags: Vec<u8>,
    look: Vec<m::BlockColour>,
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
            }
            flags.push(b);
        }
        Self { color, flags, look }
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
    wall: bool,
    found: bool,
}

/// One chunk's columns: the first block with a map colour going down from `start` (the top of
/// the chunk, or a layer: a block at the layer itself is a wall), and the water over the ground;
/// with a ceiling and no layer, from the first open space under the roof. Colours take the
/// biome tints blended across the quarts around each block (no seams between biomes), and water
/// shows the ground under it.
#[allow(clippy::too_many_arguments)]
fn scan(
    world: &horizon_client::world::World,
    chunk: &Chunk,
    palette: &Palette,
    air: u32,
    ceiling: bool,
    layer: Option<i32>,
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
            if let Some(layer) = layer {
                let (color, flags) = palette.get(chunk.get(x, layer, z));
                if flags & AIR == 0 && color != 0 && flags & FLUID == 0 {
                    wall = true;
                    y = layer;
                } else {
                    y = y.min(layer);
                }
            } else if ceiling {
                y = y.min(min + 118);
                while y > min && palette.get(chunk.get(x, y, z)).1 & AIR == 0 {
                    y -= 1;
                }
            }
            while y > min {
                let id = chunk.get(x, y, z);
                let (color, flags) = palette.get(id);
                if color == 0 {
                    y -= 1;
                    continue;
                }
                let mut hit = Hit { id, color, y, wall, found: true, ..Hit::default() };
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
            let t = m::blend_tints(&grid, x, z);
            let mut rgb = colour(h.id, &t).unwrap_or(0);
            if let Some((gid, gc)) = h.ground
                && rgb != 0
            {
                let ground = colour(gid, &t).unwrap_or_else(|| map_rgb(gc));
                rgb = m::water_over(ground, rgb, h.depth).max(1);
            }
            if h.wall {
                // Walls cut at the layer: darker, so the floor stands out.
                let base = if rgb != 0 { rgb } else { map_rgb(h.color) };
                rgb = m::tinted(base, 0x707070).max(1);
            }
            tile.color[i] = h.color;
            tile.rgb[i] = rgb;
            tile.height[i] = h.y.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            tile.depth[i] = h.depth;
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
    loading: Option<Receiver<Vec<((i32, i32), m::Tile)>>>,
}

impl Explored {
    fn open(world: &Path, dim: &'static str) -> Self {
        let dir = world.join(m::folder_name(dim));
        let (tx, rx) = mpsc::channel();
        let read = dir.clone();
        // The saved regions load off the frame; the session's own tiles stay over them.
        let _ = std::thread::Builder::new()
            .name("lumos-map-load".into())
            .spawn(move || {
                let mut all = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&read) {
                    for e in entries.flatten() {
                        let name = e.file_name();
                        let Some(region) = name.to_str().and_then(m::region_from_file) else {
                            continue;
                        };
                        if let Ok(bytes) = std::fs::read(e.path())
                            && let Some(tiles) = m::decode_region(region, &bytes)
                        {
                            all.extend(tiles);
                        }
                    }
                }
                let _ = tx.send(all);
            });
        Self {
            dim,
            dir,
            store: m::Store::default(),
            seen: HashSet::new(),
            queue: VecDeque::new(),
            refilled: None,
            near: (None, 0),
            saved: Instant::now(),
            loading: Some(rx),
        }
    }

    /// Writes the regions changed since the last save, off the frame.
    fn save(&mut self) {
        self.saved = Instant::now();
        if self.store.dirty.is_empty() {
            return;
        }
        let regions: Vec<(i32, i32)> = self.store.dirty.drain().collect();
        let files: Vec<(PathBuf, Vec<u8>)> = regions
            .into_iter()
            .map(|r| (self.dir.join(m::region_file(r)), self.store.encode_region(r)))
            .collect();
        let dir = self.dir.clone();
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
}

/// Tiles of one layer (under the player's head, or the world map's slice), scanned live from the
/// loaded chunks and not kept.
#[derive(Default)]
struct Layer {
    y: i32,
    tiles: HashMap<(i32, i32), (m::Tile, u64)>,
    queue: VecDeque<(i32, i32)>,
    revision: u64,
    /// Chunks scanned since the maps last looked, and a count that goes up when it starts over.
    changed: Vec<(i32, i32)>,
    epoch: u64,
}

impl Layer {
    fn column(&self, x: i32, z: i32) -> Option<m::Col> {
        self.tiles.get(&(x >> 4, z >> 4))?.0.col(x, z)
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
    /// Entity faces in the slot's icon row.
    icons: Icons,
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
    /// The world map saved as a PNG on the next frame.
    pending_export: bool,
    /// A notice on the map's bar, until when.
    notice: Option<(String, Instant)>,
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
    p.map(|c| c.floor() as i32)
}

fn save_book(s: &State) {
    let Some(dir) = &s.world_dir else {
        return;
    };
    let _ = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join("waypoints.txt"), s.book.to_text()));
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
    let State { explored, palette, tints, .. } = s;
    let (Some(e), Some(palette)) = (explored.as_mut(), palette.as_ref()) else {
        return;
    };
    if let Some(rx) = &e.loading
        && let Ok(tiles) = rx.try_recv()
    {
        e.store.merge_loaded(tiles);
        e.loading = None;
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
            .copied()
            .collect();
        if world.revision != e.near.1 && e.near.0.is_none_or(|t| t.elapsed() >= NEAR_RESCAN) {
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
    let ceiling = world.shape.has_ceiling;
    let mut tint = |b: u16, x: i32, z: i32| tints_of(game, tints, b, x, z);
    while started.elapsed() < budget {
        let Some(k) = e.queue.pop_front() else {
            break;
        };
        let Some(chunk) = world.chunks.get(&k) else {
            continue;
        };
        e.store.put(k, scan(world, chunk, palette, air, ceiling, None, &mut tint));
        e.seen.insert(k);
    }
    if e.saved.elapsed() >= SAVE_EVERY {
        e.save();
    }
}

/// Whether the minimap shows the floor under the player's head: in a level with a ceiling, or
/// where no sky light reaches the head.
fn cave_height(game: &Game<'_>, settings: &m::Settings) -> Option<i32> {
    if !settings.cave {
        return None;
    }
    let p = game.player.as_ref()?;
    let head = (p.position.y + f64::from(p.dimensions.eye_height)).floor() as i32;
    let (bx, bz) = (p.position.x.floor() as i32, p.position.z.floor() as i32);
    let covered = game.world.light(bx, head, bz) >> 4 == 0;
    (game.world.shape.has_ceiling || covered).then_some(head)
}

/// Keeps a layer at `y` scanned within `radius` chunks of `centre`, within `budget`: the cave
/// layer, or the world map's slice.
#[allow(clippy::too_many_arguments)]
fn follow_layer(
    game: &Game<'_>,
    s: &mut State,
    slice: bool,
    y: i32,
    centre: (i32, i32),
    radius: i32,
    budget: Duration,
) {
    let started = Instant::now();
    let State { cave, slice: sliced, palette, tints, .. } = s;
    let Some(palette) = palette.as_ref() else {
        return;
    };
    let layer = if slice { sliced } else { cave }.get_or_insert_with(Layer::default);
    let world = &game.world;
    // A new height (by more than a step) or changed blocks: scanned again, nearest first.
    if (layer.y - y).abs() >= 2 || layer.tiles.is_empty() {
        if !layer.tiles.is_empty() || layer.y != y {
            layer.epoch += 1;
        }
        layer.y = y;
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
    let air = world.air.0;
    let mut tint = |b: u16, x: i32, z: i32| tints_of(game, tints, b, x, z);
    while started.elapsed() < budget {
        let Some(k) = layer.queue.pop_front() else {
            break;
        };
        let Some(chunk) = world.chunks.get(&k) else {
            continue;
        };
        let tile = scan(world, chunk, palette, air, false, Some(layer.y), &mut tint);
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
    }
}

/// The world map follows the player (when set) and keeps its layer scanned.
fn follow_map(game: &Game<'_>, s: &mut State) {
    if s.screen.follow
        && let Some(p) = player_pos(game)
    {
        s.screen.view.x = p[0];
        s.screen.view.z = p[2];
    }
    if let Some(y) = s.screen.slice {
        let view = s.screen.view;
        let centre = ((view.x.floor() as i32) >> 4, (view.z.floor() as i32) >> 4);
        let radius = game.settings.render_distance.max(2) + 1;
        follow_layer(game, s, true, y, centre, radius, CAVE_BUDGET * 2);
    } else {
        s.slice = None;
    }
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
fn mini_bpp(settings: &m::Settings) -> f32 {
    m::ZOOMS[settings.zoom.min(m::ZOOMS.len() - 1)]
}

/// Underground: the cave layer under the head, kept scanned around the player.
fn follow_cave(game: &Game<'_>, s: &mut State, settings: &m::Settings) {
    let Some(p) = player_pos(game) else {
        return;
    };
    if let Some(y) = cave_height(game, settings) {
        let size = m::SIZES[settings.size];
        let reach = size / 2.0 * mini_bpp(settings) * std::f32::consts::SQRT_2;
        let radius = ((reach / 16.0).ceil() as i32 + 1).min(game.settings.render_distance.max(2));
        let centre = ((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4);
        follow_layer(game, s, false, y, centre, radius, CAVE_BUDGET);
    } else {
        s.cave = None;
    }
}

/// A source's epoch for its cells: they are drawn again when it changes.
fn epoch_of(s: &State, source: u8) -> u64 {
    let own = match source {
        0 => s.explored.as_ref().map_or(0, |e| e.store.epoch),
        1 => s.cave.as_ref().map_or(0, |l| l.epoch),
        _ => s.slice.as_ref().map_or(0, |l| l.epoch),
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
                Some(e) => m::compose_cell(|x, z| e.store.column(x, z), &MAP_COLORS, look, key, &mut px),
                None => continue,
            },
            1 => match s.cave.as_ref() {
                Some(l) => m::compose_cell(|x, z| l.column(x, z), &MAP_COLORS, look, key, &mut px),
                None => continue,
            },
            _ => match s.slice.as_ref() {
                Some(l) => m::compose_cell(|x, z| l.column(x, z), &MAP_COLORS, look, key, &mut px),
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
    with(|s| {
        follow_colours(game, s);
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
        if open {
            if s.screen.page == m::Page::Map {
                follow_map(game, s);
            }
        } else if settings.minimap && !game.ui.hide_gui {
            follow_cave(game, s, &settings);
        }
        draw_wanted_cells(game, s, &settings, if open { CELL_BUDGET_OPEN } else { CELL_BUDGET });
        s.icons.follow(game);
    });
}

/// The client left its level: the explored map is saved and let go.
pub(super) fn on_level_unload(game: &mut Game<'_>) {
    with(|s| {
        if let Some(mut e) = s.explored.take() {
            e.save();
        }
        s.cave = None;
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
    clip: &[[f32; 2]],
    alpha: u8,
) {
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
            for q in m::fan_quads(&m::clip_polygon(&poly, clip)) {
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

/// `native_key`: M opens (or closes) the world map, B adds a waypoint where you stand.
pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || !game.native_on(m::ID) || game.player.is_none() {
        return;
    }
    if map_open(game) {
        let typing = with(|s| matches!(s.screen.page, m::Page::Edit { .. } | m::Page::NewSet { .. }));
        if !typing && keys::pressed(game, keys::MAP, key, true) {
            game.native_mod_screen_close(m::ID);
        }
        return;
    }
    if !game.native_playing() {
        return;
    }
    if keys::pressed(game, keys::MAP, key, true) {
        let Some(p) = player_pos(game) else {
            return;
        };
        if game.native_mod_screen_open(m::ID) {
            with(|s| {
                s.screen.view.x = p[0];
                s.screen.view.z = p[2];
                s.screen.page = m::Page::Map;
                s.screen.press = None;
            });
        }
    } else if keys::pressed(game, keys::WAYPOINT, key, true) {
        let Some(p) = player_pos(game) else {
            return;
        };
        let dim = game.dim.id();
        let added = with(|s| {
            s.world_dir.as_ref()?;
            let i = s.book.add(block_of(p), dim, now_secs());
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
        } else if keys::pressed(game, keys::CAVE, key, true) {
            s.cave = !s.cave;
            Some(format!("Cave view: {}", if s.cave { "On" } else { "Off" }))
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
    if !game.native_corner_shown(at) {
        return;
    }
    let size = m::SIZES[s.size];
    let bpp = mini_bpp(&s);
    let block = block_of(p);
    let mut lines: Vec<String> = Vec::new();
    if s.coords {
        lines.push(format!("{} {} {}", block[0], block[1], block[2]));
    }
    if s.biome {
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
    if s.light {
        let l = game.world.light(block[0], block[1], block[2]);
        lines.push(format!("Light {} (sky {})", l & 15, l >> 4));
    }
    if s.time {
        lines.push(m::clock(game.world.day_time));
    }
    // Room for the frame round the map.
    let pad = 3.0;
    let info_h = lines.len() as f32 * 10.0;
    let (x, y0) = corners.place(at, size + pad * 2.0, size + pad * 2.0 + info_h + if lines.is_empty() { 0.0 } else { 1.0 });
    let x = x + pad;
    let top = matches!(at, Corner::TopLeft | Corner::TopRight);
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
        for w in st.book.list.iter().filter(|w| st.book.visible(w)) {
            let Some(c) = w.place_in(dim, s.convert) else {
                continue;
            };
            let ((u, v), _) = m::clamp_to_map(to_map(c[0], c[2]), radius - 1.0, s.round);
            let (wx, wy) = ((cx + u).round(), (cy + v).round());
            game.native_fill(batch, [wx - 2.5, wy - 2.5, 5.0, 5.0], 0xFF00_0000);
            game.native_fill(batch, [wx - 1.5, wy - 1.5, 3.0, 3.0], 0xFF00_0000 | w.rgb());
        }
    });
    if s.compass {
        let reach = radius - 3.0;
        for (letter, dx, dy) in m::compass(game.yaw, s.rotate) {
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

/// A round dot (an octagon) with a dark rim.
fn draw_dot(game: &Game<'_>, batch: &mut HudBatch, at: [f32; 2], r: f32, argb: u32) {
    let rim = m::outline(at, r + 1.0, true);
    fill_polygon(game, batch, &rim, (argb & 0xFF00_0000) / 4 * 3 & 0xFF00_0000);
    fill_polygon(game, batch, &m::outline(at, r, true), argb);
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
    let size = m::ICON_SIZES[s.icon_size.min(m::ICON_SIZES.len() - 1)];
    let range = s.max_range();
    let t = game.clock.fraction();
    let near = |x: f64, z: f64| range.is_none_or(|r| (x - p[0]).powi(2) + (z - p[2]).powi(2) <= r * r);
    // Above or below you by more than a few blocks: fainter.
    let alpha = |y: f64| if (y - p[1]).abs() > 8.0 { 150u8 } else { 255 };
    // Dots, or (holding the player list key, or always) faces as faint badges with names.
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
            .filter(|(_, at)| near(at[0], at[2]))
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
        if list.len() >= MAX_DOTS {
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
        if !near(at.x, at.z) {
            continue;
        }
        let (model, texture) = horizon_client::mobs::render::body(mob);
        let name = if kind == m::DotKind::Player {
            s.names.then(|| game.remote_player_name(mob.id).map(str::to_owned)).flatten()
        } else {
            Some(mob.custom_name.clone().unwrap_or_else(|| m::pretty_id(&mob.kind.id)))
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
        let a = alpha(at[1]);
        let rgb = s.dot_rgb(kind);
        if !details {
            let r = if kind == m::DotKind::Player { 2.2 } else { 1.7 };
            draw_dot(game, batch, pos, r, u32::from(a) << 24 | rgb);
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
        let text = name.unwrap_or_default();
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
    if round {
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
    fill_polygon(game, batch, &outline, shade(0xFF) | 0x00_1A1C_20);
    let bpp = f64::from(mini_bpp(settings));
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
        let source = if st.cave.is_some() && cave_height(game, settings).is_some() { 1 } else { 0 };
        draw_cells(game, st, batch, source, lod, area, &to_screen, &outline, alpha);
    });
    // The frame: a dark rim with a faint light edge inside it.
    if round {
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
    if !s.markers {
        return;
    }
    let Some(cam) = game.native_camera() else {
        return;
    };
    let (w, h) = (batch.width, batch.height);
    let dim = game.dim.id();
    let range: Option<f64> = m::WP_RANGES.get(s.wp_range).and_then(|r| r.parse().ok());
    let eye = cam.position;
    // (distance, screen point, pushed to the edge, edge angle, waypoint data)
    let mut shown: Vec<(f64, [f32; 2], bool, f32, String, String, u32)> = Vec::new();
    with(|st| {
        for wp in st.book.list.iter().filter(|w| st.book.visible(w)) {
            let Some(c) = wp.place_in(dim, s.convert) else {
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
            shown.push((d, q, pushed || !front, angle, wp.initials(), wp.name.clone(), wp.rgb()));
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
    for (i, (d, q, edge, angle, initials, name, rgb)) in shown.into_iter().enumerate() {
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
        // Initials, dark on a light colour and light on a dark one.
        let lum = ((rgb >> 16) & 255) * 3 + ((rgb >> 8) & 255) * 6 + (rgb & 255);
        let ink = if lum > 1300 { 0x000000 } else { 0xFFFFFF };
        let text: String = initials.chars().take(if s.wp_shape == 0 { 1 } else { 2 }).collect();
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
        let show_name = match s.wp_names {
            1 => true,
            0 => looked == Some(i),
            _ => false,
        };
        if show_name && !name.is_empty() {
            let nw = game.native_text_width(&name);
            let (nx, ny) = ((x - nw / 2.0).round(), (y - r - 12.0).round());
            game.native_fill(batch, [nx - 2.0, ny - 1.0, nw + 4.0, 10.0], (alpha * 0x90 / 255) << 24);
            game.native_text(batch, &name, [nx, ny], rgb, false);
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
    OnMe,
    Follow,
    Grid,
    Slime,
    LayerDown,
    LayerUp,
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
    let mut out = vec![(Tool::Waypoints, [x, y, full, 16.0])];
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
        Tool::OnMe => "On me".into(),
        Tool::Follow => format!("Follow {}", on(s.screen.follow)),
        Tool::Grid => format!("Grid {}", on(s.screen.grid)),
        Tool::Slime => format!("Slime {}", on(s.screen.slime)),
        Tool::LayerDown => "Down".into(),
        Tool::LayerUp => "Up".into(),
        Tool::Filter => format!("Show: {}", m::MAP_FILTERS[s.screen.filter]),
        Tool::Png => "Save as PNG".into(),
        Tool::World => s
            .subworlds
            .as_ref()
            .map_or_else(String::new, |(_, w)| format!("Map: {}", w.current_name())),
        Tool::NewWorld => "New map world".into(),
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
fn list_layout(w: f32, h: f32) -> ([f32; 4], usize, [[f32; 4]; 4], [[f32; 4]; 4]) {
    let pw = 352.0f32.min(w - 12.0);
    let panel = [(w - pw) / 2.0, 10.0, pw, h - 20.0];
    let rows = m::list_rows(h);
    let x = panel[0] + 6.0;
    let unit = (pw - 12.0 - 6.0) / 11.0;
    let head = panel[1] + 22.0;
    let by = h - 34.0;
    let r = |at: f32, y: f32, w: f32| [x + at * unit + (at.ceil()) * 2.0, y, w * unit, 18.0];
    (
        panel,
        rows,
        [r(0.0, head, 4.0), r(4.0, head, 2.5), r(6.5, head, 2.0), r(8.5, head, 2.5)],
        [r(0.0, by, 2.5), r(2.5, by, 3.0), r(5.5, by, 3.0), r(8.5, by, 2.5)],
    )
}

fn list_row(panel: [f32; 4], i: usize) -> [f32; 4] {
    [panel[0] + 6.0, panel[1] + 44.0 + i as f32 * 22.0, panel[2] - 12.0, 20.0]
}

/// The editor's name box and buttons: colour, shown, set, move here, share, delete, done.
fn edit_layout(w: f32, h: f32) -> ([f32; 4], [[f32; 4]; 7]) {
    let pw = 220.0;
    let x = (w - pw) / 2.0;
    let y = (h / 2.0 - 110.0).max(4.0);
    let b = |i: f32| [x, y + 52.0 + i * 21.0, pw, 19.0];
    (
        [x, y + 14.0, pw, 18.0],
        [b(0.0), b(1.0), b(2.0), b(3.0), b(4.0), b(5.0), b(6.0)],
    )
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
    s.book.visible(w)
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
    game.native_fill(batch, [0.0, 0.0, w, h], 0xFF10_1014);
    let settings = settings(game);
    with(|s| match s.screen.page.clone() {
        m::Page::Map => draw_map(game, batch, s, &settings, mouse),
        m::Page::List { scroll } => draw_list(game, batch, s, &settings, scroll, mouse),
        m::Page::Edit { index, name } => draw_edit(game, batch, s, index, &name, mouse),
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
        let source = if s.slice.is_some() { 2 } else { 0 };
        let clip = [
            [rect[0], rect[1]],
            [rect[0] + rect[2], rect[1]],
            [rect[0] + rect[2], rect[1] + rect[3]],
            [rect[0], rect[1] + rect[3]],
        ];
        let to_screen = |x: f64, z: f64| view.screen_of(rect, x, z);
        draw_cells(game, s, batch, source, lod, [x0, z0, x1, z1], &to_screen, &clip, 255);
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
    let dim = game.dim.id();
    // Waypoints with their initials; names shown when zoomed in or pointed at.
    for wp in s.book.list.iter().filter(|w| on_map(s, w)) {
        let Some(c) = wp.place_in(dim, settings.convert) else {
            continue;
        };
        let [x, y] = view.screen_of(rect, c[0], c[2]);
        if !inside(rect, [x, y]) {
            continue;
        }
        let initials = wp.initials();
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
    // You: an arrow the way you face.
    if let Some(p) = player_pos(game) {
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
    let layer = s.screen.slice.map_or_else(|| "Surface".to_owned(), |y| format!("Y {y}"));
    let title = format!("World Map - {} - {layer}", m::pretty_id(dim));
    game.native_text(batch, &title, [6.0, 7.0], 0xFFFFFF, true);
    let slime = game.native_world_seed().is_some();
    for (tool, r) in tools(w, s.subworlds.is_some(), slime) {
        if tool == Tool::LayerDown {
            game.native_text(batch, &format!("Layer: {layer}"), [r[0], r[1] - 10.0], 0xC0C0C0, false);
        }
        button(game, batch, r, &tool_label(tool, s), mouse);
    }
    let hint = "Drag or WASD to move, wheel to zoom, [ ] layers, click to add a waypoint";
    let bottom = match &s.notice {
        Some((text, until)) if Instant::now() < *until => text.clone(),
        _ if inside(rect, mouse) => {
            let (x, z) = view.world_at(rect, mouse);
            let (bx, bz) = (x.floor() as i32, z.floor() as i32);
            let y = match (&s.slice, &s.explored) {
                (Some(l), _) => l.column(bx, bz),
                (None, Some(e)) => e.store.column(bx, bz),
                _ => None,
            }
            .map_or_else(String::new, |c| format!(" Y {}", c.height));
            let other = match dim {
                m::OVERWORLD => format!("   Nether {} {}", bx.div_euclid(8), bz.div_euclid(8)),
                m::NETHER => format!("   Overworld {} {}", bx * 8, bz * 8),
                _ => String::new(),
            };
            format!("X {bx}{y} Z {bz}{other}   {} block(s) a pixel", view.bpp())
        }
        _ => hint.to_owned(),
    };
    game.native_text(batch, &bottom, [6.0, h - BOTTOM + 3.0], 0xC0C0C0, false);
}

fn draw_list(game: &Game<'_>, batch: &mut HudBatch, s: &State, settings: &m::Settings, scroll: usize, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let (panel, rows, [set, set_shown, new_set, del_set], [back, order, from_chat, add]) = list_layout(w, h);
    game.native_fill(batch, panel, 0xC000_0000);
    let title = format!("Waypoints ({})  -  right-click a row to hide it", s.book.list.len());
    game.native_text(batch, &title, [panel[0] + 6.0, panel[1] + 8.0], 0xFFFFFF, true);
    let cur = &s.book.sets[s.book.current.min(s.book.sets.len() - 1)];
    button(game, batch, set, &format!("Set: {}", cur.name), mouse);
    button(game, batch, set_shown, &format!("Shown: {}", if cur.shown { "ON" } else { "OFF" }), mouse);
    button(game, batch, new_set, "New set", mouse);
    button(game, batch, del_set, "Delete set", mouse);
    let here = player_pos(game);
    let dim = game.dim.id();
    let ordered = s.book.order(settings.sort, here);
    for (i, &index) in ordered.iter().skip(scroll).take(rows).enumerate() {
        let wp = &s.book.list[index];
        let r = list_row(panel, i);
        let hover = inside(r, mouse);
        game.native_fill(batch, r, if hover { 0x6050_5070 } else { 0x4030_3040 });
        game.native_fill(batch, [r[0] + 4.0, r[1] + 6.0, 8.0, 8.0], 0xFF00_0000 | wp.rgb());
        let colour = if s.book.visible(wp) { 0xFFFFFF } else { 0x707070 };
        let label = if wp.set == s.book.current_set() { wp.name.clone() } else { format!("{} ({})", wp.name, wp.set) };
        game.native_text(batch, &label, [r[0] + 16.0, r[1] + 6.0], colour, false);
        let mut info = format!("{} {} {}", wp.pos[0], wp.pos[1], wp.pos[2]);
        if wp.dim != dim {
            info = format!("{info}  {}", m::pretty_id(&wp.dim));
        } else if let Some(p) = here {
            info = format!("{info}  {}", m::format_distance(m::distance(p, wp.centre())));
        }
        let iw = game.native_text_width(&info);
        game.native_text(batch, &info, [r[0] + r[2] - iw - 4.0, r[1] + 6.0], 0xA0A0A0, false);
    }
    if s.book.list.is_empty() {
        let t = "No waypoints yet: B in the world, a click on the map, or Add here";
        let tw = game.native_text_width(t);
        game.native_text(batch, t, [(w - tw) / 2.0, panel[1] + 60.0], 0xA0A0A0, false);
    }
    button(game, batch, back, "Back to map", mouse);
    button(game, batch, order, &format!("Order: {}", m::SORTS[settings.sort]), mouse);
    let chat = match s.chat.back() {
        Some(_) => format!("From chat ({})", s.chat.len()),
        None => "From chat".to_owned(),
    };
    button(game, batch, from_chat, &chat, mouse);
    button(game, batch, add, "Add here", mouse);
    if inside(from_chat, mouse)
        && let Some((line, at)) = s.chat.back()
    {
        let tip = format!("{line} -> {} {} {}", at[0], at[1], at[2]);
        let tw = game.native_text_width(&tip);
        game.native_fill(batch, [from_chat[0] - 2.0, from_chat[1] - 14.0, tw + 6.0, 12.0], 0xF000_0000);
        game.native_text(batch, &tip, [from_chat[0] + 1.0, from_chat[1] - 12.0], 0xFFFFA0, false);
    }
}

fn draw_edit(game: &Game<'_>, batch: &mut HudBatch, s: &State, index: usize, name: &str, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let Some(wp) = s.book.list.get(index) else {
        return;
    };
    let (field, [colour, shown, set, here, share, delete, done]) = edit_layout(w, h);
    game.native_fill(batch, [field[0] - 8.0, field[1] - 18.0, field[2] + 16.0, 214.0], 0xC000_0000);
    let title = format!("Waypoint at {} {} {} ({})", wp.pos[0], wp.pos[1], wp.pos[2], m::pretty_id(&wp.dim));
    game.native_text(batch, &title, [field[0], field[1] - 12.0], 0xFFFFFF, true);
    text_box(game, batch, field, name);
    let other = match wp.dim.as_str() {
        m::OVERWORLD => m::converted(wp.pos, m::OVERWORLD, m::NETHER).map(|p| format!("In the Nether: {} {} {}", p[0], p[1], p[2])),
        m::NETHER => m::converted(wp.pos, m::NETHER, m::OVERWORLD).map(|p| format!("In the Overworld: {} {} {}", p[0], p[1], p[2])),
        _ => None,
    };
    if let Some(t) = other {
        game.native_text(batch, &t, [field[0], field[1] + 23.0], 0xA0A0A0, false);
    }
    let on = |b: bool| if b { "ON" } else { "OFF (hidden for now)" };
    button(game, batch, colour, &format!("Colour: {}", m::COLOURS[wp.colour.min(m::COLOURS.len() - 1)].0), mouse);
    game.native_fill(batch, [colour[0] + 4.0, colour[1] + 5.0, 8.0, 8.0], 0xFF00_0000 | wp.rgb());
    button(game, batch, shown, &format!("Shown: {}", on(wp.shown)), mouse);
    button(game, batch, set, &format!("Set: {}", wp.set), mouse);
    button(game, batch, here, "Move to where I stand", mouse);
    button(game, batch, share, "Share in chat", mouse);
    button(game, batch, delete, "Delete", mouse);
    button(game, batch, done, "Done", mouse);
}

/// Puts the edited name on its waypoint, saved; back to `to`.
fn finish_edit(s: &mut State, to: m::Page) {
    if let m::Page::Edit { index, name } = &s.screen.page {
        let name = m::clean_name(name);
        if let Some(w) = s.book.list.get_mut(*index)
            && !name.is_empty()
        {
            w.name = name;
        }
        save_book(s);
    }
    s.screen.page = to;
}

fn edit(s: &mut State, index: usize) {
    let name = s.book.list[index].name.clone();
    s.screen.page = m::Page::Edit { index, name };
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
    with(|s| match s.screen.page.clone() {
        m::Page::Map if button == 0 => {
            let slime = game.native_world_seed().is_some();
            let hit = tools(w, s.subworlds.is_some(), slime).into_iter().find(|(_, r)| inside(*r, mouse));
            if let Some((tool, _)) = hit {
                let feet = here.map_or(64, |p| p[1]);
                let top = game.world.shape.max_y();
                match tool {
                    Tool::Waypoints => s.screen.page = m::Page::List { scroll: 0 },
                    Tool::OnMe => {
                        if let Some(p) = player_pos(game) {
                            s.screen.view.x = p[0];
                            s.screen.view.z = p[2];
                        }
                    }
                    Tool::Follow => s.screen.follow = !s.screen.follow,
                    Tool::Grid => s.screen.grid = !s.screen.grid,
                    Tool::Slime => s.screen.slime = !s.screen.slime,
                    Tool::LayerDown => s.screen.slice = layer_step(s.screen.slice, feet, true, top),
                    Tool::LayerUp => s.screen.slice = layer_step(s.screen.slice, feet, false, top),
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
            let (panel, rows, [set, set_shown, new_set, del_set], [back, order, from_chat, add]) = list_layout(w, h);
            let ordered = s.book.order(settings.sort, player_pos(game));
            let row = (0..rows).find(|&i| inside(list_row(panel, i), mouse)).and_then(|i| ordered.get(scroll + i).copied());
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
            if inside(set, mouse) {
                s.book.next_set();
                save_book(s);
            } else if inside(set_shown, mouse) {
                let c = s.book.current;
                s.book.sets[c].shown = !s.book.sets[c].shown;
                save_book(s);
            } else if inside(new_set, mouse) {
                s.screen.page = m::Page::NewSet { name: String::new() };
            } else if inside(del_set, mouse) {
                s.book.remove_set();
                save_book(s);
            } else if inside(back, mouse) {
                s.screen.page = m::Page::Map;
            } else if inside(order, mouse) {
                settings.sort = (settings.sort + 1) % m::SORTS.len();
                settings_changed = true;
            } else if inside(from_chat, mouse) {
                if let Some((_, at)) = s.chat.pop_back() {
                    let i = s.book.add(at, dim, now_secs());
                    save_book(s);
                    edit(s, i);
                }
            } else if inside(add, mouse) {
                if let Some(pos) = here {
                    let i = s.book.add(pos, dim, now_secs());
                    save_book(s);
                    edit(s, i);
                }
            } else if let Some(i) = row {
                edit(s, i);
            }
        }
        m::Page::Edit { index, .. } if button == 0 => {
            let (_, [colour, shown, set, move_here, share, delete, done]) = edit_layout(w, h);
            if index >= s.book.list.len() {
                s.screen.page = m::Page::List { scroll: 0 };
                return;
            }
            if inside(colour, mouse) {
                let wp = &mut s.book.list[index];
                wp.colour = (wp.colour + 1) % m::COLOURS.len();
                save_book(s);
            } else if inside(shown, mouse) {
                s.book.list[index].shown = !s.book.list[index].shown;
                save_book(s);
            } else if inside(set, mouse) {
                let at = s.book.set_index(&s.book.list[index].set).unwrap_or(0);
                let next = s.book.sets[(at + 1) % s.book.sets.len()].name.clone();
                s.book.list[index].set = next;
                save_book(s);
            } else if inside(move_here, mouse) {
                if let Some(pos) = here {
                    s.book.list[index].pos = pos;
                    s.book.list[index].dim = dim.to_owned();
                    save_book(s);
                }
            } else if inside(share, mouse) {
                finish_edit(s, m::Page::Map);
                if let Some(wp) = s.book.list.get(index) {
                    s.pending_chat = Some(m::share_text(wp));
                }
            } else if inside(delete, mouse) {
                s.book.list.remove(index);
                save_book(s);
                s.screen.page = m::Page::List { scroll: 0 };
            } else if inside(done, mouse) {
                finish_edit(s, m::Page::List { scroll: 0 });
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
    let dim = game.dim.id();
    let convert = settings(game).convert;
    let fallback_y = player_pos(game).map_or(64, |p| p[1].floor() as i32);
    with(|s| {
        if s.screen.page != m::Page::Map || !s.screen.release() || !inside(rect, mouse) {
            return;
        }
        let view = s.screen.view;
        let hit = s.book.list.iter().position(|wp| {
            let Some(c) = wp.place_in(dim, convert) else {
                return false;
            };
            let [x, y] = view.screen_of(rect, c[0], c[2]);
            on_map(s, wp) && (mouse[0] - x).abs() <= 6.0 && (mouse[1] - y).abs() <= 6.0
        });
        let index = hit.unwrap_or_else(|| {
            let (x, z) = view.world_at(rect, mouse);
            let (x, z) = (x.floor() as i32, z.floor() as i32);
            let column = match (&s.slice, &s.explored) {
                (Some(l), _) => l.column(x, z),
                (None, Some(e)) => e.store.column(x, z),
                _ => None,
            };
            let y = column.map_or(fallback_y, |c| i32::from(c.height) + 1);
            let i = s.book.add([x, y, z], dim, now_secs());
            save_book(s);
            i
        });
        edit(s, index);
    });
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
    with(|s| match &mut s.screen.page {
        m::Page::Map => s.screen.view.zoom_at(map_rect(w, h), mouse, step, 0, m::MAP_ZOOM_STEPS - 1),
        m::Page::List { scroll } => {
            let rows = m::list_rows(h);
            let max = s.book.list.len().saturating_sub(rows);
            *scroll = (*scroll as i64 - i64::from(step)).clamp(0, max as i64) as usize;
        }
        _ => {}
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
        let typing = matches!(s.screen.page, m::Page::Edit { .. } | m::Page::NewSet { .. });
        match key {
            "key.keyboard.escape" | "key.keyboard.enter" | "key.keyboard.keypad.enter" if typing => {
                if let m::Page::NewSet { name } = s.screen.page.clone() {
                    if key != "key.keyboard.escape" {
                        s.book.add_set(&name);
                        save_book(s);
                    }
                    s.screen.page = m::Page::List { scroll: 0 };
                } else {
                    finish_edit(s, m::Page::List { scroll: 0 });
                }
                *canceled = true;
            }
            "key.keyboard.escape" if matches!(s.screen.page, m::Page::List { .. }) => {
                s.screen.page = m::Page::Map;
                *canceled = true;
            }
            "key.keyboard.backspace" if typing => {
                match &mut s.screen.page {
                    m::Page::Edit { name, .. } | m::Page::NewSet { name } => {
                        name.pop();
                    }
                    _ => {}
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
                    s.screen.slice = layer_step(s.screen.slice, feet, true, top);
                    *canceled = true;
                } else if key == "key.keyboard.right.bracket" {
                    s.screen.slice = layer_step(s.screen.slice, feet, false, top);
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
        if let m::Page::Edit { name, .. } | m::Page::NewSet { name } = &mut s.screen.page {
            if !c.is_control() && name.chars().count() < m::NAME_MAX {
                name.push(c);
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
    buttons.extend(settings(game).labels());
}

pub(super) fn on_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != m::ID || button >= m::Settings::COUNT {
        return;
    }
    let mut s = settings(game);
    s.click(button);
    m::set_settings(s);
    for (key, value) in s.stored() {
        game.native_set_setting(m::ID, key, &value);
    }
}
