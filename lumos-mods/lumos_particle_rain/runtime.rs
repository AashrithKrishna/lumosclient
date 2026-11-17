// Lumos Particle Rain: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client/src/game.rs` as `mod lumos_particle_rain`; `glue.rs` reads
// the game and calls in here. It names nothing outside `std`, so it also compiles alone:
// `rustc --edition 2024 --test runtime.rs` runs the tests below.
use std::cell::RefCell;

pub const ID: &str = "lumos_particle_rain";
pub const NAME: &str = "Particle Rain";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "Rain falls as drops that splash on the blocks they hit, snow \
drifts down in flakes on the wind, and heavy weather brings a mist. Bounded for performance.";

pub const DENSITIES: [&str; 3] = ["Light", "Normal", "Heavy"];
pub const MISTS: [&str; 3] = ["Off", "Light", "Thick"];

/// Drops and flakes a tick at full rain, by density, with Particles on All.
pub const DROPS: [u32; 3] = [18, 36, 64];
/// Extra ground splashes a tick at full rain, by density, with Particles on All.
pub const SPLASHES: [u32; 3] = [8, 16, 28];
/// Live particles past which this mod adds fewer, and past which none (the engine holds 16384).
pub const SOFT_LIMIT: usize = 5000;
pub const HARD_LIMIT: usize = 9000;
/// Horizontal reach of drops and flakes around the camera, in blocks. The engine drops any
/// particle more than 32 blocks from the camera, so this, the heights below and the vertical
/// reach of a column stay well inside that.
pub const DROP_RADIUS: f64 = 14.0;
pub const SPLASH_RADIUS: f64 = 10.0;
/// A drop starts this many blocks over the eye; a flake, the snow range.
pub const DROP_HEIGHT: (f64, f64) = (2.0, 12.0);
pub const FLAKE_HEIGHT: (f64, f64) = (-1.0, 8.0);
/// A splash is skipped where the ground is further above or below the eye than this.
pub const SPLASH_REACH: f64 = 12.0;

/// The drop's colour, before Dust's own per-particle shading.
pub const DROP_RGB: [f32; 3] = [0.62, 0.72, 0.95];
/// Dust size for a drop: smaller lives shorter, and a drop crosses the view in under a second.
pub const DROP_SCALE: f32 = 0.7;
/// Dust keeps a tenth of the velocity it is given: this falls about 1.4 blocks a tick at first.
pub const DROP_FALL: f64 = -14.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Index in `DENSITIES`.
    pub density: usize,
    /// Hide vanilla's rain and snow sheets while the particles fall.
    pub hide_sheets: bool,
    /// Drifting snow flakes in snowy places.
    pub snow: bool,
    /// Extra splashes on the ground around you.
    pub splashes: bool,
    /// Index in `MISTS`.
    pub mist: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            density: 1,
            hide_sheets: true,
            snow: true,
            splashes: true,
            mist: 1,
        }
    }
}

impl Settings {
    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        let index = |key: &str, len: usize, default: usize| {
            get(key)
                .and_then(|v| v.trim().parse::<usize>().ok())
                .filter(|v| *v < len)
                .unwrap_or(default)
        };
        let flag = |key: &str, default: bool| get(key).map_or(default, |v| v.trim() == "true");
        Self {
            density: index("density", DENSITIES.len(), d.density),
            hide_sheets: flag("hide_sheets", d.hide_sheets),
            snow: flag("snow", d.snow),
            splashes: flag("splashes", d.splashes),
            mist: index("mist", MISTS.len(), d.mist),
        }
    }

    pub fn labels(&self) -> Vec<String> {
        let on = |b: bool| if b { "ON" } else { "OFF" };
        vec![
            format!("Density: {}", DENSITIES[self.density]),
            format!(
                "Vanilla rain sheets: {}",
                if self.hide_sheets { "Hidden" } else { "Shown" }
            ),
            format!("Snow flakes: {}", on(self.snow)),
            format!("Ground splashes: {}", on(self.splashes)),
            format!("Mist: {}", MISTS[self.mist]),
        ]
    }

    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.density = (self.density + 1) % DENSITIES.len(),
            1 => self.hide_sheets = !self.hide_sheets,
            2 => self.snow = !self.snow,
            3 => self.splashes = !self.splashes,
            4 => self.mist = (self.mist + 1) % MISTS.len(),
            _ => {}
        }
    }

    pub fn stored(&self) -> [(&'static str, String); 5] {
        [
            ("density", self.density.to_string()),
            ("hide_sheets", self.hide_sheets.to_string()),
            ("snow", self.snow.to_string()),
            ("splashes", self.splashes.to_string()),
            ("mist", self.mist.to_string()),
        ]
    }
}

/// This tick's spawns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub drops: u32,
    pub splashes: u32,
}

/// How many drops (or flakes) and splashes to add this tick: scaled by the rain level squared
/// (as vanilla's splashes are), halved on Particles: Decreased, none on Minimal, thinned past
/// `SOFT_LIMIT` live particles and stopped at `HARD_LIMIT`.
pub fn budget(rain: f32, settings: &Settings, particle_level: u8, live: usize) -> Budget {
    if !rain.is_finite() || rain <= 0.0 || particle_level >= 2 || live >= HARD_LIMIT {
        return Budget {
            drops: 0,
            splashes: 0,
        };
    }
    let rain = rain.min(1.0);
    let mut scale = rain * rain;
    if particle_level == 1 {
        scale *= 0.5;
    }
    if live > SOFT_LIMIT {
        scale *= (HARD_LIMIT - live) as f32 / (HARD_LIMIT - SOFT_LIMIT) as f32;
    }
    let of = |n: u32| (n as f32 * scale).round() as u32;
    Budget {
        drops: of(DROPS[settings.density]),
        splashes: if settings.splashes {
            of(SPLASHES[settings.density])
        } else {
            0
        },
    }
}

/// Whether the vanilla sheets are hidden: only while this mod's particles stand in for them.
pub fn hides_sheets(settings: &Settings, particle_level: u8, weather_on: bool) -> bool {
    settings.hide_sheets && weather_on && particle_level < 2
}

/// The wind at `tick`, blocks a tick along x and z: a slow, smooth swing (periods of about
/// 40 and 65 seconds) so the snow sways and the rain slants a little.
pub fn wind(tick: u64) -> [f64; 2] {
    let t = tick as f64;
    let x = (t / 127.0).sin() * 0.6 + (t / 383.0).sin() * 0.4;
    let z = (t / 211.0).cos() * 0.6 + (t / 517.0).sin() * 0.4;
    [x * 0.05, z * 0.05]
}

/// The atmospheric fog with the mist: both distances pulled in by up to 45% (Light) or 70%
/// (Thick) of the nearer of the environmental and render-distance ends, at full rain.
/// Returns the new (start, end), or None when nothing changes.
pub fn mist(
    start: f32,
    end: f32,
    render_end: f32,
    rain: f32,
    strength: usize,
) -> Option<(f32, f32)> {
    let pull = match strength {
        1 => 0.45,
        2 => 0.70,
        _ => return None,
    };
    // An environmental end may be unbounded (no fog of its own): the render distance's then.
    if !(rain.is_finite() && start.is_finite() && render_end.is_finite())
        || end.is_nan()
        || rain <= 0.0
        || end <= 0.0
    {
        return None;
    }
    let reach = end.min(render_end).max(0.0);
    let target = (reach * (1.0 - pull * rain.min(1.0))).max(16.0);
    if target >= end {
        return None;
    }
    let new_start = start.min(target * 0.3).max(0.0);
    Some((new_start, target))
}

/// A small, fast generator (xorshift64*), seeded once: particle placement needs no more.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in 0..1.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Uniform in `lo..hi`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// The mod's state between ticks.
#[derive(Clone, Debug)]
pub struct State {
    pub tick: u64,
    pub rng: Rng,
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
    static STATE: RefCell<State> = RefCell::new(State {
        tick: 0,
        rng: Rng::new(0x9E37_79B9_7F4A_7C15),
    });
}

pub fn settings(get: impl Fn(&str) -> Option<String>) -> Settings {
    SETTINGS.with_borrow_mut(|s| *s.get_or_insert_with(|| Settings::read(get)))
}

pub fn set_settings(new: Settings) {
    SETTINGS.with_borrow_mut(|s| *s = Some(new));
}

pub fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with_borrow_mut(f)
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
                density: 2,
                hide_sheets: false,
                snow: false,
                splashes: false,
                mist: 2
            }
        );
        let pairs = s.stored();
        let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(Settings::read(stored(&refs)), s);
        assert_eq!(Settings::read(stored(&[("density", "5")])).density, 1);
    }

    #[test]
    fn budget_follows_rain_and_the_particles_option() {
        let s = Settings::default();
        let full = budget(1.0, &s, 0, 0);
        assert_eq!(full, Budget { drops: 36, splashes: 16 });
        assert_eq!(budget(0.0, &s, 0, 0).drops, 0);
        assert_eq!(budget(0.5, &s, 0, 0).drops, 9);
        assert_eq!(budget(1.0, &s, 1, 0).drops, 18);
        assert_eq!(budget(1.0, &s, 2, 0).drops, 0);
        assert_eq!(budget(f32::NAN, &s, 0, 0).drops, 0);
        let no_splash = Settings {
            splashes: false,
            ..s
        };
        assert_eq!(budget(1.0, &no_splash, 0, 0).splashes, 0);
    }

    #[test]
    fn budget_backs_off_when_the_engine_is_busy() {
        let s = Settings {
            density: 2,
            ..Settings::default()
        };
        let calm = budget(1.0, &s, 0, 0).drops;
        let busy = budget(1.0, &s, 0, (SOFT_LIMIT + HARD_LIMIT) / 2).drops;
        assert!(busy < calm && busy > 0);
        assert_eq!(budget(1.0, &s, 0, HARD_LIMIT).drops, 0);
        assert_eq!(budget(1.0, &s, 0, 20_000).drops, 0);
    }

    #[test]
    fn spawns_stay_inside_the_engines_32_block_reach() {
        let far = (DROP_RADIUS * DROP_RADIUS * 2.0 + DROP_HEIGHT.1 * DROP_HEIGHT.1).sqrt();
        assert!(far < 32.0, "{far}");
        let splash = (SPLASH_RADIUS * SPLASH_RADIUS * 2.0 + SPLASH_REACH * SPLASH_REACH).sqrt();
        assert!(splash < 32.0, "{splash}");
    }

    #[test]
    fn sheets_hide_only_while_particles_replace_them() {
        let s = Settings::default();
        assert!(hides_sheets(&s, 0, true));
        assert!(hides_sheets(&s, 1, true));
        assert!(!hides_sheets(&s, 2, true));
        assert!(!hides_sheets(&s, 0, false));
        let shown = Settings {
            hide_sheets: false,
            ..s
        };
        assert!(!hides_sheets(&shown, 0, true));
    }

    #[test]
    fn wind_is_gentle_and_smooth() {
        for t in (0..100_000).step_by(37) {
            let [x, z] = wind(t);
            assert!(x.abs() <= 0.05 + 1e-9 && z.abs() <= 0.05 + 1e-9);
            let [x2, z2] = wind(t + 1);
            assert!((x - x2).abs() < 0.001 && (z - z2).abs() < 0.001);
        }
    }

    #[test]
    fn mist_pulls_the_fog_in() {
        assert_eq!(mist(10.0, 200.0, 192.0, 1.0, 0), None);
        assert_eq!(mist(10.0, 200.0, 192.0, 0.0, 1), None);
        let (s, e) = mist(150.0, 200.0, 192.0, 1.0, 1).unwrap();
        assert!((e - 192.0 * 0.55).abs() < 0.01);
        assert!(s <= e * 0.3 + 0.01);
        let (_, thick) = mist(0.0, 200.0, 192.0, 1.0, 2).unwrap();
        assert!(thick < e);
        // Never closer than 16 blocks, and never pushed further out.
        assert_eq!(mist(0.0, 20.0, 20.0, 1.0, 2), Some((0.0, 16.0)));
        assert_eq!(mist(0.0, 12.0, 12.0, 1.0, 2), None);
        assert_eq!(mist(0.0, f32::INFINITY, 100.0, 1.0, 1), Some((0.0, 55.0)));
        assert_eq!(mist(0.0, 200.0, f32::NAN, 1.0, 1), None);
    }

    #[test]
    fn rng_stays_in_range() {
        let mut r = Rng::new(42);
        for _ in 0..10_000 {
            let v = r.range(-3.0, 5.0);
            assert!((-3.0..5.0).contains(&v));
        }
    }
}
