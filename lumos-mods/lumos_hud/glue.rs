// LumosHUD: between Horizon's `Game` and the rules in `lumos_hud`.
use crate::game::Game;
use crate::game::lumos_hud as m;
use crate::game::lumos_hud_keys as keys;
use crate::mod_api::{NativeMod, NativeScreenView};
use horizon_client::extras_hud::Corners;
use horizon_client::hud::HudBatch;
use horizon_items::items::ItemStack;
use std::sync::OnceLock;
use std::time::Instant;

const ARROWS: [&str; 3] = ["minecraft:arrow", "minecraft:spectral_arrow", "minecraft:tipped_arrow"];

/// Seconds since the mod first looked.
fn now() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// The layout, read from the mod's data folder once.
fn config(game: &Game<'_>) -> m::Config {
    m::with(|s| kept_config(game, &mut s.config).clone())
}

/// The layout, without a copy (read once, as `config`).
fn kept_config<'s>(game: &Game<'_>, config: &'s mut Option<m::Config>) -> &'s m::Config {
    config.get_or_insert_with(|| m::Config::parse(&game.native_setting(m::ID, m::LAYOUT_KEY).unwrap_or_default()))
}

/// Keeps `cfg`, and writes it when the editor changed something.
fn store(game: &Game<'_>, cfg: m::Config, force: bool) {
    let dirty = m::with(|s| {
        s.config = Some(cfg.clone());
        std::mem::take(&mut s.editor.dirty)
    });
    if dirty || force {
        game.native_set_setting(m::ID, m::LAYOUT_KEY, &cfg.serialize());
    }
}

/// Clicks per second and keys, which need the game mutably.
fn input(game: &mut Game<'_>) {
    let cps = game.native_clicks();
    let mut keys = [false; 7];
    let mut labels: [String; 4] = Default::default();
    for (i, name) in m::KEYS.iter().enumerate() {
        if let Some((down, label)) = game.native_mapping(name) {
            keys[i] = down;
            if i < 4 {
                labels[i] = label;
            }
        }
    }
    m::with(|s| {
        s.cps = [cps.0, cps.1];
        s.keys = keys;
        s.key_labels = labels;
    });
}

/// What the modules show this frame.
fn gather(game: &Game<'_>) -> m::Data {
    let now = now();
    let mut d = m::Data::default();
    d.fps = game.native_frame_rate().0;
    let speed = m::with(|s| {
        // Effect and biome names are looked up again each second (a language change).
        if !(0.0..1.0).contains(&(now - s.names_at)) {
            s.names_at = now;
            s.effect_names.clear();
            s.biome_name = None;
        }
        d.cps = s.cps;
        d.keys = s.keys;
        d.key_labels = s.key_labels.clone();
        d.combo = s.combo.count(now);
        d.reach = m::reach_shown(s.reach, now);
        s.speed.value
    });
    if let Some(p) = game.native_player() {
        d.yaw = p.yaw;
        if !game.reduced_debug_info() {
            d.pos = Some([p.x, p.y, p.z]);
        }
        let at = [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32];
        d.biome = game.native_biome(at).map(|id| {
            m::with(|s| match &s.biome_name {
                Some((kept, shown)) if *kept == id => shown.clone(),
                _ => {
                    let key = format!("biome.{}", id.replace(':', "."));
                    let shown = game.pack.names.display_get(&key).unwrap_or_else(|| m::pretty_id(&id));
                    s.biome_name = Some((id, shown.clone()));
                    shown
                }
            })
        });
        d.speed = Some(speed);
    }
    // `Game::armor` holds the head first.
    for (i, slot) in game.armor.iter().enumerate().take(4) {
        d.armor[i] = slot.as_ref().map(|s| m::Piece {
            damage: s.damage as u32,
            max: game.items().stack_max_damage(s) as u32,
        });
    }
    let rate = game.clock.rate();
    for e in game.potions.effects.list.iter() {
        let secs = if e.duration == -1 { -1 } else { (e.duration as f32 / rate) as i32 };
        let (name, sprite) = m::with(|s| {
            let at = (e.id, e.amplifier);
            if let Some((_, name, sprite)) = s.effect_names.iter().find(|n| n.0 == at) {
                return (name.clone(), sprite.clone());
            }
            let (name, sprite) = effect_name(game, e.id, e.amplifier);
            s.effect_names.push((at, name.clone(), sprite.clone()));
            (name, sprite)
        });
        d.effects.push(m::Effect { name, secs, sprite });
    }
    let multiplayer = game.remote.is_some() && !game.has_singleplayer_server();
    d.ping = game.own_latency().filter(|_| multiplayer);
    d.server = game
        .server_address()
        .filter(|_| multiplayer)
        .map(horizon_client::net::shown_address);
    d.clock = Some(m::with(|s| match &s.clock {
        Some((t, at)) if now - at < 1.0 => t.clone(),
        _ => {
            use chrono::Timelike;
            let t = chrono::Local::now();
            let shown = format!("{:02}:{:02}", t.hour(), t.minute());
            s.clock = Some((shown.clone(), now));
            shown
        }
    }));
    d.day_time = game.native_clock().map(|c| c.day_time);
    d.held = game.held().and_then(|held| {
        let ranged = matches!(&*held.item, "minecraft:bow" | "minecraft:crossbow");
        let counts = |s: &ItemStack| if ranged { ARROWS.contains(&&*s.item) } else { s.item == held.item };
        let mut first: Option<String> = None;
        let mut total = 0u32;
        for s in game.inventory.iter().chain(std::iter::once(&game.offhand)).flatten() {
            if counts(s) {
                first.get_or_insert_with(|| s.item.to_string());
                total = total.saturating_add(s.count);
            }
        }
        match first {
            None if ranged => Some(("minecraft:arrow".to_owned(), 0)),
            other => other.map(|f| (f, total)),
        }
    });
    d.memory = m::with(|s| match s.memory {
        Some((v, at)) if now - at < 1.0 => Some(v),
        _ => {
            let v = horizon_client::debug_overlay::ProcessMemory::read().map(|r| (r.used, r.max));
            s.memory = v.map(|v| (v, now));
            v
        }
    });
    if let Some(p) = game.player.as_ref() {
        let toggles = &game.settings;
        d.sprint = if game.native_sneaking() {
            let s = if toggles.toggle_crouch { "Sneaking (Toggled)" } else { "Sneaking (Key Held)" };
            Some((s.to_owned(), true))
        } else if p.sprinting || game.mapped_key_down("key.sprint") {
            let held = game.mapped_key_down("key.sprint");
            let s = if toggles.toggle_sprint {
                "Sprinting (Toggled)"
            } else if held {
                "Sprinting (Key Held)"
            } else {
                "Sprinting (Vanilla)"
            };
            Some((s.to_owned(), false))
        } else {
            None
        };
    }
    d
}

/// An effect's name as the list shows it ("Speed II") and its HUD sprite.
fn effect_name(game: &Game<'_>, id: &str, amplifier: i32) -> (String, String) {
    let id = format!("{}", id);
    let key = horizon_client_core::potions::description_id(&id);
    let mut name = game.pack.names.display_get(&key).unwrap_or_else(|| m::pretty_id(&id));
    let amplifier = amplifier as i64;
    if amplifier > 0 {
        name = format!("{name} {}", m::roman(amplifier as u32 + 1));
    }
    let sprite = match id.split_once(':') {
        Some((ns, path)) if ns == "minecraft" => format!("mob_effect/{path}"),
        Some((ns, path)) => format!("{ns}:mob_effect/{path}"),
        None => format!("mob_effect/{id}"),
    };
    (name, sprite)
}

fn paint(game: &Game<'_>, batch: &mut HudBatch, prims: &[m::Prim]) {
    for p in prims {
        match p {
            m::Prim::Fill { rect, argb } => game.native_fill(batch, *rect, *argb),
            m::Prim::Text { text, at, argb, shadow, scale } => {
                let start = batch.vertices.len();
                game.native_text(batch, text, *at, *argb & 0xFF_FFFF, *shadow);
                if *scale != 1.0 {
                    batch.scale_since(start, at[0], at[1], *scale, *scale);
                }
            }
            m::Prim::Item { item, at } => match item {
                m::ItemRef::Armor(i) => {
                    if let Some(Some(s)) = game.armor.get(*i) {
                        game.draw_item(batch, s, at[0], at[1]);
                    }
                }
                m::ItemRef::Id(id) => game.draw_item_model(batch, &ItemStack::new(id, 1), at[0], at[1]),
            },
            m::Prim::Sprite { name, rect, alpha } => {
                if let Some([w, h]) = game.native_sprite_size(name) {
                    let start = batch.vertices.len();
                    game.native_sprite(batch, name, [0.0, 0.0, w, h], *rect);
                    if *alpha < 1.0 {
                        batch.alpha_since(start, *alpha);
                    }
                }
            }
        }
    }
}

fn editor_open(game: &Game<'_>) -> bool {
    game.native_mod_screen() == Some(m::ID)
}

/// `native_hud`: the modules that are on, where the player put them.
pub(super) fn on_hud(game: &mut Game<'_>, batch: &mut HudBatch, _corners: &mut Corners) {
    if !game.native_on(m::ID) || game.player.is_none() || game.debug || editor_open(game) {
        return;
    }
    if m::with(|s| s.hidden) {
        return;
    }
    input(game);
    let d = gather(game);
    let (sw, sh) = (batch.width, batch.height);
    let g = &*game;
    let width = |t: &str| g.native_text_width(t);
    // The last frame's modules again when nothing they show changed; painted every frame.
    let placed = m::with(|s| {
        let cfg = kept_config(g, &mut s.config);
        m::frame_kept(&mut s.kept, cfg, d, &mut s.anim, sw, sh, now(), &width)
    });
    for p in &placed {
        paint(g, batch, &p.prims);
    }
    m::with(|s| s.kept.give_back(placed));
}

/// The vanilla effect icons stay out while the Potion Effects module shows them.
pub(super) fn on_layer(game: &Game<'_>, layer: &str, canceled: &mut bool) {
    if layer != "minecraft:effects" || !game.native_on(m::ID) || game.debug || m::with(|s| s.hidden) {
        return;
    }
    let i = m::index_of("potions").unwrap_or(0);
    if m::with(|s| kept_config(game, &mut s.config).slots[i].on) {
        *canceled = true;
    }
}

/// `native_key`: the editor key opens and closes the editor, the toggle key hides the HUD.
pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || !game.native_on(m::ID) {
        return;
    }
    if keys::pressed(game, "editor", key, pressed) {
        if editor_open(game) {
            game.native_mod_screen_close(m::ID);
        } else if game.native_playing() && game.player.is_some() {
            m::with(|s| s.editor.reset());
            let _ = game.native_mod_screen_open(m::ID);
        }
    } else if keys::pressed(game, "toggle", key, pressed) && game.native_playing() {
        m::with(|s| s.hidden = !s.hidden);
    }
}

/// The client's tick: speed, the editor's input, and its Done button.
pub(super) fn on_tick(game: &mut Game<'_>) {
    if !game.native_on(m::ID) {
        return;
    }
    let pos = game.native_player().map(|p| (p.x, p.z));
    m::with(|s| s.speed.tick(pos));
    if editor_open(game) {
        input(game);
        if m::with(|s| std::mem::take(&mut s.editor.close)) {
            game.native_mod_screen_close(m::ID);
        }
    }
}

fn mine(game: &Game<'_>, screen: &NativeScreenView<'_>) -> bool {
    screen.id == "mod" && editor_open(game) && game.native_on(m::ID)
}

/// The editor: the modules with samples, their outlines, the buttons and the module list.
pub(super) fn on_render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if !mine(game, screen) {
        return;
    }
    let cfg = config(game);
    let d = gather(game);
    let (sw, sh) = (batch.width, batch.height);
    let width = |t: &str| game.native_text_width(t);
    let prims = m::with(|s| {
        let placed = m::frame(&cfg, &d, &mut s.anim, sw, sh, true, now(), &width);
        s.editor.draw(&cfg, placed, screen.mouse, sw, sh, &width)
    });
    paint(game, batch, &prims);
}

pub(super) fn on_press(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    if !mine(game, screen) {
        return;
    }
    *canceled = true;
    let (sw, sh) = game.gui_size();
    let mut cfg = config(game);
    m::with(|s| s.editor.press(&mut cfg, screen.mouse, button, sw, sh));
    store(game, cfg, false);
}

pub(super) fn on_release(game: &Game<'_>, screen: &NativeScreenView<'_>, canceled: &mut bool) {
    if !mine(game, screen) {
        return;
    }
    *canceled = true;
    m::with(|s| s.editor.release());
    store(game, config(game), false);
}

pub(super) fn on_drag(game: &Game<'_>, screen: &NativeScreenView<'_>, modifiers: u32, canceled: &mut bool) {
    if !mine(game, screen) {
        return;
    }
    *canceled = true;
    let (sw, sh) = game.gui_size();
    let mut cfg = config(game);
    // Shift places freely.
    m::with(|s| s.editor.moved(&mut cfg, screen.mouse, modifiers & 1 != 0, sw, sh));
    m::with(|s| s.config = Some(cfg));
}

/// This player attacked entity `id`, `distance` from the eyes to where the crosshair met it.
pub(super) fn on_attacked(game: &Game<'_>, id: i32, distance: f64) {
    if game.native_on(m::ID) {
        let now = now();
        m::with(|s| {
            s.combo.attacked(id, now);
            s.reach = Some((distance, now));
        });
    }
}

/// The server reports entity `id` hurt (`own`: this player).
pub(super) fn on_hurt(game: &Game<'_>, id: i32, own: bool, by_me: Option<bool>) {
    if game.native_on(m::ID) {
        let now = now();
        m::with(|s| s.combo.hurt(id, own, by_me, now));
    }
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
    buttons.extend(config(game).labels());
}

pub(super) fn on_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != m::ID {
        return;
    }
    let mut cfg = config(game);
    cfg.click(button);
    store(game, cfg, true);
}
