// Lumos Freelook: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_freelook`; `glue.rs` reads the
// game and calls in here. It names nothing outside `std`, so it also compiles alone:
// `rustc --edition 2024 --test runtime.rs` runs the tests below.
//
// While the key is held (or toggled on) the mouse turns the camera only: the player keeps the
// yaw and pitch it had, so it walks, aims and faces where it did. Letting go brings the camera
// back to the player's view, at once or eased over a moment.
use std::cell::RefCell;
use std::time::Duration;

pub const ID: &str = "lumos_freelook";
pub const NAME: &str = "Freelook";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "Hold a key (Left Alt unless you change it) to look around \
while you keep facing and moving the way you were. Letting go turns the camera back.";

/// Vanilla's third-person distance (`minecraft:camera_distance`'s default).
pub const THIRD_PERSON_DISTANCE: f32 = 4.0;
/// `Entity.turn`: a mouse count is 0.15 degrees.
pub const TURN_SCALE: f64 = 0.15;
/// How fast the eased return closes the gap: the part left after one second is e^-RETURN_RATE.
pub const RETURN_RATE: f32 = 14.0;
/// Close enough to call the camera back (degrees).
pub const RETURN_DONE: f32 = 0.3;
/// The longest frame the easing counts, so a stall doesn't jump.
pub const MAX_STEP: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Hold the key (true) or press it once to start and again to stop.
    pub hold: bool,
    /// Turn the up/down of the free camera the other way.
    pub invert: bool,
    /// Ease the camera back instead of snapping it.
    pub smooth: bool,
    /// Works when started from first person.
    pub first_person: bool,
    /// Works when started from third person (either side).
    pub third_person: bool,
    /// Started from first person: show third person (from behind) while it is on.
    pub switch_view: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hold: true,
            invert: false,
            smooth: true,
            first_person: true,
            third_person: true,
            switch_view: true,
        }
    }
}

impl Settings {
    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        let flag = |key: &str, default: bool| get(key).map_or(default, |v| v.trim() == "true");
        Self {
            hold: flag("hold", d.hold),
            invert: flag("invert", d.invert),
            smooth: flag("smooth", d.smooth),
            first_person: flag("first_person", d.first_person),
            third_person: flag("third_person", d.third_person),
            switch_view: flag("switch_view", d.switch_view),
        }
    }

    pub fn labels(&self) -> Vec<String> {
        let on = |b: bool| if b { "ON" } else { "OFF" };
        vec![
            format!("Key: {}", if self.hold { "Hold" } else { "Toggle" }),
            format!("Invert up/down: {}", on(self.invert)),
            format!("Smooth return: {}", on(self.smooth)),
            format!("Use in first person: {}", on(self.first_person)),
            format!("Use in third person: {}", on(self.third_person)),
            format!("Third person while looking: {}", on(self.switch_view)),
        ]
    }

    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.hold = !self.hold,
            1 => self.invert = !self.invert,
            2 => self.smooth = !self.smooth,
            3 => self.first_person = !self.first_person,
            4 => self.third_person = !self.third_person,
            5 => self.switch_view = !self.switch_view,
            _ => {}
        }
    }

    pub fn stored(&self) -> [(&'static str, String); 6] {
        [
            ("hold", self.hold.to_string()),
            ("invert", self.invert.to_string()),
            ("smooth", self.smooth.to_string()),
            ("first_person", self.first_person.to_string()),
            ("third_person", self.third_person.to_string()),
            ("switch_view", self.switch_view.to_string()),
        ]
    }

    /// Whether freelook may start from the view the player is in.
    pub fn allowed(&self, first_person: bool) -> bool {
        if first_person { self.first_person } else { self.third_person }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Phase {
    #[default]
    Off,
    /// The mouse turns the camera.
    Looking,
    /// Easing back to the player's view.
    Returning,
}

/// The free camera.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Look {
    pub phase: Phase,
    pub yaw: f32,
    pub pitch: f32,
    /// It switched first person to third person, to switch back at the end.
    pub switched: bool,
    /// Back at the player's view: the glue ends it (and switches the view back).
    pub done: bool,
}

/// `b - a` the short way round, in -180..180.
pub fn angle_delta(a: f32, b: f32) -> f32 {
    (b - a + 180.0).rem_euclid(360.0) - 180.0
}

impl Look {
    pub fn active(&self) -> bool {
        self.phase != Phase::Off
    }

    /// Starts from the player's own view.
    pub fn start(&mut self, yaw: f32, pitch: f32, switched: bool) {
        *self = Self {
            phase: Phase::Looking,
            yaw,
            pitch,
            switched,
            done: false,
        };
    }

    /// A turn of the mouse (or a pad) in `Entity.turn` units, while looking.
    pub fn turn(&mut self, x: f64, y: f64, invert: bool) {
        if self.phase != Phase::Looking {
            return;
        }
        let y = if invert { -y } else { y };
        self.yaw += (x * TURN_SCALE) as f32;
        self.pitch = (self.pitch + (y * TURN_SCALE) as f32).clamp(-90.0, 90.0);
    }

    /// The key let go (or toggled off): eased back, or back at once.
    pub fn release(&mut self, smooth: bool) {
        if self.phase != Phase::Looking {
            return;
        }
        if smooth {
            self.phase = Phase::Returning;
        } else {
            self.done = true;
        }
    }

    /// One frame of `step` towards the player's view (`yaw`, `pitch`) while returning.
    pub fn frame(&mut self, step: Duration, yaw: f32, pitch: f32) {
        if self.phase != Phase::Returning || self.done {
            return;
        }
        let dt = step.min(MAX_STEP).as_secs_f32();
        let keep = (-RETURN_RATE * dt).exp();
        let dy = angle_delta(self.yaw, yaw);
        let dp = pitch - self.pitch;
        if dy.abs() < RETURN_DONE && dp.abs() < RETURN_DONE {
            self.yaw = yaw;
            self.pitch = pitch;
            self.done = true;
            return;
        }
        // Keep the free yaw near the player's, so it can't wind up past a turn.
        self.yaw = yaw - dy * keep;
        self.pitch = pitch - dp * keep;
    }

    /// The camera's yaw and pitch while it is free; None when it is the player's.
    pub fn camera(&self) -> Option<(f32, f32)> {
        (self.active() && !self.done).then_some((self.yaw, self.pitch))
    }

    /// Ends it; whether it had switched the view.
    pub fn end(&mut self) -> bool {
        let switched = self.active() && self.switched;
        *self = Self::default();
        switched
    }
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
    static LOOK: RefCell<Look> = RefCell::new(Look::default());
}

pub fn settings(get: impl Fn(&str) -> Option<String>) -> Settings {
    SETTINGS.with_borrow_mut(|s| *s.get_or_insert_with(|| Settings::read(get)))
}

pub fn set_settings(new: Settings) {
    SETTINGS.with_borrow_mut(|s| *s = Some(new));
}

pub fn with_look<R>(f: impl FnOnce(&mut Look) -> R) -> R {
    LOOK.with_borrow_mut(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        assert_eq!(Settings::read(|_| None), Settings::default());
        let mut s = Settings::default();
        for b in 0..6 {
            s.click(b);
        }
        assert_eq!(
            s,
            Settings {
                hold: false,
                invert: true,
                smooth: false,
                first_person: false,
                third_person: false,
                switch_view: false
            }
        );
        let stored = s.stored();
        let read = Settings::read(|k| stored.iter().find(|p| p.0 == k).map(|p| p.1.clone()));
        assert_eq!(read, s);
        assert_eq!(s.labels().len(), 6);
        assert!(Settings::default().allowed(true) && Settings::default().allowed(false));
        assert!(!s.allowed(true) && !s.allowed(false));
    }

    #[test]
    fn turning_moves_only_the_camera_and_clamps_pitch() {
        let mut l = Look::default();
        assert_eq!(l.camera(), None);
        l.turn(100.0, 0.0, false);
        assert_eq!(l.camera(), None);
        l.start(10.0, 5.0, true);
        assert_eq!(l.camera(), Some((10.0, 5.0)));
        l.turn(100.0, 100.0, false);
        let (y, p) = l.camera().unwrap();
        assert!((y - 25.0).abs() < 1e-4 && (p - 20.0).abs() < 1e-4);
        l.turn(0.0, 100.0, true);
        assert!((l.camera().unwrap().1 - 5.0).abs() < 1e-4);
        l.turn(0.0, 10_000.0, false);
        assert_eq!(l.camera().unwrap().1, 90.0);
        l.turn(0.0, -100_000.0, false);
        assert_eq!(l.camera().unwrap().1, -90.0);
    }

    #[test]
    fn snap_return_ends_at_once() {
        let mut l = Look::default();
        l.start(0.0, 0.0, true);
        l.turn(600.0, 0.0, false);
        l.release(false);
        assert!(l.done);
        assert_eq!(l.camera(), None);
        assert!(l.end());
        assert!(!l.active());
        assert!(!l.end());
    }

    #[test]
    fn smooth_return_eases_the_short_way_and_finishes() {
        let mut l = Look::default();
        l.start(170.0, 0.0, false);
        // 40 degrees right of the player, across the -180/180 seam.
        l.yaw = 210.0;
        l.pitch = -30.0;
        l.release(true);
        assert_eq!(l.phase, Phase::Returning);
        // Turning while returning goes to the player, not the camera.
        l.turn(1000.0, 0.0, false);
        assert_eq!(l.yaw, 210.0);
        l.frame(Duration::from_millis(16), -190.0, 0.0);
        let left = angle_delta(l.yaw, -190.0).abs();
        assert!(left < 40.0 && left > 20.0, "{left}");
        for _ in 0..200 {
            l.frame(Duration::from_millis(16), -190.0, 0.0);
        }
        assert!(l.done);
        assert_eq!(l.camera(), None);
        assert!(!l.end());
    }

    #[test]
    fn a_long_stall_does_not_jump() {
        let mut l = Look::default();
        l.start(0.0, 0.0, false);
        l.yaw = 90.0;
        l.release(true);
        l.frame(Duration::from_secs(5), 0.0, 0.0);
        assert!(l.yaw > 10.0, "{}", l.yaw);
    }

    #[test]
    fn deltas_wrap() {
        assert_eq!(angle_delta(170.0, -170.0), 20.0);
        assert_eq!(angle_delta(-170.0, 170.0), -20.0);
        assert_eq!(angle_delta(0.0, 90.0), 90.0);
    }
}
