// Lumos Freelook: between Horizon's `Game` and the rules in `lumos_freelook`.
use crate::game::Game;
use crate::game::lumos_freelook as m;
use crate::game::lumos_freelook_keys as keys;
use crate::game::perspective::CameraType;
use crate::mod_api::NativeMod;
use std::cell::Cell;
use std::time::Instant;

thread_local! {
    /// The last camera frame, for the eased return's step.
    static LAST_FRAME: Cell<Option<Instant>> = const { Cell::new(None) };
}

fn settings(game: &Game<'_>) -> m::Settings {
    m::settings(|key| game.native_setting(m::ID, key))
}

/// Starts freelook from the player's view, switching first person to third when asked.
fn start(game: &mut Game<'_>, s: m::Settings) {
    let first = game.camera_type.first_person();
    if !s.allowed(first) || game.studio.flying() {
        return;
    }
    let switched = first && s.switch_view;
    if switched {
        game.camera_type = CameraType::ThirdPersonBack;
        game.check_entity_post_effect(false);
    }
    let (yaw, pitch) = (game.yaw, game.pitch);
    m::with_look(|l| l.start(yaw, pitch, switched));
    LAST_FRAME.set(None);
}

/// Ends freelook now, back in first person if it switched from there (and the player hasn't
/// changed the view since).
fn finish(game: &mut Game<'_>) {
    if m::with_look(|l| l.end()) && game.camera_type == CameraType::ThirdPersonBack {
        game.camera_type = CameraType::FirstPerson;
        game.check_entity_post_effect(true);
    }
}

fn release(game: &mut Game<'_>, smooth: bool) {
    m::with_look(|l| l.release(smooth));
    if m::with_look(|l| l.done) {
        finish(game);
    }
}

/// `native_key`: the freelook key going down starts it (or, toggling, ends it); going up ends
/// a held one.
pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !game.native_on(m::ID) {
        return;
    }
    let s = settings(game);
    let active = m::with_look(|l| l.phase == m::Phase::Looking);
    if !pressed {
        if active && s.hold && key == keys::bound(game, keys::LOOK) {
            release(game, s.smooth);
        }
        return;
    }
    if !game.native_playing() || game.player.is_none() || !keys::pressed(game, keys::LOOK, key, true) {
        return;
    }
    if active {
        if !s.hold {
            release(game, s.smooth);
        }
    } else {
        // A return still easing is cut short by a new look.
        finish(game);
        start(game, s);
    }
}

/// `native_frame`: a held look whose key is up (it went up behind a screen), a look
/// left in another screen, the mod turned off, or a finished return: ended.
pub(super) fn on_frame(game: &mut Game<'_>) {
    let look = m::with_look(|l| *l);
    if !look.active() {
        return;
    }
    if !game.native_on(m::ID) || game.player.is_none() || game.studio.flying() {
        finish(game);
        return;
    }
    if look.done {
        finish(game);
        return;
    }
    let s = settings(game);
    if look.phase == m::Phase::Looking && s.hold && !keys::held(game, keys::LOOK) {
        release(game, s.smooth);
    }
}

/// `look_turn`: while looking, the mouse (or a pad) turns the camera and not the player.
pub(super) fn on_turn(game: &mut Game<'_>, x: f64, y: f64) -> bool {
    if !game.native_on(m::ID) || !m::with_look(|l| l.phase == m::Phase::Looking) {
        return false;
    }
    let invert = settings(game).invert;
    let zoom = game.zoom_turn_scale();
    m::with_look(|l| l.turn(x * zoom, y * zoom, invert));
    true
}

/// `native_camera_pose`: the camera turned to the free look. In third person it backs off from
/// the eye along the free look, pulled in where blocks are in the way (`Camera.getMaxZoom`).
pub(super) fn on_camera_pose(game: &Game<'_>, position: &mut [f64; 3], yaw: &mut f32, pitch: &mut f32) {
    if !game.native_on(m::ID) {
        return;
    }
    let now = Instant::now();
    let step = LAST_FRAME.replace(Some(now)).map_or_else(Default::default, |t| now - t);
    let Some((free_yaw, free_pitch)) = m::with_look(|l| {
        l.frame(step, game.yaw, game.pitch);
        l.camera()
    }) else {
        return;
    };
    let (y, p) = match game.camera_type {
        CameraType::ThirdPersonFront => (free_yaw + 180.0, -free_pitch),
        _ => (free_yaw, free_pitch),
    };
    if !game.camera_type.first_person() && game.view.detached {
        let mut cam = game.view;
        let eye = cam.eye;
        // Vanilla's distance (4, or the mount's or scale's), unless walls pulled it in.
        let vanilla = (0..3).map(|i| (position[i] - eye[i]).powi(2)).sum::<f64>().sqrt() as f32;
        let full = vanilla.max(m::THIRD_PERSON_DISTANCE);
        cam.position = eye;
        cam.set_rotation(y, p);
        let zoom = cam.max_zoom(full, &mut |from, to| clip(game, from, to));
        cam.move_by(-zoom, 0.0, 0.0);
        *position = cam.position;
    }
    *yaw = y;
    *pitch = p;
}

/// Whether `at` is inside a block the camera stops at: one with a full collision box, or a solid
/// block's outline (slabs, stairs). Glass, panes and bars let it through, as vanilla's visual
/// shape does.
fn blocks_camera(game: &Game<'_>, at: [f64; 3]) -> bool {
    let [x, y, z] = at.map(|c| c.floor() as i32);
    let facts = game.res.facts().get(game.world.block(x, y, z));
    let name = &*facts.name;
    if name.ends_with("glass") || name.ends_with("glass_pane") || name.ends_with("_bars") {
        return false;
    }
    if facts.full_collision {
        return true;
    }
    if !facts.legacy_solid {
        return false;
    }
    let local = [at[0] - f64::from(x), at[1] - f64::from(y), at[2] - f64::from(z)];
    facts
        .outline
        .iter()
        .any(|b| (0..3).all(|i| local[i] >= b[i] && local[i] <= b[i + 3]))
}

/// `Level.clip` for the camera's rays: the first point along `from`..`to` inside a block that
/// stops the camera, in steps of a twentieth of a block.
fn clip(game: &Game<'_>, from: [f64; 3], to: [f64; 3]) -> Option<[f64; 3]> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let steps = (len / 0.05).ceil().max(1.0) as u32;
    (0..=steps)
        .map(|i| {
            let t = f64::from(i) / f64::from(steps);
            [from[0] + d[0] * t, from[1] + d[1] * t, from[2] + d[2] * t]
        })
        .find(|&p| blocks_camera(game, p))
}

/// The client left its level: nothing carries over.
pub(super) fn on_level_unload(game: &mut Game<'_>) {
    finish(game);
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
