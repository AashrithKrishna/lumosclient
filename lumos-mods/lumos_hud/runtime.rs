// LumosHUD: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_hud`; `glue.rs` reads the game
// and calls in here. It names nothing outside `std`, so it also compiles alone:
// `rustc --edition 2024 --test runtime.rs` runs the tests below.
//
// The HUD is a list of draw primitives (`Prim`): rectangles, text, items and sprites, in GUI
// pixels. Each module turns what the game shows (`Data`) into primitives at its place; the
// editor adds its outlines, buttons and module list on top. The glue only paints them.

use std::cell::RefCell;

pub const ID: &str = "lumos_hud";
pub const NAME: &str = "LumosHUD";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "Lunar-style HUD modules: FPS, CPS, coordinates, keystrokes, \
armour, potion effects and more. Right Shift opens the editor: drag modules, switch them on \
and off, and pick the Lunar or Minimal style.";

/// The mod's hotkeys: id, label (Options > Mods lists them, rebindable) and default key, by
/// vanilla's key names. `keys.rs` is the one place that reads them.
pub const HOTKEYS: [(&str, &str, &str); 2] = [
    ("editor", "Open the HUD editor", "key.keyboard.right.shift"),
    ("toggle", "Show or hide the HUD", "key.keyboard.unknown"),
];
/// The setting (a file in the mod's data folder) that keeps the layout.
pub const LAYOUT_KEY: &str = "layout";

// ── colours (ARGB) ──

pub const ACCENT: u32 = 0xFFFF_6B2C;
pub const WHITE: u32 = 0xFFFF_FFFF;
pub const RADIUS: i32 = 3;
/// The panels' opacity in percent.
pub const PANEL_OPACITY: u32 = 45;
pub const TEXT: u32 = WHITE;

pub fn with_opacity(rgb: u32, percent: u32) -> u32 {
    let a = ((percent.min(100) * 255) as f32 / 100.0).round() as u32;
    (a << 24) | (rgb & 0xFF_FFFF)
}

/// a → b by t (0..1), every channel including alpha.
pub fn lerp(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let mut out = 0;
    for sh in [0, 8, 16, 24] {
        let ca = ((a >> sh) & 0xFF) as f32;
        let cb = ((b >> sh) & 0xFF) as f32;
        out |= ((ca + (cb - ca) * t).round() as u32 & 0xFF) << sh;
    }
    out
}

/// The colour with its alpha multiplied by f (0..1).
pub fn fade(argb: u32, f: f32) -> u32 {
    let a = (((argb >> 24) & 0xFF) as f32 * f.clamp(0.0, 1.0)).round() as u32;
    (a << 24) | (argb & 0xFF_FFFF)
}

pub fn background() -> u32 {
    with_opacity(0x00_0000, PANEL_OPACITY)
}

/// Labels and units next to a value: the text colour, dimmed.
pub fn dim() -> u32 {
    lerp(TEXT, 0xFF00_0000, 0.32)
}

/// The ring around a Lunar panel: the panel's colour lifted toward white, a bit more opaque.
pub fn border_for(fill: u32) -> u32 {
    let a = (fill >> 24) as f32;
    let lifted = lerp(fill | 0xFF00_0000, WHITE, 0.28);
    let alpha = ((a * 0.9 + 18.0).round() as u32).min(255);
    (alpha << 24) | (lifted & 0xFF_FFFF)
}

/// Vanilla's durability bar colour (`ItemStack.getBarColor`): green to red by what is left.
pub fn durability_colour(damage: u32, max: u32) -> u32 {
    if max == 0 {
        return WHITE;
    }
    let left = max.saturating_sub(damage) as f32 / max as f32;
    let hue = left.max(0.0) / 3.0;
    let [r, g, b] = hsv(hue, 1.0, 1.0);
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

fn hsv(h: f32, s: f32, v: f32) -> [u32; 3] {
    let i = ((h * 6.0).floor() as i32).rem_euclid(6);
    let f = h * 6.0 - (h * 6.0).floor();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b].map(|c| (c * 255.0).round().clamp(0.0, 255.0) as u32)
}

// ── what the glue paints ──

#[derive(Clone, Debug, PartialEq)]
pub enum ItemRef {
    /// The armour slot (0 head … 3 feet) as the player wears it, with its decorations.
    Armor(usize),
    /// A plain stack of this item (the item counter, the editor's samples).
    Id(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Fill { rect: [f32; 4], argb: u32 },
    /// `scale` shrinks the text about its top left (keystrokes' CPS line).
    Text { text: String, at: [f32; 2], argb: u32, shadow: bool, scale: f32 },
    Item { item: ItemRef, at: [f32; 2] },
    /// A GUI sprite (`mob_effect/speed`) stretched into `rect`, faded by `alpha`.
    Sprite { name: String, rect: [f32; 4], alpha: f32 },
}

impl Prim {
    fn moved(self, dx: f32, dy: f32) -> Self {
        match self {
            Prim::Fill { rect: [x, y, w, h], argb } => Prim::Fill { rect: [x + dx, y + dy, w, h], argb },
            Prim::Text { text, at: [x, y], argb, shadow, scale } => {
                Prim::Text { text, at: [x + dx, y + dy], argb, shadow, scale }
            }
            Prim::Item { item, at: [x, y] } => Prim::Item { item, at: [x + dx, y + dy] },
            Prim::Sprite { name, rect: [x, y, w, h], alpha } => {
                Prim::Sprite { name, rect: [x + dx, y + dy, w, h], alpha }
            }
        }
    }
}

fn fill(out: &mut Vec<Prim>, x: f32, y: f32, w: f32, h: f32, argb: u32) {
    if argb >> 24 != 0 && w > 0.0 && h > 0.0 {
        out.push(Prim::Fill { rect: [x, y, w, h], argb });
    }
}

fn text(out: &mut Vec<Prim>, s: &str, x: f32, y: f32, argb: u32) {
    out.push(Prim::Text { text: s.to_owned(), at: [x, y], argb: argb | 0xFF00_0000, shadow: true, scale: 1.0 });
}

// ── rounded panels ──

/// How far row `y` (0 = top) of a top corner is cut in, for radius r.
fn inset(y: i32, r: i32) -> i32 {
    if r <= 0 || y >= r {
        return 0;
    }
    if r == 1 {
        return 1;
    }
    let dy = f64::from(r) - f64::from(y) - 0.5;
    r - ((f64::from(r * r) - dy * dy).sqrt() + 0.25).floor() as i32
}

fn row_inset(y: i32, h: i32, r: i32) -> i32 {
    if y < r {
        inset(y, r)
    } else if y >= h - r {
        inset(h - 1 - y, r)
    } else {
        0
    }
}

/// A rounded w×h panel with radius r as rectangles `(x0, y0, x1, y1, is_border)`: the inside
/// and (with `border`) a 1-pixel ring along the same curve. They cover the shape exactly once.
pub fn panel_runs(w: i32, h: i32, r: i32, border: bool) -> Vec<(i32, i32, i32, i32, bool)> {
    let mut out = Vec::new();
    if w <= 0 || h <= 0 {
        return out;
    }
    let r = r.clamp(0, (w.min(h)) / 2);
    let ir = (r - 1).max(0);
    let mut start = 0;
    let mut prev: Option<(i32, i32)> = None;
    for y in 0..=h {
        let cur = (y < h).then(|| {
            let o = row_inset(y, h, r);
            if !border {
                return (o, -2);
            }
            let mut i = -1;
            if y >= 1 && y < h - 1 && w > 2 {
                let rr = ir.min((w - 2).min(h - 2) / 2);
                i = (o + 1).max(1 + row_inset(y - 1, h - 2, rr));
                if i >= w - i {
                    i = -1;
                }
            }
            (o, i)
        });
        if let Some(p) = prev
            && cur != Some(p)
        {
            emit(&mut out, w, start, y, p.0, p.1);
            start = y;
        }
        if prev.is_none() {
            start = y;
        }
        prev = cur;
    }
    out
}

fn emit(out: &mut Vec<(i32, i32, i32, i32, bool)>, w: i32, y0: i32, y1: i32, o: i32, i: i32) {
    match i {
        -2 => out.push((o, y0, w - o, y1, false)),
        -1 => out.push((o, y0, w - o, y1, true)),
        _ => {
            out.push((o, y0, i, y1, true));
            out.push((i, y0, w - i, y1, false));
            out.push((w - i, y0, w - o, y1, true));
        }
    }
}

/// A rounded panel at (x, y): `fill` inside and a faint lighter ring (none on a clear panel).
pub fn panel(out: &mut Vec<Prim>, x: f32, y: f32, w: f32, h: f32, radius: i32, colour: u32) {
    let ring = if colour >> 24 == 0 { 0 } else { border_for(colour) };
    for (x0, y0, x1, y1, is_border) in panel_runs(w.round() as i32, h.round() as i32, radius, ring != 0) {
        let c = if is_border { ring } else { colour };
        fill(out, x + x0 as f32, y + y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32, c);
    }
}

fn outline(out: &mut Vec<Prim>, [x, y, w, h]: [f32; 4], c: u32) {
    let (l, t, r, b) = (x.round(), y.round(), (x + w).round(), (y + h).round());
    fill(out, l, t, r - l, 1.0, c);
    fill(out, l, b - 1.0, r - l, 1.0, c);
    fill(out, l, t, 1.0, b - t, c);
    fill(out, r - 1.0, t, 1.0, b - t, c);
}

// ── icons: 9×9 pixel art. `.` clear, `#` text colour, `d` faded, `a` accent, Y G O R fixed ──

pub const ICON: f32 = 9.0;
pub type Grid = [[u8; 9]; 9];

const fn grid(rows: [&str; 9]) -> Grid {
    let mut g = [[b'.'; 9]; 9];
    let mut r = 0;
    while r < 9 {
        let bytes = rows[r].as_bytes();
        let mut c = 0;
        while c < 9 {
            g[r][c] = bytes[c];
            c += 1;
        }
        r += 1;
    }
    g
}

pub const FPS_ICON: Grid = grid(["#########", "#.......#", "#.....a.#", "#.a..a..#", "#..aa...#", "#.......#", "#########", "...###...", "..#####.."]);
pub const CPS_ICON: Grid = grid(["..#####..", ".#aa#..#.", ".#aa#..#.", ".#aa#..#.", ".#######.", ".#.....#.", ".#.....#.", ".#.....#.", "..#####.."]);
pub const COORDS_ICON: Grid = grid(["..#####..", ".#.....#.", "#..aaa..#", "#..a.a..#", "#..aaa..#", ".#.....#.", "..#...#..", "...#.#...", "....#...."]);
pub const BIOME_ICON: Grid = grid(["....a....", "...aaa...", "..aaaaa..", "...aaa...", "..aaaaa..", ".aaaaaaa.", "....#....", "....#....", "..#####.."]);
pub const CLOCK_ICON: Grid = grid(["..#####..", ".#..#..#.", "#...#...#", "#...#...#", "#...aaa.#", "#.......#", "#.......#", ".#.....#.", "..#####.."]);
pub const SERVER_ICON: Grid = grid(["..#####..", ".#..a..#.", "#..a.a..#", "#aaaaaaa#", "#..a.a..#", "#aaaaaaa#", "#..a.a..#", ".#..a..#.", "..#####.."]);
pub const SUN_ICON: Grid = grid(["....Y....", ".Y.....Y.", "...YYY...", "..YYYYY..", "Y.YYYYY.Y", "..YYYYY..", "...YYY...", ".Y.....Y.", "....Y...."]);
pub const MOON_ICON: Grid = grid(["..####...", ".##d.....", "##d......", "##d......", "##d......", "##d......", "##d......", ".##d.....", "..####..."]);
pub const SPEED_ICON: Grid = grid([".........", ".....#...", ".....##..", "dd.#####.", "...######", "dd.#####.", ".....##..", ".....#...", "........."]);
pub const REACH_ICON: Grid = grid([".......##", "......#a#", ".....#a#.", "....#a#..", ".#.#a#...", "..#a#....", "..##.....", ".#..#....", "#........"]);
pub const COMBO_ICON: Grid = grid(["....a....", "...aa....", "...aaa.a.", "..aaaaaa.", ".aaaYaaa.", ".aaYYYaa.", ".aaYYYaa.", "..aaYaa..", "...aaa..."]);
pub const MEMORY_ICON: Grid = grid(["..#.#.#..", ".#######.", "##.....##", ".#.aaa.#.", "##.aaa.##", ".#.aaa.#.", "##.....##", ".#######.", "..#.#.#.."]);
pub const SPRINT_ICON: Grid = grid([".....aa..", ".....aa..", "...###...", "..#.##.#.", "....##...", "...#..#..", "..#....#.", ".#.....#.", "........."]);
pub const SNEAK_ICON: Grid = grid([".........", ".........", "......aa.", "....#.aa.", "...####..", "..#.##...", "....###..", "...#...#.", "..##...##"]);
const RING: Grid = grid(["...ddd...", ".dd...dd.", ".d.....d.", "d.......d", "d.......d", "d.......d", ".d.....d.", ".dd...dd.", "...ddd..."]);

/// A compass whose needle points the way the player faces, north up (yaw 0 = south, 90 = west).
pub fn compass(yaw: f32) -> Grid {
    let mut g = RING;
    let r = f64::from(yaw).to_radians();
    let (dx, dy) = (-r.sin(), r.cos());
    for i in 1..=4 {
        let x = (4.0 + dx * f64::from(i) * 0.85).round() as i32;
        let y = (4.0 + dy * f64::from(i) * 0.85).round() as i32;
        if (0..9).contains(&x) && (0..9).contains(&y) {
            g[y as usize][x as usize] = b'a';
        }
    }
    for i in 1..=2 {
        let x = (4.0 - dx * f64::from(i) * 0.85).round() as i32;
        let y = (4.0 - dy * f64::from(i) * 0.85).round() as i32;
        if (0..9).contains(&x) && (0..9).contains(&y) && g[y as usize][x as usize] == b'.' {
            g[y as usize][x as usize] = b'#';
        }
    }
    g[4][4] = b'#';
    g
}

/// How many of 4 signal bars a latency lights.
pub fn signal_bars(ms: i32) -> u32 {
    match ms {
        ..0 => 0,
        0..=75 => 4,
        76..=150 => 3,
        151..=300 => 2,
        _ => 1,
    }
}

pub fn signal(ms: i32) -> Grid {
    let lit = signal_bars(ms);
    let c = match lit {
        4 => b'G',
        3 => b'Y',
        2 => b'O',
        _ => b'R',
    };
    let mut g = [[b'.'; 9]; 9];
    for b in 0..4u32 {
        let x = (1 + b * 2) as usize;
        let h = 3 + b * 2;
        for y in (9 - h) as usize..9 {
            g[y][x] = if b < lit { c } else { b'd' };
        }
    }
    g
}

fn fixed(c: u8) -> u32 {
    match c {
        b'Y' => 0xFFFF_D34A,
        b'G' => 0xFF5A_DC6E,
        b'O' => 0xFFFF_9A3C,
        b'R' => 0xFFFF_5050,
        _ => 0,
    }
}

/// A 9×9 icon at (x, y); runs of one colour in a row are one rectangle.
pub fn icon(out: &mut Vec<Prim>, g: &Grid, x: f32, y: f32, primary: u32, accent: u32) {
    let dim = fade(primary, 0.45);
    for (row, line) in g.iter().enumerate() {
        let mut col = 0;
        while col < 9 {
            let ch = line[col];
            let mut end = col + 1;
            while end < 9 && line[end] == ch {
                end += 1;
            }
            let c = match ch {
                b'.' => 0,
                b'#' => primary,
                b'd' => dim,
                b'a' => accent,
                other => fixed(other),
            };
            fill(out, x + col as f32, y + row as f32, (end - col) as f32, 1.0, c);
            col = end;
        }
    }
}

// ── text helpers ──

const FACING: [&str; 4] = ["South", "West", "North", "East"];
const AXES: [&str; 4] = ["+Z", "-X", "-Z", "+X"];

/// Yaw (0 = south, 90 = west) → 0..3 for South, West, North, East.
pub fn quadrant(yaw: f32) -> usize {
    let y = yaw.rem_euclid(360.0);
    (((y + 45.0) / 90.0).floor() as usize) % 4
}

pub fn facing(yaw: f32) -> String {
    let q = quadrant(yaw);
    format!("{} ({})", FACING[q], AXES[q])
}

/// Effect time left from whole seconds: "1:05", "1:02:03", "∞" for -1.
pub fn duration(secs: i32) -> String {
    if secs < 0 {
        return "\u{221E}".into();
    }
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

pub fn roman(n: u32) -> String {
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    let table = [
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
        (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];
    let (mut n, mut s) = (n, String::new());
    for (v, r) in table {
        while n >= v {
            n -= v;
            s.push_str(r);
        }
    }
    s
}

/// "minecraft:dark_forest" → "Dark Forest", for a name with no translation.
pub fn pretty_id(id: &str) -> String {
    let path = id.split_once(':').map_or(id, |(_, p)| p);
    let path = path.rsplit('/').next().unwrap_or(path);
    path.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ── modules and their places ──

/// Where on the screen a module is placed from: row * 3 + column (0 top left … 8 bottom right).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub on: bool,
    pub anchor: u8,
    pub x: f32,
    pub y: f32,
}

pub const TL: u8 = 0;
pub const TC: u8 = 1;
pub const TR: u8 = 2;
pub const ML: u8 = 3;
pub const MR: u8 = 5;
pub const BL: u8 = 6;
pub const BC: u8 = 7;
pub const BR: u8 = 8;

/// Every module in drawing order: id, name, and its default place.
pub const MODULES: [(&str, &str, Slot); 18] = [
    ("fps", "FPS", Slot { on: true, anchor: TL, x: 4.0, y: 4.0 }),
    ("cps", "CPS", Slot { on: true, anchor: TL, x: 4.0, y: 22.0 }),
    ("coordinates", "Coordinates", Slot { on: true, anchor: TL, x: 4.0, y: 40.0 }),
    ("keystrokes", "Keystrokes", Slot { on: true, anchor: TL, x: 4.0, y: 58.0 }),
    ("direction", "Direction", Slot { on: false, anchor: ML, x: 4.0, y: -36.0 }),
    ("biome", "Biome", Slot { on: false, anchor: ML, x: 4.0, y: -18.0 }),
    ("armor", "Armor Status", Slot { on: true, anchor: MR, x: -4.0, y: 0.0 }),
    ("potions", "Potion Effects", Slot { on: true, anchor: TR, x: -4.0, y: 4.0 }),
    ("ping", "Ping", Slot { on: false, anchor: BR, x: -4.0, y: -4.0 }),
    ("clock", "Clock", Slot { on: false, anchor: TC, x: 0.0, y: 4.0 }),
    ("server", "Server Address", Slot { on: false, anchor: BR, x: -4.0, y: -22.0 }),
    ("day", "Day Counter", Slot { on: false, anchor: BR, x: -4.0, y: -40.0 }),
    ("speed", "Speed", Slot { on: false, anchor: ML, x: 4.0, y: 0.0 }),
    ("reach", "Reach Display", Slot { on: false, anchor: ML, x: 4.0, y: 18.0 }),
    ("combo", "Combo Counter", Slot { on: false, anchor: ML, x: 4.0, y: 36.0 }),
    ("itemcounter", "Item Counter", Slot { on: false, anchor: BC, x: 110.0, y: -4.0 }),
    ("memory", "Memory", Slot { on: false, anchor: BR, x: -4.0, y: -58.0 }),
    ("sprint", "Toggle Sprint", Slot { on: false, anchor: BL, x: 4.0, y: -48.0 }),
];
pub const COUNT: usize = MODULES.len();

pub fn index_of(id: &str) -> Option<usize> {
    MODULES.iter().position(|m| m.0 == id)
}

/// The modules drawn with no background by default.
fn plain(i: usize) -> bool {
    matches!(MODULES[i].0, "armor" | "itemcounter" | "sprint")
}

/// lumos_hud's layout file: the style, then one line per module (`fps 1 0 4 4`: on, anchor, x, y).
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub lunar: bool,
    pub slots: [Slot; COUNT],
}

impl Default for Config {
    fn default() -> Self {
        Self { lunar: true, slots: MODULES.map(|m| m.2) }
    }
}

impl Config {
    pub fn parse(text: &str) -> Self {
        let mut c = Self::default();
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts.as_slice() {
                ["style", s] => c.lunar = *s != "minimal",
                [id, on, anchor, x, y] => {
                    let Some(i) = index_of(id) else { continue };
                    let (Ok(anchor), Ok(x), Ok(y)) = (anchor.parse::<u8>(), x.parse::<f32>(), y.parse::<f32>()) else {
                        continue;
                    };
                    if anchor > 8 || !x.is_finite() || !y.is_finite() {
                        continue;
                    }
                    c.slots[i] = Slot { on: *on == "1", anchor, x, y };
                }
                _ => {}
            }
        }
        c
    }

    pub fn serialize(&self) -> String {
        let mut s = format!("style {}\n", if self.lunar { "lunar" } else { "minimal" });
        for (i, slot) in self.slots.iter().enumerate() {
            s += &format!("{} {} {} {} {}\n", MODULES[i].0, u8::from(slot.on), slot.anchor, slot.x, slot.y);
        }
        s
    }

    /// Options > Mods: the style, a reset, then each module.
    pub fn labels(&self) -> Vec<String> {
        let mut v = vec![
            format!("Style: {}", if self.lunar { "Lunar" } else { "Minimal" }),
            "Reset layout".to_owned(),
        ];
        for (i, m) in MODULES.iter().enumerate() {
            v.push(format!("{}: {}", m.1, if self.slots[i].on { "ON" } else { "OFF" }));
        }
        v
    }

    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.lunar = !self.lunar,
            1 => *self = Self { lunar: self.lunar, ..Self::default() },
            b if b - 2 < COUNT => self.slots[b - 2].on = !self.slots[b - 2].on,
            _ => {}
        }
    }
}

fn anchor_f(anchor: u8) -> (f32, f32) {
    (f32::from(anchor % 3) / 2.0, f32::from(anchor / 3) / 2.0)
}

/// The box of a w×h module placed at anchor + offset, kept on screen.
pub fn place(slot: &Slot, w: f32, h: f32, sw: f32, sh: f32) -> [f32; 4] {
    let (fx, fy) = anchor_f(slot.anchor);
    let left = (fx * sw - fx * w + slot.x).clamp(0.0, (sw - w).max(0.0));
    let top = (fy * sh - fy * h + slot.y).clamp(0.0, (sh - h).max(0.0));
    [left, top, w, h]
}

/// Anchor and offset for a box dropped at (left, top): the anchor of the screen third its
/// centre is in.
pub fn from_absolute(left: f32, top: f32, w: f32, h: f32, sw: f32, sh: f32) -> (u8, f32, f32) {
    let left = left.clamp(0.0, (sw - w).max(0.0));
    let top = top.clamp(0.0, (sh - h).max(0.0));
    let (cx, cy) = (left + w / 2.0, top + h / 2.0);
    let col = if cx < sw / 3.0 { 0 } else if cx > sw * 2.0 / 3.0 { 2 } else { 1 };
    let row = if cy < sh / 3.0 { 0 } else if cy > sh * 2.0 / 3.0 { 2 } else { 1 };
    let anchor = row * 3 + col;
    let (fx, fy) = anchor_f(anchor);
    let x = left - fx * sw + fx * w;
    let y = top - fy * sh + fy * h;
    (anchor, (x * 2.0).round() / 2.0, (y * 2.0).round() / 2.0)
}

/// Snaps a dragged box's edges or centre to the screen's and to the other boxes' edges and
/// centres within `threshold`; the guide lines it snapped to.
pub fn snap(moving: [f32; 4], others: &[[f32; 4]], sw: f32, sh: f32, threshold: f32) -> ([f32; 2], [Option<f32>; 2]) {
    let [l, t, w, h] = moving;
    let mut xs = vec![0.0, sw / 2.0, sw];
    let mut ys = vec![0.0, sh / 2.0, sh];
    for o in others {
        xs.extend([o[0], o[0] + o[2] / 2.0, o[0] + o[2]]);
        ys.extend([o[1], o[1] + o[3] / 2.0, o[1] + o[3]]);
    }
    let best = |own: [f32; 3], targets: &[f32]| {
        let mut best: Option<(f32, f32)> = None;
        for e in own {
            for &t in targets {
                let d = t - e;
                if d.abs() <= threshold && best.is_none_or(|b| d.abs() < b.0.abs()) {
                    best = Some((d, t));
                }
            }
        }
        best
    };
    let bx = best([l, l + w / 2.0, l + w], &xs);
    let by = best([t, t + h / 2.0, t + h], &ys);
    let left = (l + bx.map_or(0.0, |b| b.0)).clamp(0.0, (sw - w).max(0.0));
    let top = (t + by.map_or(0.0, |b| b.0)).clamp(0.0, (sh - h).max(0.0));
    ([left, top], [bx.map(|b| b.1), by.map(|b| b.1)])
}

// ── what the game shows ──

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Piece {
    pub damage: u32,
    pub max: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effect {
    /// "Speed II".
    pub name: String,
    /// Whole seconds left, -1 for an infinite one.
    pub secs: i32,
    /// Its HUD sprite (`mob_effect/speed`).
    pub sprite: String,
}

/// Keystrokes' keys: forward, left, back, right, jump, attack, use.
pub const KEYS: [&str; 7] = ["key.forward", "key.left", "key.back", "key.right", "key.jump", "key.attack", "key.use"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Data {
    pub fps: u32,
    pub cps: [u32; 2],
    /// The player's feet, None with reduced debug info or no player.
    pub pos: Option<[f64; 3]>,
    pub yaw: f32,
    pub keys: [bool; 7],
    /// W A S D as the player bound them.
    pub key_labels: [String; 4],
    pub biome: Option<String>,
    pub armor: [Option<Piece>; 4],
    pub effects: Vec<Effect>,
    pub ping: Option<i32>,
    pub clock: Option<String>,
    pub server: Option<String>,
    pub day_time: Option<i64>,
    pub speed: Option<f64>,
    pub reach: Option<f64>,
    pub combo: u32,
    /// The counted item (arrows for a bow) and how many the player carries.
    pub held: Option<(String, u32)>,
    /// Used and total memory in bytes.
    pub memory: Option<(u64, u64)>,
    /// "Sprinting (Toggled)" and the like, and whether it is a sneak.
    pub sprint: Option<(String, bool)>,
}

/// Keystrokes' glow, which fades out after a key is let go.
#[derive(Clone, Debug, Default)]
pub struct Anim {
    glow: [f32; 7],
    last: Option<f64>,
}

impl Anim {
    fn step(&mut self, keys: [bool; 7], now: f64) {
        let dt = self.last.map_or(0.0, |l| ((now - l) * 1000.0).clamp(0.0, 250.0) as f32);
        self.last = Some(now);
        for (g, down) in self.glow.iter_mut().zip(keys) {
            *g = approach(*g, if down { 1.0 } else { 0.0 }, dt, 35.0, 200.0);
        }
    }
}

/// Moves `cur` toward `target`: rising over `rise` ms, falling over `fall` ms.
pub fn approach(cur: f32, target: f32, dt: f32, rise: f32, fall: f32) -> f32 {
    if dt <= 0.0 {
        return cur;
    }
    let span = if target > cur { rise } else { fall };
    if span <= 0.0 {
        return target;
    }
    let step = dt / span;
    if target > cur { (cur + step).min(target) } else { (cur - step).max(target) }
}

pub type Width<'a> = &'a dyn Fn(&str) -> f32;

struct Ctx<'a> {
    lunar: bool,
    preview: bool,
    bg: bool,
    now: f64,
    width: Width<'a>,
}

const L_PAD: f32 = 5.0;
const L_GAP: f32 = 4.0;
const L_H: f32 = 17.0;
const PAD: f32 = 4.0;

impl Ctx<'_> {
    fn bg(&self) -> u32 {
        if self.bg { background() } else { 0 }
    }
    fn boxed(&self, out: &mut Vec<Prim>, x: f32, y: f32, w: f32, h: f32) {
        if !self.bg {
            return;
        }
        if self.lunar {
            panel(out, x, y, w, h, RADIUS, background());
        } else {
            fill(out, x, y, w, h, background());
        }
    }
    fn w(&self, s: &str) -> f32 {
        (self.width)(s)
    }
}

type Drawn = (f32, f32, Vec<Prim>);

/// A one-line module. Minimal: "FPS: 144" in a flat box. Lunar: a rounded panel with the icon,
/// then the value with its label or unit dimmed ("144 FPS", "Biome Plains").
fn line(c: &Ctx, label: Option<&str>, after: bool, value: &str, grid: &Grid) -> Drawn {
    let mut out = Vec::new();
    if !c.lunar {
        let s = match label {
            Some(l) => format!("{l}: {value}"),
            None => value.to_owned(),
        };
        let w = (c.w(&s) + PAD * 2.0).max(28.0);
        let h = 8.0 + PAD * 2.0;
        c.boxed(&mut out, 0.0, 0.0, w, h);
        text(&mut out, &s, ((w - c.w(&s)) / 2.0).floor(), PAD, TEXT);
        return (w, h, out);
    }
    let (pre, post) = match label {
        Some(l) if after => (None, Some(l)),
        Some(l) => (Some(l), None),
        None => (None, None),
    };
    let mut tw = c.w(value);
    if let Some(p) = pre {
        tw += c.w(&format!("{p} "));
    }
    if let Some(p) = post {
        tw += c.w(&format!(" {p}"));
    }
    let iw = ICON + L_GAP;
    let w = (L_PAD * 2.0 + iw + tw).max(24.0);
    let h = L_H;
    c.boxed(&mut out, 0.0, 0.0, w, h);
    let mut x = ((w - iw - tw) / 2.0).floor();
    icon(&mut out, grid, x, ((h - ICON) / 2.0).floor(), TEXT, ACCENT);
    x += iw;
    let ty = ((h - 7.0) / 2.0).floor();
    if let Some(p) = pre {
        text(&mut out, p, x, ty, dim());
        x += c.w(&format!("{p} "));
    }
    text(&mut out, value, x, ty, TEXT);
    x += c.w(value);
    if let Some(p) = post {
        text(&mut out, &format!(" {p}"), x, ty, dim());
    }
    (w, h, out)
}

fn coordinates(c: &Ctx, d: &Data) -> Option<Drawn> {
    let p = d.pos.or(c.preview.then_some([128.0, 64.0, -256.0]))?;
    let [x, y, z] = p.map(|v| v.floor() as i64);
    if !c.lunar {
        let s = format!("XYZ: {x}, {y}, {z}");
        return Some(line(c, None, false, &s, &COORDS_ICON));
    }
    Some(line(c, Some("XYZ"), false, &format!("{x} {y} {z}"), &COORDS_ICON))
}

fn keystrokes(c: &Ctx, d: &Data, anim: &Anim) -> Drawn {
    const K: f32 = 22.0;
    const GAP: f32 = 2.0;
    const W: f32 = K * 3.0 + GAP * 2.0;
    let mut out = Vec::new();
    let h = K * 2.0 + GAP + GAP + K + GAP + 10.0;
    let key = |out: &mut Vec<Prim>, i: usize, x: f32, y: f32, w: f32, kh: f32, label: &str, sub: Option<String>| {
        let down = d.keys[i];
        let t = if c.lunar { anim.glow[i] } else if down { 1.0 } else { 0.0 };
        if c.lunar {
            let lit = with_opacity(ACCENT, 85);
            let rest = c.bg();
            let f = lerp(if rest == 0 { lit & 0xFF_FFFF } else { rest }, lit, t);
            if f >> 24 != 0 {
                panel(out, x, y, w, kh, RADIUS, f);
            }
        } else if down {
            fill(out, x, y, w, kh, with_opacity(WHITE, 75));
        } else if c.bg {
            fill(out, x, y, w, kh, background());
        }
        let colour = if c.lunar { lerp(TEXT, WHITE, t) } else if down { 0xFF00_0000 } else { TEXT };
        let shadow = c.lunar || !down;
        let lw = c.w(label);
        match sub {
            None => out.push(Prim::Text {
                text: label.to_owned(),
                at: [x + ((w - lw) / 2.0).floor(), y + ((kh - 7.0) / 2.0).floor()],
                argb: colour,
                shadow,
                scale: 1.0,
            }),
            Some(sub) => {
                let cx = x + w / 2.0;
                out.push(Prim::Text { text: label.to_owned(), at: [(cx - lw / 2.0).floor(), y + 3.0], argb: colour, shadow, scale: 1.0 });
                let sw = c.w(&sub) * 0.75;
                let sc = if c.lunar { lerp(dim(), WHITE, t) } else { colour };
                out.push(Prim::Text { text: sub, at: [cx - sw / 2.0, y + 13.0], argb: sc, shadow, scale: 0.75 });
            }
        }
        t
    };
    let label = |i: usize, fallback: &str| -> String {
        let s = d.key_labels[i].trim();
        let s = if s.is_empty() { fallback } else { s };
        s.chars().take(3).collect()
    };
    key(&mut out, 0, K + GAP, 0.0, K, K, &label(0, "W"), None);
    key(&mut out, 1, 0.0, K + GAP, K, K, &label(1, "A"), None);
    key(&mut out, 2, K + GAP, K + GAP, K, K, &label(2, "S"), None);
    key(&mut out, 3, (K + GAP) * 2.0, K + GAP, K, K, &label(3, "D"), None);
    let mut y = K * 2.0 + GAP * 2.0;
    let mw = ((W - GAP) / 2.0).floor();
    key(&mut out, 5, 0.0, y, mw, K, "LMB", Some(format!("{} CPS", d.cps[0])));
    key(&mut out, 6, mw + GAP, y, W - mw - GAP, K, "RMB", Some(format!("{} CPS", d.cps[1])));
    y += K + GAP;
    let t = key(&mut out, 4, 0.0, y, W, 10.0, "", None);
    out.retain(|p| !matches!(p, Prim::Text { text, .. } if text.is_empty()));
    let bar = if c.lunar { lerp(TEXT, WHITE, t) } else if d.keys[4] { 0xFF00_0000 } else { TEXT };
    fill(&mut out, W / 2.0 - 10.0, y + 4.0, 20.0, 1.0, bar);
    (W, h, out)
}

const PREVIEW_ARMOR: [&str; 4] = [
    "minecraft:diamond_helmet",
    "minecraft:diamond_chestplate",
    "minecraft:diamond_leggings",
    "minecraft:diamond_boots",
];

fn armor(c: &Ctx, d: &Data) -> Option<Drawn> {
    let mut pieces: Vec<(ItemRef, String, u32)> = Vec::new();
    for (i, p) in d.armor.iter().enumerate() {
        if let Some(p) = p {
            let (s, colour) = if p.max > 0 {
                (p.max.saturating_sub(p.damage).to_string(), durability_colour(p.damage, p.max))
            } else {
                (String::new(), TEXT)
            };
            pieces.push((ItemRef::Armor(i), s, colour));
        }
    }
    if pieces.is_empty() && c.preview {
        for id in PREVIEW_ARMOR {
            pieces.push((ItemRef::Id(id.into()), "363".into(), durability_colour(0, 363)));
        }
    }
    if pieces.is_empty() {
        return None;
    }
    let tw = pieces.iter().map(|p| c.w(&p.1)).fold(0.0, f32::max);
    let w = 16.0 + if tw > 0.0 { 3.0 + tw } else { 0.0 } + if c.lunar { 7.0 } else { 4.0 };
    let h = pieces.len() as f32 * 17.0 + 3.0;
    let mut out = Vec::new();
    c.boxed(&mut out, 0.0, 0.0, w, h);
    let ix = if c.lunar { 3.0 } else { 2.0 };
    let mut y = 2.0;
    for (item, s, colour) in pieces {
        out.push(Prim::Item { item, at: [ix, y] });
        if !s.is_empty() {
            text(&mut out, &s, ix + 19.0, y + 4.0, colour);
        }
        y += 17.0;
    }
    Some((w, h, out))
}

fn potions(c: &Ctx, d: &Data) -> Option<Drawn> {
    let mut list = d.effects.clone();
    if list.is_empty() && c.preview {
        list = vec![
            Effect { name: "Speed II".into(), secs: 95, sprite: "mob_effect/speed".into() },
            Effect { name: "Fire Resistance".into(), secs: 8, sprite: "mob_effect/fire_resistance".into() },
        ];
    }
    if list.is_empty() {
        return None;
    }
    const ROW: f32 = 22.0;
    let tw = list
        .iter()
        .map(|e| c.w(&e.name).max(c.w(&duration(e.secs))))
        .fold(0.0, f32::max);
    let icons = 20.0;
    let (w, h) = if c.lunar {
        (icons + tw + 10.0, list.len() as f32 * ROW + 4.0)
    } else {
        (icons + tw + PAD * 2.0, list.len() as f32 * ROW + 2.0)
    };
    let mut out = Vec::new();
    c.boxed(&mut out, 0.0, 0.0, w, h);
    let mut y = if c.lunar { 3.0 } else { 2.0 };
    for e in &list {
        let x = if c.lunar { 5.0 } else { PAD };
        let ending = c.lunar && e.secs >= 0 && e.secs <= 10;
        // Fades in and out twice a second in its last 10 seconds.
        let alpha = if ending {
            0.3 + 0.7 * (0.5 + 0.5 * (c.now * std::f64::consts::PI * 4.0).cos()) as f32
        } else {
            1.0
        };
        out.push(Prim::Sprite { name: e.sprite.clone(), rect: [x - 2.0, y + 1.0, 18.0, 18.0], alpha });
        text(&mut out, &e.name, x + 20.0, y + 2.0, TEXT);
        let tc = if ending { ACCENT } else if c.lunar { dim() } else { 0xFFAA_AAAA };
        text(&mut out, &duration(e.secs), x + 20.0, y + 12.0, tc);
        y += ROW;
    }
    Some((w, h, out))
}

fn item_counter(c: &Ctx, d: &Data) -> Option<Drawn> {
    let (item, n) = d.held.clone().or(c.preview.then(|| ("minecraft:arrow".to_owned(), 64)))?;
    let s = n.to_string();
    let w = 16.0 + 3.0 + c.w(&s) + if c.lunar { 8.0 } else { 4.0 };
    let h = 20.0;
    let mut out = Vec::new();
    c.boxed(&mut out, 0.0, 0.0, w, h);
    let x = if c.lunar { 3.0 } else { 2.0 };
    out.push(Prim::Item { item: ItemRef::Id(item), at: [x, 2.0] });
    text(&mut out, &s, x + 19.0, 6.0, TEXT);
    Some((w, h, out))
}

/// Module `i` at (0, 0), or None when it has nothing to show.
fn module(i: usize, c: &Ctx, d: &Data, anim: &Anim) -> Option<Drawn> {
    let p = c.preview;
    let or = |v: Option<String>, sample: &str| v.or(p.then(|| sample.to_owned()));
    Some(match MODULES[i].0 {
        "fps" => line(c, Some("FPS"), true, &d.fps.to_string(), &FPS_ICON),
        "cps" => line(c, Some("CPS"), true, &format!("{} | {}", d.cps[0], d.cps[1]), &CPS_ICON),
        "coordinates" => coordinates(c, d)?,
        "keystrokes" => keystrokes(c, d, anim),
        "direction" => {
            let yaw = if d.pos.is_some() || !p { d.yaw } else { 180.0 };
            line(c, Some("Facing"), false, &facing(yaw), &compass(yaw))
        }
        "biome" => line(c, Some("Biome"), false, &or(d.biome.clone(), "Plains")?, &BIOME_ICON),
        "armor" => armor(c, d)?,
        "potions" => potions(c, d)?,
        "ping" => {
            let ms = d.ping.or(p.then_some(42))?;
            line(c, Some("Ping"), false, &format!("{ms} ms"), &signal(ms))
        }
        "clock" => line(c, None, false, &or(d.clock.clone(), "12:00")?, &CLOCK_ICON),
        "server" => line(c, None, false, &or(d.server.clone(), "play.example.net")?, &SERVER_ICON),
        "day" => {
            let t = d.day_time.or(p.then_some(11 * 24000))?;
            let tod = t.rem_euclid(24000);
            let grid = if !(12500..=23500).contains(&tod) { &SUN_ICON } else { &MOON_ICON };
            line(c, Some("Day"), false, &(t.div_euclid(24000) + 1).to_string(), grid)
        }
        "speed" => {
            let s = d.speed.or(p.then_some(5.61))?;
            line(c, Some("Speed"), false, &format!("{s:.2} m/s"), &SPEED_ICON)
        }
        "reach" => {
            let r = d.reach.or(p.then_some(3.0))?;
            line(c, Some("Reach"), false, &format!("{r:.2} blocks"), &REACH_ICON)
        }
        "combo" => {
            let n = if d.combo == 0 && p { 3 } else { d.combo };
            let s = if n == 0 { "No Combo".to_owned() } else { format!("{n} Combo") };
            line(c, None, false, &s, &COMBO_ICON)
        }
        "itemcounter" => item_counter(c, d)?,
        "memory" => {
            let (used, max) = d.memory.or(p.then_some((2 << 30, 8 << 30)))?;
            let s = format!("{}% {}/{} MB", used * 100 / max.max(1), used >> 20, max >> 20);
            line(c, Some("Mem"), false, &s, &MEMORY_ICON)
        }
        "sprint" => {
            let (s, sneak) = d.sprint.clone().or(p.then(|| ("Sprinting (Toggled)".to_owned(), false)))?;
            let s = if c.lunar { s } else { format!("[{s}]") };
            line(c, None, false, &s, if sneak { &SNEAK_ICON } else { &SPRINT_ICON })
        }
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub module: usize,
    pub rect: [f32; 4],
    pub prims: Vec<Prim>,
}

/// Every module that is on and has something to show, at its place on a sw×sh screen.
pub fn frame(cfg: &Config, d: &Data, anim: &mut Anim, sw: f32, sh: f32, preview: bool, now: f64, width: Width) -> Vec<Placed> {
    anim.step(d.keys, now);
    let mut out = Vec::new();
    for i in 0..COUNT {
        let slot = &cfg.slots[i];
        if !slot.on {
            continue;
        }
        let c = Ctx { lunar: cfg.lunar, preview, bg: !plain(i), now, width };
        let Some((w, h, prims)) = module(i, &c, d, anim) else { continue };
        let rect = place(slot, w, h, sw, sh);
        let prims = prims.into_iter().map(|p| p.moved(rect[0], rect[1])).collect();
        out.push(Placed { module: i, rect, prims });
    }
    out
}

// ── the editor (Right Shift) ──

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Modules,
    Style,
    Reset,
    Done,
    Toggle(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub rect: [f32; 4],
    pub label: String,
    pub action: Action,
    pub on: bool,
}

fn hit([x, y, w, h]: [f32; 4], [mx, my]: [f32; 2]) -> bool {
    mx >= x && mx < x + w && my >= y && my < y + h
}

const SNAP: f32 = 4.0;
pub const HINT: &str = "Drag to move \u{b7} Right-click to hide \u{b7} Shift: no snapping";

#[derive(Clone, Debug, Default)]
pub struct Editor {
    pub panel: bool,
    /// The module being dragged and where it was grabbed, from its top left.
    drag: Option<(usize, f32, f32)>,
    guides: [Option<f32>; 2],
    /// The boxes drawn last frame, for clicks.
    placed: Vec<(usize, [f32; 4])>,
    /// "Done" was clicked: the glue closes the screen.
    pub close: bool,
    /// The layout changed and should be saved.
    pub dirty: bool,
}

impl Editor {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn buttons(&self, cfg: &Config, sw: f32, sh: f32) -> Vec<Button> {
        let labels = [
            ("Modules", Action::Modules),
            (if cfg.lunar { "Style: Lunar" } else { "Style: Minimal" }, Action::Style),
            ("Reset", Action::Reset),
            ("Done", Action::Done),
        ];
        let (bw, gap) = (80.0, 5.0);
        let total = labels.len() as f32 * bw + (labels.len() - 1) as f32 * gap;
        let x = ((sw - total) / 2.0).floor();
        let y = (sh / 2.0 - 10.0).floor();
        labels
            .iter()
            .enumerate()
            .map(|(i, (l, a))| Button {
                rect: [x + i as f32 * (bw + gap), y, bw, 20.0],
                label: (*l).to_owned(),
                action: *a,
                on: *a == Action::Modules && self.panel,
            })
            .collect()
    }

    /// The module list: its panel and one toggle per module.
    pub fn cells(&self, cfg: &Config, sw: f32, sh: f32) -> ([f32; 4], Vec<Button>) {
        let (cols, cw, ch, gap) = (3usize, 122.0, 18.0, 4.0);
        let rows = COUNT.div_ceil(cols);
        let pw = cols as f32 * cw + (cols - 1) as f32 * gap + 16.0;
        let ph = rows as f32 * ch + (rows - 1) as f32 * gap + 34.0;
        let px = ((sw - pw) / 2.0).floor();
        let py = ((sh - ph) / 2.0).floor().max(4.0);
        let cells = (0..COUNT)
            .map(|i| Button {
                rect: [
                    px + 8.0 + (i % cols) as f32 * (cw + gap),
                    py + 24.0 + (i / cols) as f32 * (ch + gap),
                    cw,
                    ch,
                ],
                label: MODULES[i].1.to_owned(),
                action: Action::Toggle(i),
                on: cfg.slots[i].on,
            })
            .collect();
        ([px, py, pw, ph], cells)
    }

    fn at(&self, mouse: [f32; 2]) -> Option<(usize, [f32; 4])> {
        self.placed.iter().rev().find(|(_, r)| hit(*r, mouse)).copied()
    }

    fn act(&mut self, cfg: &mut Config, a: Action) {
        match a {
            Action::Modules => self.panel = !self.panel,
            Action::Style => cfg.lunar = !cfg.lunar,
            Action::Reset => *cfg = Config { lunar: cfg.lunar, ..Config::default() },
            Action::Done => self.close = true,
            Action::Toggle(i) => cfg.slots[i].on = !cfg.slots[i].on,
        }
        if a != Action::Modules && a != Action::Done {
            self.dirty = true;
        }
    }

    /// A mouse button went down at `mouse` (0 left, 1 right).
    pub fn press(&mut self, cfg: &mut Config, mouse: [f32; 2], button: i32, sw: f32, sh: f32) {
        if self.panel {
            let (_, cells) = self.cells(cfg, sw, sh);
            match cells.iter().find(|b| hit(b.rect, mouse)) {
                Some(b) => self.act(cfg, b.action),
                None => self.panel = false,
            }
            return;
        }
        if let Some(b) = self.buttons(cfg, sw, sh).into_iter().find(|b| hit(b.rect, mouse)) {
            self.act(cfg, b.action);
            return;
        }
        let Some((i, r)) = self.at(mouse) else { return };
        match button {
            1 => {
                cfg.slots[i].on = false;
                self.dirty = true;
            }
            0 => self.drag = Some((i, mouse[0] - r[0], mouse[1] - r[1])),
            _ => {}
        }
    }

    /// The pointer moved to `mouse` with a button held.
    pub fn moved(&mut self, cfg: &mut Config, mouse: [f32; 2], free: bool, sw: f32, sh: f32) {
        let Some((i, gx, gy)) = self.drag else { return };
        let Some(&(_, r)) = self.placed.iter().find(|(m, _)| *m == i) else { return };
        let moving = [mouse[0] - gx, mouse[1] - gy, r[2], r[3]];
        let [l, t] = if free {
            self.guides = [None; 2];
            [moving[0], moving[1]]
        } else {
            let others: Vec<[f32; 4]> = self.placed.iter().filter(|(m, _)| *m != i).map(|p| p.1).collect();
            let (at, guides) = snap(moving, &others, sw, sh, SNAP);
            self.guides = guides;
            at
        };
        let (anchor, x, y) = from_absolute(l, t, r[2], r[3], sw, sh);
        let s = &mut cfg.slots[i];
        (s.anchor, s.x, s.y) = (anchor, x, y);
        // Where it is now, until the next frame lays it out again.
        if let Some(p) = self.placed.iter_mut().find(|(m, _)| *m == i) {
            p.1 = place(s, r[2], r[3], sw, sh);
        }
    }

    pub fn release(&mut self) {
        if self.drag.take().is_some() {
            self.dirty = true;
        }
        self.guides = [None; 2];
    }

    /// The editor over a sw×sh screen: a dim background, the modules (with samples where
    /// there is nothing to show), their outlines, the guides, the buttons and the module list.
    pub fn draw(&mut self, cfg: &Config, placed: Vec<Placed>, mouse: [f32; 2], sw: f32, sh: f32, width: Width) -> Vec<Prim> {
        self.placed = placed.iter().map(|p| (p.module, p.rect)).collect();
        let mut out = Vec::new();
        fill(&mut out, 0.0, 0.0, sw, sh, 0x6000_0000);
        let guide = (ACCENT & 0xFF_FFFF) | 0xC000_0000;
        if let Some(x) = self.guides[0] {
            fill(&mut out, x.round(), 0.0, 1.0, sh, guide);
        }
        if let Some(y) = self.guides[1] {
            fill(&mut out, 0.0, y.round(), sw, 1.0, guide);
        }
        let hover = if self.panel { None } else { self.at(mouse).map(|h| h.0) };
        for p in placed {
            out.extend(p.prims);
            let active = Some(p.module) == hover || self.drag.is_some_and(|d| d.0 == p.module);
            outline(&mut out, p.rect, if active { ACCENT } else { 0x80FF_FFFF });
        }
        if let Some(h) = hover
            && self.drag.is_none()
            && let Some(&(_, r)) = self.placed.iter().find(|(m, _)| *m == h)
        {
            let tip = MODULES[h].1;
            let tw = width(tip) + 6.0;
            let tx = r[0].clamp(2.0, (sw - tw - 2.0).max(2.0));
            let ty = if r[1] >= 14.0 { r[1] - 13.0 } else { r[1] + r[3] + 2.0 };
            fill(&mut out, tx, ty, tw, 12.0, 0xE010_1014);
            out.push(Prim::Text { text: tip.to_owned(), at: [tx + 3.0, ty + 2.0], argb: WHITE, shadow: false, scale: 1.0 });
        }
        let hw = width(HINT);
        out.push(Prim::Text {
            text: HINT.to_owned(),
            at: [((sw - hw) / 2.0).floor(), (sh / 2.0 - 34.0).floor()],
            argb: 0xFFE0_E0E0,
            shadow: true,
            scale: 1.0,
        });
        for b in self.buttons(cfg, sw, sh) {
            button(&mut out, &b, !self.panel && hit(b.rect, mouse), width);
        }
        if self.panel {
            let ([px, py, pw, ph], cells) = self.cells(cfg, sw, sh);
            panel(&mut out, px, py, pw, ph, 4, 0xF010_1014);
            let title = "Modules \u{2014} click to switch on or off";
            out.push(Prim::Text { text: title.into(), at: [px + 8.0, py + 8.0], argb: WHITE, shadow: false, scale: 1.0 });
            for b in &cells {
                button(&mut out, b, hit(b.rect, mouse), width);
            }
        }
        out
    }
}

fn button(out: &mut Vec<Prim>, b: &Button, hover: bool, width: Width) {
    let bg = if b.on {
        with_opacity(ACCENT, if hover { 94 } else { 80 })
    } else if hover {
        0xE040_4048
    } else {
        0xD020_2026
    };
    let [x, y, w, h] = b.rect;
    panel(out, x, y, w, h, 3, bg);
    if hover {
        outline(out, b.rect, (ACCENT & 0xFF_FFFF) | 0x9000_0000);
    }
    let lw = width(&b.label);
    out.push(Prim::Text {
        text: b.label.clone(),
        at: [x + ((w - lw) / 2.0).floor(), y + ((h - 8.0) / 2.0).floor()],
        argb: WHITE,
        shadow: false,
        scale: 1.0,
    });
}

// ── what the HUD measures between frames ──

/// Hits in a row that the server confirmed, until this player is hurt or 3 seconds pass.
#[derive(Clone, Debug, Default)]
pub struct Combo {
    pending: Option<(i32, f64)>,
    count: u32,
    last: f64,
}

impl Combo {
    pub fn attacked(&mut self, id: i32, now: f64) {
        self.pending = Some((id, now));
    }
    /// The server reports entity `id` hurt (`own`: this player), `by_me` when it says who.
    pub fn hurt(&mut self, id: i32, own: bool, by_me: Option<bool>, now: f64) {
        if own {
            self.count = 0;
            self.pending = None;
            return;
        }
        if by_me == Some(false) {
            return;
        }
        if let Some((target, at)) = self.pending
            && target == id
            && now - at <= 0.5
        {
            self.count = if now - self.last > 3.0 { 1 } else { self.count + 1 };
            self.last = now;
            self.pending = None;
        }
    }
    pub fn count(&self, now: f64) -> u32 {
        if now - self.last > 3.0 { 0 } else { self.count }
    }
}

/// Horizontal speed from the player's position each tick, in blocks a second.
#[derive(Clone, Debug, Default)]
pub struct Speed {
    prev: Option<(f64, f64)>,
    pub value: f64,
}

impl Speed {
    pub fn tick(&mut self, pos: Option<(f64, f64)>) {
        let Some((x, z)) = pos else {
            *self = Self::default();
            return;
        };
        if let Some((px, pz)) = self.prev {
            let inst = ((x - px).powi(2) + (z - pz).powi(2)).sqrt() * 20.0;
            self.value = if inst > 100.0 { 0.0 } else { self.value * 0.5 + inst * 0.5 };
            if self.value < 0.005 {
                self.value = 0.0;
            }
        }
        self.prev = Some((x, z));
    }
}

/// Everything kept between hooks on the window's thread.
#[derive(Default)]
pub struct State {
    pub config: Option<Config>,
    /// The toggle key hid the HUD (until it is pressed again or the game restarts).
    pub hidden: bool,
    pub anim: Anim,
    pub editor: Editor,
    pub combo: Combo,
    pub speed: Speed,
    /// The last hit's reach and when (seconds).
    pub reach: Option<(f64, f64)>,
    /// CPS and keys, read where the game can be borrowed mutably.
    pub cps: [u32; 2],
    pub keys: [bool; 7],
    pub key_labels: [String; 4],
    /// The memory and clock readings, and when they were taken.
    pub memory: Option<((u64, u64), f64)>,
    pub clock: Option<(String, f64)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// The reach shown: the last hit's, for 3 seconds.
pub fn reach_shown(reach: Option<(f64, f64)>, now: f64) -> Option<f64> {
    reach.filter(|(_, at)| now - at <= 3.0).map(|r| r.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> f32 {
        s.chars().count() as f32 * 6.0
    }

    #[test]
    fn config_round_trips_and_defaults() {
        let mut c = Config::default();
        assert!(c.lunar);
        let on: Vec<&str> = (0..COUNT).filter(|i| c.slots[*i].on).map(|i| MODULES[i].0).collect();
        assert_eq!(on, ["fps", "cps", "coordinates", "keystrokes", "armor", "potions"]);
        c.lunar = false;
        c.slots[3] = Slot { on: false, anchor: 8, x: -12.5, y: -3.0 };
        assert_eq!(Config::parse(&c.serialize()), c);
    }

    #[test]
    fn config_ignores_bad_lines() {
        let c = Config::parse("style minimal\nfps 0 9 1 1\nnope 1 0 0 0\ncps 0 2 -4 x\nbiome 1 3 4 5\n");
        assert!(!c.lunar);
        assert!(c.slots[0].on, "anchor 9 is out of range");
        assert!(c.slots[1].on);
        assert_eq!(c.slots[5], Slot { on: true, anchor: 3, x: 4.0, y: 5.0 });
    }

    #[test]
    fn settings_page_toggles() {
        let mut c = Config::default();
        assert_eq!(c.labels().len(), COUNT + 2);
        c.click(0);
        assert!(!c.lunar);
        c.click(2);
        assert!(!c.slots[0].on);
        c.slots[4].x = 99.0;
        c.click(1);
        assert_eq!(c.slots, Config::default().slots);
        assert!(!c.lunar, "reset keeps the style");
        c.click(500);
    }

    #[test]
    fn placement_round_trips() {
        let (sw, sh) = (480.0, 270.0);
        for (left, top) in [(4.0, 4.0), (300.0, 120.0), (450.0, 250.0), (200.0, 10.0)] {
            let (anchor, x, y) = from_absolute(left, top, 30.0, 17.0, sw, sh);
            let r = place(&Slot { on: true, anchor, x, y }, 30.0, 17.0, sw, sh);
            assert!((r[0] - left).abs() <= 0.5 && (r[1] - top).abs() <= 0.5, "{r:?} {left} {top}");
        }
        assert_eq!(from_absolute(450.0, 250.0, 30.0, 17.0, sw, sh).0, BR);
        // Kept on screen after the window shrinks.
        let r = place(&Slot { on: true, anchor: TL, x: 900.0, y: 900.0 }, 30.0, 17.0, sw, sh);
        assert_eq!(r, [450.0, 253.0, 30.0, 17.0]);
    }

    #[test]
    fn snapping() {
        let (at, guides) = snap([2.0, 50.0, 20.0, 10.0], &[], 200.0, 100.0, 4.0);
        assert_eq!(at, [0.0, 50.0]);
        assert_eq!(guides, [Some(0.0), Some(50.0)]);
        let (at, _) = snap([60.0, 30.0, 20.0, 10.0], &[[83.0, 0.0, 10.0, 10.0]], 200.0, 100.0, 4.0);
        assert_eq!(at[0], 63.0);
    }

    #[test]
    fn panel_covers_shape_once() {
        for (w, h, r) in [(30, 17, 3), (10, 10, 6), (5, 3, 1), (1, 1, 0)] {
            for border in [false, true] {
                let mut seen = vec![0; (w * h) as usize];
                for (x0, y0, x1, y1, _) in panel_runs(w, h, r, border) {
                    for y in y0..y1 {
                        for x in x0..x1 {
                            seen[(y * w + x) as usize] += 1;
                        }
                    }
                }
                assert!(seen.iter().all(|n| *n <= 1), "{w}x{h} r{r}");
                // Every middle row is full width.
                let mid = (h / 2) as usize;
                assert!(seen[mid * w as usize..(mid + 1) * w as usize].iter().all(|n| *n == 1));
            }
        }
        // Rounded corners leave the corner pixel clear.
        let runs = panel_runs(30, 17, 3, true);
        assert!(!runs.iter().any(|&(x0, y0, x1, y1, _)| x0 <= 0 && 0 < x1 && y0 <= 0 && 0 < y1));
    }

    #[test]
    fn colours() {
        assert_eq!(border_for(background()), 0x7A47_4747);
        assert_eq!(with_opacity(ACCENT, 85) >> 24, 217);
        assert_eq!(durability_colour(0, 100), 0xFF00_FF00);
        assert_eq!(durability_colour(100, 100), 0xFFFF_0000);
        assert_eq!(lerp(0, 0xFFFF_FFFF, 0.5), 0x8080_8080);
        assert_eq!(fade(0xFF12_3456, 0.0), 0x0012_3456);
    }

    #[test]
    fn text_helpers() {
        assert_eq!(facing(0.0), "South (+Z)");
        assert_eq!(facing(180.0), "North (-Z)");
        assert_eq!(facing(-90.0), "East (+X)");
        assert_eq!(duration(65), "1:05");
        assert_eq!(duration(3723), "1:02:03");
        assert_eq!(duration(-1), "\u{221E}");
        assert_eq!(roman(4), "IV");
        assert_eq!(pretty_id("minecraft:dark_forest"), "Dark Forest");
        assert_eq!(signal_bars(42), 4);
        assert_eq!(signal_bars(-1), 0);
        assert_eq!(signal_bars(500), 1);
    }

    #[test]
    fn compass_points_the_way() {
        // Facing north (yaw 180) the needle runs up from the centre.
        let g = compass(180.0);
        assert_eq!(g[1][4], b'a');
        assert_eq!(g[4][4], b'#');
        let g = compass(0.0);
        assert_eq!(g[7][4], b'a');
    }

    #[test]
    fn default_frame_draws_the_default_modules() {
        let cfg = Config::default();
        let mut anim = Anim::default();
        let d = Data { fps: 144, pos: Some([1.5, 64.0, -3.2]), ..Data::default() };
        let placed = frame(&cfg, &d, &mut anim, 480.0, 270.0, false, 0.0, &w);
        let ids: Vec<&str> = placed.iter().map(|p| MODULES[p.module].0).collect();
        // No armour or effects to show: those stay out.
        assert_eq!(ids, ["fps", "cps", "coordinates", "keystrokes"]);
        let texts: Vec<&str> = placed[0]
            .prims
            .iter()
            .filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.as_str()) } else { None })
            .collect();
        assert_eq!(texts, ["144", " FPS"]);
        let coords = &placed[2].prims;
        assert!(coords.iter().any(|p| matches!(p, Prim::Text { text, .. } if text == "1 64 -4")));
        // Nothing overlaps in the default layout's left column.
        for pair in placed.windows(2) {
            assert!(pair[0].rect[1] + pair[0].rect[3] <= pair[1].rect[1], "{pair:?}");
        }
    }

    #[test]
    fn preview_shows_every_module() {
        let mut cfg = Config::default();
        for s in &mut cfg.slots {
            s.on = true;
        }
        let mut anim = Anim::default();
        let placed = frame(&cfg, &Data::default(), &mut anim, 480.0, 270.0, true, 0.0, &w);
        assert_eq!(placed.len(), COUNT);
        cfg.lunar = false;
        let placed = frame(&cfg, &Data::default(), &mut anim, 480.0, 270.0, true, 0.0, &w);
        assert_eq!(placed.len(), COUNT);
        let fps = &placed[0].prims;
        assert!(fps.iter().any(|p| matches!(p, Prim::Text { text, .. } if text == "FPS: 0")));
    }

    #[test]
    fn minimal_has_no_icons_lunar_does() {
        let mut cfg = Config::default();
        let mut anim = Anim::default();
        let d = Data::default();
        let lunar = frame(&cfg, &d, &mut anim, 480.0, 270.0, false, 0.0, &w);
        cfg.lunar = false;
        let minimal = frame(&cfg, &d, &mut anim, 480.0, 270.0, false, 0.0, &w);
        let fills = |p: &Placed| p.prims.iter().filter(|p| matches!(p, Prim::Fill { .. })).count();
        assert!(fills(&lunar[0]) > fills(&minimal[0]) + 10);
        assert_eq!(fills(&minimal[0]), 1);
    }

    #[test]
    fn armour_and_effects() {
        let mut d = Data::default();
        d.armor[1] = Some(Piece { damage: 10, max: 528 });
        d.effects.push(Effect { name: "Speed II".into(), secs: 95, sprite: "mob_effect/speed".into() });
        let mut anim = Anim::default();
        let placed = frame(&Config::default(), &d, &mut anim, 480.0, 270.0, false, 0.0, &w);
        let armour = placed.iter().find(|p| MODULES[p.module].0 == "armor").unwrap();
        assert!(armour.prims.iter().any(|p| matches!(p, Prim::Item { item: ItemRef::Armor(1), .. })));
        assert!(armour.prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text == "518")));
        let effects = placed.iter().find(|p| MODULES[p.module].0 == "potions").unwrap();
        assert!(effects.prims.iter().any(|p| matches!(p, Prim::Sprite { name, .. } if name == "mob_effect/speed")));
        assert!(effects.prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text == "1:35")));
        // Top right, 4 pixels in.
        assert_eq!(effects.rect[0] + effects.rect[2], 476.0);
    }

    #[test]
    fn keystroke_glow_rises_and_falls() {
        let mut a = Anim::default();
        let mut keys = [false; 7];
        keys[0] = true;
        a.step(keys, 0.0);
        a.step(keys, 0.05);
        assert_eq!(a.glow[0], 1.0);
        a.step([false; 7], 0.15);
        assert!(a.glow[0] > 0.0 && a.glow[0] < 1.0);
        a.step([false; 7], 0.5);
        assert_eq!(a.glow[0], 0.0);
    }

    fn editor_with_frame(cfg: &Config, e: &mut Editor) {
        let mut anim = Anim::default();
        let placed = frame(cfg, &Data::default(), &mut anim, 480.0, 270.0, true, 0.0, &w);
        e.draw(cfg, placed, [0.0, 0.0], 480.0, 270.0, &w);
    }

    #[test]
    fn editor_drags_a_module() {
        let mut cfg = Config::default();
        let mut e = Editor::default();
        editor_with_frame(&cfg, &mut e);
        // Grab FPS (top left, 4,4) and drop it near the bottom right.
        e.press(&mut cfg, [8.0, 8.0], 0, 480.0, 270.0);
        e.moved(&mut cfg, [404.0, 204.0], true, 480.0, 270.0);
        e.release();
        assert!(e.dirty);
        assert_eq!(cfg.slots[0].anchor, BR);
        editor_with_frame(&cfg, &mut e);
        let r = e.placed.iter().find(|p| p.0 == 0).unwrap().1;
        assert_eq!([r[0], r[1]], [400.0, 200.0]);
    }

    #[test]
    fn editor_buttons_and_list() {
        let mut cfg = Config::default();
        let mut e = Editor::default();
        editor_with_frame(&cfg, &mut e);
        let b = e.buttons(&cfg, 480.0, 270.0);
        let centre = |r: [f32; 4]| [r[0] + r[2] / 2.0, r[1] + r[3] / 2.0];
        e.press(&mut cfg, centre(b[1].rect), 0, 480.0, 270.0);
        assert!(!cfg.lunar);
        e.press(&mut cfg, centre(b[0].rect), 0, 480.0, 270.0);
        assert!(e.panel);
        let (_, cells) = e.cells(&cfg, 480.0, 270.0);
        e.press(&mut cfg, centre(cells[4].rect), 0, 480.0, 270.0);
        assert!(cfg.slots[4].on, "Direction switched on");
        e.press(&mut cfg, [1.0, 1.0], 0, 480.0, 270.0);
        assert!(!e.panel, "a click outside closes the list");
        e.press(&mut cfg, centre(b[3].rect), 0, 480.0, 270.0);
        assert!(e.close);
    }

    #[test]
    fn editor_right_click_hides() {
        let mut cfg = Config::default();
        let mut e = Editor::default();
        editor_with_frame(&cfg, &mut e);
        e.press(&mut cfg, [8.0, 8.0], 1, 480.0, 270.0);
        assert!(!cfg.slots[0].on);
    }

    #[test]
    fn combo_counts_confirmed_hits() {
        let mut c = Combo::default();
        c.attacked(7, 10.0);
        c.hurt(7, false, Some(true), 10.1);
        c.attacked(7, 10.6);
        c.hurt(7, false, None, 10.7);
        assert_eq!(c.count(10.8), 2);
        c.attacked(7, 11.0);
        c.hurt(8, false, None, 11.1);
        assert_eq!(c.count(11.2), 2, "another entity's hurt is not a hit");
        c.hurt(1, true, None, 11.3);
        assert_eq!(c.count(11.4), 0);
        c.attacked(7, 20.0);
        c.hurt(7, false, Some(true), 20.1);
        assert_eq!(c.count(24.0), 0, "3 seconds end it");
    }

    #[test]
    fn speed_smooths() {
        let mut s = Speed::default();
        s.tick(Some((0.0, 0.0)));
        s.tick(Some((0.2806, 0.0)));
        assert!((s.value - 2.806).abs() < 1e-9);
        s.tick(Some((1000.0, 0.0)));
        assert_eq!(s.value, 0.0, "a teleport is not speed");
        s.tick(None);
        assert_eq!(s.value, 0.0);
    }

    #[test]
    fn reach_holds_three_seconds() {
        assert_eq!(reach_shown(Some((3.1, 5.0)), 7.0), Some(3.1));
        assert_eq!(reach_shown(Some((3.1, 5.0)), 8.5), None);
    }
}
