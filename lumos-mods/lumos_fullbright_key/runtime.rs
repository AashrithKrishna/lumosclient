// Lumos Fullbright Key: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_fullbright_key`; `glue.rs` reads
// the game and calls in here. It names nothing outside `std`, so it also compiles alone:
// `rustc --edition 2024 --test runtime.rs` runs the tests below.
//
// The mod keeps no Fullbright of its own: the key flips Horizon's (Options > Video Settings >
// Quality, `fullbright` in horizon-extras.txt), so that row and the key always agree.
use std::cell::RefCell;

pub const ID: &str = "lumos_fullbright_key";
pub const NAME: &str = "Fullbright Key";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "A key (G unless you change it) that turns Horizon's Fullbright \
on and off, with a short notice. Not on other people's servers, where Horizon holds it off.";

pub const GREEN: u32 = 0x55FF55;
pub const GREY: u32 = 0xAAAAAA;
pub const RED: u32 = 0xFF5555;

/// What the key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    /// Horizon holds Fullbright off here (another's server): nothing changes.
    Held,
    /// Fullbright becomes this.
    Set(bool),
}

/// The key pressed with Fullbright `on` and Horizon holding it off or not.
pub fn toggle(held: bool, on: bool) -> Toggle {
    if held { Toggle::Held } else { Toggle::Set(!on) }
}

/// The notice for what the key did, and its colour.
pub fn notice(t: Toggle) -> (&'static str, u32) {
    match t {
        Toggle::Held => ("Fullbright isn't allowed here", RED),
        Toggle::Set(true) => ("Fullbright: On", GREEN),
        Toggle::Set(false) => ("Fullbright: Off", GREY),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The notice over the hotbar after the key.
    pub notify: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { notify: true }
    }
}

impl Settings {
    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        Self {
            notify: get("notify").map_or(d.notify, |v| v.trim() == "true"),
        }
    }

    /// The page's buttons after the key row: Fullbright now (Horizon's own setting), then ours.
    pub fn labels(&self, fullbright: bool, held: bool) -> Vec<String> {
        let on = |b: bool| if b { "ON" } else { "OFF" };
        let now = if held {
            format!("Fullbright: {} (held off on this server)", on(fullbright))
        } else {
            format!("Fullbright: {}", on(fullbright))
        };
        vec![now, format!("Notice on toggle: {}", on(self.notify))]
    }

    pub fn stored(&self) -> [(&'static str, String); 1] {
        [("notify", self.notify.to_string())]
    }
}

/// Page buttons.
pub const BUTTON_FULLBRIGHT: usize = 0;
pub const BUTTON_NOTICE: usize = 1;

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
    fn the_key_flips_unless_held() {
        assert_eq!(toggle(false, false), Toggle::Set(true));
        assert_eq!(toggle(false, true), Toggle::Set(false));
        assert_eq!(toggle(true, true), Toggle::Held);
        assert_eq!(toggle(true, false), Toggle::Held);
    }

    #[test]
    fn notices() {
        assert_eq!(notice(Toggle::Set(true)).0, "Fullbright: On");
        assert_eq!(notice(Toggle::Set(false)).0, "Fullbright: Off");
        assert_eq!(notice(Toggle::Held), ("Fullbright isn't allowed here", RED));
    }

    #[test]
    fn settings_round_trip() {
        assert_eq!(Settings::read(|_| None), Settings::default());
        let s = Settings { notify: false };
        let stored = s.stored();
        let read = Settings::read(|k| stored.iter().find(|p| p.0 == k).map(|p| p.1.clone()));
        assert_eq!(read, s);
        assert_eq!(s.labels(true, false), vec!["Fullbright: ON", "Notice on toggle: OFF"]);
        assert!(s.labels(true, true)[0].contains("held off"));
    }
}
