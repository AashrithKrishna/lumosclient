//! The instance page's HUD block: LumosHUD's `config/lumoshud.json` for Fabric instances, and
//! Horizon 26.3's own HUD switches in `horizon-extras.txt`.
//!
//! `config/lumoshud.json` (written by the mod too, see lumoshud/src/main/java/dev/lumosclient/hud/HudConfig.java):
//! ```json
//! { "version": 1, "editorKey": "key.keyboard.right.shift", "snapping": true,
//!   "theme": { "style": "lunar", "accent": "#FF6B2C", "cornerRadius": 3, "panelOpacity": 45 },
//!   "modules": { "fps": { "enabled": true, "anchor": "top_left", "x": 4, "y": 4, "scale": 1.0,
//!                         "textColor": "#FFFFFF", "background": true, "backgroundColor": "#000000",
//!                         "backgroundOpacity": 45, "shadow": true, "icon": true, "...module options": "..." } } }
//! ```
//! Missing keys take the mod's defaults, so the launcher only writes what the player changed and
//! keeps every key it doesn't know (positions, module options).

use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;

pub const LUMOS_FILE: &str = "config/lumoshud.json";
pub const HORIZON_FILE: &str = "horizon-extras.txt";

/// LumosHUD's modules: id, name, on at first, box behind it at first, has an icon (Lunar style,
/// switchable with "icon"). Same order and defaults as the mod's Modules.create().
pub const MODULES: &[(&str, &str, bool, bool, bool)] = &[
    ("fps", "FPS", true, true, true),
    ("cps", "CPS", true, true, true),
    ("coordinates", "Coordinates", true, true, true),
    ("keystrokes", "Keystrokes", true, true, false),
    ("direction", "Direction", false, true, true),
    ("biome", "Biome", false, true, true),
    ("armor", "Armor Status", true, false, false),
    ("potions", "Potion Effects", true, true, true),
    ("ping", "Ping", false, true, true),
    ("clock", "Clock", false, true, true),
    ("server", "Server Address", false, true, true),
    ("day", "Day Counter", false, true, true),
    ("speed", "Speed", false, true, true),
    ("reach", "Reach Display", false, true, true),
    ("combo", "Combo Counter", false, true, true),
    ("itemcounter", "Item Counter", false, false, false),
    ("memory", "Memory", false, true, true),
    ("sprint", "Toggle Sprint", false, false, true),
];

const DEFAULT_EDITOR_KEY: &str = "key.keyboard.right.shift";
/// The theme's defaults (the mod's core/Theme.java).
const DEFAULT_STYLE: &str = "lunar";
const DEFAULT_ACCENT: &str = "#FF6B2C";
const DEFAULT_RADIUS: i64 = 3;
const MAX_RADIUS: i64 = 6;
const DEFAULT_OPACITY: i64 = 45;

/// Horizon 26.3's HUD switches in horizon-extras.txt: key, label, default (from
/// horizon-client-render's extras.rs and pvp.rs).
pub const HORIZON_SWITCHES: &[(&str, &str, bool)] = &[
    ("hudFps", "FPS", false),
    ("hudCps", "CPS", false),
    ("hudKeys", "Keystrokes", false),
    ("hudCoords", "Coordinates", false),
    ("hudArmour", "Armour", false),
    ("hudEffects", "Effects", false),
    ("hudHeld", "Held item count", false),
    ("hudPing", "Ping", false),
    ("hudServer", "Server address", false),
    ("hudClock", "Clock", false),
    ("hudReach", "Reach", false),
    ("hudSpeed", "Speed", false),
    ("hudCombo", "Combo", false),
    ("hudBackground", "Widget backgrounds", true),
    ("hudShadow", "Text shadow", true),
    ("crosshair", "Custom crosshair", false),
    ("fps", "Classic FPS counter", false),
    ("keystrokes", "Classic keystrokes", false),
    ("minimap", "Minimap", false),
    ("zoom", "Zoom", false),
];

/// What the HUD block shows for an instance: `kind` is "lumos", "horizon" or "none".
pub fn state(game_dir: &Path, lumos: bool, horizon: bool) -> Value {
    if horizon {
        let switches: Vec<Value> = read_horizon(game_dir)
            .into_iter()
            .map(|(key, label, on)| json!({ "key": key, "label": label, "on": on }))
            .collect();
        return json!({ "kind": "horizon", "switches": switches });
    }
    if lumos {
        let mut v = read_lumos(game_dir);
        v["kind"] = json!("lumos");
        return v;
    }
    json!({ "kind": "none" })
}

/// Applies a change from the HUD block. Lumos patch: `{editorKey?, snapping?, reset?,
/// theme?: {style?, accent?, cornerRadius?, panelOpacity?}, all?: {style},
/// modules?: {id: {enabled?, scale?, textColor?, background?, backgroundOpacity?, shadow?, icon?}}}`.
/// Horizon patch: `{switches: {key: bool}}`.
pub fn apply(game_dir: &Path, lumos: bool, horizon: bool, patch: &Value) -> Result<(), String> {
    if horizon {
        let changes: Vec<(String, bool)> = patch
            .get("switches")
            .and_then(Value::as_object)
            .map(|m| m.iter().filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b))).collect())
            .unwrap_or_default();
        return set_horizon(game_dir, &changes);
    }
    if lumos {
        return apply_lumos(game_dir, patch);
    }
    Err("This version has no HUD settings.".into())
}

// ───────────────────────── LumosHUD ─────────────────────────

fn load_lumos(game_dir: &Path) -> Map<String, Value> {
    fs::read_to_string(game_dir.join(LUMOS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// The "theme" object with every key filled in (as the mod reads it).
fn read_theme(root: &Map<String, Value>) -> Value {
    let t = root.get("theme").and_then(Value::as_object);
    let style = t.and_then(|t| t.get("style")).and_then(Value::as_str).map(str::to_ascii_lowercase);
    let style = if style.as_deref() == Some("minimal") { "minimal" } else { DEFAULT_STYLE };
    let accent = t
        .and_then(|t| t.get("accent"))
        .and_then(Value::as_str)
        .filter(|c| valid_colour(c))
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or_else(|| DEFAULT_ACCENT.into());
    let num = |k: &str, d: i64, max: i64| {
        t.and_then(|t| t.get(k)).and_then(Value::as_f64).filter(|n| n.is_finite()).map_or(d, |n| (n.round() as i64).clamp(0, max))
    };
    json!({
        "style": style, "accent": accent,
        "cornerRadius": num("cornerRadius", DEFAULT_RADIUS, MAX_RADIUS),
        "panelOpacity": num("panelOpacity", DEFAULT_OPACITY, 100),
    })
}

pub fn read_lumos(game_dir: &Path) -> Value {
    let root = load_lumos(game_dir);
    let theme = read_theme(&root);
    let panel_opacity = theme["panelOpacity"].as_i64().unwrap_or(DEFAULT_OPACITY);
    let mods = root.get("modules").and_then(Value::as_object);
    let modules: Vec<Value> = MODULES
        .iter()
        .map(|&(id, name, on, bg, has_icon)| {
            let m = mods.and_then(|m| m.get(id)).and_then(Value::as_object);
            let b = |k: &str, d: bool| m.and_then(|m| m.get(k)).and_then(Value::as_bool).unwrap_or(d);
            let scale = m.and_then(|m| m.get("scale")).and_then(Value::as_f64).unwrap_or(1.0);
            let color = m
                .and_then(|m| m.get("textColor"))
                .and_then(Value::as_str)
                .filter(|c| valid_colour(c))
                .unwrap_or("#FFFFFF");
            let opacity = m.and_then(|m| m.get("backgroundOpacity")).and_then(Value::as_f64).map_or(panel_opacity, |o| o.round() as i64);
            // Potion Effects had "showIcons" before "icon"; the mod still reads it when "icon" is missing.
            let icon = m.and_then(|m| m.get("icon")).and_then(Value::as_bool).unwrap_or_else(|| b("showIcons", true));
            json!({
                "id": id, "name": name, "enabled": b("enabled", on),
                "scale": clamp_scale(scale), "textColor": color.to_ascii_uppercase(),
                "background": b("background", bg), "backgroundOpacity": opacity.clamp(0, 100),
                "shadow": b("shadow", true), "hasIcon": has_icon, "icon": has_icon && icon,
            })
        })
        .collect();
    json!({
        "editorKey": root.get("editorKey").and_then(Value::as_str).unwrap_or(DEFAULT_EDITOR_KEY),
        "snapping": root.get("snapping").and_then(Value::as_bool).unwrap_or(true),
        "theme": theme,
        "modules": modules,
    })
}

fn clamp_scale(s: f64) -> f64 {
    if !s.is_finite() {
        return 1.0;
    }
    (s.clamp(0.5, 3.0) * 100.0).round() / 100.0
}

fn valid_colour(c: &str) -> bool {
    let h = c.strip_prefix('#').unwrap_or("");
    (h.len() == 6 || h.len() == 8) && h.chars().all(|c| c.is_ascii_hexdigit())
}

/// The style fields a patch may set on a module, checked and clamped.
fn style_fields(src: &Map<String, Value>, dest: &mut Map<String, Value>) -> Result<(), String> {
    for (k, v) in src {
        match (k.as_str(), v) {
            ("enabled" | "background" | "shadow" | "icon", Value::Bool(_)) => {
                dest.insert(k.clone(), v.clone());
            }
            ("scale", Value::Number(n)) => {
                dest.insert(k.clone(), json!(clamp_scale(n.as_f64().unwrap_or(1.0))));
            }
            ("backgroundOpacity", Value::Number(n)) => {
                dest.insert(k.clone(), json!((n.as_f64().unwrap_or(45.0).round() as i64).clamp(0, 100)));
            }
            ("textColor", Value::String(c)) if valid_colour(c) => {
                dest.insert(k.clone(), json!(c.to_ascii_uppercase()));
            }
            ("textColor", _) => return Err("Colours look like #FFAA00.".into()),
            _ => {}
        }
    }
    Ok(())
}

pub fn apply_lumos(game_dir: &Path, patch: &Value) -> Result<(), String> {
    let path = game_dir.join(LUMOS_FILE);
    let mut root = load_lumos(game_dir);
    if patch.get("reset").and_then(Value::as_bool) == Some(true) {
        root.remove("modules");
    }
    if let Some(k) = patch.get("editorKey").and_then(Value::as_str) {
        if !(k.starts_with("key.keyboard.") || k.starts_with("key.mouse.")) || k.len() > 64 {
            return Err("That key name isn't one Minecraft knows.".into());
        }
        root.insert("editorKey".into(), json!(k));
    }
    if let Some(b) = patch.get("snapping").and_then(Value::as_bool) {
        root.insert("snapping".into(), json!(b));
    }
    if let Some(t) = patch.get("theme").and_then(Value::as_object) {
        let theme = root.entry("theme").or_insert_with(|| json!({}));
        if !theme.is_object() {
            *theme = json!({});
        }
        let theme = theme.as_object_mut().expect("object");
        for (k, v) in t {
            match (k.as_str(), v) {
                ("style", Value::String(s)) if s == "lunar" || s == "minimal" => {
                    theme.insert(k.clone(), v.clone());
                }
                ("style", _) => return Err("The HUD style is lunar or minimal.".into()),
                ("accent", Value::String(c)) if valid_colour(c) => {
                    theme.insert(k.clone(), json!(c.to_ascii_uppercase()));
                }
                ("accent", _) => return Err("Colours look like #FFAA00.".into()),
                ("cornerRadius", Value::Number(n)) => {
                    theme.insert(k.clone(), json!((n.as_f64().unwrap_or(3.0).round() as i64).clamp(0, MAX_RADIUS)));
                }
                ("panelOpacity", Value::Number(n)) => {
                    theme.insert(k.clone(), json!((n.as_f64().unwrap_or(45.0).round() as i64).clamp(0, 100)));
                }
                _ => {}
            }
        }
    }
    let all = patch.get("all").and_then(Value::as_object);
    let per = patch.get("modules").and_then(Value::as_object);
    if all.is_some() || per.is_some() {
        let mods = root.entry("modules").or_insert_with(|| json!({}));
        if !mods.is_object() {
            *mods = json!({});
        }
        let mods = mods.as_object_mut().expect("object");
        for &(id, ..) in MODULES {
            let src_all = all.map(|a| {
                let mut a = a.clone();
                a.remove("enabled"); // "all" sets style, never switches every module on
                a
            });
            let src_one = per.and_then(|p| p.get(id)).and_then(Value::as_object);
            if src_all.is_none() && src_one.is_none() {
                continue;
            }
            let m = mods.entry(id).or_insert_with(|| json!({}));
            if !m.is_object() {
                *m = json!({});
            }
            let m = m.as_object_mut().expect("object");
            if let Some(a) = &src_all {
                style_fields(a, m)?;
            }
            if let Some(o) = src_one {
                style_fields(o, m)?;
            }
        }
    }
    root.insert("version".into(), json!(1));
    let text = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())? + "\n";
    write_atomic(&path, &text)
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("Couldn't save {}: {e}", path.display()))
}

// ───────────────────────── Horizon 26.3 ─────────────────────────

/// A `key:value` line's key, as Horizon reads it (`Extras::parse`).
fn line_key(line: &str) -> Option<&str> {
    line.split_once(':').map(|(k, _)| k.trim())
}

pub fn read_horizon(game_dir: &Path) -> Vec<(&'static str, &'static str, bool)> {
    let text = fs::read_to_string(game_dir.join(HORIZON_FILE)).unwrap_or_default();
    HORIZON_SWITCHES
        .iter()
        .map(|&(key, label, def)| {
            // The last line wins, as in Horizon.
            let mut on = def;
            for line in text.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    if k.trim() == key {
                        on = match v.trim() {
                            "true" => true,
                            "false" => false,
                            _ => on,
                        };
                    }
                }
            }
            (key, label, on)
        })
        .collect()
}

/// Sets Horizon switches, rewriting only their lines (every other line, its order and line
/// endings stay as they are); switches with no line yet are added at the end.
pub fn set_horizon(game_dir: &Path, changes: &[(String, bool)]) -> Result<(), String> {
    let changes: Vec<&(String, bool)> = changes
        .iter()
        .filter(|(k, _)| HORIZON_SWITCHES.iter().any(|s| s.0 == k))
        .collect();
    if changes.is_empty() {
        return Ok(());
    }
    let path = game_dir.join(HORIZON_FILE);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut out = String::with_capacity(text.len() + 64);
    let mut seen = vec![false; changes.len()];
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let ending = &line[body.len()..];
        match line_key(body).and_then(|k| changes.iter().position(|(c, _)| c == k)) {
            Some(i) => {
                seen[i] = true;
                out.push_str(&format!("{}:{}", changes[i].0, changes[i].1));
                out.push_str(ending);
            }
            None => out.push_str(line),
        }
    }
    for (i, (k, v)) in changes.iter().enumerate() {
        if !seen[i] {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("{k}:{v}\n"));
        }
    }
    write_atomic(&path, &out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("lumos-hud-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn lumos_defaults_without_a_file() {
        let d = dir("defaults");
        let v = read_lumos(&d);
        assert_eq!(v["editorKey"], DEFAULT_EDITOR_KEY);
        assert_eq!(v["snapping"], true);
        let mods = v["modules"].as_array().unwrap();
        assert_eq!(mods.len(), MODULES.len());
        let fps = mods.iter().find(|m| m["id"] == "fps").unwrap();
        assert_eq!((fps["enabled"].clone(), fps["scale"].clone(), fps["textColor"].clone()), (json!(true), json!(1.0), json!("#FFFFFF")));
        let armor = mods.iter().find(|m| m["id"] == "armor").unwrap();
        assert_eq!(armor["background"], false);
        let ping = mods.iter().find(|m| m["id"] == "ping").unwrap();
        assert_eq!(ping["enabled"], false);
        assert_eq!((ping["hasIcon"].clone(), ping["icon"].clone()), (json!(true), json!(true)));
        let keys = mods.iter().find(|m| m["id"] == "keystrokes").unwrap();
        assert_eq!((keys["hasIcon"].clone(), keys["icon"].clone()), (json!(false), json!(false)));
        let potions = mods.iter().find(|m| m["id"] == "potions").unwrap();
        assert_eq!(potions["background"], true, "effects sit in a panel by default");
        assert_eq!(v["theme"], json!({"style": "lunar", "accent": "#FF6B2C", "cornerRadius": 3, "panelOpacity": 45}));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn lumos_patch_keeps_positions_and_unknown_keys() {
        let d = dir("patch");
        fs::create_dir_all(d.join("config")).unwrap();
        fs::write(
            d.join(LUMOS_FILE),
            r#"{"version":1,"future":42,"modules":{"fps":{"anchor":"top_right","x":-4.0,"y":4.0,"showLabel":false}}}"#,
        )
        .unwrap();
        let patch = json!({
            "snapping": false,
            "modules": {"fps": {"enabled": false, "scale": 9, "textColor": "#ffaa00", "anchor": "center"},
                        "nope": {"enabled": true}},
            "all": {"backgroundOpacity": 140.4, "enabled": true}
        });
        apply_lumos(&d, &patch).unwrap();
        let raw: Value = serde_json::from_str(&fs::read_to_string(d.join(LUMOS_FILE)).unwrap()).unwrap();
        assert_eq!(raw["future"], 42);
        let fps = &raw["modules"]["fps"];
        assert_eq!(fps["anchor"], "top_right", "the launcher never moves modules");
        assert_eq!(fps["x"], -4.0);
        assert_eq!(fps["showLabel"], false);
        assert_eq!(fps["enabled"], false);
        assert_eq!(fps["scale"], 3.0);
        assert_eq!(fps["textColor"], "#FFAA00");
        assert_eq!(fps["backgroundOpacity"], 100);
        assert!(raw["modules"].get("nope").is_none());
        assert!(raw["modules"]["cps"].get("enabled").is_none(), "\"all\" doesn't switch modules on");
        assert_eq!(raw["modules"]["cps"]["backgroundOpacity"], 100);
        let v = read_lumos(&d);
        assert_eq!(v["snapping"], false);
        assert!(apply_lumos(&d, &json!({"modules": {"fps": {"textColor": "red"}}})).is_err());
        assert!(apply_lumos(&d, &json!({"editorKey": "rm -rf"})).is_err());
        apply_lumos(&d, &json!({"editorKey": "key.keyboard.h"})).unwrap();
        assert_eq!(read_lumos(&d)["editorKey"], "key.keyboard.h");
        apply_lumos(&d, &json!({"reset": true})).unwrap();
        let raw: Value = serde_json::from_str(&fs::read_to_string(d.join(LUMOS_FILE)).unwrap()).unwrap();
        assert!(raw.get("modules").is_none());
        assert_eq!(raw["editorKey"], "key.keyboard.h");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn lumos_theme_and_icons() {
        let d = dir("theme");
        fs::create_dir_all(d.join("config")).unwrap();
        fs::write(
            d.join(LUMOS_FILE),
            r#"{"theme":{"panelOpacity":70,"extra":"kept"},"modules":{"potions":{"showIcons":false},"fps":{"backgroundOpacity":10}}}"#,
        )
        .unwrap();
        let v = read_lumos(&d);
        let m = |id: &str| v["modules"].as_array().unwrap().iter().find(|m| m["id"] == id).unwrap().clone();
        assert_eq!(m("cps")["backgroundOpacity"], 70, "modules without their own opacity follow the theme");
        assert_eq!(m("fps")["backgroundOpacity"], 10);
        assert_eq!(m("potions")["icon"], false, "the older showIcons still counts");
        apply_lumos(&d, &json!({"theme": {"style": "minimal", "accent": "#00aaff", "cornerRadius": 40, "bogus": 1},
                                "modules": {"fps": {"icon": false}, "potions": {"icon": true}}})).unwrap();
        let raw: Value = serde_json::from_str(&fs::read_to_string(d.join(LUMOS_FILE)).unwrap()).unwrap();
        assert_eq!(raw["theme"], json!({"panelOpacity": 70, "extra": "kept", "style": "minimal", "accent": "#00AAFF", "cornerRadius": 6}));
        assert_eq!(raw["modules"]["fps"]["icon"], false);
        let v = read_lumos(&d);
        assert_eq!(v["theme"]["style"], "minimal");
        assert_eq!(v["modules"][0]["icon"], false);
        assert_eq!(v["modules"].as_array().unwrap().iter().find(|m| m["id"] == "potions").unwrap()["icon"], true);
        assert!(apply_lumos(&d, &json!({"theme": {"style": "fancy"}})).is_err());
        assert!(apply_lumos(&d, &json!({"theme": {"accent": "orange"}})).is_err());
        apply_lumos(&d, &json!({"reset": true})).unwrap();
        assert_eq!(read_lumos(&d)["theme"]["accent"], "#00AAFF", "reset keeps the theme");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_broken_file_reads_as_defaults() {
        let d = dir("broken");
        fs::create_dir_all(d.join("config")).unwrap();
        fs::write(d.join(LUMOS_FILE), "{not json").unwrap();
        assert_eq!(read_lumos(&d)["modules"][0]["enabled"], true);
        apply_lumos(&d, &json!({"snapping": false})).unwrap();
        assert_eq!(read_lumos(&d)["snapping"], false);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn horizon_switches_rewrite_only_their_lines() {
        let d = dir("horizon");
        let before = "packBrowser:false\r\nhudFps : false\nhudPlace:fps,10,20,1\nzoomKey:key.keyboard.z\nhudFps:false\nweird line\n";
        fs::write(d.join(HORIZON_FILE), before).unwrap();
        let st = read_horizon(&d);
        assert_eq!(st.iter().find(|s| s.0 == "hudFps").unwrap().2, false);
        assert_eq!(st.iter().find(|s| s.0 == "hudBackground").unwrap().2, true);
        set_horizon(&d, &[("hudFps".into(), true), ("minimap".into(), true), ("packBrowser".into(), true)]).unwrap();
        let after = fs::read_to_string(d.join(HORIZON_FILE)).unwrap();
        assert_eq!(
            after,
            "packBrowser:false\r\nhudFps:true\nhudPlace:fps,10,20,1\nzoomKey:key.keyboard.z\nhudFps:true\nweird line\nminimap:true\n"
        );
        let st = read_horizon(&d);
        assert!(st.iter().find(|s| s.0 == "hudFps").unwrap().2);
        assert!(st.iter().find(|s| s.0 == "minimap").unwrap().2);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn horizon_file_is_created_when_missing() {
        let d = dir("horizon-new");
        apply(&d, false, true, &json!({"switches": {"hudCoords": true, "bogus": true}})).unwrap();
        assert_eq!(fs::read_to_string(d.join(HORIZON_FILE)).unwrap(), "hudCoords:true\n");
        assert_eq!(state(&d, false, true)["kind"], "horizon");
        assert_eq!(state(&d, true, false)["kind"], "lumos");
        assert_eq!(state(&d, false, false)["kind"], "none");
        let _ = fs::remove_dir_all(&d);
    }
}
