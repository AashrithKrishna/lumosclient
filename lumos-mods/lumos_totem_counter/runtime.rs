// Lumos Totem Counter: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_totem_counter`; `glue.rs` reads
// the game and calls in here. It names nothing outside `std`, so it also compiles alone:
// `rustc --edition 2024 --test runtime.rs` runs the tests below.
use std::cell::RefCell;
use std::time::{Duration, Instant};

pub const ID: &str = "lumos_totem_counter";
pub const NAME: &str = "Totem Counter";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "A small HUD counter of the Totems of Undying you carry, \
coloured as it runs low, and a notice each time one of yours pops.";

pub const TOTEM: &str = "minecraft:totem_of_undying";

pub const POSITIONS: [&str; 7] = [
    "Right of hotbar",
    "Left of hotbar",
    "Above hotbar",
    "Top left",
    "Top right",
    "Bottom left",
    "Bottom right",
];
pub const WARN_AT: [u32; 4] = [0, 1, 2, 3];

/// How long the counter flashes after the count drops.
pub const FLASH: Duration = Duration::from_millis(2500);
/// How long after a pop the notice waits, so the used totem has left the inventory.
pub const NOTICE_DELAY: Duration = Duration::from_millis(150);

pub const WHITE: u32 = 0xFFFFFF;
pub const GOLD: u32 = 0xFFAA00;
pub const RED: u32 = 0xFF5555;
pub const YELLOW: u32 = 0xFFFF55;
pub const BACKGROUND: u32 = 0x6000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Index in `POSITIONS`.
    pub position: usize,
    /// The count at or below which the number turns gold (0: never).
    pub warn_at: u32,
    /// Keep the counter up (greyed) when you carry none.
    pub show_zero: bool,
    /// A notice over the hotbar when one of your totems pops.
    pub notify: bool,
    /// A dark box behind the counter.
    pub background: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            position: 0,
            warn_at: 1,
            show_zero: false,
            notify: true,
            background: false,
        }
    }
}

impl Settings {
    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let number = |key: &str| get(key).and_then(|v| v.trim().parse::<usize>().ok());
        let flag = |key: &str, default: bool| get(key).map_or(default, |v| v.trim() == "true");
        let d = Self::default();
        Self {
            position: number("position")
                .filter(|p| *p < POSITIONS.len())
                .unwrap_or(d.position),
            warn_at: number("warn_at")
                .map(|w| w as u32)
                .filter(|w| WARN_AT.contains(w))
                .unwrap_or(d.warn_at),
            show_zero: flag("show_zero", d.show_zero),
            notify: flag("notify", d.notify),
            background: flag("background", d.background),
        }
    }

    pub fn labels(&self) -> Vec<String> {
        let on = |b: bool| if b { "ON" } else { "OFF" };
        vec![
            format!("Position: {}", POSITIONS[self.position]),
            if self.warn_at == 0 {
                "Low warning: OFF".to_owned()
            } else {
                format!("Low warning: at {} or fewer", self.warn_at)
            },
            format!("Show with no totems: {}", on(self.show_zero)),
            format!("Pop notice: {}", on(self.notify)),
            format!("Background: {}", on(self.background)),
        ]
    }

    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.position = (self.position + 1) % POSITIONS.len(),
            1 => {
                let i = WARN_AT.iter().position(|w| *w == self.warn_at).unwrap_or(0);
                self.warn_at = WARN_AT[(i + 1) % WARN_AT.len()];
            }
            2 => self.show_zero = !self.show_zero,
            3 => self.notify = !self.notify,
            4 => self.background = !self.background,
            _ => {}
        }
    }

    pub fn stored(&self) -> [(&'static str, String); 5] {
        [
            ("position", self.position.to_string()),
            ("warn_at", self.warn_at.to_string()),
            ("show_zero", self.show_zero.to_string()),
            ("notify", self.notify.to_string()),
            ("background", self.background.to_string()),
        ]
    }
}

/// What the counter remembers between frames.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    /// The count last frame; None before the first frame in a world.
    last: Option<u32>,
    /// When the count last went down.
    dropped_at: Option<Instant>,
    /// A pop heard, waiting for its notice.
    pop_at: Option<Instant>,
    /// Your pops since the game started.
    pub pops: u32,
}

impl Tracker {
    /// One of your totems popped (entity event 35 on you).
    pub fn popped(&mut self, now: Instant) {
        self.pops = self.pops.saturating_add(1);
        self.pop_at = Some(now);
        self.dropped_at = Some(now);
    }

    /// A frame carrying `count` totems. Returns the notice to show now, if any:
    /// (pops so far, totems left).
    pub fn frame(&mut self, count: u32, now: Instant) -> Option<(u32, u32)> {
        if let Some(last) = self.last
            && count < last
        {
            self.dropped_at = Some(now);
        }
        self.last = Some(count);
        let due = self
            .pop_at
            .is_some_and(|at| now.saturating_duration_since(at) >= NOTICE_DELAY);
        if due {
            self.pop_at = None;
            return Some((self.pops, count));
        }
        None
    }

    /// Leaving a world: the next one starts without a flash.
    pub fn reset_count(&mut self) {
        self.last = None;
        self.dropped_at = None;
        self.pop_at = None;
    }

    /// Whether the counter flashes now (it blinks four times a second while it does).
    pub fn flashing(&self, now: Instant) -> bool {
        self.dropped_at
            .is_some_and(|at| now.saturating_duration_since(at) < FLASH)
    }

    pub fn blink_on(&self, now: Instant) -> bool {
        self.dropped_at.is_some_and(|at| {
            (now.saturating_duration_since(at).as_millis() / 125) % 2 == 0
        })
    }
}

/// The number's colour for `count` with `warn_at`, flashing or not.
pub fn count_colour(count: u32, warn_at: u32, flashing: bool, blink_on: bool) -> u32 {
    if flashing {
        return if blink_on { RED } else { YELLOW };
    }
    if count == 0 {
        0xAAAAAA
    } else if count <= warn_at {
        GOLD
    } else {
        WHITE
    }
}

/// The notice's text.
pub fn notice(pops: u32, left: u32) -> String {
    let left = match left {
        0 => "no totems left!".to_owned(),
        1 => "1 totem left".to_owned(),
        n => format!("{n} totems left"),
    };
    format!("Totem popped (#{pops}): {left}")
}

/// Where the counter's box of `w` x `h` goes for `position` 0..=2 (the hotbar ones), in a
/// GUI of `width` x `height`, with the off hand slot on the `offhand_left`/`offhand_right` side.
/// Corners (3 and up) are placed by the HUD's corner stacking instead and give None.
pub fn hotbar_place(
    position: usize,
    width: f32,
    height: f32,
    w: f32,
    h: f32,
    offhand_left: bool,
    offhand_right: bool,
) -> Option<(f32, f32)> {
    let center = (width / 2.0).floor();
    let bottom = height - h - 2.0;
    match position {
        0 => {
            let edge = center + 91.0 + if offhand_right { 29.0 } else { 0.0 } + 4.0;
            Some((edge.min(width - w - 2.0), bottom))
        }
        1 => {
            let edge = center - 91.0 - if offhand_left { 29.0 } else { 0.0 } - 4.0;
            Some(((edge - w).max(2.0), bottom))
        }
        // Over the hotbar, above the health, armour, food and air rows (vanilla's highest
        // stack is about 49 pixels over the bottom).
        2 => Some(((center - w / 2.0).floor(), (height - 59.0 - h).max(2.0))),
        _ => None,
    }
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
    static TRACKER: RefCell<Tracker> = RefCell::new(Tracker::default());
}

pub fn settings(get: impl Fn(&str) -> Option<String>) -> Settings {
    SETTINGS.with_borrow_mut(|s| *s.get_or_insert_with(|| Settings::read(get)))
}

pub fn set_settings(new: Settings) {
    SETTINGS.with_borrow_mut(|s| *s = Some(new));
}

pub fn with_tracker<R>(f: impl FnOnce(&mut Tracker) -> R) -> R {
    TRACKER.with_borrow_mut(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |key| pairs.iter().find(|p| p.0 == key).map(|p| p.1.to_owned())
    }

    #[test]
    fn settings_round_trip() {
        assert_eq!(Settings::read(stored(&[])), Settings::default());
        let mut s = Settings::default();
        for b in 0..5 {
            s.click(b);
        }
        assert_eq!(
            s,
            Settings {
                position: 1,
                warn_at: 2,
                show_zero: true,
                notify: false,
                background: true
            }
        );
        let pairs = s.stored();
        let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(Settings::read(stored(&refs)), s);
        assert_eq!(Settings::read(stored(&[("warn_at", "7")])).warn_at, 1);
        assert_eq!(Settings::read(stored(&[("position", "99")])).position, 0);
    }

    #[test]
    fn warning_cycles_through_off() {
        let mut s = Settings::default();
        let mut seen = Vec::new();
        for _ in 0..4 {
            s.click(1);
            seen.push(s.warn_at);
        }
        assert_eq!(seen, vec![2, 3, 0, 1]);
        s.warn_at = 0;
        assert!(s.labels()[1].ends_with("OFF"));
    }

    #[test]
    fn a_drop_flashes_and_a_pop_gives_one_notice() {
        let t0 = Instant::now();
        let mut t = Tracker::default();
        assert_eq!(t.frame(3, t0), None);
        assert!(!t.flashing(t0));
        // Picking one up does not flash.
        assert_eq!(t.frame(4, t0), None);
        assert!(!t.flashing(t0));
        // Dropping one does.
        assert_eq!(t.frame(3, t0), None);
        assert!(t.flashing(t0));
        assert!(!t.flashing(t0 + FLASH));
        // A pop: the notice waits for the inventory to catch up, then comes once.
        t.popped(t0);
        assert_eq!(t.frame(3, t0 + Duration::from_millis(10)), None);
        assert_eq!(t.frame(2, t0 + NOTICE_DELAY), Some((1, 2)));
        assert_eq!(t.frame(2, t0 + NOTICE_DELAY * 2), None);
        t.popped(t0);
        assert_eq!(t.frame(1, t0 + NOTICE_DELAY), Some((2, 1)));
        // A new world starts calm.
        t.reset_count();
        assert_eq!(t.frame(5, t0), None);
        assert!(!t.flashing(t0));
    }

    #[test]
    fn colours() {
        assert_eq!(count_colour(5, 1, false, false), WHITE);
        assert_eq!(count_colour(1, 1, false, false), GOLD);
        assert_eq!(count_colour(2, 0, false, false), WHITE);
        assert_eq!(count_colour(0, 1, false, false), 0xAAAAAA);
        assert_eq!(count_colour(5, 1, true, true), RED);
        assert_eq!(count_colour(5, 1, true, false), YELLOW);
    }

    #[test]
    fn notices() {
        assert_eq!(notice(1, 2), "Totem popped (#1): 2 totems left");
        assert_eq!(notice(3, 1), "Totem popped (#3): 1 totem left");
        assert_eq!(notice(4, 0), "Totem popped (#4): no totems left!");
    }

    #[test]
    fn hotbar_places_clear_the_hotbar_and_offhand() {
        // 427 x 240 GUI: the hotbar spans 122..304.
        let (x, y) = hotbar_place(0, 427.0, 240.0, 30.0, 18.0, false, false).unwrap();
        assert!(x >= 304.0 && y + 18.0 <= 240.0);
        let (x2, _) = hotbar_place(0, 427.0, 240.0, 30.0, 18.0, false, true).unwrap();
        assert!(x2 >= 304.0 + 29.0);
        let (x3, _) = hotbar_place(1, 427.0, 240.0, 30.0, 18.0, true, false).unwrap();
        assert!(x3 + 30.0 <= 122.0 - 29.0);
        // A tiny window keeps the box on screen.
        let (x4, _) = hotbar_place(0, 200.0, 120.0, 30.0, 18.0, false, true).unwrap();
        assert!(x4 + 30.0 <= 200.0);
        assert_eq!(hotbar_place(4, 427.0, 240.0, 30.0, 18.0, false, false), None);
    }
}
