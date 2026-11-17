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
    pub short: String,
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
    Some(Build { short, asset, sha256: s("sha256").to_ascii_lowercase(), size: b.get("size").and_then(Value::as_u64).unwrap_or(0), mods })
}

/// The build for this platform that has `mod_id`, if one was published.
pub fn build_for_mod(mod_id: &str) -> Option<Build> {
    let plat = platform()?;
    let idx = index()?;
    idx.get("builds")?
        .as_array()?
        .iter()
        .filter(|b| b.get("platform").and_then(Value::as_str) == Some(plat))
        .filter_map(parse_build)
        .find(|b| b.mods.iter().any(|(id, _, _)| id == mod_id))
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
    (st.build.len() == 8 && !st.mods.is_empty()).then_some(st)
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
    (client.is_file() && prepare.is_file() && root.join("research-lock.json").is_file()).then_some((client, prepare, root, st.build))
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
    let b = build_for_mod(mod_id).ok_or("This native mod hasn't been built for Lumos yet. Try again later.")?;
    let mut st = read_state(game_dir).unwrap_or_default();
    if !st.mods.is_empty() && st.build != b.short {
        let names: Vec<String> = st.mods.iter().map(|m| st.known.iter().find(|k| &k.0 == m).map_or(m.clone(), |k| k.1.clone())).collect();
        return Err(format!(
            "This mod is made for a different Horizon version than {} in this instance. Put it in a new Horizon instance, or remove those first.",
            names.join(", ")
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
    }
}
