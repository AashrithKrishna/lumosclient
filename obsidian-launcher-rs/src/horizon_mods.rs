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
    let idx = Arc::new(idx);
    *INDEX.lock().unwrap() = Some((Instant::now(), idx.clone()));
    Ok(idx)
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Which registry kinds a Discover tab shows.
fn kinds_for(tab: &str) -> &'static [&'static str] {
    match tab {
        "shader" => &["shader"],
        "resourcepack" => &["resourcepack"],
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

/// Discover results in the shape the page already renders for Modrinth, plus `source`,
/// `native` (listed, not installable) and `installed` for this instance.
pub fn search(game_dir: &Path, query: &str, tab: &str, sort: &str, offset: usize) -> Value {
    let idx = match index() {
        Ok(i) => i,
        Err(e) => return json!({"hits": [], "total_hits": 0, "error": e, "source": "horizon"}),
    };
    let kinds = kinds_for(tab);
    let q = query.trim().to_lowercase();
    let installed = installed_files(game_dir);
    let mut hits: Vec<(&Value, &Value)> = idx
        .get("projects")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter(|p| kinds.contains(&s(p, "kind")))
        .filter_map(|p| current_version(p).map(|v| (p, v)))
        .filter(|(p, v)| revoked(&idx, s(p, "id"), s(&v["file"], "sha256")).is_none())
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
                "signed": p.get("key").is_some_and(|k| !k.is_null()),
                "installed": installed.iter().any(|f| f == file),
                "view_url": format!("{SITE}/{}", s(p, "id")),
            })
        })
        .collect();
    json!({"hits": page, "total_hits": total, "source": "horizon"})
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

/// Installs a registry project into the instance. Returns the saved file name.
pub fn install(game_dir: &Path, id: &str) -> Result<String, String> {
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
    let folder = folder_for(kind).ok_or("Lumos can't install that kind of Horizon project yet.")?;
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
    let dir = game_dir.join(folder);
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    if kind == "mod" {
        let manifest_id = manifest_id_from_bytes(&bytes).ok_or(format!("{name} isn't a valid Horizon mod."))?;
        if manifest_id != id {
            return Err(format!("{name} says it is \"{manifest_id}\", not \"{id}\", so it wasn't installed."));
        }
        // Replace an older version of the same mod.
        for old in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let path = old.path();
            if path.file_name().is_some_and(|n| n != name) && manifest_id_of(&path).as_deref() == Some(id) {
                let _ = fs::remove_file(&path);
            }
        }
    }
    let tmp = dir.join(format!(".{name}.part"));
    fs::write(&tmp, &bytes).map_err(|e| format!("Couldn't save {name}: {e}"))?;
    fs::rename(&tmp, dir.join(name)).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Couldn't save {name}: {e}")
    })?;
    if kind == "mod" {
        set_enabled(game_dir, id, true)?;
    }
    Ok(name.to_owned())
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
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "id").then(|| v.trim().trim_matches('"').to_owned())
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
        let mods = search(&dir, "", "mod", "downloads", 0);
        let hits = mods["hits"].as_array().expect("hits");
        assert!(mods["total_hits"].as_u64().unwrap() > 50, "{mods}");
        assert!(hits.iter().any(|h| h["native"] == true) && hits.iter().all(|h| h["source"] == "horizon"));
        let shaders = search(&dir, "", "shader", "downloads", 0);
        assert!(!shaders["hits"].as_array().unwrap().is_empty());
        let found = search(&dir, "leather", "mod", "relevance", 0);
        assert_eq!(found["hits"][0]["slug"], "flesh_to_leather", "{found}");
        assert_eq!(install(&dir, "flesh_to_leather").unwrap(), "flesh_to_leather-1.0.0.hmod");
        assert!(dir.join("horizon-mods/flesh_to_leather-1.0.0.hmod").is_file());
        assert!(is_enabled(&dir, "flesh_to_leather"));
        assert_eq!(search(&dir, "leather", "mod", "relevance", 0)["hits"][0]["installed"], true);
        assert!(install(&dir, "zoom").unwrap_err().contains("native Rust mod"));
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
