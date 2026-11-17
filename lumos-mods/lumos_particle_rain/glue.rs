// Lumos Particle Rain: between Horizon's `Game` and the rules in `lumos_particle_rain`.
use crate::game::Game;
use crate::game::lumos_particle_rain as m;
use crate::mod_api::NativeMod;
use horizon_client::fog::{FogData, FogType};
use horizon_client::particles::{ParticleOptions, ParticleType};

fn settings(game: &Game<'_>) -> m::Settings {
    m::settings(|key| game.native_setting(m::ID, key))
}

/// What one tick adds: the particle, where, and its velocity.
type Spawn = (ParticleOptions, [f64; 3], [f64; 3]);

/// `native_tick`, on the window only: this tick's drops, flakes and splashes around the camera.
pub(super) fn on_tick(game: &mut Game<'_>) {
    if !game.native_on(m::ID)
        || !game.native_window()
        || game.player.is_none()
        || !game.settings.horizon.weather
    {
        return;
    }
    let settings = settings(game);
    let (rain, _) = game.weather_levels(1.0);
    let budget = m::budget(rain, &settings, game.settings.particles, game.particles.len());
    if budget.drops == 0 && budget.splashes == 0 {
        return;
    }
    let eye = game.particles.camera;
    if eye.iter().any(|v| !v.is_finite()) {
        return;
    }
    let mut spawns: Vec<Spawn> = Vec::with_capacity((budget.drops + budget.splashes * 2) as usize);
    let g = &*game;
    m::with_state(|state| {
        state.tick = state.tick.wrapping_add(1);
        let [wind_x, wind_z] = m::wind(state.tick);
        let r = &mut state.rng;
        // Drops and flakes, falling from above the eye where the sky is open.
        for _ in 0..budget.drops {
            let x = eye[0] + r.range(-m::DROP_RADIUS, m::DROP_RADIUS);
            let z = eye[2] + r.range(-m::DROP_RADIUS, m::DROP_RADIUS);
            let (bx, bz) = (x.floor() as i32, z.floor() as i32);
            let top = f64::from(g.rain_height(bx, bz));
            // Rain or snow is the biome's, at the height it starts from; None is a dry biome
            // or a chunk that isn't here.
            let snowy = match g.precipitation(bx, eye[1].floor() as i32, bz) {
                Some(snowy) => snowy,
                None => continue,
            };
            if snowy {
                if !settings.snow {
                    continue;
                }
                let y = eye[1] + r.range(m::FLAKE_HEIGHT.0, m::FLAKE_HEIGHT.1);
                if y < top {
                    continue;
                }
                // Snowflake keeps the velocity it is given, slowing a little each tick.
                let v = [
                    wind_x + r.range(-0.02, 0.02),
                    -0.04 - r.range(0.0, 0.03),
                    wind_z + r.range(-0.02, 0.02),
                ];
                spawns.push((ParticleOptions::simple(ParticleType::Snowflake), [x, y, z], v));
            } else {
                let y = eye[1] + r.range(m::DROP_HEIGHT.0, m::DROP_HEIGHT.1);
                if y < top {
                    continue;
                }
                // Dust keeps a tenth of this; the wind slants it.
                let v = [wind_x * 20.0, m::DROP_FALL, wind_z * 20.0];
                spawns.push((ParticleOptions::dust(m::DROP_RGB, m::DROP_SCALE), [x, y, z], v));
            }
        }
        // Splashes on the ground (or the water) where it rains.
        for _ in 0..budget.splashes {
            let x = eye[0] + r.range(-m::SPLASH_RADIUS, m::SPLASH_RADIUS);
            let z = eye[2] + r.range(-m::SPLASH_RADIUS, m::SPLASH_RADIUS);
            let (bx, bz) = (x.floor() as i32, z.floor() as i32);
            let top = g.rain_height(bx, bz);
            let y = f64::from(top);
            if (y - eye[1]).abs() > m::SPLASH_REACH
                || g.precipitation(bx, top, bz) != Some(false)
            {
                continue;
            }
            spawns.push((
                ParticleOptions::simple(ParticleType::Rain),
                [x, y + 0.05, z],
                [0.0; 3],
            ));
            if r.unit() < 0.3 {
                let v = [r.range(-0.08, 0.08), 0.0, r.range(-0.08, 0.08)];
                spawns.push((ParticleOptions::simple(ParticleType::Splash), [x, y + 0.05, z], v));
            }
        }
    });
    for (options, at, v) in &spawns {
        game.particles.add(options, *at, *v);
    }
}

/// `weather_geometry`, cancellable: true hides vanilla's rain and snow sheets this frame.
pub(super) fn hide_sheets(game: &Game<'_>) -> bool {
    game.native_on(m::ID)
        && m::hides_sheets(
            &settings(game),
            game.settings.particles,
            game.settings.horizon.weather,
        )
}

/// `native_render_fog`: the mist, pulling the atmospheric fog in while it rains or snows.
pub(super) fn on_fog(game: &Game<'_>, partial: f32, camera: [f64; 3], fog_type: FogType, fog: &mut FogData) {
    if fog_type != FogType::Atmospheric
        || !game.native_on(m::ID)
        || !game.settings.horizon.weather
        || camera.iter().any(|v| !v.is_finite())
    {
        return;
    }
    let settings = settings(game);
    if settings.mist == 0 {
        return;
    }
    // A dry biome (a desert, a badlands) gets no mist.
    let at = [
        camera[0].floor() as i32,
        camera[1].floor() as i32,
        camera[2].floor() as i32,
    ];
    if game.precipitation(at[0], at[1], at[2]).is_none() {
        return;
    }
    let (rain, _) = game.weather_levels(partial);
    if let Some((start, end)) = m::mist(
        fog.environmental_start,
        fog.environmental_end,
        fog.render_distance_end,
        rain,
        settings.mist,
    ) {
        fog.environmental_start = start;
        fog.environmental_end = end;
    }
}

pub(super) fn on_list(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: m::ID.into(),
        title: format!("{} {}", m::NAME, m::VERSION),
        description: m::DESCRIPTION.into(),
        keys: Vec::new(),
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
    let mut new = settings(game);
    new.click(button);
    m::set_settings(new);
    for (key, value) in new.stored() {
        game.native_set_setting(m::ID, key, &value);
    }
}
