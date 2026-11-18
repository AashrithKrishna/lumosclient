//! Horizon's native Rust mods, as ready-to-play games built in the cloud
//! (.github/workflows/lumos-native.yml). A native mod is compiled into the game itself, for the
//! one Horizon commit it patches, so a Horizon instance with native mods plays a build made for
//! that commit with them inside, instead of the bundled engine.
//!
//! Each instance records its choice in `lumos-native.json` beside `options.txt`:
//! `{"build": "<short commit>", "mods": [ids the player installed]}`. The build's other mods
//! are switched off in `horizon-mods.txt` (those that let themselves be switched off).

use crate::{horizon_mods, net};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const INDEX_URL: &str = "https://github.com/AashrithKrishna/lumosclient/releases/download/native-builds/native-index.json";
const FRESH: Duration = Duration::from_secs(600);
const MAX_BUILD: u64 = 600 << 20;
const STATE_FILE: &str = "lumos-native.json";

static INDEX: Mutex<Option<(Instant, Arc<Value>)>> = Mutex::new(None);

/// This computer's platform name in the builds, when builds exist for it.
pub fn platform() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-arm64")
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some("windows-x64")
    } else {
        None
    }
}

/// The list of built games, fetched at most every ten minutes. `None` when it can't be read
/// (no builds published yet, or offline): native mods then show as not built yet.
pub fn index() -> Option<Arc<Value>> {
    if let Some((at, idx)) = INDEX.lock().unwrap().as_ref() {
        if at.elapsed() < FRESH {
            return Some(idx.clone());
        }
    }
    let idx = net::get_json(INDEX_URL).ok().filter(|v| v.get("schema").and_then(Value::as_u64) == Some(1))?;
    let idx = Arc::new(idx);
    *INDEX.lock().unwrap() = Some((Instant::now(), idx.clone()));
    Some(idx)
}

#[derive(Clone, Debug)]
pub struct Build {
    /// What instances record and the folder name: the short commit, or "all-<short>" for the
    /// combined build (every native mod retargeted to one commit).
    pub short: String,
    pub combined: bool,
    pub asset: String,
    pub sha256: String,
    pub size: u64,
    /// (id, name, can_turn_off)
    pub mods: Vec<(String, String, bool)>,
}

fn parse_build(b: &Value) -> Option<Build> {
    let s = |k: &str| b.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let short = s("short");
    let asset = s("asset");
    let ok_short = short.len() == 8 && short.chars().all(|c| c.is_ascii_hexdigit());
    if !ok_short || !crate::safe_file_name(&asset) || !asset.ends_with(".zip") {
        return None;
    }
    let combined = b.get("combined").and_then(Value::as_bool).unwrap_or(false);
    let short = if combined { format!("all-{short}") } else { short };
    let mods = b
        .get("mods")?
        .as_array()?
        .iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_owned();
            let name = m.get("name").and_then(Value::as_str).unwrap_or(&id).to_owned();
            Some((id, name, m.get("can_turn_off").and_then(Value::as_bool).unwrap_or(false)))
        })
        .collect();
    Some(Build { short, combined, asset, sha256: s("sha256").to_ascii_lowercase(), size: b.get("size").and_then(Value::as_u64).unwrap_or(0), mods })
}

/// True when the combined (all-in-one) game has `mod_id`, so it mixes with everything in it.
pub fn in_combined(mod_id: &str) -> bool {
    builds().iter().any(|b| b.combined && b.mods.iter().any(|(id, _, _)| id == mod_id))
}

/// The build for this platform that has `mod_id`, if one was published.
pub fn build_for_mod(mod_id: &str) -> Option<Build> {
    builds().into_iter().find(|b| b.mods.iter().any(|(id, _, _)| id == mod_id))
}

/// This platform's builds, the combined one first (it lets any mix of native mods share one
/// instance), then the per-commit ones.
fn builds() -> Vec<Build> {
    let (Some(plat), Some(idx)) = (platform(), index()) else { return Vec::new() };
    let mut v: Vec<Build> = idx
        .get("builds")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|b| b.get("platform").and_then(Value::as_str) == Some(plat)).filter_map(parse_build).collect())
        .unwrap_or_default();
    v.sort_by_key(|b| (!b.combined, std::cmp::Reverse(b.mods.len())));
    v
}

/// A build that has every one of `ids` (the combined one when it does).
pub fn build_with_all(ids: &[String]) -> Option<Build> {
    builds().into_iter().find(|b| ids.iter().all(|i| b.mods.iter().any(|(id, _, _)| id == i)))
}

/// Moves an instance onto the combined build when that has all of its native mods (so more can
/// be added later). Returns true when it moved. Downloads the build if needed.
pub fn upgrade_to_combined(base_dir: &Path, game_dir: &Path, report: &dyn Fn(&str)) -> Result<bool, String> {
    let Some(mut st) = read_state(game_dir) else { return Ok(false) };
    let Some(b) = build_with_all(&st.mods).filter(|b| b.combined && b.short != st.build) else { return Ok(false) };
    ensure_build(base_dir, &b, report)?;
    st.build = b.short.clone();
    st.known = b.mods.clone();
    write_state(game_dir, &st)?;
    apply_switches(game_dir, &st)?;
    Ok(true)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InstanceNative {
    pub build: String,
    pub mods: Vec<String>,
    /// Names and whether each can be switched off, kept so the Mods tab works offline.
    #[serde(default)]
    pub known: Vec<(String, String, bool)>,
}

pub fn read_state(game_dir: &Path) -> Option<InstanceNative> {
    let st: InstanceNative = serde_json::from_str(&fs::read_to_string(game_dir.join(STATE_FILE)).ok()?).ok()?;
    let key = st.build.strip_prefix("all-").unwrap_or(&st.build);
    (key.len() == 8 && key.chars().all(|c| c.is_ascii_hexdigit()) && !st.mods.is_empty()).then_some(st)
}

fn write_state(game_dir: &Path, st: &InstanceNative) -> Result<(), String> {
    let path = game_dir.join(STATE_FILE);
    if st.mods.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    let text = serde_json::to_string_pretty(st).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("Couldn't save {STATE_FILE}: {e}"))
}

/// Where a build is unpacked.
pub fn build_dir(base_dir: &Path, short: &str) -> PathBuf {
    base_dir.join("native").join(format!("{short}-{}", platform().unwrap_or("unknown")))
}

/// Downloads and unpacks a build once (checked against its SHA-256), then reuses it.
pub fn ensure_build(base_dir: &Path, b: &Build, report: &dyn Fn(&str)) -> Result<PathBuf, String> {
    let dir = build_dir(base_dir, &b.short);
    let stamp = dir.join(".sha256");
    if fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == b.sha256) {
        return Ok(dir);
    }
    if b.sha256.len() != 64 || b.size == 0 || b.size > MAX_BUILD {
        return Err("The native build's entry is malformed, so it wasn't installed.".into());
    }
    report(&format!("Downloading the Horizon build for this mod ({} MB)...", b.size / (1 << 20)));
    let url = format!("https://github.com/AashrithKrishna/lumosclient/releases/download/native-builds/{}", b.asset);
    let bytes = net::get_bytes(&url, b.size)?;
    if bytes.len() as u64 != b.size || format!("{:x}", Sha256::digest(&bytes)) != b.sha256 {
        return Err("The native build failed its integrity check, so it wasn't installed.".into());
    }
    report("Unpacking...");
    let tmp = dir.with_extension("part");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("The native build can't be opened: {e}"))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = f.enclosed_name() else { return Err("The native build has an unsafe file name.".into()) };
        let out = tmp.join(rel);
        if f.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut data = Vec::new();
        f.read_to_end(&mut data).map_err(|e| e.to_string())?;
        fs::write(&out, &data).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = f.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode & 0o755));
        }
    }
    let _ = fs::remove_dir_all(&dir);
    fs::rename(&tmp, &dir).map_err(|e| format!("Couldn't save the native build: {e}"))?;
    fs::write(&stamp, &b.sha256).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn exe(dir: &Path, name: &str) -> PathBuf {
    dir.join("bin").join(if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() })
}

/// What a Horizon launch of this instance uses: (game, preparer, support-files folder) of its
/// native build, or `None` for the bundled engine.
pub fn launch_parts(base_dir: &Path, game_dir: &Path) -> Option<(PathBuf, PathBuf, PathBuf, String)> {
    let st = read_state(game_dir)?;
    let dir = build_dir(base_dir, &st.build);
    let (client, prepare, root) = (exe(&dir, "horizon-client"), exe(&dir, "horizon-prepare"), dir.join("horizon-root"));
    if !(client.is_file() && prepare.is_file() && root.join("research-lock.json").is_file()) {
        return None;
    }
    pin_manifest(&root);
    Some((client, prepare, root, st.build))
}

/// An older build's preparer only accepts the Minecraft version file it was made with, and
/// Mojang has republished 26.3's since. Saves that exact file (checked by its SHA-1) where the
/// preparer looks first, so it never trips over the launcher's newer copy.
fn pin_manifest(root: &Path) {
    let Ok(lock) = fs::read_to_string(root.join("research-lock.json")) else { return };
    let Ok(lock) = serde_json::from_str::<Value>(&lock) else { return };
    let (Some(ver), Some(sha1)) = (lock["minecraft"]["version"].as_str(), lock["minecraft"]["manifest_sha1"].as_str()) else { return };
    let safe = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_');
    if !safe(ver) || sha1.len() != 40 || !sha1.chars().all(|c| c.is_ascii_hexdigit()) {
        return;
    }
    let dest = root.join(".local").join("minecraft").join(ver).join("manifest.json");
    if net::sha1_file(&dest).is_some_and(|s| s.eq_ignore_ascii_case(sha1)) {
        return;
    }
    if let Some(dir) = dest.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let url = format!("https://piston-meta.mojang.com/v1/packages/{sha1}/{ver}.json");
    let _ = net::download_verified(&url, &dest, Some(sha1), None);
}

/// Writes the switches: the player's native mods on, the build's others off.
fn apply_switches(game_dir: &Path, st: &InstanceNative) -> Result<(), String> {
    for (id, _, _) in &st.known {
        horizon_mods::set_enabled(game_dir, id, st.mods.contains(id))?;
    }
    Ok(())
}

/// Installs a native mod into an instance. Returns a message for the player.
pub fn install(base_dir: &Path, game_dir: &Path, mod_id: &str, report: &dyn Fn(&str)) -> Result<String, String> {
    if platform().is_none() {
        return Err("Native mods are built for Apple silicon Macs and Windows x64 only.".into());
    }
    let mut st = read_state(game_dir).unwrap_or_default();
    // Prefer a build that has this mod AND everything already in the instance (the combined one).
    let mut wanted = st.mods.clone();
    wanted.push(mod_id.to_owned());
    let b = build_with_all(&wanted)
        .or_else(|| build_for_mod(mod_id))
        .ok_or("This native mod hasn't been built for Lumos yet. Try again later.")?;
    if !st.mods.is_empty() && st.build != b.short && !st.mods.iter().all(|m| b.mods.iter().any(|(id, _, _)| id == m)) {
        let names: Vec<String> = st.mods.iter().map(|m| st.known.iter().find(|k| &k.0 == m).map_or(m.clone(), |k| k.1.clone())).collect();
        // Name the mods whose removal would let this one in (they're the ones no build shares with it).
        let blockers: Vec<String> = st.mods.iter().filter(|m| build_with_all(&[(*m).clone(), mod_id.to_owned()]).is_none())
            .map(|m| st.known.iter().find(|k| &k.0 == m).map_or(m.clone(), |k| k.1.clone())).collect();
        if !blockers.is_empty() && blockers.len() < st.mods.len() {
            return Err(format!("This mod can't share a game with {} in this instance. Remove {} first, or put this mod in a new Horizon instance.",
                blockers.join(", "), if blockers.len() == 1 { "it" } else { "them" }));
        }
        return Err(format!(
            "This mod is in a different native build than {} in this instance, so they can't be mixed yet. {}",
            names.join(", "),
            if builds().iter().any(|b| b.combined) {
                "Put it in a new Horizon instance, or remove those first."
            } else {
                "Lumos is building one game with every native mod; once it's out (it updates on its own) you can mix any of them."
            }
        ));
    }
    ensure_build(base_dir, &b, report)?;
    st.build = b.short.clone();
    st.known = b.mods.clone();
    if !st.mods.iter().any(|m| m == mod_id) {
        st.mods.push(mod_id.to_owned());
    }
    fs::create_dir_all(game_dir).map_err(|e| e.to_string())?;
    write_state(game_dir, &st)?;
    apply_switches(game_dir, &st)?;
    let always_on: Vec<&str> = b.mods.iter().filter(|(id, _, off)| !off && !st.mods.contains(id)).map(|(_, n, _)| n.as_str()).collect();
    let name = b.mods.iter().find(|m| m.0 == mod_id).map_or(mod_id, |m| m.1.as_str());
    Ok(if always_on.is_empty() {
        format!("Installed {name} (native)")
    } else {
        format!("Installed {name} (native). Built into the same game and always on: {}", always_on.join(", "))
    })
}

/// Removes a native mod from an instance; the last one takes the instance back to the bundled engine.
pub fn remove(game_dir: &Path, mod_id: &str) -> Result<(), String> {
    let Some(mut st) = read_state(game_dir) else { return Ok(()) };
    st.mods.retain(|m| m != mod_id);
    let _ = horizon_mods::set_enabled(game_dir, mod_id, false);
    write_state(game_dir, &st)?;
    if st.mods.is_empty() {
        for (id, _, _) in &st.known {
            let _ = horizon_mods::forget(game_dir, id);
        }
    }
    Ok(())
}

/// Native mods for the instance's Mods tab: (id, name, on).
pub fn list(game_dir: &Path) -> Vec<(String, String, bool)> {
    let Some(st) = read_state(game_dir) else { return Vec::new() };
    st.mods
        .iter()
        .map(|id| {
            let name = st.known.iter().find(|k| &k.0 == id).map_or(id.clone(), |k| k.1.clone());
            (id.clone(), name, horizon_mods::is_enabled(game_dir, id))
        })
        .collect()
}

/// Ids of native mods this instance has.
pub fn installed_ids(game_dir: &Path) -> Vec<String> {
    read_state(game_dir).map(|s| s.mods).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trip_and_removal() {
        let dir = std::env::temp_dir().join(format!("lumos-native-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let st = InstanceNative {
            build: "6f8c7a0e".into(),
            mods: vec!["zoom".into(), "minimap".into()],
            known: vec![("zoom".into(), "Zoom".into(), true), ("minimap".into(), "Minimap".into(), true), ("x".into(), "X".into(), false)],
        };
        write_state(&dir, &st).unwrap();
        apply_switches(&dir, &st).unwrap();
        assert!(horizon_mods::is_enabled(&dir, "zoom") && !horizon_mods::is_enabled(&dir, "x"));
        assert_eq!(list(&dir).len(), 2);
        remove(&dir, "zoom").unwrap();
        assert_eq!(installed_ids(&dir), vec!["minimap".to_owned()]);
        remove(&dir, "minimap").unwrap();
        assert!(read_state(&dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Live: installs Zoom from the published cloud build. Run with --ignored.
    #[test]
    #[ignore]
    fn live_install_native_zoom() {
        let base = std::env::temp_dir().join(format!("lumos-native-live-{}", std::process::id()));
        let game = base.join("instance");
        let msg = install(&base, &game, "zoom", &|m| eprintln!("{m}")).expect("install");
        eprintln!("{msg}");
        let (client, prepare, root, short) = launch_parts(&base, &game).expect("parts");
        assert!(client.is_file() && prepare.is_file() && root.join("exporters/manifest.json").is_file(), "{short}");
        assert!(horizon_mods::is_enabled(&game, "zoom") && !horizon_mods::is_enabled(&game, "minimap"));
        let out = std::process::Command::new(&client).arg("--help").output().expect("runs");
        assert!(out.status.success() || !out.stdout.is_empty() || !out.stderr.is_empty(), "game binary doesn't run");
        // A second mod from the same build joins it.
        install(&base, &game, "minimap", &|_| {}).expect("second");
        assert_eq!(installed_ids(&game), vec!["zoom".to_owned(), "minimap".to_owned()]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn builds_with_odd_names_are_ignored() {
        let ok = serde_json::json!({"short": "6f8c7a0e", "asset": "lumos-native-6f8c7a0e-macos-arm64.zip", "sha256": "ab", "size": 3, "mods": []});
        assert!(parse_build(&ok).is_some());
        let bad = serde_json::json!({"short": "../x", "asset": "a.zip", "mods": []});
        assert!(parse_build(&bad).is_none());
        let bad2 = serde_json::json!({"short": "6f8c7a0e", "asset": "../../a.zip", "mods": []});
        assert!(parse_build(&bad2).is_none());
        let all = serde_json::json!({"short": "9f42064c", "combined": true, "asset": "lumos-native-all-9f42064c-macos-arm64.zip", "sha256": "ab", "size": 3, "mods": []});
        let b = parse_build(&all).unwrap();
        assert!(b.combined && b.short == "all-9f42064c");
        // An instance on the combined build reads back fine.
        let dir = std::env::temp_dir().join(format!("lumos-native-all-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        write_state(&dir, &InstanceNative { build: "all-9f42064c".into(), mods: vec!["zoom".into()], known: vec![] }).unwrap();
        assert_eq!(read_state(&dir).unwrap().build, "all-9f42064c");
        let _ = fs::remove_dir_all(&dir);
    }
}
