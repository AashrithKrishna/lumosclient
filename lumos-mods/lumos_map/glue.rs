// Lumos Map: between Horizon's `Game` and the rules in `lumos_map`.
//
// The minimap is Horizon's own (`native_minimap`: its tiles, cave view underground and image);
// this mod sets it up and draws over it: entity dots, waypoints, coordinates and biome. The world
// map is the mod's own: a tile per explored chunk, scanned a few at a time from the chunks the
// client has loaded, kept in region files per world or server, and drawn into the mod image only
// when the view or the tiles change.
use crate::game::lumos_map as m;
use crate::game::lumos_map_keys as keys;
use crate::game::{Game, Screen};
use crate::mod_api::{NativeMinimap, NativeMod, NativeScreenView, NativeWorldId};
use horizon_client::blocks::{Fluid, MAP_COLORS};
use horizon_client::chunk::Chunk;
use horizon_client::extras::Corner;
use horizon_client::extras_hud::Corners;
use horizon_client::hud::{HudBatch, MOD_IMAGE_SPRITE};
use horizon_client::text::Text;
use horizon_client::ui;
use horizon_client::world_primitives::WorldPrimitives;
use horizon_world::registry::BlockStateId;
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// Scanning time a frame may take, with the map closed and open.
const BUDGET: Duration = Duration::from_micros(700);
const BUDGET_OPEN: Duration = Duration::from_micros(2500);
/// How often the loaded chunks are looked over for new ones.
const REFILL: Duration = Duration::from_secs(1);
/// How often the chunks around the player are scanned again after blocks changed.
const NEAR_RESCAN: Duration = Duration::from_secs(2);
/// How often changed regions are written.
const SAVE_EVERY: Duration = Duration::from_secs(30);
/// While only the tiles change (exploring with the map open), recompose at most this often.
const RECOMPOSE: Duration = Duration::from_millis(250);
/// The most entity dots drawn.
const MAX_DOTS: usize = 400;

/// The map screen's bars, in GUI pixels.
const TOP: f32 = 22.0;
const BOTTOM: f32 = 14.0;

// Column flags of the scan's palette.
const FLUID: u8 = 3;
const AIR: u8 = 4;

/// Per block state: its map colour and flags.
struct Palette {
    color: Vec<u8>,
    flags: Vec<u8>,
}

impl Palette {
    fn new(game: &Game<'_>) -> Self {
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
        Self { color, flags }
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

/// One chunk's columns: the first block with a map colour going down from the top (in a level
/// with a ceiling, from the first open space under it), and the water over the ground.
fn scan(chunk: &Chunk, palette: &Palette, air: u32, ceiling: bool) -> m::Tile {
    let min = chunk.min_y();
    let top = (0..chunk.sections.len())
        .rev()
        .find(|&s| !chunk.sections[s].is_uniform(air))
        .map_or(min, |s| min + s as i32 * 16 + 15);
    let mut tile = m::Tile::default();
    for z in 0..16 {
        for x in 0..16 {
            let mut y = top;
            if ceiling {
                // Under the roof: past the solid top, to the first open block.
                y = y.min(min + 118);
                while y > min && palette.get(chunk.get(x, y, z)).1 & AIR == 0 {
                    y -= 1;
                }
            }
            while y > min {
                let (color, flags) = palette.get(chunk.get(x, y, z));
                if color == 0 {
                    y -= 1;
                    continue;
                }
                let mut depth = 0u8;
                if flags & FLUID == 1 {
                    let mut below = y - 1;
                    while below > min && palette.get(chunk.get(x, below, z)).1 & FLUID == 1 {
                        depth = depth.saturating_add(1);
                        below -= 1;
                    }
                }
                let i = (z * 16 + x) as usize;
                tile.color[i] = color;
                tile.height[i] = y.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
                tile.depth[i] = depth;
                break;
            }
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
    /// Chunks scanned since they were loaded.
    seen: HashSet<(i32, i32)>,
    queue: VecDeque<(i32, i32)>,
    refilled: Option<Instant>,
    /// The near chunks' last rescan, and the world's revision then.
    near: (Option<Instant>, u64),
    saved: Instant,
    loading: Option<Receiver<Vec<((i32, i32), m::Tile)>>>,
}

impl Explored {
    fn open(world: String, dim: &'static str, base: &Path) -> Self {
        let dir = base.join(&world).join(m::folder_name(dim));
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
        let files: Vec<(PathBuf, Vec<u8>)> = self
            .store
            .dirty
            .drain()
            .collect::<Vec<_>>()
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

#[derive(Default)]
struct State {
    explored: Option<Explored>,
    /// The world folder the waypoints are of, and the waypoints.
    waypoints_of: Option<String>,
    waypoints: Vec<m::Waypoint>,
    palette: Option<Palette>,
    /// The last look for the world's id (it is asked at most once a second).
    world_asked: Option<Instant>,
    was_dead: bool,
    screen: m::MapScreen,
    image: Vec<u8>,
    /// What the uploaded image shows: the view, the tiles' revision, its size, and when.
    composed: Option<(m::MapView, u64, (u32, u32), Instant)>,
    /// The biome at the player's block, by block.
    biome: Option<([i32; 3], String)>,
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

fn save_waypoints(game: &Game<'_>, s: &State) {
    let Some(world) = &s.waypoints_of else {
        return;
    };
    let dir = base(game).join(world);
    let _ = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(dir.join("waypoints.txt"), m::waypoints_to_text(&s.waypoints)));
}

/// Follows the world and dimension played: the explored map and the waypoints of each.
fn follow_world(game: &Game<'_>, s: &mut State) {
    let dim = game.dim.id();
    let same = s.explored.as_ref().is_some_and(|e| e.dim == dim);
    if same {
        return;
    }
    if s.world_asked.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
        return;
    }
    s.world_asked = Some(Instant::now());
    let Some(world) = world_folder(game) else {
        return;
    };
    if let Some(mut old) = s.explored.take() {
        old.save();
    }
    if s.waypoints_of.as_deref() != Some(world.as_str()) {
        let file = base(game).join(&world).join("waypoints.txt");
        s.waypoints = std::fs::read_to_string(file)
            .map(|t| m::waypoints_from_text(&t))
            .unwrap_or_default();
        s.waypoints_of = Some(world.clone());
    }
    s.explored = Some(Explored::open(world, dim, &base(game)));
    s.composed = None;
}

/// Scans chunks for the world map within `budget`: new ones nearest first, then the ones
/// around the player again after blocks changed.
fn explore(game: &Game<'_>, s: &mut State, budget: Duration) {
    let started = Instant::now();
    let Some(p) = player_pos(game) else {
        return;
    };
    if s.palette.is_none() {
        s.palette = Some(Palette::new(game));
    }
    let (Some(e), Some(palette)) = (s.explored.as_mut(), s.palette.as_ref()) else {
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
        let d = |k: &(i32, i32)| (k.0 - centre.0).abs().max((k.1 - centre.1).abs());
        fresh.sort_by_key(d);
        e.queue.extend(fresh);
    }
    let air = world.air.0;
    let ceiling = world.shape.has_ceiling;
    while started.elapsed() < budget {
        let Some(k) = e.queue.pop_front() else {
            break;
        };
        let Some(chunk) = world.chunks.get(&k) else {
            continue;
        };
        e.store.put(k, scan(chunk, palette, air, ceiling));
        e.seen.insert(k);
    }
    if e.saved.elapsed() >= SAVE_EVERY {
        e.save();
    }
}

/// The world map's area of a `w` x `h` GUI.
fn map_rect(w: f32, h: f32) -> [f32; 4] {
    [0.0, TOP, w, (h - TOP - BOTTOM).max(1.0)]
}

fn map_open(game: &Game<'_>) -> bool {
    game.native_mod_screen() == Some(m::ID)
}

/// Composes and uploads the world map's image when the view or the tiles changed.
fn refresh_image(game: &mut Game<'_>, s: &mut State) {
    let (w, h) = game.gui_size();
    let rect = map_rect(w, h);
    let (iw, ih, scale) = m::image_size(rect[2], rect[3]);
    let Some(e) = s.explored.as_ref() else {
        return;
    };
    let view = s.screen.view;
    let rev = e.store.revision;
    let fresh = s.composed.is_some_and(|(v, r, size, at)| {
        v == view && size == (iw, ih) && (r == rev || at.elapsed() < RECOMPOSE)
    });
    if fresh {
        return;
    }
    m::compose(&e.store, &MAP_COLORS, &view, iw, ih, scale, &mut s.image);
    if game.native_mod_image(iw, ih, &s.image) {
        s.composed = Some((view, rev, (iw, ih), Instant::now()));
    }
}

/// `native_frame`: the world followed, chunks explored, a death marked, the map's image kept.
pub(super) fn on_frame(game: &mut Game<'_>) {
    if !game.native_on(m::ID) || game.player.is_none() {
        return;
    }
    let open = map_open(game);
    let dead = game.screen == Screen::Dead;
    let death = settings(game).death;
    with(|s| {
        follow_world(game, s);
        explore(game, s, if open { BUDGET_OPEN } else { BUDGET });
        if dead && !s.was_dead && death && s.waypoints_of.is_some() {
            if let Some(p) = player_pos(game) {
                m::add_death(&mut s.waypoints, block_of(p), game.dim.id());
                save_waypoints(game, s);
            }
        }
        s.was_dead = dead;
        if open && s.screen.page == m::Page::Map {
            refresh_image(game, s);
        }
    });
}

/// The client left its level: the explored map is saved and let go.
pub(super) fn on_level_unload(game: &mut Game<'_>) {
    with(|s| {
        if let Some(mut e) = s.explored.take() {
            e.save();
        }
        s.composed = None;
        s.world_asked = None;
        s.biome = None;
        s.was_dead = false;
    });
    let _ = game;
}

/// `native_key`: M opens (or closes) the world map, B adds a waypoint where you stand.
pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || !game.native_on(m::ID) || game.player.is_none() {
        return;
    }
    if map_open(game) {
        let editing = with(|s| matches!(s.screen.page, m::Page::Edit { .. }));
        if !editing && keys::pressed(game, keys::MAP, key, true) {
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
                s.composed = None;
            });
        }
    } else if keys::pressed(game, keys::WAYPOINT, key, true) {
        let Some(p) = player_pos(game) else {
            return;
        };
        let dim = game.dim.id();
        let added = with(|s| {
            if s.waypoints_of.is_none() {
                return None;
            }
            let i = m::add_waypoint(&mut s.waypoints, block_of(p), dim);
            save_waypoints(game, s);
            let w = &s.waypoints[i];
            Some((w.name.clone(), w.pos, w.rgb()))
        });
        if let Some((name, pos, rgb)) = added {
            let text = format!("Added {name} at {} {} {}", pos[0], pos[1], pos[2]);
            game.native_client_action_bar(Text::colored(text, rgb), false);
        }
    }
}

/// `native_minimap`: Horizon's minimap, set up from the settings.
pub(super) fn on_minimap(game: &Game<'_>, minimap: &mut Option<NativeMinimap>) {
    if minimap.is_some() || !game.native_on(m::ID) {
        return;
    }
    let s = settings(game);
    if !s.minimap {
        return;
    }
    *minimap = Some(NativeMinimap {
        corner: corner(s.corner),
        size: m::SIZES[s.size],
        blocks_per_pixel: m::ZOOMS[s.zoom],
        rotate: s.rotate,
        round: s.round,
    });
}

fn corner(i: usize) -> Corner {
    match i {
        0 => Corner::TopLeft,
        1 => Corner::TopRight,
        2 => Corner::BottomLeft,
        _ => Corner::BottomRight,
    }
}

/// `native_hud`: over the minimap, the mobs and players near you and your waypoints; under it,
/// your coordinates and biome.
pub(super) fn on_hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    if !game.native_on(m::ID) {
        return;
    }
    let s = settings(game);
    let Some(p) = player_pos(game) else {
        return;
    };
    if !s.minimap {
        return;
    }
    let at = corner(s.corner);
    if !game.native_corner_shown(at) {
        return;
    }
    let size = m::SIZES[s.size];
    let bpp = m::ZOOMS[s.zoom];
    let block = block_of(p);
    let mut lines: Vec<String> = Vec::new();
    if s.info {
        lines.push(format!("{} {} {}", block[0], block[1], block[2]));
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
    let info_h = lines.len() as f32 * 10.0;
    // The minimap took the corner's first place: the box placed after it is under it (over
    // it at the bottom), which says where the map is.
    let (x, y) = corners.place(at, size, info_h);
    let map_y = if matches!(at, Corner::TopLeft | Corner::TopRight) {
        y - 2.0 - size
    } else {
        y + info_h + 2.0
    };
    for (i, line) in lines.iter().enumerate() {
        let w = game.native_text_width(line);
        game.native_text(batch, line, [x + (size - w) / 2.0, y + 1.0 + i as f32 * 10.0], 0xFFFFFF, true);
    }
    let (cx, cy) = (x + size / 2.0, map_y + size / 2.0);
    let scale = size / m::MINIMAP_TEXELS;
    let radius = size / 2.0 - 2.0;
    let to_map = |wx: f64, wz: f64| {
        let (u, v) = m::minimap_point(wx - p[0], wz - p[2], game.yaw, s.rotate, bpp);
        (u * scale, v * scale)
    };
    // Mobs and players.
    let mut drawn = 0;
    for mob in game.mobs.iter() {
        if drawn >= MAX_DOTS {
            break;
        }
        if mob.removed || mob.invisible {
            continue;
        }
        let kind = m::dot_kind(&mob.kind.id, &mob.kind.category);
        if !s.shows(kind) {
            continue;
        }
        let (u, v) = to_map(mob.position.x, mob.position.z);
        let (_, outside) = m::clamp_to_map((u, v), radius, s.round);
        if outside {
            continue;
        }
        let d = if kind == m::DotKind::Player { 3.0 } else { 2.0 };
        let (dx, dy) = ((cx + u - d / 2.0).round(), (cy + v - d / 2.0).round());
        game.native_fill(batch, [dx - 0.5, dy - 0.5, d + 1.0, d + 1.0], 0xA000_0000);
        game.native_fill(batch, [dx, dy, d, d], m::dot_colour(kind));
        drawn += 1;
    }
    // Waypoints: on the rim when they are farther than the map reaches.
    let dim = game.dim.id();
    with(|st| {
        for w in st.waypoints.iter().filter(|w| w.shown && w.dim == dim) {
            let c = w.centre();
            let ((u, v), _) = m::clamp_to_map(to_map(c[0], c[2]), radius - 1.0, s.round);
            let (wx, wy) = ((cx + u).round(), (cy + v).round());
            game.native_fill(batch, [wx - 2.5, wy - 2.5, 5.0, 5.0], 0xFF00_0000);
            game.native_fill(batch, [wx - 1.5, wy - 1.5, 3.0, 3.0], 0xFF00_0000 | w.rgb());
        }
    });
}

/// `native_world_primitives`: each waypoint's name and distance, seen through walls, and a beam.
pub(super) fn on_world(game: &Game<'_>, camera: [f64; 3], primitives: &mut WorldPrimitives) {
    if !game.native_on(m::ID) || game.player.is_none() {
        return;
    }
    let s = settings(game);
    if !s.markers && !s.beams {
        return;
    }
    let dim = game.dim.id();
    let top = game.world.shape.max_y();
    with(|st| {
        for w in st.waypoints.iter().filter(|w| w.shown && w.dim == dim) {
            let c = w.centre();
            if s.beams {
                let height = (top - w.pos[1]).clamp(1, 2048) as u32;
                primitives.beam(c, height, 0xFF00_0000 | w.rgb());
            }
            if s.markers {
                let label_at = [c[0], c[1] + 1.5, c[2]];
                let d = m::distance(camera, label_at);
                // Far ones are drawn nearer along the same line, at the size they would have.
                let near = 24.0;
                let at = if d > near {
                    let t = near / d;
                    [
                        camera[0] + (label_at[0] - camera[0]) * t,
                        camera[1] + (label_at[1] - camera[1]) * t,
                        camera[2] + (label_at[2] - camera[2]) * t,
                    ]
                } else {
                    label_at
                };
                let text = format!("{} ({})", w.name, m::format_distance(d));
                primitives
                    .text(at, text, 0xFF00_0000 | w.rgb(), 1.0)
                    .through_walls()
                    .scale_with_distance()
                    .background();
            }
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

/// The top bar's buttons: Waypoints, Centre on me.
fn top_buttons(w: f32) -> [[f32; 4]; 2] {
    [[w - 132.0, 2.0, 70.0, 18.0], [w - 60.0, 2.0, 58.0, 18.0]]
}

/// The list's panel and its rows' rects, and its two buttons (Back, Add here).
fn list_layout(w: f32, h: f32) -> ([f32; 4], usize, [[f32; 4]; 2]) {
    let pw = 300.0f32.min(w - 20.0);
    let panel = [(w - pw) / 2.0, 10.0, pw, h - 20.0];
    let rows = m::list_rows(h);
    let by = h - 34.0;
    (
        panel,
        rows,
        [
            [panel[0] + 6.0, by, 90.0, 18.0],
            [panel[0] + panel[2] - 96.0, by, 90.0, 18.0],
        ],
    )
}

fn list_row(panel: [f32; 4], i: usize) -> [f32; 4] {
    [panel[0] + 6.0, panel[1] + 26.0 + i as f32 * 22.0, panel[2] - 12.0, 20.0]
}

/// The editor's name box and buttons: colour, shown, move here, delete, done.
fn edit_layout(w: f32, h: f32) -> ([f32; 4], [[f32; 4]; 5]) {
    let pw = 220.0;
    let x = (w - pw) / 2.0;
    let y = (h / 2.0 - 80.0).max(4.0);
    let b = |i: f32| [x, y + 40.0 + i * 22.0, pw, 20.0];
    ([x, y + 14.0, pw, 18.0], [b(0.0), b(1.0), b(2.0), b(3.0), b(4.0)])
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

/// `native_screen_render_post`: the map, the list or the editor.
pub(super) fn on_render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if !ours(game, screen.id) {
        return;
    }
    let (w, h) = (batch.width, batch.height);
    let mouse = screen.mouse;
    game.native_fill(batch, [0.0, 0.0, w, h], 0xFF10_1014);
    with(|s| match s.screen.page.clone() {
        m::Page::Map => draw_map(game, batch, s, mouse),
        m::Page::List { scroll } => draw_list(game, batch, s, scroll, mouse),
        m::Page::Edit { index, name } => draw_edit(game, batch, s, index, &name, mouse),
    });
}

fn draw_map(game: &Game<'_>, batch: &mut HudBatch, s: &State, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let rect = map_rect(w, h);
    let view = s.screen.view;
    if let Some((v, _, (iw, ih), _)) = s.composed
        && v == view
    {
        let _ = game.native_sprite(batch, MOD_IMAGE_SPRITE, [0.0, 0.0, iw as f32, ih as f32], rect);
    }
    let dim = game.dim.id();
    // Waypoints, names shown when zoomed in or pointed at.
    for wp in s.waypoints.iter().filter(|w| w.shown && w.dim == dim) {
        let c = wp.centre();
        let [x, y] = view.screen_of(rect, c[0], c[2]);
        if !inside(rect, [x, y]) {
            continue;
        }
        game.native_fill(batch, [x - 3.0, y - 3.0, 6.0, 6.0], 0xFF00_0000);
        game.native_fill(batch, [x - 2.0, y - 2.0, 4.0, 4.0], 0xFF00_0000 | wp.rgb());
        let hover = (mouse[0] - x).abs() <= 4.0 && (mouse[1] - y).abs() <= 4.0;
        if hover || view.bpp() <= 4.0 {
            let tw = game.native_text_width(&wp.name);
            game.native_text(batch, &wp.name, [x - tw / 2.0, y - 13.0], wp.rgb(), true);
        }
    }
    // You: an arrow the way you face.
    if let Some(p) = player_pos(game) {
        let [x, y] = view.screen_of(rect, p[0], p[2]);
        if inside(rect, [x, y]) {
            // Yaw 0 faces south (down the map); the arrow's tip points that way.
            let (sn, cs) = game.yaw.to_radians().sin_cos();
            let dir = [-sn, cs];
            let side = [cs, sn];
            let pt = |f: f32, r: f32| [x + dir[0] * f + side[0] * r, y + dir[1] * f + side[1] * r];
            game.native_quad(batch, [pt(7.0, 0.0), pt(-5.0, 5.5), pt(-2.5, 0.0), pt(-5.0, -5.5)], 0xFF00_0000);
            game.native_quad(batch, [pt(5.0, 0.0), pt(-3.5, 3.5), pt(-1.5, 0.0), pt(-3.5, -3.5)], 0xFFFF_FFFF);
        }
    }
    // The bars.
    game.native_fill(batch, [0.0, 0.0, w, TOP], 0xE000_0000);
    game.native_fill(batch, [0.0, h - BOTTOM, w, BOTTOM], 0xE000_0000);
    let title = format!("World Map - {}", m::pretty_id(dim));
    game.native_text(batch, &title, [6.0, 7.0], 0xFFFFFF, true);
    let [list, centre] = top_buttons(w);
    button(game, batch, list, "Waypoints", mouse);
    button(game, batch, centre, "On me", mouse);
    let bottom = if inside(rect, mouse) {
        let (x, z) = view.world_at(rect, mouse);
        format!(
            "X {}  Z {}   {} block(s) per pixel   Drag to move, wheel to zoom, click to add a waypoint",
            x.floor() as i64,
            z.floor() as i64,
            view.bpp()
        )
    } else {
        "Drag to move, wheel to zoom, click to add a waypoint".to_owned()
    };
    game.native_text(batch, &bottom, [6.0, h - BOTTOM + 3.0], 0xC0C0C0, false);
}

fn draw_list(game: &Game<'_>, batch: &mut HudBatch, s: &State, scroll: usize, mouse: [f32; 2]) {
    let (w, h) = (batch.width, batch.height);
    let (panel, rows, [back, add]) = list_layout(w, h);
    game.native_fill(batch, panel, 0xC000_0000);
    let title = format!("Waypoints ({})", s.waypoints.len());
    game.native_text(batch, &title, [panel[0] + 6.0, panel[1] + 8.0], 0xFFFFFF, true);
    let here = player_pos(game);
    let dim = game.dim.id();
    for (i, (index, wp)) in s.waypoints.iter().enumerate().skip(scroll).take(rows).enumerate() {
        let r = list_row(panel, i);
        let hover = inside(r, mouse);
        game.native_fill(batch, r, if hover { 0x6050_5070 } else { 0x4030_3040 });
        game.native_fill(batch, [r[0] + 4.0, r[1] + 6.0, 8.0, 8.0], 0xFF00_0000 | wp.rgb());
        let colour = if wp.shown { 0xFFFFFF } else { 0x808080 };
        game.native_text(batch, &wp.name, [r[0] + 16.0, r[1] + 6.0], colour, false);
        let mut info = format!("{} {} {}", wp.pos[0], wp.pos[1], wp.pos[2]);
        if wp.dim != dim {
            info = format!("{info}  {}", m::pretty_id(&wp.dim));
        } else if let Some(p) = here {
            info = format!("{info}  {}", m::format_distance(m::distance(p, wp.centre())));
        }
        let iw = game.native_text_width(&info);
        game.native_text(batch, &info, [r[0] + r[2] - iw - 4.0, r[1] + 6.0], 0xA0A0A0, false);
        let _ = index;
    }
    if s.waypoints.is_empty() {
        let t = "No waypoints yet: add one here, with B in the world, or by clicking the map";
        let tw = game.native_text_width(t);
        game.native_text(batch, t, [(w - tw) / 2.0, panel[1] + 40.0], 0xA0A0A0, false);
    }
    button(game, batch, back, "Back to map", mouse);
    button(game, batch, add, "Add here", mouse);
}

fn draw_edit(
    game: &Game<'_>,
    batch: &mut HudBatch,
    s: &State,
    index: usize,
    name: &str,
    mouse: [f32; 2],
) {
    let (w, h) = (batch.width, batch.height);
    let Some(wp) = s.waypoints.get(index) else {
        return;
    };
    let (field, [colour, shown, here, delete, done]) = edit_layout(w, h);
    game.native_fill(
        batch,
        [field[0] - 8.0, field[1] - 18.0, field[2] + 16.0, 166.0],
        0xC000_0000,
    );
    let title = format!("Waypoint at {} {} {}", wp.pos[0], wp.pos[1], wp.pos[2]);
    game.native_text(batch, &title, [field[0], field[1] - 12.0], 0xFFFFFF, true);
    game.native_fill(batch, field, 0xFFA0_A0A0);
    game.native_fill(
        batch,
        [field[0] + 1.0, field[1] + 1.0, field[2] - 2.0, field[3] - 2.0],
        0xFF00_0000,
    );
    let caret = if (game.started.elapsed().as_millis() / 500) % 2 == 0 { "_" } else { "" };
    game.native_text(batch, &format!("{name}{caret}"), [field[0] + 4.0, field[1] + 5.0], 0xFFFFFF, false);
    let on = |b: bool| if b { "ON" } else { "OFF" };
    button(game, batch, colour, &format!("Colour: {}", m::COLOURS[wp.colour.min(m::COLOURS.len() - 1)].0), mouse);
    game.native_fill(batch, [colour[0] + 4.0, colour[1] + 6.0, 8.0, 8.0], 0xFF00_0000 | wp.rgb());
    button(game, batch, shown, &format!("Shown: {}", on(wp.shown)), mouse);
    button(game, batch, here, "Move to where I stand", mouse);
    button(game, batch, delete, "Delete", mouse);
    button(game, batch, done, "Done", mouse);
}

/// Puts the edited name on its waypoint, saved; back to `to`.
fn finish_edit(game: &Game<'_>, s: &mut State, to: m::Page) {
    if let m::Page::Edit { index, name } = &s.screen.page {
        let name = m::clean_name(name);
        if let Some(w) = s.waypoints.get_mut(*index)
            && !name.is_empty()
        {
            w.name = name;
        }
        save_waypoints(game, s);
    }
    s.screen.page = to;
}

/// `native_screen_mouse_button_pressed_pre`: buttons, rows, and a press on the map.
pub(super) fn on_press(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    if !ours(game, screen.id) || *canceled {
        return;
    }
    *canceled = true;
    if button != 0 {
        return;
    }
    let (w, h) = game.gui_size();
    let mouse = screen.mouse;
    let here = player_pos(game).map(block_of);
    let dim = game.dim.id();
    with(|s| match s.screen.page.clone() {
        m::Page::Map => {
            let [list, centre] = top_buttons(w);
            if inside(list, mouse) {
                s.screen.page = m::Page::List { scroll: 0 };
            } else if inside(centre, mouse) {
                if let Some(p) = player_pos(game) {
                    s.screen.view.x = p[0];
                    s.screen.view.z = p[2];
                }
            } else if inside(map_rect(w, h), mouse) {
                s.screen.press = Some((mouse, false));
            }
        }
        m::Page::List { scroll } => {
            let (panel, rows, [back, add]) = list_layout(w, h);
            if inside(back, mouse) {
                s.screen.page = m::Page::Map;
            } else if inside(add, mouse) {
                if let Some(pos) = here {
                    let i = m::add_waypoint(&mut s.waypoints, pos, dim);
                    save_waypoints(game, s);
                    let name = s.waypoints[i].name.clone();
                    s.screen.page = m::Page::Edit { index: i, name };
                }
            } else {
                for i in 0..rows {
                    let index = scroll + i;
                    if index < s.waypoints.len() && inside(list_row(panel, i), mouse) {
                        let name = s.waypoints[index].name.clone();
                        s.screen.page = m::Page::Edit { index, name };
                        break;
                    }
                }
            }
        }
        m::Page::Edit { index, .. } => {
            let (_, [colour, shown, move_here, delete, done]) = edit_layout(w, h);
            if index >= s.waypoints.len() {
                s.screen.page = m::Page::List { scroll: 0 };
                return;
            }
            if inside(colour, mouse) {
                let wp = &mut s.waypoints[index];
                wp.colour = (wp.colour + 1) % m::COLOURS.len();
                save_waypoints(game, s);
            } else if inside(shown, mouse) {
                s.waypoints[index].shown = !s.waypoints[index].shown;
                save_waypoints(game, s);
            } else if inside(move_here, mouse) {
                if let Some(pos) = here {
                    s.waypoints[index].pos = pos;
                    s.waypoints[index].dim = dim.to_owned();
                    save_waypoints(game, s);
                }
            } else if inside(delete, mouse) {
                s.waypoints.remove(index);
                save_waypoints(game, s);
                s.screen.page = m::Page::List { scroll: 0 };
            } else if inside(done, mouse) {
                finish_edit(game, s, m::Page::List { scroll: 0 });
            }
        }
    });
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
    let fallback_y = player_pos(game).map_or(64, |p| p[1].floor() as i32);
    with(|s| {
        if s.screen.page != m::Page::Map || !s.screen.release() || !inside(rect, mouse) {
            return;
        }
        let view = s.screen.view;
        let hit = s.waypoints.iter().position(|wp| {
            let c = wp.centre();
            let [x, y] = view.screen_of(rect, c[0], c[2]);
            wp.shown && wp.dim == dim && (mouse[0] - x).abs() <= 4.0 && (mouse[1] - y).abs() <= 4.0
        });
        let index = hit.unwrap_or_else(|| {
            let (x, z) = view.world_at(rect, mouse);
            let (x, z) = (x.floor() as i32, z.floor() as i32);
            let y = s
                .explored
                .as_ref()
                .and_then(|e| e.store.column(x, z))
                .map_or(fallback_y, |c| i32::from(c.1) + 1);
            let i = m::add_waypoint(&mut s.waypoints, [x, y, z], dim);
            save_waypoints(game, s);
            i
        });
        let name = s.waypoints[index].name.clone();
        s.screen.page = m::Page::Edit { index, name };
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
        m::Page::Map => s.screen.view.zoom_at(map_rect(w, h), mouse, step),
        m::Page::List { scroll } => {
            let rows = m::list_rows(h);
            let max = s.waypoints.len().saturating_sub(rows);
            *scroll = (*scroll as i64 - i64::from(step)).clamp(0, max as i64) as usize;
        }
        m::Page::Edit { .. } => {}
    });
}

/// `native_screen_key_pressed_pre`: typing the name; Escape and Enter step back a page
/// instead of closing the map.
pub(super) fn on_key_pressed(game: &Game<'_>, screen: &NativeScreenView<'_>, key: &str, canceled: &mut bool) {
    if !ours(game, screen.id) || *canceled {
        return;
    }
    with(|s| {
        let editing = matches!(s.screen.page, m::Page::Edit { .. });
        match key {
            "key.keyboard.escape" | "key.keyboard.enter" | "key.keyboard.keypad.enter" if editing => {
                finish_edit(game, s, m::Page::List { scroll: 0 });
                *canceled = true;
            }
            "key.keyboard.escape" if matches!(s.screen.page, m::Page::List { .. }) => {
                s.screen.page = m::Page::Map;
                *canceled = true;
            }
            "key.keyboard.backspace" if editing => {
                if let m::Page::Edit { name, .. } = &mut s.screen.page {
                    name.pop();
                }
                *canceled = true;
            }
            // Letters type, they don't act (the inventory key doesn't close the map).
            _ if editing => *canceled = true,
            _ => {}
        }
    });
}

/// `native_screen_char_typed_pre`: a character of the name.
pub(super) fn on_char(game: &Game<'_>, screen: &NativeScreenView<'_>, c: char, canceled: &mut bool) {
    if !ours(game, screen.id) {
        return;
    }
    with(|s| {
        if let m::Page::Edit { name, .. } = &mut s.screen.page {
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
    if mod_id != m::ID {
        return;
    }
    let mut s = settings(game);
    s.click(button);
    m::set_settings(s);
    for (key, value) in s.stored() {
        game.native_set_setting(m::ID, key, &value);
    }
}
