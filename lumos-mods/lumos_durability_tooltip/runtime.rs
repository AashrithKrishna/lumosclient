// Lumos Durability Tooltip: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_durability_tooltip`; `glue.rs`
// reads the game and calls in here. It names nothing outside `std`, so it also compiles alone:
// `rustc --edition 2024 --test runtime.rs` runs the tests below.
use std::cell::RefCell;

pub const ID: &str = "lumos_durability_tooltip";
pub const NAME: &str = "Durability Tooltip";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "Every damageable item's tooltip shows \"Durability: 123 / 250\", \
coloured from green to red by what is left.";

/// `item.durability` in English, for a language that lacks the key.
pub const FALLBACK: &str = "Durability: %s / %s";

pub const STYLES: [&str; 3] = ["123 / 250", "123 / 250 (49%)", "49%"];
pub const COLOURS: [&str; 3] = ["Green to red", "Three steps", "Plain grey"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Index in `STYLES`.
    pub style: usize,
    /// Index in `COLOURS`.
    pub colour: usize,
    /// Also on an item that has taken no damage yet.
    pub show_full: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            style: 0,
            colour: 0,
            show_full: true,
        }
    }
}

impl Settings {
    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let index = |key: &str, len: usize| {
            get(key)
                .and_then(|v| v.trim().parse::<usize>().ok())
                .filter(|v| *v < len)
                .unwrap_or(0)
        };
        Self {
            style: index("style", STYLES.len()),
            colour: index("colour", COLOURS.len()),
            show_full: get("show_full").is_none_or(|v| v.trim() != "false"),
        }
    }

    pub fn labels(&self) -> Vec<String> {
        vec![
            format!("Format: {}", STYLES[self.style]),
            format!("Colour: {}", COLOURS[self.colour]),
            format!(
                "Undamaged items: {}",
                if self.show_full { "Shown" } else { "Hidden" }
            ),
        ]
    }

    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.style = (self.style + 1) % STYLES.len(),
            1 => self.colour = (self.colour + 1) % COLOURS.len(),
            2 => self.show_full = !self.show_full,
            _ => {}
        }
    }

    pub fn stored(&self) -> [(&'static str, String); 3] {
        [
            ("style", self.style.to_string()),
            ("colour", self.colour.to_string()),
            ("show_full", self.show_full.to_string()),
        ]
    }
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

/// The settings, read from the mod's settings file once and kept.
pub fn settings(get: impl Fn(&str) -> Option<String>) -> Settings {
    SETTINGS.with_borrow_mut(|s| *s.get_or_insert_with(|| Settings::read(get)))
}

pub fn set_settings(new: Settings) {
    SETTINGS.with_borrow_mut(|s| *s = Some(new));
}

/// A stack's durability: what is left of `max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Durability {
    pub remaining: u32,
    pub max: u32,
}

impl Durability {
    /// None for an item that can't be damaged (`max` 0: not damageable, or unbreakable).
    pub fn of(damage: u32, max: u32) -> Option<Self> {
        (max > 0).then(|| Self {
            remaining: max.saturating_sub(damage),
            max,
        })
    }

    pub fn full(&self) -> bool {
        self.remaining >= self.max
    }

    /// What is left, 0.0 to 1.0.
    pub fn ratio(&self) -> f32 {
        (self.remaining as f32 / self.max as f32).clamp(0.0, 1.0)
    }

    /// Whole percent left, rounded down except that a scratched item never reads 100%
    /// and a working one never reads 0%.
    pub fn percent(&self) -> u32 {
        let p = (u64::from(self.remaining) * 100 / u64::from(self.max)) as u32;
        if self.remaining == 0 {
            0
        } else if self.full() {
            100
        } else {
            p.clamp(1, 99)
        }
    }

    /// The text after "Durability: " for `style` (an index in `STYLES`), or None when
    /// the line is the translated `item.durability` with the two numbers alone.
    pub fn suffix(&self, style: usize) -> Option<String> {
        match style {
            1 => Some(format!(" ({}%)", self.percent())),
            2 => Some(format!("{}%", self.percent())),
            _ => None,
        }
    }

    /// The line's colour as 0xRRGGBB for `colour` (an index in `COLOURS`).
    pub fn colour(&self, colour: usize) -> u32 {
        let ratio = self.ratio();
        match colour {
            // Three steps, as the hotbar's chat colours: green, gold, red.
            1 => {
                if ratio > 0.5 {
                    0x55FF55
                } else if ratio > 0.2 {
                    0xFFAA00
                } else {
                    0xFF5555
                }
            }
            2 => 0xAAAAAA,
            // The item bar's hue (green at full, through yellow, red when broken), a little
            // lighter so it reads on the tooltip's dark box.
            _ => lighten(hsv_to_rgb(ratio / 3.0, 1.0, 1.0), 0.25),
        }
    }
}

/// Hue 0..1, saturation and value 0..1, to 0xRRGGBB.
pub fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> u32 {
    let h = (hue.rem_euclid(1.0) * 6.0).min(5.999_999);
    let sector = h.floor() as u32;
    let f = h - sector as f32;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - f * saturation);
    let t = value * (1.0 - (1.0 - f) * saturation);
    let (r, g, b) = match sector {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

/// Moves each channel `amount` (0..1) of the way to white.
pub fn lighten(rgb: u32, amount: f32) -> u32 {
    let mix = |shift: u32| {
        let c = (rgb >> shift & 0xFF) as f32;
        ((c + (255.0 - c) * amount).round() as u32).min(255) << shift
    };
    mix(16) | mix(8) | mix(0)
}

/// Where the line goes among `count` lines whose plain texts `plain(i)` gives: before the
/// advanced tooltip's item id (`minecraft:diamond_sword`) when it is there, else last.
pub fn insert_at(count: usize, item_id: &str, plain: impl Fn(usize) -> String) -> usize {
    (1..count).find(|&i| plain(i) == item_id).unwrap_or(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |key| pairs.iter().find(|p| p.0 == key).map(|p| p.1.to_owned())
    }

    #[test]
    fn only_damageable_items_have_durability() {
        assert_eq!(Durability::of(0, 0), None);
        assert_eq!(Durability::of(5, 0), None);
        let d = Durability::of(127, 250).unwrap();
        assert_eq!((d.remaining, d.max), (123, 250));
        // Damage past the maximum (a server's odd stack) reads 0, not a wrapped number.
        assert_eq!(Durability::of(400, 250).unwrap().remaining, 0);
    }

    #[test]
    fn percent_never_hides_a_scratch_or_a_last_use() {
        assert_eq!(Durability::of(0, 250).unwrap().percent(), 100);
        assert_eq!(Durability::of(1, 1561).unwrap().percent(), 99);
        assert_eq!(Durability::of(1560, 1561).unwrap().percent(), 1);
        assert_eq!(Durability::of(1561, 1561).unwrap().percent(), 0);
        assert_eq!(Durability::of(127, 250).unwrap().percent(), 49);
    }

    #[test]
    fn suffixes_follow_the_style() {
        let d = Durability::of(127, 250).unwrap();
        assert_eq!(d.suffix(0), None);
        assert_eq!(d.suffix(1).as_deref(), Some(" (49%)"));
        assert_eq!(d.suffix(2).as_deref(), Some("49%"));
    }

    #[test]
    fn colour_goes_from_green_to_red() {
        let full = Durability::of(0, 100).unwrap().colour(0);
        let half = Durability::of(50, 100).unwrap().colour(0);
        let broken = Durability::of(100, 100).unwrap().colour(0);
        assert_eq!(full, lighten(0x00FF00, 0.25));
        // Half way is yellow: red and green both full.
        assert_eq!(half >> 16 & 0xFF, 0xFF);
        assert_eq!(half >> 8 & 0xFF, 0xFF);
        assert_eq!(broken, lighten(0xFF0000, 0.25));
        assert_eq!(Durability::of(10, 100).unwrap().colour(1), 0x55FF55);
        assert_eq!(Durability::of(60, 100).unwrap().colour(1), 0xFFAA00);
        assert_eq!(Durability::of(90, 100).unwrap().colour(1), 0xFF5555);
        assert_eq!(Durability::of(90, 100).unwrap().colour(2), 0xAAAAAA);
    }

    #[test]
    fn hsv_corners() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), 0xFF0000);
        assert_eq!(hsv_to_rgb(1.0 / 3.0, 1.0, 1.0), 0x00FF00);
        assert_eq!(hsv_to_rgb(2.0 / 3.0, 1.0, 1.0), 0x0000FF);
        assert_eq!(hsv_to_rgb(0.5, 0.0, 1.0), 0xFFFFFF);
    }

    #[test]
    fn line_goes_before_the_item_id() {
        let lines = ["Diamond Sword", "", "When in Main Hand:", "minecraft:diamond_sword"];
        let at = insert_at(lines.len(), "minecraft:diamond_sword", |i| lines[i].to_owned());
        assert_eq!(at, 3);
        let short = ["Diamond Sword", "When in Main Hand:"];
        assert_eq!(
            insert_at(short.len(), "minecraft:diamond_sword", |i| short[i].to_owned()),
            2
        );
    }

    #[test]
    fn settings_round_trip_and_cycle() {
        assert_eq!(Settings::read(stored(&[])), Settings::default());
        let s = Settings::read(stored(&[("style", "2"), ("colour", "1"), ("show_full", "false")]));
        assert_eq!(
            s,
            Settings {
                style: 2,
                colour: 1,
                show_full: false
            }
        );
        let pairs = s.stored();
        let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(Settings::read(stored(&refs)), s);
        // Out-of-range values fall back to the default.
        assert_eq!(Settings::read(stored(&[("style", "9")])).style, 0);
        let mut c = Settings::default();
        c.click(0);
        c.click(1);
        c.click(2);
        assert_eq!((c.style, c.colour, c.show_full), (1, 1, false));
        assert_eq!(c.labels().len(), 3);
    }
}
