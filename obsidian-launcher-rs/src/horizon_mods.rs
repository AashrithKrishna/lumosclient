//! Horizon's own mod registry for Horizon 26.3 instances: content mods (`.hmod`), Horizon
//! shader packs (`.hshader`) and resource packs, listed in one `index.json`
//! (docs/modding/src/reference/registry-index.md). Every download is checked against the
//! index (size, SHA-256, the file's own manifest) before it reaches the instance, and files the
//! registry took down are refused.
//!
//! Native mods (Rust source compiled on the player's PC against the exact official Horizon
//! release each one pins) are listed but not installed: Lumos plays its own Horizon build.

use crate::net;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const INDEX_URL: &str = "https://horizon-rust.github.io/horizon-mods/index.json";
const SITE: &str = "https://playhorizon.dev/mods";
const GAME: &str = "26.3";
const FRESH: Duration = Duration::from_secs(600);
/// The registry's own limit for a file.
const MAX_FILE: u64 = 64 << 20;
/// Content mods live here and are turned on in `ENABLED_FILE` (beside `options.txt`).
pub const MODS_DIR: &str = "horizon-mods";
const ENABLED_FILE: &str = "horizon-mods.txt";
const PAGE: usize = 20;

static INDEX: Mutex<Option<(Instant, Arc<Value>)>> = Mutex::new(None);

/// The registry index, fetched at most every ten minutes.
fn index() -> Result<Arc<Value>, String> {
    if let Some((at, idx)) = INDEX.lock().unwrap().as_ref() {
        if at.elapsed() < FRESH {
            return Ok(idx.clone());
        }
    }
    let idx = net::get_json(INDEX_URL)?;
    match idx.get("schema").and_then(Value::as_u64) {
        Some(1) => {}
        Some(_) => return Err("Horizon's mod list uses a newer format. Update Lumos to browse it.".into()),
        None => return Err("Horizon's mod list couldn't be read.".into()),
    }
    let idx = Arc::new(with_frozen_native(idx));
    *INDEX.lock().unwrap() = Some((Instant::now(), idx.clone()));
    Ok(idx)
}

/// Horizon's registry moved its native-v1 mods into an archive on 2026-11-18 (their downloads are
/// unchanged). Lumos publishes a frozen copy of those listings beside its native builds; merge it
/// in for any id the live index no longer lists, so Discover and installs keep working.
const FROZEN_NATIVE_URL: &str = "https://github.com/AashrithKrishna/lumosclient/releases/download/native-builds/native-v1-index.json";

fn with_frozen_native(mut idx: Value) -> Value {
    let Ok(frozen) = net::get_json(FROZEN_NATIVE_URL) else { return idx };
    merge_frozen(&mut idx, &frozen);
    idx
}

fn merge_frozen(idx: &mut Value, frozen: &Value) {
    let live: std::collections::HashSet<String> = idx.get("projects").and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|p| p.get("id").and_then(Value::as_str).map(str::to_owned)).collect()).unwrap_or_default();
    let extra: Vec<Value> = frozen.get("projects").and_then(Value::as_array).map(|a| a.iter()
        .filter(|p| matches!(p.get("kind").and_then(Value::as_str), Some("native-mod" | "shader" | "mod")))
        .filter(|p| p.get("id").and_then(Value::as_str).is_some_and(|i| !live.contains(i))).cloned().collect()).unwrap_or_default();
    if let Some(arr) = idx.get_mut("projects").and_then(Value::as_array_mut) {
        arr.extend(extra);
    }
    if let (Some(rv), Some(frv)) = (idx.get_mut("revoked").and_then(Value::as_array_mut), frozen.get("revoked").and_then(Value::as_array)) {
        rv.extend(frv.iter().cloned());
    }
}

#[cfg(test)]
mod frozen_tests {
    use super::*;
    #[test]
    fn frozen_native_fills_in_only_missing_ids() {
        let mut idx = json!({"schema": 1, "projects": [{"id": "zoom", "kind": "mod"}], "revoked": []});
        let frozen = json!({"projects": [{"id": "zoom", "kind": "native-mod"}, {"id": "minimap", "kind": "native-mod"}, {"id": "x", "kind": "shader"}, {"id": "pack", "kind": "modpack"}], "revoked": [{"id": "bad"}]});
        merge_frozen(&mut idx, &frozen);
        let ids: Vec<&str> = idx["projects"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["zoom", "minimap", "x"]);
        assert_eq!(idx["revoked"].as_array().unwrap().len(), 1);
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Which registry kinds a Discover tab shows.
fn kinds_for(tab: &str) -> &'static [&'static str] {
    match tab {
        "shader" => &["shader"],
        "resourcepack" => &["resourcepack"],
        // Horizon's registry has modpacks (.hpack) Lumos doesn't install yet.
        "modpack" => &[],
        _ => &["mod", "native-mod"],
    }
}

/// Where an installable kind goes in the instance.
fn folder_for(kind: &str) -> Option<&'static str> {
    match kind {
        "mod" => Some(MODS_DIR),
        "shader" => Some("shaderpacks"),
        "resourcepack" => Some("resourcepacks"),
        _ => None,
    }
}

fn extension_ok(kind: &str, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    match kind {
        "mod" => n.ends_with(".hmod"),
        "shader" => n.ends_with(".hshader"),
        "resourcepack" => n.ends_with(".zip"),
        _ => false,
    }
}

/// True when the registry took this project (or this exact file) down.
fn revoked(idx: &Value, id: &str, sha256: &str) -> Option<String> {
    idx.get("revoked")?.as_array()?.iter().find_map(|r| {
        let same_id = s(r, "id") == id;
        let file = s(r, "sha256");
        (same_id && (file.is_empty() || file.eq_ignore_ascii_case(sha256))).then(|| s(r, "reason").to_owned())
    })
}

/// The newest version made for 26.3.
fn current_version(p: &Value) -> Option<&Value> {
    p.get("versions")?.as_array()?.iter().find(|v| s(v, "game") == GAME)
}

// ── feat/discover: category filters for Horizon instances ──

/// True when a registry project is in every one of `cats` (none = everything).
pub fn in_categories(p: &Value, cats: &[String]) -> bool {
    let have: Vec<&str> = p.get("categories").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    cats.iter().all(|c| have.iter().any(|h| h.eq_ignore_ascii_case(c)))
}

/// The categories the registry uses for a Discover tab, as the sidebar shows them.
pub fn categories_in(idx: &Value, tab: &str) -> Value {
    let kinds = kinds_for(tab);
    let mut names: Vec<String> = idx
        .get("projects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| kinds.contains(&s(p, "kind")))
        .flat_map(|p| p.get("categories").and_then(Value::as_array).cloned().unwrap_or_default())
        .filter_map(|c| c.as_str().map(str::to_owned))
        .collect();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return json!([]);
    }
    json!([{"header": "Categories", "items": names.iter().map(|n| json!({"value": n, "label": crate::discover::pretty_tag(n)})).collect::<Vec<_>>()}])
}

/// Sidebar categories for every tab, from the live registry.
pub fn categories() -> Result<Value, String> {
    let idx = index()?;
    Ok(json!({"source": "horizon", "categories": {"mod": categories_in(&idx, "mod"), "resourcepack": categories_in(&idx, "resourcepack"), "shader": categories_in(&idx, "shader")}}))
}
// ── end feat/discover ──

/// Discover results in the shape the page already renders for Modrinth, plus `source`,
/// `native` (listed, not installable) and `installed` for this instance.
pub fn search(game_dir: &Path, query: &str, tab: &str, sort: &str, offset: usize, cats: &[String]) -> Value {
    let idx = match index() {
        Ok(i) => i,
        Err(e) => return json!({"hits": [], "total_hits": 0, "error": e, "source": "horizon"}),
    };
    let kinds = kinds_for(tab);
    let q = query.trim().to_lowercase();
    let installed = installed_files(game_dir);
    let native_installed = crate::native_builds::installed_ids(game_dir);
    let own = own_projects();
    let mut hits: Vec<(&Value, &Value)> = idx
        .get("projects")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .chain(own.iter())
        .filter(|p| kinds.contains(&s(p, "kind")))
        .filter_map(|p| current_version(p).map(|v| (p, v)))
        .filter(|(p, v)| revoked(&idx, s(p, "id"), s(&v["file"], "sha256")).is_none())
        .filter(|(p, _)| in_categories(p, cats))
        .filter(|(p, _)| {
            q.is_empty()
                || [s(p, "id"), s(p, "name"), s(p, "summary")].iter().any(|t| t.to_lowercase().contains(&q))
                || p.get("tags").and_then(Value::as_array).is_some_and(|t| t.iter().any(|x| x.as_str().is_some_and(|x| x.contains(&q))))
        })
        .collect();
    match sort {
        "newest" | "updated" => hits.sort_by(|a, b| s(b.0, "updated").cmp(s(a.0, "updated"))),
        "relevance" if !q.is_empty() => {
            hits.sort_by_key(|(p, _)| (!s(p, "name").to_lowercase().starts_with(&q), std::cmp::Reverse(p["downloads"].as_u64().unwrap_or(0))))
        }
        _ => hits.sort_by_key(|(p, _)| std::cmp::Reverse(p["downloads"].as_u64().unwrap_or(0))),
    }
    // What can be installed comes first; native mods (listed only) follow, in the same order.
    hits.sort_by_key(|(p, _)| s(p, "kind") == "native-mod");
    let total = hits.len();
    let page: Vec<Value> = hits
        .into_iter()
        .skip(offset)
        .take(PAGE)
        .map(|(p, v)| {
            let kind = s(p, "kind");
            let file = s(&v["file"], "name");
            let authors: Vec<&str> = p.get("authors").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            json!({
                "id": s(p, "id"),
                "slug": s(p, "id"),
                "title": s(p, "name"),
                "description": s(p, "summary"),
                "icon_url": s(p, "icon"),
                "author": authors.join(", "),
                "downloads": p["downloads"].as_u64().unwrap_or(0),
                "date_modified": s(p, "updated"),
                "categories": p.get("categories").cloned().unwrap_or(json!([])),
                "project_type": if tab == "shader" || tab == "resourcepack" { tab } else { "mod" },
                "source": "horizon",
                "native": kind == "native-mod",
                // A native mod installs when Lumos's cloud builds have it for this computer.
                "native_ready": kind == "native-mod" && crate::native_builds::build_for_mod(s(p, "id")).is_some(),
                // Built, but only in a smaller group game: installing it first ties a new instance to that game.
                "native_separate": kind == "native-mod" && crate::native_builds::build_for_mod(s(p, "id")).is_some() && !crate::native_builds::in_combined(s(p, "id")),
                // Built, but no build has it together with the native mods this instance already has.
                "native_blocked": kind == "native-mod" && !native_installed.is_empty() && !native_installed.iter().any(|i| i == s(p, "id"))
                    && crate::native_builds::build_for_mod(s(p, "id")).is_some()
                    && crate::native_builds::build_with_all(&native_installed.iter().cloned().chain([s(p, "id").to_owned()]).collect::<Vec<_>>()).is_none(),
                "installed": installed.iter().any(|f| f == file) || native_installed.iter().any(|i| i == s(p, "id")),
                "view_url": if s(p, "id").starts_with("lumos_") { format!("https://github.com/AashrithKrishna/lumosclient/tree/main/lumos-mods/{}", s(p, "id")) } else { format!("{SITE}/{}", s(p, "id")) },
            })
        })
        .collect();
    json!({"hits": page, "total_hits": total, "source": "horizon"})
}

/// Lumos's own native mods (lumos-mods/ in this repo): built into the combined game, not in
/// Horizon's registry, so Discover lists them from here: (id, name, summary, categories).
const OWN: &[(&str, &str, &str, &[&str])] = &[
    ("lumos_durability_tooltip", "Durability Tooltip", "Shows durability as numbers on item tooltips, coloured green to red.", &["utility", "decoration"]),
    ("lumos_totem_counter", "Totem Counter", "Counts your Totems of Undying on the HUD and tells you when one pops.", &["utility", "game-mechanics"]),
    ("lumos_particle_rain", "Particle Rain", "Rain and snow as falling particles, with splashes and optional mist.", &["decoration"]),
    ("lumos_paginated_advancements", "Paginated Advancements", "Pages for the advancement tabs, so no tab is ever cut off.", &["utility"]),
    ("lumos_hud", "LumosHUD", "Lunar-style HUD modules (FPS, CPS, keystrokes, armour, effects and more). Right Shift opens the editor.", &["utility", "decoration"]),
    ("lumos_freelook", "Freelook", "Hold Left Alt to look around while you keep moving the way you face.", &["utility"]),
    ("lumos_map", "Map", "Crisp minimap and saved world map in the game's own colours, mob faces on Tab, on-screen waypoints and a full waypoint manager.", &["utility", "adventure"]),
    ("lumos_fullbright_key", "Fullbright Key", "A key (G) that turns Horizon's Fullbright on and off, with a notice.", &["utility"]),
    ("lumos_discover", "Lumos Discover", "Browse and install mods, resource packs and shaders from inside the game. Right Control opens it.", &["utility"]),
    ("lumos_controls", "Lumos Controls", "Every native mod's keys in Options > Controls > Key Binds, a section per mod, with clashes in red.", &["utility"]),
];

fn own_projects() -> Vec<Value> {
    OWN.iter()
        .map(|(id, name, summary, cats)| {
            json!({"id": id, "name": name, "summary": summary, "kind": "native-mod", "authors": ["Lumos"],
                   "categories": cats, "tags": cats, "downloads": 0, "updated": "",
                   "versions": [{"game": GAME, "file": {"name": "", "sha256": ""}}]})
        })
        .collect()
}

/// True when the registry lists `id` as a native (compiled-in) mod.
pub fn is_native(id: &str) -> bool {
    if OWN.iter().any(|o| o.0 == id) {
        return true;
    }
    index().ok().and_then(|idx| {
        idx.get("projects")?.as_array()?.iter().find(|p| s(p, "id") == id).map(|p| s(p, "kind") == "native-mod")
    }) == Some(true)
}

/// File names already in the instance's Horizon folders.
fn installed_files(game_dir: &Path) -> Vec<String> {
    [MODS_DIR, "shaderpacks", "resourcepacks"]
        .iter()
        .filter_map(|d| fs::read_dir(game_dir.join(d)).ok())
        .flat_map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect()
}

/// Accepts only the download shapes the registry documents (GitHub release assets).
fn url_ok(url: &str) -> bool {
    url.starts_with("https://github.com/") && url.contains("/releases/download/") && !url.contains(char::is_whitespace)
}

/// Installs a registry project into the instance, unplanned. Returns the saved file name.
/// (The launcher plans installs through mod_plan.rs; kept for the live test.)
#[cfg_attr(not(test), allow(dead_code))]
pub fn install(game_dir: &Path, id: &str) -> Result<String, String> {
    let f = fetch(id)?;
    save(game_dir, &f)?;
    Ok(f.name)
}

/// A registry file downloaded and checked (size, SHA-256, signature, manifest id), not saved yet.
pub struct Fetched {
    pub id: String,
    /// The registry's display name.
    pub title: String,
    /// Registry kind: `mod`, `shader` or `resourcepack`.
    pub kind: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The registry kind of `id` (`mod`, `native-mod`, ...) and its display name, when listed.
pub fn project_kind(id: &str) -> Option<(String, String)> {
    let idx = index().ok()?;
    let p = idx.get("projects")?.as_array()?.iter().find(|p| s(p, "id") == id)?;
    Some((s(p, "kind").to_owned(), s(p, "name").to_owned()))
}

/// Downloads and checks a registry project's current file without touching the instance.
pub fn fetch(id: &str) -> Result<Fetched, String> {
    let idx = index()?;
    let p = idx
        .get("projects")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|p| s(p, "id") == id))
        .ok_or("Horizon's mod list doesn't have that project anymore.")?;
    let kind = s(p, "kind");
    if kind == "native-mod" {
        return Err(format!(
            "{} is a native Rust mod. Those are compiled on your PC against an official Horizon release, which Lumos doesn't do yet.",
            s(p, "name")
        ));
    }
    folder_for(kind).ok_or("Lumos can't install that kind of Horizon project yet.")?;
    let v = current_version(p).ok_or("This project has no version for Horizon 26.3.")?;
    let f = &v["file"];
    let (name, url, sha) = (s(f, "name"), s(f, "url"), s(f, "sha256").to_ascii_lowercase());
    let size = f.get("size").and_then(Value::as_u64).unwrap_or(0);
    if let Some(reason) = revoked(&idx, id, &sha) {
        return Err(format!("Horizon's registry took this down: {reason}"));
    }
    if !crate::safe_file_name(name) || !extension_ok(kind, name) || !url_ok(url) || sha.len() != 64 || size == 0 || size > MAX_FILE {
        return Err("Horizon's mod list has a malformed entry for this project, so it wasn't installed.".into());
    }
    let bytes = net::get_bytes(url, size)?;
    if bytes.len() as u64 != size {
        return Err(format!("{name} downloaded with the wrong size. Try again."));
    }
    let got = format!("{:x}", Sha256::digest(&bytes));
    if got != sha {
        return Err(format!("{name} failed its integrity check, so it wasn't installed."));
    }
    // A project with an author key: the file must carry that author's signature.
    if let Some(key) = p.get("key").and_then(Value::as_str).filter(|k| !k.is_empty()) {
        verify_signature(key, s(f, "signature"), &bytes).map_err(|why| format!("{name}'s signature doesn't check out ({why}), so it wasn't installed."))?;
    }
    if kind == "mod" {
        let manifest_id = manifest_id_from_bytes(&bytes).ok_or(format!("{name} isn't a valid Horizon mod."))?;
        if manifest_id != id {
            return Err(format!("{name} says it is \"{manifest_id}\", not \"{id}\", so it wasn't installed."));
        }
    }
    Ok(Fetched { id: id.to_owned(), title: s(p, "name").to_owned(), kind: kind.to_owned(), name: name.to_owned(), bytes })
}

/// Saves a fetched file into the instance (a content mod replaces its older copies and is turned on).
pub fn save(game_dir: &Path, f: &Fetched) -> Result<(), String> {
    let folder = folder_for(&f.kind).ok_or("Lumos can't install that kind of Horizon project yet.")?;
    let name = f.name.as_str();
    let dir = game_dir.join(folder);
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{name}.part"));
    fs::write(&tmp, &f.bytes).map_err(|e| format!("Couldn't save {name}: {e}"))?;
    fs::rename(&tmp, dir.join(name)).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Couldn't save {name}: {e}")
    })?;
    if f.kind == "mod" {
        // Only now that the new file is saved: older versions of the same mod go (two copies
        // of one id stop Horizon from loading mods).
        remove_other_copies(&dir, &f.id, name);
        set_enabled(game_dir, &f.id, true)?;
    }
    Ok(())
}

/// Checks a registry file's minisign signature against its project's public key.
fn verify_signature(key: &str, signature: &str, bytes: &[u8]) -> Result<(), String> {
    let pk = minisign_verify::PublicKey::from_base64(key.trim()).map_err(|_| "bad key".to_owned())?;
    let sig = minisign_verify::Signature::decode(&signature.replace("\r\n", "\n")).map_err(|_| "no valid signature".to_owned())?;
    pk.verify(bytes, &sig, false).map_err(|_| "signed by someone else".to_owned())
}

/// Deletes every other file in `dir` that is the same content mod (same manifest id).
pub fn remove_other_copies(dir: &Path, id: &str, keep: &str) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let path = e.path();
            if path.file_name().is_some_and(|n| n != keep) && manifest_id_of(&path).as_deref() == Some(id) {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// True when another file in `dir` (besides `except`) is the content mod `id`.
pub fn other_copy_exists(dir: &Path, id: &str, except: &str) -> bool {
    fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten().any(|e| {
            let path = e.path();
            path.file_name().is_some_and(|n| n != except) && manifest_id_of(&path).as_deref() == Some(id)
        })
    })
}

/// The registry's take-down reason for an installed file, from the last index fetched (no network).
pub fn revoked_reason(id: &str, path: &Path) -> Option<String> {
    let idx = INDEX.lock().unwrap().as_ref().map(|(_, i)| i.clone())?;
    let list = idx.get("revoked")?.as_array()?;
    let mut sha: Option<String> = None;
    list.iter().find_map(|r| {
        if s(r, "id") != id {
            return None;
        }
        let file = s(r, "sha256");
        if !file.is_empty() {
            let got = sha.get_or_insert_with(|| fs::read(path).map(|b| format!("{:x}", Sha256::digest(&b))).unwrap_or_default());
            if !got.eq_ignore_ascii_case(file) {
                return None;
            }
        }
        Some(s(r, "reason").to_owned())
    })
}

/// A content mod's id, from the `horizon-mod.toml` inside it.
pub fn manifest_id_of(path: &Path) -> Option<String> {
    let f = fs::File::open(path).ok()?;
    manifest_id_from_reader(f)
}

pub fn manifest_id_of_bytes(bytes: &[u8]) -> Option<String> {
    manifest_id_from_bytes(bytes)
}

fn manifest_id_from_bytes(bytes: &[u8]) -> Option<String> {
    manifest_id_from_reader(std::io::Cursor::new(bytes))
}

fn manifest_id_from_reader<R: std::io::Read + std::io::Seek>(r: R) -> Option<String> {
    let mut zip = zip::ZipArchive::new(r).ok()?;
    let mut text = String::new();
    zip.by_name("horizon-mod.toml").ok()?.take(64 * 1024).read_to_string(&mut text).ok()?;
    // The top-level `id` (before any [table]); TOML basic or literal string, with an optional comment.
    text.lines()
        .take_while(|l| !l.trim_start().starts_with('['))
        .find_map(|l| {
            let (k, v) = l.split_once('=')?;
            if k.trim() != "id" {
                return None;
            }
            let v = v.trim();
            let q = v.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let rest = &v[1..];
            Some(rest[..rest.find(q)?].to_owned())
        })
    .filter(|id| !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
}

fn enabled_path(game_dir: &Path) -> PathBuf {
    game_dir.join(ENABLED_FILE)
}

/// Turns a content mod on or off in `horizon-mods.txt`, keeping every other line.
pub fn set_enabled(game_dir: &Path, id: &str, on: bool) -> Result<(), String> {
    edit_entry(game_dir, id, Some(on))
}

/// Drops a removed mod's line from `horizon-mods.txt`.
pub fn forget(game_dir: &Path, id: &str) -> Result<(), String> {
    edit_entry(game_dir, id, None)
}

fn edit_entry(game_dir: &Path, id: &str, on: Option<bool>) -> Result<(), String> {
    let path = enabled_path(game_dir);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let key = format!("mod.{id}");
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| l.split_once(':').map(|(k, _)| k.trim()) != Some(key.as_str()))
        .map(str::to_owned)
        .collect();
    if let Some(on) = on {
        lines.push(format!("{key}:{on}"));
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    let tmp = path.with_extension("txt.part");
    fs::write(&tmp, out).and_then(|_| fs::rename(&tmp, &path)).map_err(|e| format!("Couldn't update {ENABLED_FILE}: {e}"))
}

/// Whether `horizon-mods.txt` turns a mod on (mods are off until turned on).
pub fn is_enabled(game_dir: &Path, id: &str) -> bool {
    let key = format!("mod.{id}");
    fs::read_to_string(enabled_path(game_dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once(':'))
        .filter(|(k, _)| k.trim() == key)
        .last()
        .is_some_and(|(_, v)| v.trim() == "true")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn hmod(id: &str) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            z.start_file("horizon-mod.toml", zip::write::SimpleFileOptions::default()).unwrap();
            write!(z, "id = \"{id}\"\nname = \"X\"\nversion = \"1.0.0\"\n").unwrap();
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn manifest_ids_are_read_and_checked() {
        assert_eq!(manifest_id_from_bytes(&hmod("flesh_to_leather")).as_deref(), Some("flesh_to_leather"));
        assert_eq!(manifest_id_from_bytes(&hmod("../evil")), None);
        assert_eq!(manifest_id_from_bytes(b"not a zip"), None);
        let toml = |body: &str| {
            let mut buf = std::io::Cursor::new(Vec::new());
            {
                let mut z = zip::ZipWriter::new(&mut buf);
                z.start_file("horizon-mod.toml", zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(body.as_bytes()).unwrap();
                z.finish().unwrap();
            }
            manifest_id_from_bytes(&buf.into_inner())
        };
        assert_eq!(toml("id = 'lit_id'\n").as_deref(), Some("lit_id"));
        assert_eq!(toml("id = \"x\" # the id\n").as_deref(), Some("x"));
        assert_eq!(toml("name = \"n\"\n[native]\nid = \"inner\"\n"), None);
    }

    #[test]
    fn the_enabled_file_keeps_other_lines() {
        let dir = std::env::temp_dir().join(format!("lumos-hmods-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(ENABLED_FILE), "mod.zoom:true\nkey.zoom.use:key.keyboard.c\nwasm.memory_cap_mib:64\n").unwrap();
        set_enabled(&dir, "salvage", true).unwrap();
        assert!(is_enabled(&dir, "salvage") && is_enabled(&dir, "zoom"));
        set_enabled(&dir, "salvage", false).unwrap();
        assert!(!is_enabled(&dir, "salvage"));
        forget(&dir, "salvage").unwrap();
        let text = fs::read_to_string(dir.join(ENABLED_FILE)).unwrap();
        assert!(!text.contains("salvage") && text.contains("key.zoom.use:key.keyboard.c") && text.contains("wasm.memory_cap_mib:64"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn categories_filter_and_list() {
        let idx = json!({"projects": [
            {"id": "a", "kind": "mod", "categories": ["utility", "hud"]},
            {"id": "b", "kind": "native-mod", "categories": ["utility"]},
            {"id": "c", "kind": "shader", "categories": ["realistic"]}
        ]});
        let p = &idx["projects"][0];
        assert!(in_categories(p, &[]) && in_categories(p, &["HUD".into()]) && in_categories(p, &["utility".into(), "hud".into()]));
        assert!(!in_categories(p, &["realistic".into()]));
        let mods = categories_in(&idx, "mod");
        assert_eq!(mods[0]["items"], json!([{"value": "hud", "label": "Hud"}, {"value": "utility", "label": "Utility"}]));
        assert_eq!(categories_in(&idx, "resourcepack"), json!([]));
    }

    #[test]
    fn revoked_files_and_projects_are_refused() {
        let idx = json!({"revoked": [{"id": "a", "reason": "gone"}, {"id": "b", "sha256": "ff", "reason": "bad file"}]});
        assert_eq!(revoked(&idx, "a", "00").as_deref(), Some("gone"));
        assert_eq!(revoked(&idx, "b", "FF").as_deref(), Some("bad file"));
        assert!(revoked(&idx, "b", "00").is_none());
        assert!(revoked(&idx, "c", "ff").is_none());
    }

    /// Live: Horizon's real registry. Run with --ignored.
    #[test]
    #[ignore]
    fn live_search_and_install() {
        let dir = std::env::temp_dir().join(format!("lumos-hlive-{}", std::process::id()));
        let mods = search(&dir, "", "mod", "downloads", 0, &[]);
        let hits = mods["hits"].as_array().expect("hits");
        assert!(mods["total_hits"].as_u64().unwrap() > 50, "{mods}");
        assert!(hits.iter().any(|h| h["native"] == true) && hits.iter().all(|h| h["source"] == "horizon"));
        let shaders = search(&dir, "", "shader", "downloads", 0, &[]);
        assert!(!shaders["hits"].as_array().unwrap().is_empty());
        let found = search(&dir, "leather", "mod", "relevance", 0, &[]);
        assert_eq!(found["hits"][0]["slug"], "flesh_to_leather", "{found}");
        assert_eq!(install(&dir, "flesh_to_leather").unwrap(), "flesh_to_leather-1.0.0.hmod");
        assert!(dir.join("horizon-mods/flesh_to_leather-1.0.0.hmod").is_file());
        assert!(is_enabled(&dir, "flesh_to_leather"));
        assert_eq!(search(&dir, "leather", "mod", "relevance", 0, &[])["hits"][0]["installed"], true);
        assert!(install(&dir, "zoom").unwrap_err().contains("native Rust mod"));
        // A signed project: its minisign signature is checked against the author's key.
        assert!(install(&dir, "compass_bar").is_ok());
        assert!(install(&dir, "notenoughanimations").is_err()); // taken down by the registry
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_registry_release_urls_are_accepted() {
        assert!(url_ok("https://github.com/horizon-rust/horizon-mods/releases/download/x-1/x.hmod"));
        assert!(!url_ok("http://github.com/a/b/releases/download/x/x.hmod"));
        assert!(!url_ok("https://evil.example/releases/download/x.hmod"));
    }
}
