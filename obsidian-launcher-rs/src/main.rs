//! LumosClient launcher: a tao + wry desktop app. The UI (ui.html) talks to this
//! backend over `window.ipc.postMessage(JSON)`; the backend answers by evaluating
//! `window.renderState(...)`, `window.onTask(...)` and friends in the page.

// No console window behind the launcher on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::{WebContext, WebView, WebViewBuilder};

mod capes;
mod elyby;
mod forge;
mod horizon_mods;
mod hud_config;
mod modpack;
mod mod_plan;
mod native_builds;
mod installer;
mod net;
// feat/updater: self-update, crash explainer, repair instance
mod crash_help;
mod repair;
mod updater;
// feat/discover: Content Hub / Content Browser (Modrinth filters, CurseForge, updates, suggestions)
mod curseforge;
mod discover;
// The in-game Discover's requests between runs (lumos-mods/lumos_discover).
mod ingame_requests;
// Instances from other launchers (import.rs, IMPORT-PLAN.md); sqlite_ro reads the Modrinth App's database.
mod import;
mod sqlite_ro;

const HORIZON_ID: &str = "Horizon 26.3 (Rust)";
const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ───────────────────────── Config ─────────────────────────

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    id: String,
    name: String,
    #[serde(default)]
    world: String,
    /// Unix epoch milliseconds of the last successful launch (0 = never).
    #[serde(default)]
    last_played: u64,
    /// Total seconds played in this profile.
    #[serde(default)]
    playtime_secs: u64,
    /// Pixel icon key chosen in the UI (empty = pick automatically from the name).
    #[serde(default)]
    icon: String,
    /// Game version this instance launches (empty = use the global selection).
    #[serde(default)]
    version: String,
    /// This instance's own settings; `None` uses the launcher-wide setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolution_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolution_height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fullscreen: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    jvm_args: Option<String>,
}

impl Profile {
    /// The launcher-wide settings with this instance's own ones on top.
    fn apply_to(&self, cfg: &mut LauncherConfig) {
        if let Some(m) = self.memory_mb {
            cfg.memory_mb = m;
        }
        if let Some(w) = self.resolution_width {
            cfg.resolution_width = w;
        }
        if let Some(h) = self.resolution_height {
            cfg.resolution_height = h;
        }
        if let Some(f) = self.fullscreen {
            cfg.fullscreen = f;
        }
        if let Some(j) = &self.jvm_args {
            cfg.jvm_args = j.clone();
        }
    }
}

/// Icon keys are short lowercase identifiers drawn by the UI; reject anything else.
fn valid_icon_key(icon: &str) -> bool {
    icon.len() <= 24 && icon.chars().all(|c| c.is_ascii_lowercase() || c == '-')
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ServerEntry {
    id: String,
    name: String,
    address: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LauncherConfig {
    #[serde(default = "default_player_name")]
    player_name: String,
    /// The official launcher's folder (read only, used to reuse versions it installed).
    #[serde(default)]
    minecraft_dir: String,
    /// Optional Java override; empty = LumosClient downloads the right Java itself.
    #[serde(default)]
    java_home: String,
    #[serde(default)]
    active_profile_id: String,
    #[serde(default = "default_version")]
    selected_version: String,
    #[serde(default = "default_width")]
    resolution_width: u32,
    #[serde(default = "default_height")]
    resolution_height: u32,
    #[serde(default)]
    fullscreen: bool,
    #[serde(default = "default_memory")]
    memory_mb: u32,
    #[serde(default)]
    jvm_args: String,
    #[serde(default = "default_true", alias = "emberHudEnabled")]
    lumos_hud_enabled: bool,
    /// Lumos capes in game (the LumosCapes mod in Fabric 1.21.11 instances).
    #[serde(default = "default_true")]
    lumos_capes_enabled: bool,
    /// Horizon 26.3's controller support. Off unless the player turns it on: on Windows a
    /// device that only looks like a controller made the game hide the mouse pointer in menus.
    #[serde(default)]
    horizon_controller: bool,
    #[serde(default = "default_accent")]
    ui_accent: String,
    #[serde(default = "default_theme")]
    ui_theme: String,
    #[serde(default = "default_true")]
    ui_motion: bool,
    #[serde(default)]
    hide_on_launch: bool,
    #[serde(default = "default_home_layout")]
    ui_home_layout: String,
    /// "offline" or "elyby".
    #[serde(default = "default_account_kind")]
    account_kind: String,
    #[serde(default)]
    ely_session: Option<elyby::ElySession>,
    #[serde(default)]
    ely_client_token: String,
    #[serde(default)]
    profiles: Vec<Profile>,
    #[serde(default)]
    servers: Vec<ServerEntry>,
    /// feat/discover: the player's own CurseForge API key (only ever in launcher.json, which is
    /// owner-only; never sent to the page or logged).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    curseforge_api_key: String,
    /// The getting-started guide was shown once (finished or skipped). Settings files from
    /// before the guide count as seen: those players already know the launcher.
    #[serde(default)]
    guide_seen: bool,
    /// Install a new LumosClient by itself when one is found at start (never while a game runs).
    #[serde(default = "default_true")]
    auto_update: bool,
}

fn default_player_name() -> String {
    "Player".into()
}

fn default_version() -> String {
    "Fabric 1.21.11".into()
}
fn default_width() -> u32 {
    1280
}
fn default_height() -> u32 {
    720
}
fn default_memory() -> u32 {
    4096
}
fn default_true() -> bool {
    true
}
fn default_accent() -> String {
    "blaze".into()
}
fn default_theme() -> String {
    "dark".into()
}
fn default_home_layout() -> String {
    "launchpad".into()
}
fn default_account_kind() -> String {
    "offline".into()
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn system_memory_mb() -> u64 {
    if cfg!(target_os = "macos") {
        return Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|b| b / 1024 / 1024)
            .unwrap_or(0);
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: `status` is a properly sized, writable MEMORYSTATUSEX with dwLength set.
        if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
            return status.ullTotalPhys / 1024 / 1024;
        }
    }
    if cfg!(target_os = "linux") {
        return fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|m| {
                m.lines()
                    .find(|l| l.starts_with("MemTotal:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|kb| kb.parse::<u64>().ok())
            })
            .map(|kb| kb / 1024)
            .unwrap_or(0);
    }
    0
}

fn valid_player_name(n: &str) -> bool {
    (3..=16).contains(&n.len()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Offline-mode UUID (stable per name; 32 hex).
fn offline_uuid_hex(name: &str) -> String {
    let input = format!("OfflinePlayer:{name}");
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x84222325cbf29ce4;
    for &b in input.as_bytes() {
        h1 = (h1 ^ u64::from(b)).wrapping_mul(0x100000001b3);
        h2 = (h2 ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    format!("{h1:016x}{h2:016x}")
}

// ───────────────────────── Bundle / dev paths ─────────────────────────

/// `Lumos.app/Contents/Resources` (macOS) or `<exe dir>/resources` elsewhere.
fn bundle_resources() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    // `resources` first: on Windows (case-insensitive) a stray "Resources" beside the exe must not win.
    [dir.join("resources"), dir.join("../Resources")].into_iter().find(|p| p.is_dir())
}

/// The source checkout this binary was built from (only exists on the developer's machine).
fn dev_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

fn resolve_binary(name: &str) -> Option<PathBuf> {
    let file = exe_name(name);
    bundle_resources()
        .map(|r| r.join("bin").join(&file))
        .into_iter()
        .chain([dev_repo_root().join("target/release").join(&file), dev_repo_root().join("target/debug").join(&file)])
        .find(|p| p.is_file())
}

enum HorizonRoot {
    /// Bundled support files, copied into a writable folder before use.
    Bundled(PathBuf),
    /// Developer checkout, used in place.
    Dev(PathBuf),
}

fn horizon_support() -> Option<HorizonRoot> {
    if let Some(b) = bundle_resources().map(|r| r.join("horizon-root")).filter(|p| p.join("research-lock.json").is_file()) {
        return Some(HorizonRoot::Bundled(b));
    }
    let dev = dev_repo_root();
    dev.join("research-lock.json").is_file().then_some(HorizonRoot::Dev(dev))
}

fn bundled_file(rel: &str, dev_rel: &str) -> Option<PathBuf> {
    bundle_resources()
        .map(|r| r.join(rel))
        .into_iter()
        .chain([PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(dev_rel)])
        .find(|p| p.is_file())
}

/// The LumosHUD jar built for a Minecraft version (lumoshud/build.gradle, `mc_versions`).
fn bundled_hud(mc: &str) -> Option<PathBuf> {
    bundled_file(&format!("mods/lumoshud-{mc}.jar"), &format!("lumoshud/lumoshud-{mc}.jar"))
}

fn bundled_capes_mod() -> Option<PathBuf> {
    bundled_file(&format!("mods/{}", capes::MOD_JAR), &format!("lumoscapes/build/libs/{}", capes::MOD_JAR))
}

fn bundled_authlib() -> Option<PathBuf> {
    bundled_file(
        &format!("authlib-injector-{}.jar", elyby::AUTHLIB_VERSION),
        &format!("vendor/authlib-injector-{}.jar", elyby::AUTHLIB_VERSION),
    )
}

/// Where removed files go, as the player knows it.
const TRASH_NAME: &str = if cfg!(target_os = "macos") {
    "the Trash"
} else if cfg!(windows) {
    "the Recycle Bin"
} else {
    "LumosClient's trash folder"
};

/// Moves a file or folder somewhere recoverable instead of deleting it: the macOS Trash,
/// the Windows Recycle Bin, or LumosClient's own trash folder elsewhere.
fn move_to_trash(base_dir: &Path, path: &Path, label: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = (base_dir, label);
        trash::delete(path).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let trash = if cfg!(target_os = "macos") {
            dirs::home_dir().unwrap_or_default().join(".Trash")
        } else {
            base_dir.join("trash")
        };
        fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
        let mut dest = trash.join(label);
        if dest.exists() {
            dest = trash.join(format!("{label} {}", now_millis()));
        }
        fs::rename(path, &dest).map_err(|e| e.to_string())
    }
}

/// Opens a folder or URL with the OS's default handler.
fn open_external(target: &str) {
    let _ = if cfg!(target_os = "macos") {
        Command::new("open").arg(target).spawn()
    } else if cfg!(windows) {
        Command::new("explorer").arg(target).spawn()
    } else {
        Command::new("xdg-open").arg(target).spawn()
    };
}

// ───────────────────────── Instance content ─────────────────────────

/// Content folders inside an instance that the UI is allowed to list/modify. A Horizon 26.3
/// instance keeps its (Rust) mods in `horizon-mods/`, turned on in `horizon-mods.txt`.
fn content_folder(kind: &str, horizon: bool) -> Option<&'static str> {
    match kind {
        "mods" if horizon => Some(horizon_mods::MODS_DIR),
        "mods" => Some("mods"),
        "resourcepacks" => Some("resourcepacks"),
        "shaderpacks" => Some("shaderpacks"),
        _ => None,
    }
}

/// File types each content folder takes when the player adds their own files.
fn content_extension_ok(kind: &str, file: &str, horizon: bool) -> bool {
    let lower = file.to_ascii_lowercase();
    match kind {
        "mods" if horizon => lower.ends_with(".hmod"),
        "mods" => lower.ends_with(".jar"),
        "shaderpacks" if horizon => lower.ends_with(".zip") || lower.ends_with(".hshader"),
        "resourcepacks" | "shaderpacks" => lower.ends_with(".zip"),
        _ => false,
    }
}

/// How the Mods tab names a native mod (not a file) in toggle/remove requests.
const NATIVE_PREFIX: &str = "native:";

/// Shown when a request names an instance that was deleted in the meantime.
const GONE_INSTANCE: &str = "That instance no longer exists, so nothing was changed.";

/// Largest file the player can add through the launcher (bigger ones go in via the folder).
const MAX_IMPORT_BYTES: usize = 256 * 1024 * 1024;

/// Rejects anything that is not one plain file name on every OS: no separators or `..`, and
/// nothing Windows reads as a drive, stream or device (`C:x`, `a:b`, `NUL`, trailing dots).
fn safe_file_name(name: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
        "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.ends_with('.')
        && !name.ends_with(' ')
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'))
        && !RESERVED.contains(&stem.as_str())
}

fn list_content_json(game_dir: &Path, kind: &str, profile_id: &str, horizon: bool) -> String {
    let mut items: Vec<serde_json::Value> = Vec::new();
    let horizon_mods = horizon && kind == "mods";
    if let Some(folder) = content_folder(kind, horizon) {
        if let Ok(rd) = fs::read_dir(game_dir.join(folder)) {
            for entry in rd.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let meta = entry.metadata().ok();
                let modified = meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                // Horizon mods are switched in horizon-mods.txt by their manifest id, not renamed.
                let mod_id = horizon_mods.then(|| horizon_mods::manifest_id_of(&entry.path())).flatten();
                if horizon_mods && mod_id.is_none() && !meta.as_ref().is_some_and(|m| m.is_dir()) {
                    continue; // not a Horizon mod (e.g. a stray file)
                }
                // A file Horizon's registry took down is switched off, with the reason shown.
                let revoked = mod_id.as_ref().and_then(|id| horizon_mods::revoked_reason(id, &entry.path()));
                if let (Some(id), Some(_)) = (&mod_id, &revoked) {
                    if horizon_mods::is_enabled(game_dir, id) {
                        let _ = horizon_mods::set_enabled(game_dir, id, false);
                    }
                }
                let enabled = match &mod_id {
                    Some(id) => horizon_mods::is_enabled(game_dir, id),
                    None => !name.ends_with(".disabled"),
                };
                items.push(json!({
                    "file": name,
                    "enabled": enabled,
                    "revoked": revoked,
                    "isDir": meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                    "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
                    "modified": modified,
                }));
            }
        }
    }
    items.sort_by_key(|a| a["file"].as_str().unwrap_or("").to_lowercase());
    // Native mods are built into the instance's game, not files: listed with their own switch.
    if horizon_mods {
        for (id, name, on) in native_builds::list(game_dir) {
            items.push(json!({"file": format!("{NATIVE_PREFIX}{id}"), "label": name, "enabled": on, "native": true, "isDir": false, "size": 0, "modified": 0}));
        }
    }
    json!({ "kind": kind, "profileId": profile_id, "horizon": horizon, "items": items }).to_string()
}

fn push_content(proxy: &EventLoopProxy<UserEvent>, game_dir: &Path, kind: &str, profile_id: &str, horizon: bool) {
    let js = format!("window.renderContent && window.renderContent({});", list_content_json(game_dir, kind, profile_id, horizon));
    let _ = proxy.send_event(UserEvent::EvalJs(js));
}

// ───────────────────────── State ─────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
enum PingState {
    Checking(&'static str),
    Online { online: bool, ms: u128 },
    Offline { online: bool },
}

enum UserEvent {
    EvalJs(String),
    PingResult { id: String, ping: PingState },
    ResourcesChecked(bool),
    ProcessExited(Option<i32>),
    SetWindowVisible(bool),
    OpenBrowser(String),
}

struct AppState {
    base_dir: PathBuf,
    config: LauncherConfig,
    paths: installer::Paths,
    pings: HashMap<String, PingState>,
    resources_ready: bool,
    catalog: Vec<installer::CatalogEntry>,
    catalog_error: Option<String>,
    active_child: Option<Child>,
    system_memory_mb: u64,
    /// Set by launches so `spawn_streaming_exe` can tell a game apart from tools: the id of the
    /// instance being launched (the player may select another one while it downloads).
    pending_launch: Option<String>,
    /// Profile id + start time of the running game session (for playtime).
    session: Option<(String, Instant)>,
    /// Horizon start command kept for one automatic retry: the engine has an intermittent
    /// startup race (exit 101 within seconds) that a second start doesn't hit.
    horizon_retry: Option<HorizonRetry>,
    /// A launch/install in progress: (what's happening, fraction or -1).
    task: Option<(String, f64)>,
}

impl AppState {
    #[cfg(test)]
    fn for_tests(base_dir: PathBuf, config: LauncherConfig) -> Self {
        AppState {
            paths: installer::Paths::new(&base_dir, None),
            base_dir,
            config,
            pings: HashMap::new(),
            resources_ready: false,
            catalog: Vec::new(),
            catalog_error: None,
            active_child: None,
            system_memory_mb: 0,
            pending_launch: None,
            session: None,
            horizon_retry: None,
            task: None,
        }
    }

    fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let base_dir = home.join("LumosClient");
        // Earlier builds used other folder names: carry the player's instances, settings and downloads over.
        let legacy = ["VyrnClient", "MystralClient", "EmberClient"].into_iter().map(|n| home.join(n)).find(|d| d.is_dir()).unwrap_or_else(|| home.join("EmberClient"));
        if !base_dir.exists() && legacy.is_dir() {
            let _ = fs::rename(&legacy, &base_dir);
        }
        for sub in ["instances/default", "logs"] {
            let _ = fs::create_dir_all(base_dir.join(sub));
        }
        let official = installer::official_minecraft_dir();
        let cfg_path = base_dir.join("launcher.json");
        let raw = fs::read_to_string(&cfg_path).ok();
        let parsed = raw.as_deref().and_then(|s| serde_json::from_str::<LauncherConfig>(s).ok());
        if raw.is_some() && parsed.is_none() {
            // Never overwrite settings we couldn't read: keep a copy for recovery.
            let _ = fs::copy(&cfg_path, base_dir.join(format!("launcher.json.unreadable-{}", now_millis())));
        }
        let known_player = parsed.is_some() && raw.as_deref().is_some_and(|r| !r.contains("\"guideSeen\""));
        let mut config = parsed.unwrap_or_else(|| LauncherConfig {
                player_name: "Player".into(),
                minecraft_dir: official.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
                java_home: String::new(),
                active_profile_id: "default".into(),
                selected_version: default_version(),
                resolution_width: default_width(),
                resolution_height: default_height(),
                fullscreen: false,
                memory_mb: default_memory(),
                jvm_args: String::new(),
                lumos_hud_enabled: true,
                lumos_capes_enabled: true,
                horizon_controller: false,
                ui_accent: default_accent(),
                ui_theme: default_theme(),
                ui_motion: true,
                hide_on_launch: false,
                ui_home_layout: default_home_layout(),
                account_kind: default_account_kind(),
                ely_session: None,
                ely_client_token: String::new(),
                profiles: vec![Profile {
                    id: "default".into(),
                    name: "Survival".into(),
                    world: String::new(),
                    last_played: 0,
                    playtime_secs: 0,
                    icon: String::new(),
                    version: default_version(),
                    ..Default::default()
                }],
                servers: vec![],
                curseforge_api_key: String::new(),
                guide_seen: false,
                auto_update: true,
            });
        if known_player {
            config.guide_seen = true;
        }
        // Older builds pinned a Homebrew Java; LumosClient now downloads the right Java itself.
        if config.java_home.contains("/opt/homebrew/opt/openjdk") {
            config.java_home.clear();
        }
        if config.ely_client_token.is_empty() {
            config.ely_client_token = elyby::new_client_token();
        }
        migrate_legacy_names(&mut config);
        if config.profiles.is_empty() {
            config.profiles.push(Profile {
                id: "default".into(),
                name: "Survival".into(),
                world: String::new(),
                last_played: 0,
                playtime_secs: 0,
                icon: String::new(),
                version: config.selected_version.clone(),
                ..Default::default()
            });
        }
        if !config.profiles.iter().any(|p| p.id == config.active_profile_id) {
            config.active_profile_id = config.profiles[0].id.clone();
        }
        if config.account_kind == "elyby" && config.ely_session.is_none() {
            config.account_kind = default_account_kind();
        }
        let official_dir = Some(PathBuf::from(&config.minecraft_dir))
            .filter(|p| !p.as_os_str().is_empty() && p.join("versions").is_dir())
            .or(official);
        let paths = installer::Paths::new(&base_dir, official_dir);
        let catalog = installer::catalog(&paths, None);
        let state = Self {
            base_dir,
            config,
            paths,
            pings: HashMap::new(),
            resources_ready: false,
            catalog,
            catalog_error: None,
            active_child: None,
            system_memory_mb: system_memory_mb(),
            pending_launch: None,
            session: None,
            horizon_retry: None,
            task: None,
        };
        state.save_config();
        state
    }

    fn save_config(&self) {
        if let Ok(text) = serde_json::to_string_pretty(&self.config) {
            let path = self.base_dir.join("launcher.json");
            let tmp = self.base_dir.join("launcher.json.part");
            let _ = fs::remove_file(&tmp);
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                // Holds the Ely.by sign-in token: owner-only from the moment it exists.
                opts.mode(0o600);
            }
            let written = opts.open(&tmp).and_then(|mut f| f.write_all(text.as_bytes()).and_then(|_| f.sync_all()));
            if written.is_ok() {
                let _ = fs::rename(&tmp, &path);
            }
        }
    }

    /// The instance a request names: its id, folder and Minecraft version. An empty id means
    /// the active instance; an id that no longer exists (deleted meanwhile) gives `None`, so
    /// content never lands in some other instance's folder.
    fn target_instance(&self, id: &str) -> Option<(String, PathBuf, String)> {
        let active = &self.config.active_profile_id;
        if id.is_empty() || id == active {
            return Some((active.clone(), self.active_game_dir(), self.config.selected_version.clone()));
        }
        let p = self.config.profiles.iter().find(|p| p.id == id && safe_file_name(&p.id))?;
        let dir = self.base_dir.join("instances").join(&p.id);
        let _ = fs::create_dir_all(&dir);
        let ver = if p.version.is_empty() { self.config.selected_version.clone() } else { p.version.clone() };
        Some((p.id.clone(), dir, ver))
    }

    fn active_game_dir(&self) -> PathBuf {
        let id = if self.config.active_profile_id.is_empty() || !safe_file_name(&self.config.active_profile_id) {
            "default"
        } else {
            &self.config.active_profile_id
        };
        let dir = self.base_dir.join("instances").join(id);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn horizon_output(&self) -> PathBuf {
        self.base_dir.join("resources").join("26.3")
    }

    fn version_list(&self) -> Vec<String> {
        let mut v = vec![HORIZON_ID.to_owned()];
        v.extend(self.catalog.iter().map(|e| e.id.clone()));
        if !v.contains(&self.config.selected_version) {
            v.insert(1, self.config.selected_version.clone());
        }
        v
    }

    fn state_json(&self) -> String {
        let mut catalog = vec![json!({"id": HORIZON_ID, "kind": "horizon", "installed": self.resources_ready})];
        catalog.extend(self.catalog.iter().map(|e| json!(e)));
        let ely = self.config.ely_session.as_ref();
        json!({
            "baseDir": self.base_dir.to_string_lossy(),
            "playerName": self.config.player_name,
            "minecraftDir": self.config.minecraft_dir,
            "javaHome": self.config.java_home,
            "activeProfileId": self.config.active_profile_id,
            "selectedVersion": self.config.selected_version,
            "resolutionWidth": self.config.resolution_width,
            "resolutionHeight": self.config.resolution_height,
            "fullscreen": self.config.fullscreen,
            "memoryMb": self.config.memory_mb,
            "jvmArgs": self.config.jvm_args,
            "lumosHudEnabled": self.config.lumos_hud_enabled,
            "lumosCapesEnabled": self.config.lumos_capes_enabled,
            "horizonController": self.config.horizon_controller,
            "versions": self.version_list(),
            "catalog": catalog,
            "catalogError": self.catalog_error,
            "profiles": self.config.profiles,
            "servers": self.config.servers,
            "pings": self.pings,
            "resourcesReady": self.resources_ready,
            "running": self.active_child.is_some(),
            "task": self.task.as_ref().map(|(l, f)| json!({"label": l, "fraction": f})),
            "systemMemoryMb": self.system_memory_mb,
            "uiAccent": self.config.ui_accent,
            "uiTheme": self.config.ui_theme,
            "uiMotion": self.config.ui_motion,
            "hideOnLaunch": self.config.hide_on_launch,
            "homeLayout": self.config.ui_home_layout,
            "sessionStartedSecsAgo": self.session.as_ref().map(|(_, t)| t.elapsed().as_secs()),
            "account": {
                "kind": self.config.account_kind,
                "elyName": ely.map(|s| s.name.clone()),
                "elyUuid": ely.map(|s| elyby::dashed_uuid(&s.uuid)),
            },
            "launcherVersion": LAUNCHER_VERSION,
            // feat/discover: only whether a key is saved, never the key.
            "curseforgeKeySet": !self.config.curseforge_api_key.trim().is_empty(),
            "guideSeen": self.config.guide_seen,
            "autoUpdate": self.config.auto_update,
        })
        .to_string()
    }
}

// ───────────────────────── UI messaging ─────────────────────────

fn push_state(proxy: &EventLoopProxy<UserEvent>, state: &AppState) {
    let js = format!("window.renderState({});", state.state_json());
    let _ = proxy.send_event(UserEvent::EvalJs(js));
}

fn push_log(proxy: &EventLoopProxy<UserEvent>, line: &str) {
    if let Ok(encoded) = serde_json::to_string(line) {
        let _ = proxy.send_event(UserEvent::EvalJs(format!("window.appendLog({encoded});")));
    }
}

fn call_js(proxy: &EventLoopProxy<UserEvent>, func: &str, payload: serde_json::Value) {
    let _ = proxy.send_event(UserEvent::EvalJs(format!("window.{func} && window.{func}({payload});")));
}

/// Thread-safe progress reporter for a background task.
struct TaskReporter {
    proxy: Mutex<EventLoopProxy<UserEvent>>,
    shared: Arc<Mutex<AppState>>,
    last: Mutex<Instant>,
}

impl TaskReporter {
    fn new(proxy: &EventLoopProxy<UserEvent>, shared: &Arc<Mutex<AppState>>) -> Self {
        Self { proxy: Mutex::new(proxy.clone()), shared: shared.clone(), last: Mutex::new(Instant::now() - Duration::from_secs(1)) }
    }
    fn report(&self, label: &str, fraction: f64) {
        {
            let mut st = lock(&self.shared);
            st.task = Some((label.to_owned(), fraction));
        }
        let mut last = self.last.lock().unwrap();
        // Phase changes always go through; progress updates at most ~8 per second.
        if fraction >= 0.0 && fraction < 1.0 && last.elapsed() < Duration::from_millis(120) {
            return;
        }
        *last = Instant::now();
        call_js(&self.proxy.lock().unwrap(), "onTask", json!({"label": label, "fraction": fraction}));
    }
    fn log(&self, line: &str) {
        push_log(&self.proxy.lock().unwrap(), line);
    }
}

// ───────────────────────── Server ping ─────────────────────────

fn ping_target(address: &str) -> String {
    let a = address.trim();
    if a.starts_with('[') {
        return if a.contains("]:") { a.to_owned() } else { format!("{a}:25565") };
    }
    match a.matches(':').count() {
        0 => format!("{a}:25565"),
        1 => a.to_owned(),
        _ => format!("[{a}]:25565"), // bare IPv6
    }
}

fn ping_servers_async(proxy: EventLoopProxy<UserEvent>, servers: Vec<ServerEntry>) {
    for srv in servers {
        let p = proxy.clone();
        let _ = p.send_event(UserEvent::PingResult { id: srv.id.clone(), ping: PingState::Checking("checking") });
        std::thread::spawn(move || {
            let addr = ping_target(&srv.address);
            let start = Instant::now();
            let result = addr
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.next())
                .and_then(|sa| TcpStream::connect_timeout(&sa, Duration::from_secs(3)).ok());
            let ping = match result {
                Some(_) => PingState::Online { online: true, ms: start.elapsed().as_millis().max(1) },
                None => PingState::Offline { online: false },
            };
            let _ = p.send_event(UserEvent::PingResult { id: srv.id, ping });
        });
    }
}

// ───────────────────────── Processes ─────────────────────────

/// Turns log4j's XML event stream (what Mojang's logging config makes the game print for
/// launchers) back into readable lines; plain lines pass through untouched.
#[derive(Default)]
struct Log4jText {
    event: Option<String>,
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let a = s.find(start)? + start.len();
    let b = s[a..].find(end)? + a;
    Some(&s[a..b])
}

impl Log4jText {
    fn feed(&mut self, line: &str) -> Vec<String> {
        let t = line.trim_start();
        if let Some(buf) = self.event.as_mut() {
            buf.push_str(line);
            buf.push('\n');
            if t.contains("</log4j:Event>") {
                let ev = self.event.take().unwrap_or_default();
                return Self::render(&ev);
            }
            return Vec::new();
        }
        if t.starts_with("<log4j:Event") {
            if t.contains("</log4j:Event>") {
                return Self::render(line);
            }
            self.event = Some(format!("{line}\n"));
            return Vec::new();
        }
        vec![line.to_owned()]
    }

    fn render(ev: &str) -> Vec<String> {
        let level = between(ev, "level=\"", "\"").unwrap_or("INFO");
        let thread = between(ev, "thread=\"", "\"").unwrap_or("");
        let msg = between(ev, "<log4j:Message><![CDATA[", "]]></log4j:Message>").unwrap_or("");
        let mut out = vec![if level == "INFO" { format!("[{thread}] {msg}") } else { format!("[{thread}/{level}] {msg}") }];
        if let Some(thr) = between(ev, "<log4j:Throwable><![CDATA[", "]]></log4j:Throwable>") {
            out.extend(thr.lines().map(str::to_owned));
        }
        out
    }
}

/// Starts a game (or tool) process, streams its output to Logs and tracks it.
#[derive(Clone)]
struct HorizonRetry {
    exe: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    session: String,
}

fn spawn_streaming_exe(
    proxy: &EventLoopProxy<UserEvent>,
    shared: &Arc<Mutex<AppState>>,
    exe: &Path,
    args: &[String],
    cwd: &Path,
    env: &[(&str, &Path)],
    stdin_payload: Option<String>,
) -> Result<(), String> {
    let mut cmd = Command::new(exe);
    installer::no_console(&mut cmd);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(if stdin_payload.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            shared.lock().unwrap().pending_launch = None;
            return Err(format!("Couldn't start {}: {e}", exe.file_name().unwrap_or_default().to_string_lossy()));
        }
    };
    if let (Some(payload), Some(mut stdin)) = (stdin_payload, child.stdin.take()) {
        let _ = stdin.write_all(payload.as_bytes());
        // Dropping closes stdin so the game knows the session is complete.
    }
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    {
        let mut st = shared.lock().unwrap();
        st.active_child = Some(child);
        if let Some(pid) = st.pending_launch.take() {
            if let Some(p) = st.config.profiles.iter_mut().find(|p| p.id == pid) {
                p.last_played = now_millis();
            }
            st.session = Some((pid, Instant::now()));
            crash_help::begin_session(); // feat/updater
            st.save_config();
            if st.config.hide_on_launch {
                let _ = proxy.send_event(UserEvent::SetWindowVisible(false));
            }
        }
        push_state(proxy, &st);
    }
    for stream in [stdout.map(|s| Box::new(s) as Box<dyn Read + Send>), stderr.map(|s| Box::new(s) as Box<dyn Read + Send>)]
        .into_iter()
        .flatten()
    {
        let p = proxy.clone();
        std::thread::spawn(move || {
            let mut xml = Log4jText::default();
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                for l in xml.feed(&line) {
                    crash_help::remember_line(&l); // feat/updater
                    push_log(&p, &l);
                }
            }
        });
    }
    let p = proxy.clone();
    let s = shared.clone();
    let started = Instant::now();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(250));
        let exited = {
            let mut st = s.lock().unwrap();
            match st.active_child.as_mut().map(|c| c.try_wait()) {
                Some(Ok(Some(status))) => {
                    st.active_child = None;
                    Some(status.code())
                }
                Some(Ok(None)) => None,
                Some(Err(_)) | None => {
                    st.active_child = None;
                    Some(None)
                }
            }
        };
        if let Some(code) = exited {
            // Horizon's intermittent startup panic: start it once more, transparently.
            if code == Some(101) && started.elapsed() < Duration::from_secs(30) {
                let retry = lock(&s).horizon_retry.take();
                if let Some(r) = retry {
                    push_log(&p, "[Horizon] The engine stopped while starting (a known startup hiccup). Starting it again…");
                    // The crash explainer judges the new run only, not the first run's panic.
                    crash_help::begin_session();
                    if spawn_streaming_exe(&p, &s, &r.exe, &r.args, &r.cwd, &[], Some(r.session)).is_ok() {
                        break;
                    }
                }
            }
            if started.elapsed() >= Duration::from_secs(30) {
                lock(&s).horizon_retry = None;
            }
            push_log(&p, &format!("[LumosClient] Game closed{}", code.map(|c| format!(" (exit code {c})")).unwrap_or_default()));
            let _ = p.send_event(UserEvent::ProcessExited(code));
            break;
        }
    });
    Ok(())
}

fn check_resources_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>) {
    std::thread::spawn(move || {
        let output = shared.lock().unwrap().horizon_output();
        let ok = match (resolve_binary("horizon-prepare"), horizon_root_for_check(&shared)) {
            (Some(bin), Some(root)) => installer::horizon_check(&bin, &root, &output),
            _ => false,
        };
        let _ = proxy.send_event(UserEvent::ResourcesChecked(ok));
    });
}

fn horizon_root_for_check(shared: &Arc<Mutex<AppState>>) -> Option<PathBuf> {
    match horizon_support()? {
        HorizonRoot::Dev(d) => Some(d),
        HorizonRoot::Bundled(_) => {
            let root = shared.lock().unwrap().paths.horizon_root();
            root.join("research-lock.json").is_file().then_some(root)
        }
    }
}

/// Mojang's version list → the version picker (in the background).
fn refresh_catalog_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>) {
    std::thread::spawn(move || {
        let paths = shared.lock().unwrap().paths.clone();
        // NeoForge/Forge version lists for the catalog (forge.rs; cached, refreshed every few hours).
        forge::refresh_index(&paths);
        let result = installer::manifest(&paths);
        let mut st = shared.lock().unwrap();
        match result {
            Ok(m) => {
                st.catalog = installer::catalog(&paths, Some(&m));
                st.catalog_error = None;
            }
            Err(e) => {
                st.catalog = installer::catalog(&paths, None);
                st.catalog_error = Some(e);
            }
        }
        push_state(&proxy, &st);
    });
}

// ───────────────────────── Launching ─────────────────────────

/// Locks the shared state even if a panicking thread poisoned it.
fn lock(shared: &Arc<Mutex<AppState>>) -> std::sync::MutexGuard<'_, AppState> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs background work, turning a panic into an ordinary error so the UI never stays busy.
fn run_guarded(f: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err("Something went wrong inside LumosClient. Check Logs, then try again.".into()))
}

struct LaunchSnapshot {
    config: LauncherConfig,
    paths: installer::Paths,
    game_dir: PathBuf,
    horizon_output: PathBuf,
}

/// The game Lumos started closed: what its in-game Discover asked for (`lumos-requests.json`:
/// native mods to add or take out, content mods to delete) is applied, and when it asked for a
/// restart the same instance starts again.
fn apply_ingame_requests(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, pid: String) {
    std::thread::spawn(move || {
        let Some((game_dir, base_dir)) = ({
            let st = lock(&shared);
            st.target_instance(&pid).filter(|t| t.2 == HORIZON_ID).map(|t| (t.1, st.base_dir.clone()))
        }) else {
            return;
        };
        let Some(req) = ingame_requests::take(&game_dir) else { return };
        push_log(&proxy, "[LumosClient] Applying what the in-game Discover asked for");
        let p = proxy.clone();
        let report = |m: &str| push_log(&p, &format!("[LumosClient] {m}"));
        let trash = |path: &Path, label: &str| move_to_trash(&base_dir, path, label);
        let mut live = ingame_requests::Live { base_dir: base_dir.clone(), game_dir: game_dir.clone(), report: &report, trash: &trash };
        let outcomes = ingame_requests::apply(&req, &mut live);
        let failed = outcomes.iter().any(|(ok, _)| !ok);
        for (ok, msg) in &outcomes {
            push_log(&proxy, &format!("[LumosClient] {}{msg}", if *ok { "" } else { "error: " }));
        }
        if !outcomes.is_empty() {
            let text = outcomes.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>().join(" ");
            call_js(&proxy, "onNotice", json!({"message": text, "kind": if failed { "err" } else { "ok" }}));
        }
        if !req.restart {
            return;
        }
        // Only the instance that asked, and only while it is still the one selected.
        let still_selected = lock(&shared).config.active_profile_id == pid;
        if still_selected {
            push_log(&proxy, "[LumosClient] Starting the game again");
            launch_game(&proxy, &shared, None);
        } else {
            push_log(&proxy, "[LumosClient] Another instance is selected now, so the game wasn't started again.");
        }
    });
}

/// Starts the selected version: installs whatever is missing, then launches. One at a time.
fn launch_game(proxy: &EventLoopProxy<UserEvent>, shared: &Arc<Mutex<AppState>>, server: Option<String>) {
    let snap = {
        let mut st = shared.lock().unwrap();
        if st.active_child.is_some() {
            call_js(proxy, "onTask", json!({"error": "A game is already running. Stop it first."}));
            return;
        }
        if st.task.is_some() {
            call_js(proxy, "onTask", json!({"error": "LumosClient is already getting a game ready."}));
            return;
        }
        st.task = Some(("Getting ready".into(), -1.0));
        push_state(proxy, &st);
        // The instance's own settings (memory, window size, Java arguments) on top of the global ones.
        let mut config = st.config.clone();
        if let Some(p) = st.config.profiles.iter().find(|p| p.id == st.config.active_profile_id) {
            p.apply_to(&mut config);
        }
        LaunchSnapshot {
            config,
            paths: st.paths.clone(),
            game_dir: st.active_game_dir(),
            horizon_output: st.horizon_output(),
        }
    };
    let proxy = proxy.clone();
    let shared = shared.clone();
    std::thread::spawn(move || {
        let reporter = TaskReporter::new(&proxy, &shared);
        let result = run_guarded(|| {
            if snap.config.selected_version == HORIZON_ID {
                launch_horizon(&proxy, &shared, &reporter, &snap, server.as_deref())
            } else {
                launch_java(&proxy, &shared, &reporter, &snap, server.as_deref())
            }
        });
        // Newly installed versions show as installed in the picker (network outside the lock).
        let manifest = installer::manifest(&snap.paths).ok();
        let catalog = installer::catalog(&snap.paths, manifest.as_ref());
        {
            let mut st = lock(&shared);
            st.task = None;
            if result.is_err() {
                st.pending_launch = None;
            }
            st.catalog = catalog;
            push_state(&proxy, &st);
        }
        match result {
            Ok(()) => call_js(&proxy, "onTask", json!({"done": true})),
            Err(e) => {
                push_log(&proxy, &format!("[LumosClient] error: {e}"));
                call_js(&proxy, "onTask", json!({"error": e}));
            }
        }
    });
}

fn build_auth(shared: &Arc<Mutex<AppState>>, cfg: &LauncherConfig, paths: &installer::Paths, rep: &TaskReporter) -> Result<installer::Auth, String> {
    if cfg.account_kind == "elyby" {
        let sess = cfg.ely_session.clone().ok_or("Sign in to Ely.by again (Account menu).")?;
        rep.report("Checking your Ely.by sign-in", -1.0);
        let fresh = match elyby::ensure_fresh(&sess) {
            Ok(f) => f,
            // Offline: keep the saved session; single-player still works.
            Err(elyby::AuthError::Network(e)) => {
                rep.log(&format!("[Ely.by] Couldn't check the sign-in ({e}); using the saved one."));
                sess.clone()
            }
            Err(elyby::AuthError::RateLimited) => {
                rep.log("[Ely.by] Sign-in check was rate-limited; using the saved one.");
                sess.clone()
            }
            Err(e) => return Err(e.to_string()),
        };
        if fresh != sess {
            let mut st = shared.lock().unwrap();
            st.config.ely_session = Some(fresh.clone());
            st.save_config();
        }
        rep.report("Getting skin support (authlib-injector)", -1.0);
        let jar = elyby::ensure_authlib_injector(&paths.runtime, bundled_authlib().as_deref())?;
        // Fetch Ely.by's sign-in details here (with a DNS fallback) and hand them to the game, so a
        // broken DNS lookup can't stop the game at startup. If Ely.by can't be reached at all, play
        // without it rather than not at all.
        match elyby::javaagent_args_prefetched(&jar) {
            Some(agent) => Ok(installer::Auth {
                name: fresh.name.clone(),
                uuid: fresh.uuid.clone(),
                access_token: fresh.access_token.clone(),
                user_type: "mojang".into(),
                javaagent: Some(agent),
            }),
            None => {
                rep.report("Ely.by can't be reached right now: launching without it (default skins and capes this session)", -1.0);
                Ok(installer::Auth { uuid: offline_uuid_hex(&fresh.name), name: fresh.name.clone(), access_token: "0".into(), user_type: "legacy".into(), javaagent: None })
            }
        }
    } else {
        let name = cfg.player_name.trim().to_owned();
        if !valid_player_name(&name) {
            return Err("Set a username first: 3–16 letters, numbers or _.".into());
        }
        Ok(installer::Auth { uuid: offline_uuid_hex(&name), name, access_token: "0".into(), user_type: "legacy".into(), javaagent: None })
    }
}

/// File name the HUD mod had before the rename; removed from instances on launch.
const LEGACY_HUD_JAR: &str = "emberhud-1.21.11.jar";

/// Settings saved by builds from before the rename.
fn migrate_legacy_names(config: &mut LauncherConfig) {
    if config.ui_accent == "ember" {
        config.ui_accent = default_accent();
    }
    for s in &mut config.servers {
        if s.name == "Local Ember Server" {
            s.name = "Local Server".into();
        }
    }
}

// ── LumosHUD versions ── (keep in step with lumoshud/gradle.properties `mc_versions` and ui.html's HUD_VERSIONS)
/// The Minecraft versions a LumosHUD jar is built for: lumoshud-<mc>.jar each.
const HUD_VERSIONS: &[&str] = &["1.21.6", "1.21.7", "1.21.8", "1.21.9", "1.21.10", "1.21.11"];

/// The Minecraft version of a Fabric instance LumosHUD has a jar for.
fn hud_mc(version: &str) -> Option<String> {
    match installer::parse_spec(version) {
        installer::Spec::Fabric(mc) if HUD_VERSIONS.contains(&mc.as_str()) => Some(mc),
        _ => None,
    }
}

fn hud_supported(version: &str) -> bool {
    hud_mc(version).is_some()
}

fn sync_lumos_hud(game_dir: &Path, version: &str, enabled: bool) {
    let mods_dir = game_dir.join("mods");
    // Earlier builds shipped the HUD under its old name; two copies would draw it twice.
    let _ = fs::remove_file(mods_dir.join(LEGACY_HUD_JAR));
    let want = hud_mc(version).filter(|_| enabled).map(|mc| format!("lumoshud-{mc}.jar"));
    // A jar for another version (the instance changed version) or a switched-off HUD goes.
    if let Ok(rd) = fs::read_dir(&mods_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("lumoshud-") && name.ends_with(".jar") && Some(&name) != want.as_ref() {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    if let (Some(name), Some(mc)) = (want, hud_mc(version)) {
        // A copy the player switched off (e.g. from the crash card) stays off.
        if mods_dir.join(format!("{name}.disabled")).exists() {
            return;
        }
        if let Some(src) = bundled_hud(&mc) {
            let _ = fs::create_dir_all(&mods_dir);
            let _ = fs::copy(src, mods_dir.join(name));
        }
    }
}
// ── end LumosHUD versions ──

fn launch_java(
    proxy: &EventLoopProxy<UserEvent>,
    shared: &Arc<Mutex<AppState>>,
    rep: &TaskReporter,
    snap: &LaunchSnapshot,
    server: Option<&str>,
) -> Result<(), String> {
    let cfg = &snap.config;
    let report = |l: &str, f: f64| rep.report(l, f);
    let auth = build_auth(shared, cfg, &snap.paths, rep)?;
    let (resolved, prepared, managed_java) = installer::install(&snap.paths, &cfg.selected_version, &report)?;
    sync_lumos_hud(&snap.game_dir, &cfg.selected_version, cfg.lumos_hud_enabled);
    let capes_arg = capes::sync_mod(
        &snap.game_dir,
        &cfg.selected_version,
        cfg.lumos_capes_enabled,
        bundled_capes_mod().as_deref(),
        capes::server_url().as_deref(),
    );
    let custom = (!cfg.java_home.trim().is_empty())
        .then(|| installer::java_binary_in_home(Path::new(cfg.java_home.trim())))
        .filter(|p| p.is_file());
    let java = custom.unwrap_or(managed_java);
    let opts = installer::LaunchOpts {
        memory_mb: cfg.memory_mb,
        extra_jvm: cfg.jvm_args.split_whitespace().map(str::to_owned).chain(capes_arg).collect(),
        width: cfg.resolution_width,
        height: cfg.resolution_height,
        fullscreen: cfg.fullscreen,
        server: server.map(str::to_owned),
        launcher_version: LAUNCHER_VERSION.into(),
    };
    let args = installer::build_args(&resolved, &prepared, &snap.game_dir, &auth, &opts);
    rep.report("Starting Minecraft", -1.0);
    lock(shared).horizon_retry = None;
    rep.log(&format!(
        "[LumosClient] Launching {} as {} with Java at {}",
        cfg.selected_version,
        auth.name,
        java.display()
    ));
    shared.lock().unwrap().pending_launch = Some(cfg.active_profile_id.clone());
    spawn_streaming_exe(proxy, shared, &java, &args, &snap.game_dir, &[], None)
}

/// Writes the player's controller choice into the instance's `horizon-controller.txt`
/// (`enabled:true|false`), keeping every other controller setting in it.
fn set_horizon_controller(game_dir: &Path, enabled: bool) {
    let path = game_dir.join("horizon-controller.txt");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> =
        text.lines().filter(|l| l.split_once(':').map(|(k, _)| k.trim()) != Some("enabled")).map(str::to_owned).collect();
    lines.insert(0, format!("enabled:{enabled}"));
    let _ = fs::create_dir_all(game_dir);
    let _ = fs::write(&path, lines.join("\n") + "\n");
}

fn launch_horizon(
    proxy: &EventLoopProxy<UserEvent>,
    shared: &Arc<Mutex<AppState>>,
    rep: &TaskReporter,
    snap: &LaunchSnapshot,
    server: Option<&str>,
) -> Result<(), String> {
    let cfg = &snap.config;
    let auth = build_auth(shared, cfg, &snap.paths, rep)?;
    let base_dir = shared.lock().unwrap().base_dir.clone();
    // An instance with native mods plays the cloud build made for them (native_builds).
    // An instance on a per-commit native build moves onto the combined build when that has all its mods.
    {
        let report = |m: &str| rep.report(m, -1.0);
        match native_builds::upgrade_to_combined(&base_dir, &snap.game_dir, &report) {
            Ok(true) => rep.log("[Horizon] Moved this instance's native mods onto the combined build"),
            Ok(false) => {}
            Err(e) => rep.log(&format!("[Horizon] Staying on this instance's native build: {e}")),
        }
    }
    let (client, resources) = match native_builds::launch_parts(&base_dir, &snap.game_dir) {
        Some((client, prepare, root, short)) => {
            let output = snap.horizon_output.with_file_name(format!("26.3-{short}"));
            rep.log(&format!("[Horizon] Using the native mod build {short}"));
            let report = |l: &str, f: f64| rep.report(l, f);
            let log = |l: &str| rep.log(l);
            rep.report("Getting Horizon ready", -1.0);
            let output = installer::ensure_horizon(&snap.paths, &prepare, &root, &output, &report, &log)?;
            (client, output)
        }
        None => {
            if native_builds::read_state(&snap.game_dir).is_some() {
                return Err("This instance's native mod build is missing. Reinstall one of its native mods from Discover.".into());
            }
            prepare_horizon(shared, rep, &snap.paths, &snap.horizon_output)?;
            let client = resolve_binary("horizon-client").ok_or("This LumosClient build is missing the Horizon engine. Reinstall LumosClient.")?;
            (client, snap.horizon_output.clone())
        }
    };
    set_horizon_controller(&snap.game_dir, cfg.horizon_controller);
    let session = json!({"name": auth.name, "uuid": auth.uuid, "access_token": auth.access_token}).to_string();
    let mut args: Vec<String> = vec![
        "--resources".into(),
        resources.to_string_lossy().into_owned(),
        "--game-dir".into(),
        snap.game_dir.to_string_lossy().into_owned(),
        "--home".into(),
        snap.game_dir.to_string_lossy().into_owned(),
        "--width".into(),
        cfg.resolution_width.max(640).to_string(),
        "--height".into(),
        cfg.resolution_height.max(480).to_string(),
        "--session-stdin".into(),
    ];
    if cfg.fullscreen {
        args.push("--fullscreen".into());
    }
    if let Some(s) = server {
        args.push("--quickPlayMultiplayer".into());
        args.push(s.trim().to_owned());
    }
    rep.report("Starting Horizon", -1.0);
    rep.log(&format!("[Horizon] Launching as {}", auth.name));
    {
        let mut st = lock(shared);
        st.pending_launch = Some(snap.config.active_profile_id.clone());
        st.horizon_retry = Some(HorizonRetry { exe: client.clone(), args: args.clone(), cwd: snap.game_dir.clone(), session: session.clone() });
    }
    spawn_streaming_exe(proxy, shared, &client, &args, &snap.game_dir, &[], Some(session))
}

/// Makes Horizon's resources ready: support files, Minecraft files, Java, preparation.
fn prepare_horizon(shared: &Arc<Mutex<AppState>>, rep: &TaskReporter, paths: &installer::Paths, output: &Path) -> Result<(), String> {
    let prepare = resolve_binary("horizon-prepare").ok_or("This LumosClient build is missing the Horizon preparer. Reinstall LumosClient.")?;
    rep.report("Getting Horizon ready", -1.0);
    let root = match horizon_support().ok_or("This LumosClient build is missing Horizon's support files. Reinstall LumosClient.")? {
        HorizonRoot::Dev(d) => d,
        HorizonRoot::Bundled(b) => {
            let dest = paths.horizon_root();
            installer::sync_horizon_root(&b, &dest)?;
            dest
        }
    };
    let report = |l: &str, f: f64| rep.report(l, f);
    let log = |l: &str| rep.log(l);
    installer::ensure_horizon(paths, &prepare, &root, output, &report, &log)?;
    shared.lock().unwrap().resources_ready = true;
    Ok(())
}

// ───────────────────────── Modrinth ─────────────────────────

/// The Minecraft version Modrinth content must match for an instance version.
fn mc_version_for(paths: &installer::Paths, selected: &str) -> String {
    if selected == HORIZON_ID {
        return "26.3".into();
    }
    if let installer::Spec::Fabric(mc) | installer::Spec::NeoForge(mc) | installer::Spec::Forge(mc) = installer::parse_spec(selected) {
        return mc;
    }
    if let Some(base) = installer::base_game_version(paths, selected) {
        return base;
    }
    mc_version_from_name(selected)
}

fn mc_version_from_name(selected: &str) -> String {
    match installer::parse_spec(selected) {
        installer::Spec::Fabric(mc) | installer::Spec::NeoForge(mc) | installer::Spec::Forge(mc) => mc,
        installer::Spec::Id(id) => {
            let first_numeric = id.split_whitespace().rev().find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()));
            first_numeric.unwrap_or(&id).to_owned()
        }
    }
}

fn is_fabric_version(selected: &str) -> bool {
    selected.starts_with(installer::FABRIC_PREFIX) || selected.to_lowercase().contains("fabric")
}

// ── Mod loaders (forge.rs): which loader an instance runs, for Modrinth filters and installs ──
fn mod_loader(paths: &installer::Paths, selected: &str) -> Option<&'static str> {
    if selected == HORIZON_ID {
        return None;
    }
    forge::instance_loader(paths, selected)
}

#[allow(clippy::too_many_arguments)]
fn search_modrinth_async(proxy: EventLoopProxy<UserEvent>, paths: installer::Paths, game_dir: PathBuf, query: String, project_type: String, sort_by: String, offset: u64, selected: String, seq: u64) {
    std::thread::spawn(move || {
        let project_type = match project_type.as_str() {
            "mod" | "resourcepack" | "shader" | "modpack" => project_type,
            _ => "mod".to_owned(),
        };
        // Horizon 26.3 can't run Java mods: its instances browse Horizon's own registry instead.
        if selected == HORIZON_ID {
            let mut payload = horizon_mods::search(&game_dir, &query, &project_type, &sort_by, offset as usize, &[]);
            payload["seq"] = json!(seq);
            call_js(&proxy, "renderModResults", payload);
            return;
        }
        let mc = mc_version_for(&paths, &selected);
        let mut facets = vec![format!("[\"project_type:{project_type}\"]")];
        if project_type == "mod" {
            // The instance's own loader (Fabric when it has none, as before).
            let loaders = forge::compatible_modrinth_loaders(mod_loader(&paths, &selected).unwrap_or("fabric"));
            facets.push(format!("[{}]", loaders.iter().map(|l| format!("\"categories:{l}\"")).collect::<Vec<_>>().join(",")));
        }
        // Modpacks make their own instance, so they aren't filtered by this one's version.
        if project_type != "shader" && project_type != "modpack" {
            facets.push(format!("[\"versions:{mc}\"]"));
        }
        let facets = format!("[{}]", facets.join(","));
        let index = match sort_by.as_str() {
            "newest" | "updated" | "follows" | "relevance" => sort_by.as_str(),
            _ => "downloads",
        };
        let url = format!(
            "https://api.modrinth.com/v2/search?query={}&facets={}&index={index}&offset={offset}&limit=20",
            url_encode(&query),
            url_encode(&facets)
        );
        let mut payload = match net::get_json(&url) {
            Ok(v) if v.get("hits").is_some() => v,
            Ok(_) => json!({"hits": [], "total_hits": 0, "error": "Modrinth sent an answer LumosClient couldn't read."}),
            Err(e) => json!({"hits": [], "total_hits": 0, "error": e}),
        };
        // Mark what this instance already has, from the files in its folder (switched-off ones
        // count), so Discover shows Installed after a restart or for files copied in by hand.
        let folder = match project_type.as_str() {
            "resourcepack" => "resourcepacks",
            "shader" => "shaderpacks",
            _ => "mods",
        };
        let installed = installed_projects_cached(&game_dir.join(folder));
        if let Some(hits) = payload.get_mut("hits").and_then(|h| h.as_array_mut()) {
            for hit in hits {
                let on = hit.get("project_id").and_then(|p| p.as_str()).is_some_and(|p| installed.contains_key(p));
                hit["installed"] = json!(on);
            }
        }
        payload["seq"] = json!(seq);
        call_js(&proxy, "renderModResults", payload);
    });
}

fn push_install_status(proxy: &EventLoopProxy<UserEvent>, slug: &str, state: &str, msg: &str, profile_id: &str) {
    if let (Ok(s), Ok(st), Ok(m), Ok(p)) =
        (serde_json::to_string(slug), serde_json::to_string(state), serde_json::to_string(msg), serde_json::to_string(profile_id))
    {
        let _ = proxy.send_event(UserEvent::EvalJs(format!("window.onInstallStatus({s}, {st}, {m}, {p});")));
    }
}

/// Saves a file the player picked (base64 from the page) into one of an instance's content folders.
fn import_content(game_dir: &Path, kind: &str, file: &str, data_b64: &str, horizon: bool) -> Result<(), String> {
    use base64::Engine as _;
    let folder = content_folder(kind, horizon).ok_or("Unknown content type.")?;
    if !safe_file_name(file) || file.starts_with('.') {
        return Err("That file name can't be used.".into());
    }
    if !content_extension_ok(kind, file, horizon) {
        return Err(match kind {
            "mods" if horizon => format!("{file} isn't a Horizon mod. Horizon 26.3 loads .hmod mods, not Java .jar files."),
            "mods" => format!("{file} isn't a mod. Mods are .jar files."),
            "shaderpacks" if horizon => format!("{file} isn't a shader pack. Packs are .hshader or .zip files."),
            _ => format!("{file} isn't a pack. Packs are .zip files."),
        });
    }
    if data_b64.len() / 4 * 3 > MAX_IMPORT_BYTES {
        return Err(format!("{file} is too big to add here. Use the Folder button and copy it in."));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(data_b64.trim()).map_err(|_| format!("Couldn't read {file}."))?;
    // A .jar or .zip is a zip archive: catch renamed or broken files before the game trips on them.
    if bytes.len() < 4 || &bytes[..2] != b"PK" {
        return Err(format!("{file} isn't a valid {} file.", file.rsplit('.').next().unwrap_or("mod")));
    }
    let horizon_id = if horizon && kind == "mods" {
        Some(horizon_mods::manifest_id_of_bytes(&bytes).ok_or(format!("{file} isn't a valid Horizon mod (no horizon-mod.toml)."))?)
    } else {
        None
    };
    let dir = game_dir.join(folder);
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{file}.{}.part", now_millis()));
    fs::write(&tmp, &bytes).map_err(|e| format!("Couldn't save {file}: {e}"))?;
    fs::rename(&tmp, dir.join(file)).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Couldn't save {file}: {e}")
    })?;
    // The new file replaces a switched-off copy of the same name (only once it's safely saved).
    let _ = fs::remove_file(dir.join(format!("{file}.disabled")));
    if let Some(id) = horizon_id {
        // An older copy of the same mod would stop Horizon from loading mods.
        horizon_mods::remove_other_copies(&dir, &id, file);
        horizon_mods::set_enabled(game_dir, &id, true)?;
    }
    Ok(())
}

/// Fabric Loader version an instance version uses (from its JSON, ours or the official launcher's).
fn detect_fabric_loader_version(paths: &installer::Paths, selected: &str) -> Option<(u64, u64, u64)> {
    let mut dirs = vec![paths.game.clone()];
    if let Some(o) = &paths.official {
        dirs.push(o.clone());
    }
    for base in dirs {
        let Ok(raw) = fs::read_to_string(base.join("versions").join(selected).join(format!("{selected}.json"))) else { continue };
        let Ok(meta) = serde_json::from_str::<serde_json::Value>(&raw) else { continue };
        for lib in meta.get("libraries").and_then(|l| l.as_array()).into_iter().flatten() {
            if let Some(ver) = lib.get("name").and_then(|n| n.as_str()).and_then(|n| n.strip_prefix("net.fabricmc:fabric-loader:")) {
                return parse_semver(ver);
            }
        }
    }
    None
}

fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let clean = s.split(['-', '+']).next()?.trim();
    let mut parts = clean.split('.');
    let maj = parts.next()?.parse().ok()?;
    let min = parts.next().unwrap_or("0").parse().ok()?;
    let pat = parts.next().unwrap_or("0").parse().ok()?;
    Some((maj, min, pat))
}

fn satisfies_loader_req(req: &str, installed: (u64, u64, u64)) -> bool {
    for part in req.split_whitespace() {
        let p = part.trim();
        if let Some(v) = p.strip_prefix(">=").and_then(parse_semver) {
            if installed < v {
                return false;
            }
        } else if let Some(v) = p.strip_prefix('>').and_then(parse_semver) {
            if installed <= v {
                return false;
            }
        } else if let Some(v) = p.strip_prefix("<=").and_then(parse_semver) {
            if installed > v {
                return false;
            }
        } else if let Some(v) = p.strip_prefix('<').and_then(parse_semver) {
            if installed >= v {
                return false;
            }
        }
    }
    true
}

fn jar_matches_fabric_loader(jar_path: &Path, installed_loader: Option<(u64, u64, u64)>) -> bool {
    let Some(installed) = installed_loader else { return true };
    let Ok(file) = fs::File::open(jar_path) else { return true };
    let Ok(mut zip) = zip::ZipArchive::new(file) else { return true };
    let Ok(mut entry) = zip.by_name("fabric.mod.json") else { return true };
    let mut raw = String::new();
    if entry.read_to_string(&mut raw).is_err() {
        return true;
    }
    let Ok(meta) = serde_json::from_str::<serde_json::Value>(&raw) else { return true };
    match meta.get("depends").and_then(|d| d.get("fabricloader")) {
        Some(serde_json::Value::String(req)) => satisfies_loader_req(req, installed),
        Some(serde_json::Value::Array(reqs)) => reqs.iter().filter_map(|r| r.as_str()).any(|r| satisfies_loader_req(r, installed)),
        _ => true,
    }
}

/// [`installed_projects`], remembered per folder until a file in it is added, removed, renamed or
/// changed, so Discover can mark what's installed on every search without hashing again.
fn installed_projects_cached(dir: &Path) -> HashMap<String, String> {
    type Seen = HashMap<PathBuf, (Vec<(String, u64, u128)>, HashMap<String, String>)>;
    static CACHE: Mutex<Option<Seen>> = Mutex::new(None);
    let mut signature: Vec<(String, u64, u128)> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let m = e.metadata().ok()?;
                    let mtime = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
                    Some((e.file_name().to_string_lossy().into_owned(), m.len(), mtime))
                })
                .collect()
        })
        .unwrap_or_default();
    signature.sort();
    if let Some((sig, found)) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(dir)) {
        if *sig == signature {
            return found.clone();
        }
    }
    let found = installed_projects(dir);
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(dir.to_path_buf(), (signature, found.clone()));
    found
}

/// project_id → file name for jars/zips already in a content folder, asked from Modrinth by hash.
fn installed_projects(dir: &Path) -> HashMap<String, String> {
    let mut by_hash: HashMap<String, String> = HashMap::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !e.path().is_file() {
                continue;
            }
            if let Some(h) = net::sha1_file(&e.path()) {
                by_hash.insert(h, name);
            }
        }
    }
    if by_hash.is_empty() {
        return HashMap::new();
    }
    let body = json!({"hashes": by_hash.keys().collect::<Vec<_>>(), "algorithm": "sha1"});
    let Ok(resp) = net::agent().post("https://api.modrinth.com/v2/version_files").send_json(body) else { return HashMap::new() };
    let Ok(map) = resp.into_json::<serde_json::Value>() else { return HashMap::new() };
    let mut out = HashMap::new();
    for (hash, ver) in map.as_object().into_iter().flatten() {
        if let (Some(pid), Some(file)) = (ver.get("project_id").and_then(|p| p.as_str()), by_hash.get(hash)) {
            out.insert(pid.to_owned(), file.clone());
        }
    }
    out
}

// ── feat/deps: install plans (mod_plan.rs) ──

/// How a planned install ended.
enum PlanOutcome {
    Installed(String),
    /// Problems the player has to see first (nothing was saved).
    Blocked(mod_plan::Summary),
    Failed(String),
}

/// Plans a Modrinth install (required dependencies, conflicts, duplicates) and, when nothing
/// stands in the way (or `force`), downloads and saves it all.
#[allow(clippy::too_many_arguments)]
fn plan_and_install_modrinth(
    proxy: &EventLoopProxy<UserEvent>,
    game_dir: &Path,
    project: &str,
    project_type: &str,
    mc_ver: &str,
    loader: &str,
    fabric_loader: Option<(u64, u64, u64)>,
    force: bool,
) -> PlanOutcome {
    let folder = match project_type {
        "resourcepack" => "resourcepacks",
        "shader" => "shaderpacks",
        _ => "mods",
    };
    let dest = game_dir.join(folder);
    let installed = match mod_plan::identify(&mod_plan::Live, mod_plan::scan_dir(&dest)) {
        Ok(i) => i,
        Err(e) => return PlanOutcome::Failed(e),
    };
    let plan = match mod_plan::plan_modrinth(&mod_plan::Live, project, project_type, mc_ver, loader, &installed) {
        Ok(p) => p,
        Err(e) => return PlanOutcome::Failed(e),
    };
    if plan.summary.blocked() && !force {
        return PlanOutcome::Blocked(plan.summary);
    }
    let download = |f: &mod_plan::FileRef, p: &Path| net::download_verified(&f.url, p, f.sha1.as_deref(), f.size);
    let loader_ok = |p: &Path| project_type != "mod" || loader != "fabric" || jar_matches_fabric_loader(p, fabric_loader);
    let log = |l: &str| push_log(proxy, &format!("[Modrinth] {l}"));
    match mod_plan::execute_modrinth(&plan, &dest, force, &download, &loader_ok, &log) {
        Ok(done) => PlanOutcome::Installed(plan.summary.success_message(&format!(" (Minecraft {mc_ver})"), &done.warnings)),
        Err(mod_plan::ExecError::Blocked(problem)) => {
            let mut s = plan.summary;
            s.problems.push(problem);
            PlanOutcome::Blocked(s)
        }
        Err(mod_plan::ExecError::Failed(e)) => PlanOutcome::Failed(e),
    }
}

/// Shows the page's warning dialog for a plan that wasn't installed.
fn send_plan(proxy: &EventLoopProxy<UserEvent>, summary: &mod_plan::Summary, slug: &str, project_type: &str, profile_id: &str) {
    call_js(proxy, "onModPlan", json!({"slug": slug, "projectType": project_type, "profileId": profile_id, "plan": summary}));
    push_install_status(proxy, slug, "blocked", "", profile_id);
}

/// "Check mods": missing dependencies, incompatible pairs and duplicates among installed mods.
fn check_mods_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, profile_id: String) {
    std::thread::spawn(move || {
        let target = {
            let st = shared.lock().unwrap();
            st.target_instance(&profile_id).map(|(pid, dir, ver)| (pid, dir, ver, st.paths.clone()))
        };
        let Some((pid, game_dir, selected, paths)) = target else {
            call_js(&proxy, "onModCheck", json!({"profileId": profile_id, "error": GONE_INSTANCE}));
            return;
        };
        let result = if selected == HORIZON_ID {
            let installed = mod_plan::scan_horizon(&game_dir);
            Ok(mod_plan::check_horizon(&mod_plan::LiveHorizon, &installed, &native_builds::installed_ids(&game_dir)))
        } else {
            match mod_loader(&paths, &selected) {
                None => Err("This instance has no mod loader, so it doesn't load mods.".to_owned()),
                Some(loader) => mod_plan::identify(&mod_plan::Live, mod_plan::scan_dir(&game_dir.join("mods")))
                    .and_then(|installed| mod_plan::check_modrinth(&mod_plan::Live, &installed, &mc_version_for(&paths, &selected), loader)),
            }
        };
        match result {
            Ok(report) => call_js(&proxy, "onModCheck", json!({"profileId": pid, "report": report})),
            Err(e) => call_js(&proxy, "onModCheck", json!({"profileId": pid, "error": e})),
        }
    });
}
// ── end feat/deps ──

fn download_modrinth_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, slug: String, project_type: String, profile_id: String, force: bool) {
    std::thread::spawn(move || {
        // Everything goes into the instance the player installed to, checked against its own version.
        let target = {
            let st = shared.lock().unwrap();
            st.target_instance(&profile_id).map(|(pid, dir, ver)| (pid, dir, ver, st.paths.clone()))
        };
        let Some((pid, game_dir, selected, paths)) = target else {
            push_install_status(&proxy, &slug, "error", GONE_INSTANCE, &profile_id);
            return;
        };
        let push_install_status = |proxy: &EventLoopProxy<UserEvent>, slug: &str, state: &str, msg: &str| push_install_status(proxy, slug, state, msg, &pid);
        if selected == HORIZON_ID && horizon_mods::is_native(&slug) {
            push_install_status(&proxy, &slug, "installing", &format!("Installing {slug}..."));
            let base_dir = shared.lock().unwrap().base_dir.clone();
            let p2 = proxy.clone();
            let report = |m: &str| push_log(&p2, &format!("[LumosClient] {m}"));
            match native_builds::install(&base_dir, &game_dir, &slug, &report) {
                Ok(msg) => push_install_status(&proxy, &slug, "installed", &msg),
                Err(err) => push_install_status(&proxy, &slug, "error", &err),
            }
            return;
        }
        if selected == HORIZON_ID {
            // feat/deps: content mods are planned against their manifest's dependencies and rules.
            push_install_status(&proxy, &slug, "installing", &format!("Checking {slug}..."));
            let installed = mod_plan::scan_horizon(&game_dir);
            let natives = native_builds::installed_ids(&game_dir);
            match mod_plan::plan_horizon(&mod_plan::LiveHorizon, &slug, &installed, &natives) {
                Err(err) => push_install_status(&proxy, &slug, "error", &err),
                Ok(plan) if plan.summary.blocked() && !force => send_plan(&proxy, &plan.summary, &slug, &project_type, &pid),
                Ok(plan) => match mod_plan::execute_horizon(&plan, &game_dir) {
                    Ok(()) => push_install_status(&proxy, &slug, "installed", &plan.summary.success_message(" (Horizon 26.3)", &[])),
                    Err(err) => push_install_status(&proxy, &slug, "error", &err),
                },
            }
            return;
        }
        let loader = mod_loader(&paths, &selected);
        if project_type == "mod" && loader.is_none() {
            let mc = mc_version_for(&paths, &selected);
            let hint = if selected == HORIZON_ID {
                "Horizon 26.3 can't load Java mods. Switch this instance to a Fabric version first.".to_owned()
            } else {
                format!("Mods need a mod loader. Switch this instance to \"Fabric {mc}\" or \"NeoForge {mc}\" first (version menu next to Launch).")
            };
            push_install_status(&proxy, &slug, "error", &hint);
            return;
        }
        let mc_ver = mc_version_for(&paths, &selected);
        let fabric_loader = detect_fabric_loader_version(&paths, &selected);
        push_install_status(&proxy, &slug, "installing", &format!("Checking compatibility for {slug}..."));
        match plan_and_install_modrinth(&proxy, &game_dir, &slug, &project_type, &mc_ver, loader.unwrap_or("fabric"), fabric_loader, force) {
            PlanOutcome::Installed(msg) => push_install_status(&proxy, &slug, "installed", &msg),
            PlanOutcome::Blocked(summary) => send_plan(&proxy, &summary, &slug, &project_type, &pid),
            PlanOutcome::Failed(err) => push_install_status(&proxy, &slug, "error", &err),
        }
    });
}

// ───────────────────────── feat/discover: Content Hub / Content Browser ─────────────────────────
// Search, tags, project pages, versions, installs (any source, any version, all through install
// plans), updates and suggestions. Logic lives in discover.rs and curseforge.rs; this block only
// finds the instance and talks to the page.
use mod_plan::Modrinth as _;

/// The instance a Discover request is about: (id, folder, version, paths).
fn discover_target(shared: &Arc<Mutex<AppState>>, profile_id: &str) -> Option<(String, PathBuf, String, installer::Paths)> {
    let st = shared.lock().unwrap();
    st.target_instance(profile_id).map(|(pid, dir, ver)| (pid, dir, ver, st.paths.clone()))
}

/// The Minecraft version Horizon plays (its Java resource packs are made for this).
const HORIZON_MC: &str = "26.3";

fn discover_folder(project_type: &str) -> &'static str {
    match project_type {
        "resourcepack" => "resourcepacks",
        "shader" => "shaderpacks",
        _ => "mods",
    }
}

fn cf_key(shared: &Arc<Mutex<AppState>>) -> Option<String> {
    Some(shared.lock().unwrap().config.curseforge_api_key.trim().to_owned()).filter(|k| !k.is_empty())
}

/// What the Content Browser filters an instance for: its Minecraft version, loader and worlds.
fn discover_context(proxy: &EventLoopProxy<UserEvent>, shared: &Arc<Mutex<AppState>>, profile_id: &str) {
    let Some((pid, dir, selected, paths)) = discover_target(shared, profile_id) else {
        call_js(proxy, "onDiscoverContext", json!({"profileId": profile_id, "error": GONE_INSTANCE}));
        return;
    };
    let horizon = selected == HORIZON_ID;
    call_js(
        proxy,
        "onDiscoverContext",
        json!({"profileId": pid, "mc": mc_version_for(&paths, &selected), "loader": mod_loader(&paths, &selected), "horizon": horizon, "worlds": curseforge::worlds(&dir)}),
    );
}

fn discover_search_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, v: serde_json::Value) {
    std::thread::spawn(move || {
        let seq = v.get("seq").and_then(|x| x.as_u64()).unwrap_or(0);
        let hub = v.get("hub").and_then(|x| x.as_bool()).unwrap_or(false);
        let source = v.get("source").and_then(|x| x.as_str()).unwrap_or("modrinth").to_owned();
        let q = discover::Query::from_json(&v);
        let target = if hub { None } else { discover_target(&shared, v.get("profileId").and_then(|x| x.as_str()).unwrap_or("")) };
        let mut payload = if !hub && target.is_none() {
            json!({"hits": [], "total_hits": 0, "error": GONE_INSTANCE})
        } else {
            match source.as_str() {
                "horizon" => match &target {
                    Some((_, dir, sel, _)) if sel == HORIZON_ID => horizon_mods::search(dir, &q.text, &q.project_type, &q.sort, q.offset as usize, &q.categories),
                    _ => json!({"hits": [], "total_hits": 0, "source": "horizon", "notice": "Horizon's registry is for Horizon 26.3 instances."}),
                },
                "curseforge" => match cf_key(&shared) {
                    None => json!({"hits": [], "total_hits": 0, "source": "curseforge", "needKey": true, "notice": curseforge::NO_KEY}),
                    Some(key) => {
                        let mut p = curseforge::search(&curseforge::Live { key }, &q);
                        if let Some((_, dir, _, _)) = &target {
                            let have = curseforge::installed_ids(&dir.join(discover_folder(&q.project_type)));
                            for h in p.get_mut("hits").and_then(|h| h.as_array_mut()).into_iter().flatten() {
                                let on = h.get("id").and_then(|i| i.as_str()).and_then(|i| i.parse::<u64>().ok()).is_some_and(|i| have.contains(&i));
                                h["installed"] = json!(on);
                            }
                        }
                        p
                    }
                },
                _ => {
                    let mut p = discover::modrinth_search(&q);
                    if let Some((_, dir, _, _)) = &target {
                        if matches!(q.project_type.as_str(), "mod" | "resourcepack" | "shader") {
                            let have = installed_projects_cached(&dir.join(discover_folder(&q.project_type)));
                            for h in p.get_mut("hits").and_then(|h| h.as_array_mut()).into_iter().flatten() {
                                let on = h.get("id").and_then(|i| i.as_str()).is_some_and(|i| have.contains_key(i));
                                h["installed"] = json!(on);
                            }
                        }
                    }
                    p
                }
            }
        };
        payload["seq"] = json!(seq);
        payload["offset"] = json!(q.offset);
        call_js(&proxy, "onDiscoverResults", payload);
    });
}

fn discover_tags_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, source: String) {
    std::thread::spawn(move || {
        let res = match source.as_str() {
            "horizon" => horizon_mods::categories(),
            "curseforge" => match cf_key(&shared) {
                None => Err(curseforge::NO_KEY.to_owned()),
                Some(key) => curseforge::categories(&curseforge::Live { key }).map(|mut c| {
                    // CurseForge uses the same Minecraft versions.
                    c["gameVersions"] = discover::modrinth_tags().map(|t| t["gameVersions"].clone()).unwrap_or(json!([]));
                    c
                }),
            },
            _ => discover::modrinth_tags(),
        };
        match res {
            Ok(mut t) => {
                t["source"] = json!(source);
                call_js(&proxy, "onDiscoverTags", t);
            }
            Err(e) => call_js(&proxy, "onDiscoverTags", json!({"source": source, "error": e})),
        }
    });
}

/// (Minecraft version, loaders) a Browser request is checked against; nothing for the Hub.
fn discover_fit(shared: &Arc<Mutex<AppState>>, v: &serde_json::Value) -> Option<(PathBuf, String, Vec<&'static str>)> {
    if v.get("hub").and_then(|x| x.as_bool()).unwrap_or(false) {
        return None;
    }
    let (_, dir, sel, paths) = discover_target(shared, v.get("profileId").and_then(|x| x.as_str()).unwrap_or(""))?;
    let loaders = mod_loader(&paths, &sel).map(forge::compatible_modrinth_loaders).unwrap_or_default();
    Some((dir, mc_version_for(&paths, &sel), loaders))
}

fn discover_project_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, v: serde_json::Value) {
    std::thread::spawn(move || {
        let seq = v.get("seq").and_then(|x| x.as_u64()).unwrap_or(0);
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_owned();
        let source = v.get("source").and_then(|x| x.as_str()).unwrap_or("modrinth");
        let fit = discover_fit(&shared, &v);
        let res = match source {
            "curseforge" => match cf_key(&shared) {
                None => Err(curseforge::NO_KEY.to_owned()),
                Some(key) => curseforge::project(&curseforge::Live { key }, &id).map(|mut p| {
                    if let Some((dir, _, _)) = &fit {
                        let t = p["project_type"].as_str().unwrap_or("mod").to_owned();
                        p["installed"] = json!(id.parse::<u64>().is_ok_and(|i| curseforge::installed_ids(&dir.join(discover_folder(&t))).contains(&i)));
                    }
                    p
                }),
            },
            _ => discover::modrinth_project(&id).map(|mut p| {
                if let Some((dir, _, _)) = &fit {
                    let t = p["project_type"].as_str().unwrap_or("mod").to_owned();
                    if matches!(t.as_str(), "mod" | "resourcepack" | "shader") {
                        let pid = p["id"].as_str().unwrap_or("").to_owned();
                        p["installed"] = json!(installed_projects_cached(&dir.join(discover_folder(&t))).contains_key(&pid));
                    }
                }
                p
            }),
        };
        match res {
            Ok(p) => call_js(&proxy, "onDiscoverProject", json!({"seq": seq, "project": p})),
            Err(e) => call_js(&proxy, "onDiscoverProject", json!({"seq": seq, "error": e})),
        }
    });
}

fn discover_versions_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, v: serde_json::Value) {
    std::thread::spawn(move || {
        let seq = v.get("seq").and_then(|x| x.as_u64()).unwrap_or(0);
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_owned();
        let source = v.get("source").and_then(|x| x.as_str()).unwrap_or("modrinth");
        let ptype = v.get("projectType").and_then(|x| x.as_str()).unwrap_or("mod");
        let fit = discover_fit(&shared, &v);
        let mc = fit.as_ref().filter(|_| ptype != "shader" && ptype != "modpack" && ptype != "world").map(|f| f.1.clone());
        let loaders: Vec<&str> = if ptype == "mod" { fit.as_ref().map(|f| f.2.clone()).unwrap_or_default() } else { Vec::new() };
        let res = match source {
            "curseforge" => match cf_key(&shared) {
                None => Err(curseforge::NO_KEY.to_owned()),
                Some(key) => curseforge::files(&curseforge::Live { key }, &id, mc.as_deref(), &loaders),
            },
            _ => discover::modrinth_versions(&id, mc.as_deref(), &loaders),
        };
        match res {
            Ok(list) => call_js(&proxy, "onDiscoverVersions", json!({"seq": seq, "id": id, "versions": list, "mc": mc, "loaders": loaders})),
            Err(e) => call_js(&proxy, "onDiscoverVersions", json!({"seq": seq, "id": id, "error": e})),
        }
    });
}

/// A plan that needs the player's say: the warning dialog, with what to send for "Install anyway".
fn send_discover_plan(proxy: &EventLoopProxy<UserEvent>, summary: &mod_plan::Summary, key: &str, project_type: &str, profile_id: &str, retry: serde_json::Value) {
    call_js(proxy, "onModPlan", json!({"slug": key, "projectType": project_type, "profileId": profile_id, "plan": summary, "retry": retry}));
    push_install_status(proxy, key, "blocked", "", profile_id);
}

/// Routes an install from Discover. Modrinth's newest compatible version keeps going through
/// download_modrinth_async; a specific version, data packs, worlds and CurseForge have their own
/// paths below, all planned (dependencies, conflicts, replaced copies) before anything is saved.
fn discover_install(proxy: &EventLoopProxy<UserEvent>, shared: &Arc<Mutex<AppState>>, v: &serde_json::Value) {
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
    let source = text("source");
    let id = text("id");
    let key = Some(text("key")).filter(|k| !k.is_empty()).unwrap_or_else(|| id.clone());
    let version_id = Some(text("versionId")).filter(|x| !x.is_empty());
    let profile_id = text("profileId");
    let force = v.get("force").and_then(|f| f.as_bool()).unwrap_or(false);
    let ptype = match text("projectType").as_str() {
        t @ ("mod" | "resourcepack" | "shader" | "modpack" | "datapack" | "world") => t.to_owned(),
        _ => "mod".to_owned(),
    };
    let retry = json!({"action": "discover_install", "source": source, "id": id, "key": key, "versionId": version_id, "projectType": ptype, "profileId": profile_id, "world": text("world"), "title": text("title")});
    match (source.as_str(), ptype.as_str()) {
        ("curseforge", "modpack") => install_cf_modpack_async(proxy.clone(), shared.clone(), key, id, version_id),
        ("curseforge", _) => install_cf_async(proxy.clone(), shared.clone(), key, id, version_id, ptype, profile_id, force, text("world"), retry),
        (_, "modpack") => install_modpack_async(
            proxy.clone(),
            shared.clone(),
            match version_id {
                Some(ver) => PackSource::ModrinthVersion(id, ver),
                None => PackSource::Modrinth(id),
            },
        ),
        // Java resource packs work in Horizon too: a Modrinth pack goes into a Horizon instance's
        // resourcepacks/ for Minecraft 26.3, checked by its install plan like any other.
        ("modrinth", "resourcepack") if discover_target(shared, &profile_id).is_some_and(|t| t.2 == HORIZON_ID) => {
            let (proxy, shared, key, pid) = (proxy.clone(), shared.clone(), key.clone(), profile_id.clone());
            std::thread::spawn(move || {
                let Some((pid, dir, _, _)) = discover_target(&shared, &pid) else { return };
                push_install_status(&proxy, &key, "installing", &format!("Installing {id}..."), &pid);
                match plan_and_install_modrinth(&proxy, &dir, &id, "resourcepack", HORIZON_MC, "minecraft", None, force) {
                    PlanOutcome::Installed(msg) => push_install_status(&proxy, &key, "installed", &msg, &pid),
                    PlanOutcome::Blocked(summary) => send_plan(&proxy, &summary, &key, "resourcepack", &pid),
                    PlanOutcome::Failed(err) => push_install_status(&proxy, &key, "error", &err, &pid),
                }
            });
        }
        // Horizon registry ids and Modrinth slugs are different namespaces: sent to the other kind
        // of instance, download_modrinth_async would look the name up in the wrong one.
        (src, _) if discover_target(shared, &profile_id).is_some_and(|t| (t.2 == HORIZON_ID) != (src == "horizon")) => {
            let msg = if src == "horizon" { "Horizon's own mods only go into Horizon 26.3 instances." } else { "Horizon 26.3 instances install from Horizon's own registry." };
            push_install_status(proxy, &key, "error", msg, &profile_id)
        }
        ("horizon", _) => download_modrinth_async(proxy.clone(), shared.clone(), id, ptype, profile_id, force),
        (_, "world") => push_install_status(proxy, &key, "error", "Modrinth doesn't host worlds.", &profile_id),
        (_, "datapack") => install_modrinth_version_async(proxy.clone(), shared.clone(), key, id, version_id, ptype, profile_id, force, text("world"), retry),
        (_, _) if version_id.is_some() => install_modrinth_version_async(proxy.clone(), shared.clone(), key, id, version_id, ptype, profile_id, force, String::new(), retry),
        (_, _) => download_modrinth_async(proxy.clone(), shared.clone(), id, ptype, profile_id, force),
    }
}

/// `saves/<world>/datapacks` of an instance, for a world the player picked.
fn datapack_dir(game_dir: &Path, world: &str) -> Result<PathBuf, String> {
    if world.is_empty() || !safe_file_name(world) || !game_dir.join("saves").join(world).join("level.dat").is_file() {
        return Err("Pick a world of this instance to add the data pack to.".into());
    }
    Ok(game_dir.join("saves").join(world).join("datapacks"))
}

/// Plans and installs one specific Modrinth version (the Versions tab, or an update).
#[allow(clippy::too_many_arguments)]
fn plan_and_install_pinned(proxy: &EventLoopProxy<UserEvent>, game_dir: &Path, version: serde_json::Value, project_type: &str, mc_ver: &str, loader: &str, fabric_loader: Option<(u64, u64, u64)>, force: bool) -> PlanOutcome {
    let dest = game_dir.join(discover_folder(project_type));
    let installed = match mod_plan::identify(&mod_plan::Live, mod_plan::scan_dir(&dest)) {
        Ok(i) => i,
        Err(e) => return PlanOutcome::Failed(e),
    };
    let loaders = forge::compatible_modrinth_loaders(loader);
    let extra = discover::pinned_problem(&version, project_type, mc_ver, &loaders);
    let pinned = discover::Pinned { inner: &mod_plan::Live, version };
    let project = pinned.project_id().to_owned();
    let mut plan = match mod_plan::plan_modrinth(&pinned, &project, project_type, mc_ver, loader, &installed) {
        Ok(p) => p,
        Err(e) => return PlanOutcome::Failed(e),
    };
    if let Some(p) = extra {
        plan.summary.problems.insert(0, p);
    }
    if plan.summary.blocked() && !force {
        return PlanOutcome::Blocked(plan.summary);
    }
    let download = |f: &mod_plan::FileRef, p: &Path| net::download_verified(&f.url, p, f.sha1.as_deref(), f.size);
    let loader_ok = |p: &Path| project_type != "mod" || loader != "fabric" || jar_matches_fabric_loader(p, fabric_loader);
    let log = |l: &str| push_log(proxy, &format!("[Modrinth] {l}"));
    match mod_plan::execute_modrinth(&plan, &dest, force, &download, &loader_ok, &log) {
        Ok(done) => PlanOutcome::Installed(plan.summary.success_message(&format!(" (Minecraft {mc_ver})"), &done.warnings)),
        Err(mod_plan::ExecError::Blocked(problem)) => {
            let mut s = plan.summary;
            s.problems.push(problem);
            PlanOutcome::Blocked(s)
        }
        Err(mod_plan::ExecError::Failed(e)) => PlanOutcome::Failed(e),
    }
}

#[allow(clippy::too_many_arguments)]
fn install_modrinth_version_async(
    proxy: EventLoopProxy<UserEvent>,
    shared: Arc<Mutex<AppState>>,
    key: String,
    project: String,
    version_id: Option<String>,
    project_type: String,
    profile_id: String,
    force: bool,
    world: String,
    retry: serde_json::Value,
) {
    std::thread::spawn(move || {
        let Some((pid, game_dir, selected, paths)) = discover_target(&shared, &profile_id) else {
            push_install_status(&proxy, &key, "error", GONE_INSTANCE, &profile_id);
            return;
        };
        let status = |state: &str, msg: &str| push_install_status(&proxy, &key, state, msg, &pid);
        if selected == HORIZON_ID {
            status("error", "Horizon 26.3 instances install from Horizon's own registry.");
            return;
        }
        let mc_ver = mc_version_for(&paths, &selected);
        let loader = mod_loader(&paths, &selected);
        if project_type == "mod" && loader.is_none() {
            status("error", &format!("Mods need a mod loader. Switch this instance to \"Fabric {mc_ver}\" or \"NeoForge {mc_ver}\" first (version menu next to Launch)."));
            return;
        }
        status("installing", "Checking compatibility...");
        let version = match &version_id {
            Some(vid) => mod_plan::Live.version(vid),
            None => mod_plan::Live
                .versions(&project, Some(&mc_ver), &["datapack"])
                .and_then(|list| list.into_iter().find(|v| v.get("version_type").and_then(|t| t.as_str()) == Some("release")).ok_or_else(|| format!("No version of this works with Minecraft {mc_ver}."))),
        };
        let version = match version {
            Ok(v) => v,
            Err(e) => return status("error", &e),
        };
        if project_type == "datapack" {
            let dir = match datapack_dir(&game_dir, &world) {
                Ok(d) => d,
                Err(e) => return status("error", &e),
            };
            let file = version.get("files").and_then(|f| f.as_array()).and_then(|a| a.iter().find(|f| f.get("primary").and_then(|p| p.as_bool()) == Some(true)).or_else(|| a.first()).cloned());
            let Some(file) = file else { return status("error", "This version has no file to download.") };
            let name = file.get("filename").and_then(|n| n.as_str()).and_then(|n| Path::new(n).file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let url = file.get("url").and_then(|u| u.as_str()).unwrap_or("");
            if !safe_file_name(&name) || !url.starts_with("https://cdn.modrinth.com/") {
                return status("error", "Modrinth listed a file Lumos can't use.");
            }
            let dest = dir.join(&name);
            return match net::download_verified(url, &dest, file.pointer("/hashes/sha1").and_then(|h| h.as_str()), file.get("size").and_then(|s| s.as_u64())) {
                Ok(()) => status("installed", &format!("Added {name} to the world \"{world}\".")),
                Err(e) => status("error", &e),
            };
        }
        let fabric_loader = detect_fabric_loader_version(&paths, &selected);
        match plan_and_install_pinned(&proxy, &game_dir, version, &project_type, &mc_ver, loader.unwrap_or("fabric"), fabric_loader, force) {
            PlanOutcome::Installed(msg) => {
                status("installed", &msg);
                push_content(&proxy, &game_dir, discover_folder(&project_type), &pid, false);
            }
            PlanOutcome::Blocked(summary) => send_discover_plan(&proxy, &summary, &key, &project_type, &pid, retry),
            PlanOutcome::Failed(err) => status("error", &err),
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn install_cf_async(
    proxy: EventLoopProxy<UserEvent>,
    shared: Arc<Mutex<AppState>>,
    key: String,
    id: String,
    file_id: Option<String>,
    project_type: String,
    profile_id: String,
    force: bool,
    world: String,
    retry: serde_json::Value,
) {
    std::thread::spawn(move || {
        let Some(api_key) = cf_key(&shared) else {
            push_install_status(&proxy, &key, "error", curseforge::NO_KEY, &profile_id);
            return;
        };
        let api = curseforge::Live { key: api_key };
        let Some((pid, game_dir, selected, paths)) = discover_target(&shared, &profile_id) else {
            push_install_status(&proxy, &key, "error", GONE_INSTANCE, &profile_id);
            return;
        };
        let status = |state: &str, msg: &str| push_install_status(&proxy, &key, state, msg, &pid);
        let (Ok(mod_id), file_id) = (id.parse::<u64>(), file_id.and_then(|f| f.parse::<u64>().ok())) else {
            return status("error", "That CurseForge project id isn't valid.");
        };
        if selected == HORIZON_ID {
            return status("error", "Horizon 26.3 instances install from Horizon's own registry.");
        }
        let mc_ver = mc_version_for(&paths, &selected);
        let loader = mod_loader(&paths, &selected);
        if project_type == "mod" && loader.is_none() {
            return status("error", &format!("Mods need a mod loader. Switch this instance to \"Fabric {mc_ver}\" or \"NeoForge {mc_ver}\" first."));
        }
        let dest = match project_type.as_str() {
            "datapack" => match datapack_dir(&game_dir, &world) {
                Ok(d) => d,
                Err(e) => return status("error", &e),
            },
            "world" => paths.cache.join("worlds"),
            t => game_dir.join(discover_folder(t)),
        };
        status("installing", "Checking compatibility...");
        let installed = if project_type == "world" { Default::default() } else { curseforge::read_sidecar(&dest) };
        let plan = match curseforge::plan(&api, mod_id, file_id, &project_type, &mc_ver, loader, &installed) {
            Ok(p) => p,
            Err(curseforge::PlanError::Manual(title, url)) => {
                call_js(&proxy, "onDiscoverManual", json!({"key": key, "title": title, "url": url, "profileId": pid}));
                return status("blocked", "");
            }
            Err(curseforge::PlanError::Failed(e)) => return status("error", &e),
        };
        if plan.summary.blocked() && !force {
            return send_discover_plan(&proxy, &plan.summary, &key, &project_type, &pid, retry);
        }
        let download = |url: &str, p: &Path, sha1: Option<&str>, size: Option<u64>| net::download_verified(url, p, sha1, size);
        let log = |l: &str| push_log(&proxy, &format!("[CurseForge] {l}"));
        if project_type == "world" {
            let res = curseforge::execute(&plan, &dest, &download, &log).and_then(|_| {
                let zip = dest.join(&plan.main.file_name);
                let r = curseforge::install_world(&zip, &game_dir.join("saves"), &plan.summary.title);
                let _ = fs::remove_file(&zip);
                r
            });
            return match res {
                Ok(folder) => status("installed", &format!("Added the world \"{folder}\" to this instance.")),
                Err(e) => status("error", &e),
            };
        }
        match curseforge::execute(&plan, &dest, &download, &log) {
            Ok(warnings) => {
                status("installed", &plan.summary.success_message(&format!(" (Minecraft {mc_ver})"), &warnings));
                if project_type != "datapack" {
                    push_content(&proxy, &game_dir, discover_folder(&project_type), &pid, false);
                }
            }
            Err(e) => status("error", &e),
        }
    });
}

/// Makes a new instance from a CurseForge modpack (manifest.json + overrides).
fn install_cf_modpack_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, key: String, id: String, file_id: Option<String>) {
    let Some(api_key) = cf_key(&shared) else {
        push_install_status(&proxy, &key, "error", curseforge::NO_KEY, "");
        return;
    };
    {
        let mut st = lock(&shared);
        if st.task.is_some() {
            push_install_status(&proxy, &key, "error", "LumosClient is busy. Try again in a moment.", "");
            return;
        }
        st.task = Some(("Reading the modpack".into(), -1.0));
        push_state(&proxy, &st);
    }
    std::thread::spawn(move || {
        let api = curseforge::Live { key: api_key };
        let reporter = TaskReporter::new(&proxy, &shared);
        let report = |l: &str, f: f64| reporter.report(l, f);
        let (base_dir, paths) = {
            let st = lock(&shared);
            (st.base_dir.clone(), st.paths.clone())
        };
        let mut manual_page = None;
        let mut done_msg = String::new();
        let result = run_guarded(|| {
            let mod_id = id.parse::<u64>().map_err(|_| "That CurseForge project id isn't valid.".to_owned())?;
            let plan = match curseforge::plan(&api, mod_id, file_id.and_then(|f| f.parse().ok()), "modpack", "", None, &Default::default()) {
                Ok(p) => p,
                Err(curseforge::PlanError::Manual(title, url)) => {
                    let e = format!("{title} can only be downloaded on CurseForge's website.");
                    manual_page = Some((title, url));
                    return Err(e);
                }
                Err(curseforge::PlanError::Failed(e)) => return Err(e),
            };
            let downloads = paths.cache.join("modpacks");
            report(&format!("Downloading {}", plan.summary.title), -1.0);
            curseforge::execute(&plan, &downloads, &|u: &str, p: &Path, h: Option<&str>, s: Option<u64>| net::download_verified(u, p, h, s), &|_| {})?;
            let zip = downloads.join(&plan.main.file_name);
            let outcome = (|| {
                let m = curseforge::read_manifest(&zip)?;
                let version = curseforge::version_spec(&m)?;
                let mut pid = format!("prof-{}", now_millis());
                while lock(&shared).config.profiles.iter().any(|p| p.id == pid) {
                    pid.push('x');
                }
                let dir = base_dir.join("instances").join(&pid);
                let filled = fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| {
                    let manual = curseforge::download_pack_files(&api, &m, &dir, &report)?;
                    report(&format!("Unpacking {}", m.name), -1.0);
                    modpack::extract_overrides(&zip, &dir)?;
                    Ok(manual)
                });
                let manual = match filled {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = fs::remove_dir_all(&dir);
                        return Err(e);
                    }
                };
                let name: String = m.name.chars().take(40).collect();
                let mut st = lock(&shared);
                st.config.profiles.push(Profile { id: pid.clone(), name: name.clone(), version: version.clone(), ..Default::default() });
                st.config.active_profile_id = pid;
                st.config.selected_version = version.clone();
                st.save_config();
                let mut msg = format!("{name} is ready as a new instance ({version}). Press Play to finish setting it up.");
                if !manual.is_empty() {
                    reporter.log(&format!("[CurseForge] Download these on CurseForge's website and add them to the instance: {}", manual.join(", ")));
                    msg.push_str(&format!(" {} file(s) can only be downloaded on CurseForge's website: see Logs.", manual.len()));
                }
                Ok(msg)
            })();
            let _ = fs::remove_file(&zip);
            done_msg = outcome?;
            Ok(())
        });
        {
            let mut st = lock(&shared);
            st.task = None;
            push_state(&proxy, &st);
        }
        match result {
            Ok(()) => push_install_status(&proxy, &key, "installed", &done_msg, ""),
            Err(e) => {
                push_log(&proxy, &format!("[LumosClient] modpack: {e}"));
                if let Some((title, url)) = manual_page {
                    // The page adds "can only be downloaded on CurseForge's website" itself.
                    call_js(&proxy, "onDiscoverManual", json!({"key": key, "title": title, "url": url, "profileId": ""}));
                    push_install_status(&proxy, &key, "blocked", "", "");
                } else {
                    push_install_status(&proxy, &key, "error", &e, "");
                }
            }
        }
        call_js(&proxy, "onTask", json!({"done": true}));
    });
}

fn discover_updates_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, profile_id: String) {
    std::thread::spawn(move || {
        let Some((pid, dir, sel, paths)) = discover_target(&shared, &profile_id) else {
            return call_js(&proxy, "onDiscoverUpdates", json!({"profileId": profile_id, "updates": []}));
        };
        if sel == HORIZON_ID {
            return call_js(&proxy, "onDiscoverUpdates", json!({"profileId": pid, "updates": []}));
        }
        match discover::check_updates(&dir, &mc_version_for(&paths, &sel), mod_loader(&paths, &sel)) {
            Ok(u) => call_js(&proxy, "onDiscoverUpdates", json!({"profileId": pid, "updates": u})),
            Err(e) => call_js(&proxy, "onDiscoverUpdates", json!({"profileId": pid, "updates": [], "error": e})),
        }
    });
}

/// Updates every file that has a newer compatible version, one at a time (each one replaces
/// its old file only once the new one and its dependencies are saved).
fn discover_update_all_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, profile_id: String) {
    std::thread::spawn(move || {
        let Some((pid, dir, sel, paths)) = discover_target(&shared, &profile_id) else {
            return call_js(&proxy, "onNotice", json!({"message": GONE_INSTANCE, "kind": "err"}));
        };
        let mc = mc_version_for(&paths, &sel);
        let loader = mod_loader(&paths, &sel);
        let updates = match discover::check_updates(&dir, &mc, loader) {
            Ok(u) => u,
            Err(e) => return call_js(&proxy, "onNotice", json!({"message": e, "kind": "err"})),
        };
        let fabric_loader = detect_fabric_loader_version(&paths, &sel);
        let (mut done, mut held) = (Vec::new(), Vec::new());
        for (i, u) in updates.iter().enumerate() {
            call_js(&proxy, "onDiscoverUpdateProgress", json!({"profileId": pid, "done": i, "total": updates.len(), "file": u.file}));
            let outcome = match mod_plan::Live.version(&u.version_id) {
                Ok(v) => plan_and_install_pinned(&proxy, &dir, v, &u.project_type, &mc, loader.unwrap_or("fabric"), fabric_loader, false),
                Err(e) => PlanOutcome::Failed(e),
            };
            match outcome {
                PlanOutcome::Installed(_) => done.push(u.file.clone()),
                PlanOutcome::Blocked(s) => held.push(format!("{} ({})", s.title, s.problems.iter().find(|p| p.blocking).map(|p| p.message.as_str()).unwrap_or("needs a look"))),
                PlanOutcome::Failed(e) => held.push(format!("{} ({e})", u.file)),
            }
        }
        for f in ["mods", "resourcepacks", "shaderpacks"] {
            push_content(&proxy, &dir, f, &pid, false);
        }
        let mut msg = if done.is_empty() { "Nothing was updated.".to_owned() } else { format!("Updated {} file{}.", done.len(), if done.len() == 1 { "" } else { "s" }) };
        if !held.is_empty() {
            msg.push_str(&format!(" Left alone: {}", held.join("; ")));
        }
        call_js(&proxy, "onNotice", json!({"message": msg, "kind": if held.is_empty() { "ok" } else { "info" }}));
        let left = discover::check_updates(&dir, &mc, loader).unwrap_or_default();
        call_js(&proxy, "onDiscoverUpdates", json!({"profileId": pid, "updates": left, "finished": true}));
    });
}

fn discover_suggest_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, profile_id: String) {
    std::thread::spawn(move || {
        let Some((pid, dir, sel, paths)) = discover_target(&shared, &profile_id) else { return };
        let Some(loader) = mod_loader(&paths, &sel).filter(|_| sel != HORIZON_ID) else {
            return call_js(&proxy, "onDiscoverSuggest", json!({"profileId": pid, "cards": []}));
        };
        match discover::suggest(&dir, &mc_version_for(&paths, &sel), loader) {
            Ok(mut v) => {
                v["profileId"] = json!(pid);
                call_js(&proxy, "onDiscoverSuggest", v);
            }
            Err(e) => call_js(&proxy, "onDiscoverSuggest", json!({"profileId": pid, "cards": [], "error": e})),
        }
    });
}
// ───────────────────────── end feat/discover ─────────────────────────

/// Performance mods "Boost FPS" adds to a Fabric instance (each only when Modrinth has it for
/// the instance's version). RetiNO stops a Retina Mac from rendering at 4x the pixels.
fn boost_mods() -> Vec<&'static str> {
    let mut v = vec!["sodium", "lithium", "ferrite-core", "immediatelyfast", "entityculling", "moreculling", "dynamic-fps"];
    if cfg!(target_os = "macos") {
        v.push("retino");
    }
    v
}

/// Java settings that cut garbage-collection stutter (G1 tuned for a game client).
const BOOST_JVM_ARGS: &str = "-XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200 -XX:+UnlockExperimentalVMOptions -XX:+DisableExplicitGC -XX:G1NewSizePercent=30 -XX:G1MaxNewSizePercent=40 -XX:G1HeapRegionSize=8M -XX:G1ReservePercent=20 -XX:G1HeapWastePercent=5 -XX:G1MixedGCCountTarget=4 -XX:InitiatingHeapOccupancyPercent=15";

/// Adds the performance mods missing from a Fabric instance and, unless the player set their
/// own, the tuned Java settings.
/// Horizon settings that cost frames for nothing visible; all are Horizon's own options.
const HORIZON_BOOST: &[(&str, &str)] = &[
    ("entityCulling", "async"), ("particleCulling", "async"), ("leafCulling", "hidden"), ("caveCulling", "true"),
    ("blockEntityCacheShulkerBoxes", "true"), ("blockEntityCacheSigns", "true"), ("blockEntityCacheDecoratedPots", "true"),
    ("blockEntityCacheBells", "true"), ("blockEntityCacheChests", "true"), ("blockEntityCacheBanners", "true"),
    ("blockEntityCacheCopperGolemStatues", "true"),
];

/// Writes HORIZON_BOOST into the instance's horizon-graphics.txt (kept once as .before-lumos-boost).
/// Returns how many settings changed.
fn boost_horizon_graphics(game_dir: &Path) -> Result<usize, String> {
    let path = game_dir.join("horizon-graphics.txt");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let backup = game_dir.join("horizon-graphics.txt.before-lumos-boost");
    if !text.is_empty() && !backup.exists() {
        let _ = fs::write(&backup, &text);
    }
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut changed = 0;
    for (k, v) in HORIZON_BOOST {
        let want = format!("{k}:{v}");
        match lines.iter_mut().find(|l| l.split_once(':').is_some_and(|(key, _)| key == *k)) {
            Some(l) if *l == want => {}
            Some(l) => { *l = want; changed += 1; }
            None => { lines.push(want); changed += 1; }
        }
    }
    if changed > 0 {
        if let Some(l) = lines.iter_mut().find(|l| l.starts_with("preset:")) { *l = "preset:custom".into(); }
        fs::write(&path, lines.join("\n") + "\n").map_err(|e| format!("Couldn't save Horizon's graphics settings: {e}"))?;
    }
    Ok(changed)
}

fn boost_instance_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, profile_id: String) {
    std::thread::spawn(move || {
        let target = {
            let st = shared.lock().unwrap();
            st.target_instance(&profile_id).map(|(pid, dir, ver)| (pid, dir, ver, st.paths.clone()))
        };
        let Some((pid, game_dir, selected, paths)) = target else {
            call_js(&proxy, "onNotice", json!({"message": GONE_INSTANCE, "kind": "err"}));
            return;
        };
        if selected == HORIZON_ID {
            // Horizon 26.3: switch on the culling and caches Horizon has but leaves off (no visual change).
            let msg = match boost_horizon_graphics(&game_dir) {
                Ok(0) => "Horizon's culling and block-entity caches were already on for this instance.".to_owned(),
                Ok(n) => format!("Boosted Horizon: entity, particle and hidden-leaf culling plus block-entity caches are on ({n} settings changed). The old settings are saved beside them. Restart the game to apply."),
                Err(e) => e,
            };
            call_js(&proxy, "onNotice", json!({"message": msg, "kind": "ok"}));
            let _ = pid;
            return;
        }
        if !is_fabric_version(&selected) {
            call_js(&proxy, "onNotice", json!({"message": "Boost FPS works on Fabric and Horizon instances. Switch this instance to a Fabric version first.", "kind": "err"}));
            return;
        }
        let mc_ver = mc_version_for(&paths, &selected);
        let loader = detect_fabric_loader_version(&paths, &selected);
        let have = installed_projects(&game_dir.join("mods"));
        let mut added = Vec::new();
        let mut skipped = Vec::new();
        let mut conflicted: Vec<String> = Vec::new();
        for slug in boost_mods() {
            // Modrinth answers by project id; ask for the slug's id to compare with what's there.
            let id = net::get_json(&format!("https://api.modrinth.com/v2/project/{slug}")).ok().and_then(|v| v.get("id").and_then(|i| i.as_str()).map(str::to_owned));
            if id.as_ref().is_some_and(|i| have.contains_key(i)) {
                continue;
            }
            call_js(&proxy, "onNotice", json!({"message": format!("Adding {slug}..."), "kind": "info"}));
            match plan_and_install_modrinth(&proxy, &game_dir, slug, "mod", &mc_ver, "fabric", loader, false) {
                PlanOutcome::Installed(_) => added.push(slug),
                PlanOutcome::Blocked(s) => conflicted.push(format!("{} ({})", s.title, s.problems.iter().find(|p| p.blocking).map(|p| p.message.as_str()).unwrap_or(""))),
                PlanOutcome::Failed(_) => skipped.push(slug),
            }
        }
        {
            let mut st = shared.lock().unwrap();
            let global_empty = st.config.jvm_args.trim().is_empty();
            if let Some(p) = st.config.profiles.iter_mut().find(|p| p.id == pid) {
                if p.jvm_args.as_deref().unwrap_or("").trim().is_empty() && global_empty {
                    p.jvm_args = Some(BOOST_JVM_ARGS.to_owned());
                }
            }
            st.save_config();
            push_state(&proxy, &st);
        }
        push_content(&proxy, &game_dir, "mods", &pid, false);
        let mut msg = if added.is_empty() { "This instance already had every performance mod.".to_owned() } else { format!("Added {}.", added.join(", ")) };
        if !skipped.is_empty() {
            msg.push_str(&format!(" Not available for Minecraft {mc_ver}: {}.", skipped.join(", ")));
        }
        if !conflicted.is_empty() {
            msg.push_str(&format!(" Left out because of conflicts: {}", conflicted.join(" ")));
        }
        msg.push_str(" Tuned Java settings are on for this instance.");
        call_js(&proxy, "onNotice", json!({"message": msg, "kind": "ok"}));
    });
}

// ───────────────────────── Modpacks (modpack.rs) ─────────────────────────

enum PackSource {
    /// A `.mrpack` the player picked: (file name, base64 data).
    File(String, String),
    /// A Modrinth modpack project (slug or id) from Discover.
    Modrinth(String),
    /// feat/discover: one version of a Modrinth modpack (project, version id).
    ModrinthVersion(String, String),
}

/// Makes a new instance from a Modrinth modpack: its version, every file it lists, its overrides.
fn install_modpack_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, source: PackSource) {
    {
        let mut st = lock(&shared);
        if st.task.is_some() {
            call_js(&proxy, "onNotice", json!({"message": "LumosClient is busy getting a game ready. Try again in a moment.", "kind": "err"}));
            if let PackSource::Modrinth(slug) | PackSource::ModrinthVersion(slug, _) = &source {
                push_install_status(&proxy, slug, "error", "LumosClient is busy. Try again in a moment.", "");
            }
            return;
        }
        st.task = Some(("Reading the modpack".into(), -1.0));
        push_state(&proxy, &st);
    }
    std::thread::spawn(move || {
        let reporter = TaskReporter::new(&proxy, &shared);
        let report = |l: &str, f: f64| reporter.report(l, f);
        let (base_dir, paths) = {
            let st = lock(&shared);
            (st.base_dir.clone(), st.paths.clone())
        };
        let slug = match &source {
            PackSource::Modrinth(s) | PackSource::ModrinthVersion(s, _) => Some(s.clone()),
            PackSource::File(..) => None,
        };
        let downloads = paths.cache.join("modpacks");
        let mut done_msg = String::new();
        let result = run_guarded(|| {
            fs::create_dir_all(&downloads).map_err(|e| format!("Couldn't create {}: {e}", downloads.display()))?;
            let mrpack = match &source {
                PackSource::Modrinth(slug) => modpack::download_from_modrinth(slug, &downloads, &report)?,
                PackSource::ModrinthVersion(slug, ver) => modpack::download_version_from_modrinth(slug, ver, &downloads, &report)?,
                PackSource::File(name, data) => {
                    use base64::Engine as _;
                    if !name.to_lowercase().ends_with(".mrpack") {
                        return Err(format!("{name} isn't a Modrinth modpack (.mrpack)."));
                    }
                    if data.len() / 4 * 3 > MAX_IMPORT_BYTES {
                        return Err(format!("{name} is too big to import here."));
                    }
                    let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|_| format!("Couldn't read {name}."))?;
                    if bytes.len() < 4 || &bytes[..2] != b"PK" {
                        return Err(format!("{name} isn't a valid modpack file."));
                    }
                    let p = downloads.join(format!("import-{}.mrpack", now_millis()));
                    fs::write(&p, &bytes).map_err(|e| format!("Couldn't save {name}: {e}"))?;
                    p
                }
            };
            let outcome = (|| {
                let idx = modpack::read_index(&mrpack)?;
                let version = modpack::version_spec(&idx)?;
                let mut id = format!("prof-{}", now_millis());
                while lock(&shared).config.profiles.iter().any(|p| p.id == id) {
                    id.push('x');
                }
                let dir = base_dir.join("instances").join(&id);
                if let Err(e) = modpack::install(&mrpack, &idx, &dir, &report) {
                    // The folder is brand new and only holds this pack's files.
                    let _ = fs::remove_dir_all(&dir);
                    return Err(e);
                }
                let name: String = idx.name.chars().take(40).collect();
                let mut st = lock(&shared);
                st.config.profiles.push(Profile { id: id.clone(), name: name.clone(), version: version.clone(), ..Default::default() });
                st.config.active_profile_id = id;
                st.config.selected_version = version.clone();
                st.save_config();
                if let Some((k, v)) = &idx.loader {
                    reporter.log(&format!("[LumosClient] {name} was made with {k} {v}; this instance uses {version}."));
                }
                Ok(format!("{name} is ready as a new instance ({version}). Press Play to finish setting it up."))
            })();
            if matches!(source, PackSource::File(..)) {
                let _ = fs::remove_file(&mrpack);
            }
            done_msg = outcome?;
            Ok(())
        });
        {
            let mut st = lock(&shared);
            st.task = None;
            push_state(&proxy, &st);
        }
        match result {
            Ok(()) => {
                let msg = done_msg;
                if let Some(s) = &slug {
                    push_install_status(&proxy, s, "installed", &msg, "");
                }
                call_js(&proxy, "onTask", json!({"done": true, "message": if slug.is_some() { serde_json::Value::Null } else { json!(msg) }}));
            }
            Err(e) => {
                push_log(&proxy, &format!("[LumosClient] modpack: {e}"));
                if let Some(s) = &slug {
                    push_install_status(&proxy, s, "error", &e, "");
                    call_js(&proxy, "onTask", json!({"done": true}));
                } else {
                    call_js(&proxy, "onTask", json!({"error": e}));
                }
            }
        }
    });
}

// ───────────────────────── Import from other launchers ─────────────────────────

/// Finds other launchers' instances (import.rs) and lists them in the import dialog.
fn import_scan_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>) {
    std::thread::spawn(move || {
        let latest = lock(&shared).catalog.iter().find(|e| e.kind == "vanilla").map(|e| e.id.clone());
        let payload = match import::Roots::system() {
            Some(roots) => import::scan_json(&roots, latest.as_deref()),
            None => json!({"items": [], "launchers": [], "error": "Couldn't find your home folder."}),
        };
        call_js(&proxy, "onImportScan", payload);
    });
}

/// Copies the chosen instances into new Lumos instances, one after another, with progress in
/// the task strip. Each becomes a normal profile only once its copy finished.
fn import_run_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, keys: Vec<String>) {
    let chosen = import::take(&keys);
    if chosen.is_empty() {
        call_js(&proxy, "onImportDone", json!({"results": [], "error": "That list is out of date. Look for instances again."}));
        return;
    }
    {
        let mut st = lock(&shared);
        if st.task.is_some() || st.active_child.is_some() {
            call_js(&proxy, "onImportDone", json!({"results": [], "error": "LumosClient is busy (a game is running or getting ready). Try again when it's done."}));
            return;
        }
        st.task = Some(("Getting ready to copy".into(), -1.0));
        push_state(&proxy, &st);
    }
    import::CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
    std::thread::spawn(move || {
        let reporter = TaskReporter::new(&proxy, &shared);
        let (base_dir, latest) = {
            let st = lock(&shared);
            (st.base_dir.clone(), st.catalog.iter().find(|e| e.kind == "vanilla").map(|e| e.id.clone()))
        };
        let mut results = Vec::new();
        let mut cancelled = false;
        for f in &chosen {
            let name: String = f.name.chars().take(40).collect();
            let counts = import::counts(&f.game_dir);
            let plan = import::plan(f, counts.mods, latest.as_deref());
            let Some(target) = plan.target.clone() else {
                results.push(json!({"name": name, "ok": false, "error": plan.notes.join(" ")}));
                continue;
            };
            if cancelled {
                results.push(json!({"name": name, "ok": false, "error": "Not copied: the import was cancelled."}));
                continue;
            }
            reporter.report(&format!("Looking through {name}"), -1.0);
            let collected = import::collect(f, &plan);
            let mut id = format!("prof-{}", now_millis());
            while lock(&shared).config.profiles.iter().any(|p| p.id == id) || base_dir.join("instances").join(&id).exists() {
                id.push('x');
            }
            let dest = base_dir.join("instances").join(&id);
            let total = collected.bytes.max(1);
            let label = format!("Copying {name}");
            let out = run_guarded(|| {
                import::copy_into(&collected.items, &dest, &import::CANCEL, &|done| reporter.report(&label, done as f64 / total as f64))
            });
            match out {
                Ok(()) => {
                    {
                        let mut st = lock(&shared);
                        st.config.profiles.push(Profile { id: id.clone(), name: name.clone(), version: target.clone(), ..Default::default() });
                        st.config.active_profile_id = id.clone();
                        st.config.selected_version = target.clone();
                        st.save_config();
                        push_state(&proxy, &st);
                    }
                    let (copied, skipped) = import::summary(counts, &plan, collected.outside_links);
                    reporter.log(&format!("[LumosClient] Imported {name} from {} into {target} ({}).", f.source.label(), dest.display()));
                    results.push(json!({"name": name, "ok": true, "id": id, "target": if target == HORIZON_ID { "Horizon 26.3".to_owned() } else { target.clone() }, "copied": copied, "skipped": skipped}));
                }
                Err(e) => {
                    // The folder is brand new and only holds this import's files.
                    let _ = fs::remove_dir_all(&dest);
                    cancelled = e == "cancelled";
                    if !cancelled {
                        push_log(&proxy, &format!("[LumosClient] import of {name}: {e}"));
                    }
                    results.push(json!({"name": name, "ok": false, "error": if cancelled { "Cancelled. Nothing was kept from this copy.".to_owned() } else { e }}));
                }
            }
        }
        {
            let mut st = lock(&shared);
            st.task = None;
            push_state(&proxy, &st);
        }
        call_js(&proxy, "onTask", json!({"done": true}));
        call_js(&proxy, "onImportDone", json!({"results": results, "cancelled": cancelled}));
    });
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ───────────────────────── Ely.by ─────────────────────────

/// Loads the current account's skin/cape for the account menu (in the background).
fn refresh_skin_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>) {
    std::thread::spawn(move || {
        let name = {
            let st = shared.lock().unwrap();
            if st.config.account_kind == "elyby" {
                st.config.ely_session.as_ref().map(|s| s.name.clone())
            } else {
                None
            }
        };
        let Some(name) = name else {
            call_js(&proxy, "onSkin", json!({"name": null}));
            return;
        };
        match elyby::fetch_textures(&name) {
            Ok(t) => {
                let skin = t.skin_url.as_deref().and_then(|u| elyby::fetch_texture_data_uri(u).ok());
                let cape = t.cape_url.as_deref().and_then(|u| elyby::fetch_texture_data_uri(u).ok());
                call_js(&proxy, "onSkin", json!({"name": name, "skin": skin, "cape": cape, "slim": t.slim}));
            }
            Err(e) => call_js(&proxy, "onSkin", json!({"name": name, "error": e})),
        }
    });
}

// ───────────────────────── Lumos capes (begin) ─────────────────────────

/// Sends the cape picker its data: the catalog with previews and the cape this account wears.
fn capes_load_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>) {
    std::thread::spawn(move || {
        let (name, enabled) = {
            let st = shared.lock().unwrap();
            let name = (st.config.account_kind == "elyby").then(|| st.config.ely_session.as_ref().map(|s| s.name.clone())).flatten();
            (name, st.config.lumos_capes_enabled)
        };
        let Some(base) = capes::server_url() else {
            call_js(&proxy, "onCapes", json!({"configured": false, "enabled": enabled, "name": name, "error": capes::NOT_SET_UP}));
            return;
        };
        let list = match capes::catalog(&capes::Net, &base) {
            Ok(l) => l,
            Err(e) => {
                call_js(&proxy, "onCapes", json!({"configured": true, "enabled": enabled, "name": name, "error": e}));
                return;
            }
        };
        let current = name.as_deref().and_then(|n| capes::player_cape(&capes::Net, &base, n).ok().flatten());
        let capes: Vec<_> = list
            .iter()
            .map(|c| json!({"id": c.id, "name": c.name, "data": capes::texture_data_uri(&c.url).ok()}))
            .collect();
        call_js(&proxy, "onCapes", json!({"configured": true, "enabled": enabled, "name": name, "current": current, "capes": capes}));
    });
}

/// Wears a cape (None = no cape) on the signed-in Ely.by account.
fn capes_select_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, cape: Option<String>) {
    std::thread::spawn(move || {
        let reply = |r: serde_json::Value| call_js(&proxy, "onCapeSelected", r);
        let Some(base) = capes::server_url() else { return reply(json!({"error": capes::NOT_SET_UP})) };
        let session = {
            let st = shared.lock().unwrap();
            if st.config.account_kind == "elyby" { st.config.ely_session.clone() } else { None }
        };
        let Some(session) = session else { return reply(json!({"error": "Sign in with Ely.by to wear a Lumos cape."})) };
        let fresh = match elyby::ensure_fresh(&session) {
            Ok(f) => f,
            Err(e) => return reply(json!({"error": e.to_string()})),
        };
        if fresh != session {
            let mut st = shared.lock().unwrap();
            st.config.ely_session = Some(fresh.clone());
            st.save_config();
        }
        match capes::select(&capes::Net, &base, &fresh, cape.as_deref()) {
            Ok(()) => reply(json!({"ok": true, "cape": cape})),
            Err(e) => reply(json!({"error": e.to_string()})),
        }
    });
}

// ───────────────────────── Lumos capes (end) ─────────────────────────

fn ely_login_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, login: String, password: String, totp: Option<String>) {
    std::thread::spawn(move || {
        let client_token = shared.lock().unwrap().config.ely_client_token.clone();
        match elyby::authenticate(&login, &password, totp.as_deref(), &client_token) {
            Ok(session) => {
                {
                    let mut st = shared.lock().unwrap();
                    st.config.ely_session = Some(session.clone());
                    st.config.account_kind = "elyby".into();
                    st.save_config();
                    push_state(&proxy, &st);
                }
                call_js(&proxy, "onAccount", json!({"ok": true, "name": session.name}));
                refresh_skin_async(proxy.clone(), shared.clone());
            }
            Err(elyby::AuthError::NeedTotp) => call_js(&proxy, "onAccount", json!({"needTotp": true})),
            Err(e) => call_js(&proxy, "onAccount", json!({"error": e.to_string(), "hadTotp": totp.is_some()})),
        }
    });
}

/// Only Ely.by pages may open in the in-app browser.
/// A small guide pinned to Ely.by pages in the catalog window: how to wear a skin in one click.
const ELY_GUIDE_JS: &str = r#"(function () {
  if (!/(^|\.)ely\.by$/.test(location.hostname) || window.__lumosGuide) return;
  window.__lumosGuide = true;
  function show() {
    if (document.getElementById('lumos-guide') || !document.body) return;
    var signedOut = !!Array.prototype.find.call(document.querySelectorAll('a'), function (a) { return /^\s*log in\s*$/i.test(a.textContent); });
    var adding = /^\/skins\/add/.test(location.pathname);
    var text = adding
      ? 'Upload your skin here, then press ✓ on it to wear it. Close this window when you\'re done.'
      : signedOut
        ? 'Log in (top right) once with your Ely.by account. LumosClient remembers it.'
        : 'Click ✓ on any skin to wear it. Close this window when you\'re done; LumosClient updates your preview.';
    var d = document.createElement('div');
    d.id = 'lumos-guide';
    d.setAttribute('role', 'status');
    d.style.cssText = 'position:fixed;left:50%;bottom:18px;transform:translateX(-50%);z-index:2147483647;max-width:min(680px,calc(100vw - 32px));' +
      'background:#18181c;color:#f4f4f5;border:1px solid rgba(255,107,44,.55);border-radius:12px;padding:10px 14px;' +
      'font:600 13px/1.4 -apple-system,Segoe UI,sans-serif;box-shadow:0 12px 30px rgba(0,0,0,.45);display:flex;gap:10px;align-items:center';
    var t = document.createElement('span');
    t.textContent = 'LumosClient · ' + text;
    var x = document.createElement('button');
    x.textContent = '×';
    x.setAttribute('aria-label', 'Hide');
    x.style.cssText = 'background:none;border:0;color:#a1a1aa;font-size:18px;cursor:pointer;padding:0 2px';
    x.onclick = function () { d.remove(); };
    d.appendChild(t); d.appendChild(x);
    document.body.appendChild(d);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', show); else show();
})();"#;

fn allowed_ely_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.contains('@') && !host.contains(':') && (host == "ely.by" || host.ends_with(".ely.by"))
}

// ───────────────────────── Platform: Linux ─────────────────────────
// (feat/platforms) Linux needs the webview placed inside tao's GTK box: `build(&window)` only
// works on X11 there, `build_gtk` works on X11 and Wayland.

fn attach_webview<'a>(builder: WebViewBuilder<'a>, window: &'a Window) -> wry::Result<WebView> {
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().expect("tao windows have a GTK box by default");
        builder.build_gtk(vbox)
    }
    #[cfg(not(target_os = "linux"))]
    builder.build(window)
}

/// Linux: the window's app id / WM_CLASS ("lumos-client", matching lumos-client.desktop), and
/// WebKitGTK's DMA-BUF renderer off unless the player chose otherwise (it shows a blank window
/// with some GPU drivers).
fn linux_setup() {
    #[cfg(target_os = "linux")]
    {
        gtk::glib::set_prgname(Some("lumos-client"));
        gtk::glib::set_application_name("Lumos Client");
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
}
// ───────────────────────── end Platform: Linux ─────────────────────────

// ───────────────────────── Main ─────────────────────────

// ═════════════════ feat/updater: self-update, crash help, repair instance ═════════════════

fn updater_ipc(action: &str, v: &serde_json::Value, proxy: &EventLoopProxy<UserEvent>, shared: &Arc<Mutex<AppState>>) {
    match action {
        "check_update" => check_update_async(proxy.clone(), v.get("manual").and_then(|m| m.as_bool()).unwrap_or(false)),
        "install_update" => install_update(proxy, shared),
        "repair_instance" => repair_instance(proxy, shared, v.get("profileId").and_then(|x| x.as_str()).unwrap_or("")),
        _ => {}
    }
}

/// Asks GitHub whether a newer LumosClient exists (`LUMOS_NO_UPDATE_CHECK` skips the startup check).
fn check_update_async(proxy: EventLoopProxy<UserEvent>, manual: bool) {
    if !manual && std::env::var_os("LUMOS_NO_UPDATE_CHECK").is_some() {
        return;
    }
    std::thread::spawn(move || match updater::check(LAUNCHER_VERSION) {
        Ok(Some(r)) => {
            let can_install = updater::platform_asset_name().is_some_and(|n| updater::select_asset(&r.assets, n).is_some());
            call_js(&proxy, "onUpdate", json!({"available": true, "manual": manual, "current": LAUNCHER_VERSION, "release": r, "canInstall": can_install}));
        }
        Ok(None) => call_js(&proxy, "onUpdate", json!({"available": false, "manual": manual, "current": LAUNCHER_VERSION})),
        Err(e) => call_js(&proxy, "onUpdate", json!({"available": false, "manual": manual, "current": LAUNCHER_VERSION, "error": e})),
    });
}

/// Downloads, verifies and installs the newer release, then restarts. Never while a game runs.
fn install_update(proxy: &EventLoopProxy<UserEvent>, shared: &Arc<Mutex<AppState>>) {
    {
        let mut st = lock(shared);
        if st.active_child.is_some() {
            call_js(proxy, "onUpdate", json!({"error": "Close the game first: LumosClient never updates while a game is running."}));
            return;
        }
        if st.task.is_some() {
            call_js(proxy, "onUpdate", json!({"error": "Wait for the current download to finish, then update."}));
            return;
        }
        // Holding a task keeps launches out until the update is done.
        st.task = Some(("Updating LumosClient".into(), -1.0));
        push_state(proxy, &st);
    }
    let proxy = proxy.clone();
    let shared = shared.clone();
    std::thread::spawn(move || {
        let rep = TaskReporter::new(&proxy, &shared);
        let release = match updater::last_found() {
            Some(r) => Ok(r),
            None => updater::check(LAUNCHER_VERSION).and_then(|r| r.ok_or_else(|| "LumosClient is already up to date.".to_owned())),
        };
        let result = match release {
            Ok(r) => {
                let page = r.page_url.clone();
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| updater::install(&r, &|l, f| rep.report(l, f))))
                    .unwrap_or_else(|_| Err(updater::UpdateError::Failed("The update stopped unexpectedly. Nothing was changed.".into())))
                    .map_err(|e| (e, page))
            }
            Err(e) => Err((updater::UpdateError::Failed(e), updater::RELEASES_PAGE.to_owned())),
        };
        match result {
            Ok(relaunch) => {
                push_log(&proxy, "[LumosClient] Update installed. Restarting…");
                call_js(&proxy, "onUpdate", json!({"restarting": true}));
                std::thread::sleep(Duration::from_millis(700));
                updater::relaunch_and_exit(relaunch);
            }
            Err((e, page)) => {
                {
                    let mut st = lock(&shared);
                    st.task = None;
                    push_state(&proxy, &st);
                }
                push_log(&proxy, &format!("[LumosClient] Update didn't install: {}", e.message()));
                let manual = matches!(e, updater::UpdateError::Manual(_));
                if manual {
                    open_external(&page);
                }
                call_js(&proxy, "onUpdate", json!({"error": e.message(), "openedPage": manual}));
            }
        }
    });
}

/// (profile id, game folder, Horizon?, how long it ran) of the session that just ended.
fn crash_context(shared: &Arc<Mutex<AppState>>) -> Option<(String, PathBuf, bool, Duration)> {
    let st = lock(shared);
    let (pid, started) = st.session.as_ref()?;
    let (id, dir, ver) = st.target_instance(pid)?;
    Some((id, dir, ver == HORIZON_ID, started.elapsed()))
}

/// When the game closed with an error, explains why (window.onCrashHelp).
fn explain_crash_async(proxy: EventLoopProxy<UserEvent>, code: Option<i32>, ctx: Option<(String, PathBuf, bool, Duration)>) {
    let Some((pid, game_dir, horizon, ran_for)) = ctx else { return };
    if !crash_help::should_explain(code) {
        return;
    }
    std::thread::spawn(move || {
        // Give the game's last log lines a moment to land on disk.
        std::thread::sleep(Duration::from_millis(400));
        let d = crash_help::diagnose(&game_dir, horizon, ran_for, code, &pid);
        push_log(&proxy, &format!("[LumosClient] What happened: {}{}", d.title, d.culprit.as_ref().map(|c| format!(" (likely {c})")).unwrap_or_default()));
        call_js(&proxy, "onCrashHelp", json!(d));
    });
}

/// Re-checks an instance's files (game jar, libraries, assets, Java; Horizon's resources)
/// and downloads whatever is damaged or missing. Reports what it fixed (window.onRepair).
fn repair_instance(proxy: &EventLoopProxy<UserEvent>, shared: &Arc<Mutex<AppState>>, profile_id: &str) {
    let job = {
        let mut st = lock(shared);
        if st.active_child.is_some() || st.task.is_some() {
            call_js(proxy, "onRepair", json!({"profileId": profile_id, "error": "Wait for the current game or download to finish."}));
            return;
        }
        let Some((pid, game_dir, version)) = st.target_instance(profile_id) else {
            call_js(proxy, "onRepair", json!({"profileId": profile_id, "error": GONE_INSTANCE}));
            return;
        };
        st.task = Some(("Repairing instance".into(), -1.0));
        push_state(proxy, &st);
        (pid, game_dir, version, st.paths.clone(), st.base_dir.clone(), st.horizon_output())
    };
    let proxy = proxy.clone();
    let shared = shared.clone();
    std::thread::spawn(move || {
        let (pid, game_dir, version, paths, base_dir, horizon_output) = job;
        let rep = TaskReporter::new(&proxy, &shared);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| repair_inner(&shared, &rep, &paths, &base_dir, &game_dir, &version, &horizon_output)))
            .unwrap_or_else(|_| Err("Something went wrong inside LumosClient. Check Logs, then try again.".into()));
        let manifest = installer::manifest(&paths).ok();
        let catalog = installer::catalog(&paths, manifest.as_ref());
        {
            let mut st = lock(&shared);
            st.task = None;
            st.catalog = catalog;
            push_state(&proxy, &st);
        }
        match result {
            Err(e) => {
                push_log(&proxy, &format!("[LumosClient] Repair stopped: {e}"));
                // onRepair shows the error (an onTask error too would toast it a second time).
                call_js(&proxy, "onTask", json!({"done": true}));
                call_js(&proxy, "onRepair", json!({"profileId": pid, "error": e}));
            }
            Ok(fixed) => {
                let message = if fixed.is_empty() { "Everything checked out: nothing needed fixing.".to_owned() } else { format!("Repaired: {}.", fixed.join("; ")) };
                for f in &fixed {
                    push_log(&proxy, &format!("[LumosClient] Repair: {f}"));
                }
                call_js(&proxy, "onTask", json!({"done": true}));
                call_js(&proxy, "onRepair", json!({"profileId": pid, "fixed": fixed, "message": message}));
            }
        }
    });
}

fn repair_inner(
    shared: &Arc<Mutex<AppState>>,
    rep: &TaskReporter,
    paths: &installer::Paths,
    base_dir: &Path,
    game_dir: &Path,
    version: &str,
    horizon_output: &Path,
) -> Result<Vec<String>, String> {
    let report = |l: &str, f: f64| rep.report(l, f);
    let mut fixed = Vec::new();
    rep.report("Removing unfinished downloads", -1.0);
    let leftovers = repair::remove_part_files(&[
        paths.game.clone(),
        paths.runtime.clone(),
        paths.cache.clone(),
        paths.horizon_root(),
        game_dir.join("mods"),
        game_dir.join("resourcepacks"),
        game_dir.join("shaderpacks"),
    ]);
    if leftovers > 0 {
        fixed.push(format!("Removed {leftovers} unfinished download{}", if leftovers == 1 { "" } else { "s" }));
    }
    if version != HORIZON_ID {
        fixed.extend(repair::verify_version(paths, version, &report).fixed);
        installer::install(paths, version, &report)?;
        return Ok(fixed);
    }
    // Horizon: its Minecraft files and Java, then its prepared resources (`--check`).
    let native = native_builds::launch_parts(base_dir, game_dir);
    let (prepare, root, output) = match &native {
        Some((_, prepare, root, short)) => (prepare.clone(), root.clone(), horizon_output.with_file_name(format!("26.3-{short}"))),
        None => {
            let prepare = resolve_binary("horizon-prepare").ok_or("This LumosClient build is missing the Horizon preparer. Reinstall LumosClient.")?;
            let root = match horizon_support().ok_or("This LumosClient build is missing Horizon's support files. Reinstall LumosClient.")? {
                HorizonRoot::Dev(d) => d,
                HorizonRoot::Bundled(b) => {
                    let dest = paths.horizon_root();
                    installer::sync_horizon_root(&b, &dest)?;
                    dest
                }
            };
            (prepare, root, horizon_output.to_path_buf())
        }
    };
    let own = paths.own_only();
    if let Ok(mc) = installer::horizon_minecraft(&prepare, &root) {
        let found = repair::verify_version(&own, &mc, &report);
        if !found.fixed.is_empty() {
            installer::install(&own, &mc, &report)?;
        }
        fixed.extend(found.fixed);
    }
    rep.report("Checking Horizon's resources", -1.0);
    let out = installer::horizon_output(&prepare, &root, &output)?;
    if !installer::horizon_check(&prepare, &root, &out) {
        match native {
            Some(_) => {
                let log = |l: &str| rep.log(l);
                installer::ensure_horizon(paths, &prepare, &root, &output, &report, &log)?;
            }
            None => prepare_horizon(shared, rep, paths, &output)?,
        }
        fixed.push("Prepared Horizon's resources again".into());
    }
    Ok(fixed)
}

// ═════════════════ end feat/updater ═════════════════

fn main() {
    linux_setup();
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("LumosClient")
        .with_inner_size(tao::dpi::LogicalSize::new(1240.0, 780.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(980.0, 620.0))
        .build(&event_loop)
        .expect("Failed to create window");

    // Standard macOS menus: without an Edit menu, ⌘C/⌘V/⌘A don't work in text fields.
    #[cfg(target_os = "macos")]
    let _menu = {
        use muda::{Menu, PredefinedMenuItem as P, Submenu};
        let menu = Menu::new();
        let app = Submenu::new("LumosClient", true);
        let _ = app.append_items(&[&P::about(None, None), &P::separator(), &P::hide(None), &P::hide_others(None), &P::show_all(None), &P::separator(), &P::quit(None)]);
        let edit = Submenu::new("Edit", true);
        let _ = edit.append_items(&[&P::undo(None), &P::redo(None), &P::separator(), &P::cut(None), &P::copy(None), &P::paste(None), &P::select_all(None)]);
        let win = Submenu::new("Window", true);
        let _ = win.append_items(&[&P::minimize(None), &P::fullscreen(None), &P::separator(), &P::close_window(None)]);
        let _ = menu.append_items(&[&app, &edit, &win]);
        menu.init_for_nsapp();
        menu
    };

    let state = Arc::new(Mutex::new(AppState::new()));
    let ipc_state = state.clone();
    let ipc_proxy = proxy.clone();

    let b64 = |bytes: &[u8]| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    let html = include_str!("ui.html")
        .replace("{{LOGO_DATA_URI}}", &format!("data:image/png;base64,{}", b64(include_bytes!("../assets/lumos_logo_128.png"))))
        .replace("{{PANORAMA_DATA_URI}}", &format!("data:image/jpeg;base64,{}", b64(include_bytes!("../assets/panorama.jpg"))));

    let builder = WebViewBuilder::new()
        .with_html(html)
        .with_ipc_handler(move |req| {
            let body = req.body();
            let Ok(mut v) = serde_json::from_str::<serde_json::Value>(body) else { return };
            let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("");
            let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();

            match action {
                "init" => {
                    {
                        let st = ipc_state.lock().unwrap();
                        push_state(&ipc_proxy, &st);
                        ping_servers_async(ipc_proxy.clone(), st.config.servers.clone());
                    }
                    check_resources_async(ipc_proxy.clone(), ipc_state.clone());
                    refresh_catalog_async(ipc_proxy.clone(), ipc_state.clone());
                    refresh_skin_async(ipc_proxy.clone(), ipc_state.clone());
                }
                "auto_update" => {
                    let mut st = ipc_state.lock().unwrap();
                    st.config.auto_update = v.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
                    st.save_config();
                    push_state(&ipc_proxy, &st);
                }
                "guide_seen" => {
                    let mut st = ipc_state.lock().unwrap();
                    if !st.config.guide_seen {
                        st.config.guide_seen = true;
                        st.save_config();
                    }
                }
                // ── Import from other launchers (import.rs) ──
                "import_scan" => import_scan_async(ipc_proxy.clone(), ipc_state.clone()),
                "import_run" => {
                    let keys: Vec<String> = v.get("keys").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|k| k.as_str()).filter(|k| k.len() <= 40).take(200).map(str::to_owned).collect()).unwrap_or_default();
                    import_run_async(ipc_proxy.clone(), ipc_state.clone(), keys);
                }
                "import_cancel" => import::CANCEL.store(true, std::sync::atomic::Ordering::Relaxed),
                "set_player_name" => {
                    let name = text("name");
                    if valid_player_name(name.trim()) {
                        let mut st = ipc_state.lock().unwrap();
                        st.config.player_name = name.trim().to_owned();
                        st.save_config();
                    }
                }
                "open_folder" => {
                    let target = text("target");
                    let dir = {
                        let st = ipc_state.lock().unwrap();
                        let inst = st.target_instance(&text("profileId")).map(|t| (t.1, t.2 == HORIZON_ID));
                        match (target.as_str(), inst) {
                            ("instance", Some((inst, _))) => inst,
                            ("mods", Some((inst, true))) => inst.join(horizon_mods::MODS_DIR),
                            ("mods" | "resourcepacks" | "shaderpacks" | "saves" | "screenshots" | "logs", Some((inst, _))) => inst.join(&target),
                            _ => st.base_dir.clone(),
                        }
                    };
                    let _ = fs::create_dir_all(&dir);
                    open_external(&dir.to_string_lossy());
                }
                "open_url" => {
                    let url = text("url");
                    if url.starts_with("https://") && !url.contains(char::is_whitespace) {
                        open_external(&url);
                    }
                }
                // ── HUD block of the instance page (hud_config.rs) ──
                "hud_get" | "hud_set" => {
                    let target = ipc_state.lock().unwrap().target_instance(&text("profileId"));
                    if let Some((pid, game_dir, ver)) = target {
                        let (lumos, horizon) = (hud_supported(&ver), ver == HORIZON_ID);
                        let mut out = Ok(());
                        if action == "hud_set" {
                            out = hud_config::apply(&game_dir, lumos, horizon, v.get("patch").unwrap_or(&serde_json::Value::Null));
                        }
                        let mut payload = hud_config::state(&game_dir, lumos, horizon);
                        payload["profileId"] = json!(pid);
                        payload["fabric"] = json!(matches!(installer::parse_spec(&ver), installer::Spec::Fabric(_)));
                        if let Err(e) = out {
                            payload["error"] = json!(e);
                        }
                        call_js(&ipc_proxy, "onHudConfig", payload);
                    }
                }
                // ── end HUD block ──
                // Discover asks again which shown native mods can't share this instance's game
                // (after an install or removal changed the instance's native mods).
                "native_blocked" => {
                    if let Some((pid, game_dir, ver)) = ipc_state.lock().unwrap().target_instance(&text("profileId")) {
                        let ids: Vec<String> = v.get("ids").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|s| s.as_str()).filter(|s| s.len() <= 64 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')).take(500).map(str::to_owned).collect()).unwrap_or_default();
                        let proxy = ipc_proxy.clone();
                        std::thread::spawn(move || {
                            let have = native_builds::installed_ids(&game_dir);
                            let blocked: Vec<&String> = if ver != HORIZON_ID || have.is_empty() { Vec::new() } else {
                                ids.iter().filter(|id| !have.contains(id) && native_builds::build_for_mod(id).is_some()
                                    && native_builds::build_with_all(&have.iter().cloned().chain([(*id).clone()]).collect::<Vec<_>>()).is_none()).collect()
                            };
                            call_js(&proxy, "onNativeBlocked", json!({"profileId": pid, "blocked": blocked, "installed": have}));
                        });
                    }
                }
                "list_content" => {
                    if let Some((pid, game_dir, ver)) = ipc_state.lock().unwrap().target_instance(&text("profileId")) {
                        push_content(&ipc_proxy, &game_dir, &text("kind"), &pid, ver == HORIZON_ID);
                    }
                }
                "import_content" => {
                    let (kind, file) = (text("kind"), text("file"));
                    let target = ipc_state.lock().unwrap().target_instance(&text("profileId"));
                    match target {
                        None => call_js(&ipc_proxy, "onNotice", json!({"message": GONE_INSTANCE, "kind": "err"})),
                        Some((pid, game_dir, ver)) => {
                            let horizon = ver == HORIZON_ID;
                            // Decoding and writing a big file takes a moment: keep the window responsive.
                            let data = match v.get_mut("data").map(serde_json::Value::take) {
                                Some(serde_json::Value::String(d)) => d,
                                _ => String::new(),
                            };
                            let proxy = ipc_proxy.clone();
                            std::thread::spawn(move || {
                                match import_content(&game_dir, &kind, &file, &data, horizon) {
                                    Ok(()) => call_js(&proxy, "onNotice", json!({"message": format!("Added {file}"), "kind": "ok"})),
                                    Err(e) => call_js(&proxy, "onNotice", json!({"message": e, "kind": "err"})),
                                }
                                push_content(&proxy, &game_dir, &kind, &pid, horizon);
                            });
                        }
                    }
                }
                "toggle_content" => {
                    let (kind, file) = (text("kind"), text("file"));
                    let enable = v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true);
                    let target = ipc_state.lock().unwrap().target_instance(&text("profileId"));
                    if let (Some(id), Some((pid, game_dir, ver))) = (file.strip_prefix(NATIVE_PREFIX), target.as_ref()) {
                        if native_builds::installed_ids(game_dir).iter().any(|m| m == id) {
                            if let Err(e) = horizon_mods::set_enabled(game_dir, id, enable) {
                                call_js(&ipc_proxy, "onNotice", json!({"message": e, "kind": "err"}));
                            }
                        }
                        push_content(&ipc_proxy, game_dir, &kind, pid, ver == HORIZON_ID);
                        return;
                    }
                    if target.is_none() {
                        call_js(&ipc_proxy, "onNotice", json!({"message": GONE_INSTANCE, "kind": "err"}));
                    }
                    let horizon = target.as_ref().is_some_and(|t| t.2 == HORIZON_ID);
                    if let (Some(folder), true, Some((pid, game_dir, _))) = (content_folder(&kind, horizon), safe_file_name(&file), target) {
                        let dir = game_dir.join(folder);
                        // A Horizon mod is switched in horizon-mods.txt; renaming it would only hide it.
                        if let Some(id) = (horizon && kind == "mods").then(|| horizon_mods::manifest_id_of(&dir.join(&file))).flatten() {
                            if let Err(e) = horizon_mods::set_enabled(&game_dir, &id, enable) {
                                call_js(&ipc_proxy, "onNotice", json!({"message": e, "kind": "err"}));
                            }
                            push_content(&ipc_proxy, &game_dir, &kind, &pid, horizon);
                            return;
                        }
                        let target = if enable {
                            file.strip_suffix(".disabled").unwrap_or(&file).to_owned()
                        } else if file.ends_with(".disabled") {
                            file.clone()
                        } else {
                            format!("{file}.disabled")
                        };
                        if target != file {
                            if dir.join(&target).exists() {
                                // Both copies exist: renaming would silently overwrite one of them.
                                call_js(&ipc_proxy, "onNotice", json!({"message": format!("{target} already exists in this instance. Remove one of the two copies first."), "kind": "err"}));
                            } else if let Err(e) = fs::rename(dir.join(&file), dir.join(&target)) {
                                call_js(&ipc_proxy, "onNotice", json!({"message": format!("Couldn't switch {file}: {e}"), "kind": "err"}));
                            }
                        }
                        push_content(&ipc_proxy, &game_dir, &kind, &pid, horizon);
                    }
                }
                "trash_content" => {
                    let (kind, file) = (text("kind"), text("file"));
                    let target = ipc_state.lock().unwrap().target_instance(&text("profileId"));
                    if let (Some(id), Some((pid, game_dir, ver))) = (file.strip_prefix(NATIVE_PREFIX), target.as_ref()) {
                        match native_builds::remove(game_dir, id) {
                            Ok(()) => call_js(&ipc_proxy, "onNotice", json!({"message": "Removed. It leaves the instance's game the next time you play.", "kind": "info"})),
                            Err(e) => call_js(&ipc_proxy, "onNotice", json!({"message": e, "kind": "err"})),
                        }
                        push_content(&ipc_proxy, game_dir, &kind, pid, ver == HORIZON_ID);
                        return;
                    }
                    if target.is_none() {
                        call_js(&ipc_proxy, "onNotice", json!({"message": GONE_INSTANCE, "kind": "err"}));
                    }
                    let horizon = target.as_ref().is_some_and(|t| t.2 == HORIZON_ID);
                    if let (Some(folder), true, Some((pid, game_dir, _))) = (content_folder(&kind, horizon), safe_file_name(&file), target) {
                        let src = game_dir.join(folder).join(&file);
                        let mod_id = (horizon && kind == "mods").then(|| horizon_mods::manifest_id_of(&src)).flatten();
                        let base = ipc_state.lock().unwrap().base_dir.clone();
                        match move_to_trash(&base, &src, &file) {
                            Ok(_) => {
                                // Keep the switch while another copy of the same mod is still there.
                                if let Some(id) = mod_id.as_ref().filter(|id| !horizon_mods::other_copy_exists(&game_dir.join(folder), id, &file)) {
                                    let _ = horizon_mods::forget(&game_dir, id);
                                }
                                call_js(&ipc_proxy, "onNotice", json!({"message": format!("Moved {file} to {TRASH_NAME}"), "kind": "info"}))
                            }
                            Err(e) => call_js(&ipc_proxy, "onNotice", json!({"message": format!("Couldn't move {file} to {TRASH_NAME}: {e}"), "kind": "err"})),
                        }
                        push_content(&ipc_proxy, &game_dir, &kind, &pid, horizon);
                    }
                }
                "set_profile_icon" => {
                    let (id, icon) = (text("id"), text("icon"));
                    if valid_icon_key(&icon) {
                        let mut st = ipc_state.lock().unwrap();
                        if let Some(p) = st.config.profiles.iter_mut().find(|p| p.id == id) {
                            p.icon = icon;
                        }
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                "rename_profile" => {
                    let (id, name) = (text("id"), text("name"));
                    let name = name.trim();
                    if !name.is_empty() && name.chars().count() <= 40 {
                        let mut st = ipc_state.lock().unwrap();
                        if let Some(p) = st.config.profiles.iter_mut().find(|p| p.id == id) {
                            p.name = name.to_owned();
                        }
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                "add_profile" => {
                    let name = text("name");
                    let name = name.trim();
                    let icon = Some(text("icon")).filter(|i| valid_icon_key(i)).unwrap_or_default();
                    let version = text("version");
                    if !name.is_empty() && name.chars().count() <= 40 {
                        let mut st = ipc_state.lock().unwrap();
                        let mut id = format!("prof-{}", now_millis());
                        while st.config.profiles.iter().any(|p| p.id == id) {
                            id.push('x');
                        }
                        let _ = fs::create_dir_all(st.base_dir.join("instances").join(&id));
                        let version = if version.trim().is_empty() { st.config.selected_version.clone() } else { version.trim().to_owned() };
                        st.config.profiles.push(Profile {
                            id: id.clone(),
                            name: name.to_owned(),
                            world: String::new(),
                            last_played: 0,
                            playtime_secs: 0,
                            icon,
                            version: version.clone(),
                            ..Default::default()
                        });
                        st.config.active_profile_id = id;
                        st.config.selected_version = version;
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                // Discover's "Needs another instance": make a fresh Horizon 26.3 instance for the
                // mod and install it there, instead of leaving the player to do it by hand.
                "native_new_instance" => {
                    let slug = text("slug");
                    let title = text("title");
                    if slug.len() <= 64 && !slug.is_empty() && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                        let new_id = {
                            let mut st = ipc_state.lock().unwrap();
                            let mut id = format!("prof-{}", now_millis());
                            while st.config.profiles.iter().any(|p| p.id == id) {
                                id.push('x');
                            }
                            let _ = fs::create_dir_all(st.base_dir.join("instances").join(&id));
                            let label: String = format!("26.3 · {}", if title.trim().is_empty() { slug.as_str() } else { title.trim() }).chars().take(40).collect();
                            st.config.profiles.push(Profile { id: id.clone(), name: label, version: HORIZON_ID.to_owned(), ..Default::default() });
                            st.config.active_profile_id = id.clone();
                            st.config.selected_version = HORIZON_ID.to_owned();
                            st.save_config();
                            push_state(&ipc_proxy, &st);
                            id
                        };
                        download_modrinth_async(ipc_proxy.clone(), ipc_state.clone(), slug, "mod".into(), new_id, false);
                    }
                }
                "select_profile" => {
                    let id = text("id");
                    let mut st = ipc_state.lock().unwrap();
                    if st.config.profiles.iter().any(|p| p.id == id) {
                        st.config.active_profile_id = id.clone();
                        let ver = st.config.profiles.iter().find(|p| p.id == id).map(|p| p.version.clone()).unwrap_or_default();
                        if !ver.is_empty() {
                            st.config.selected_version = ver;
                        }
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                "delete_profile" => {
                    let id = text("id");
                    let mut st = ipc_state.lock().unwrap();
                    if st.config.profiles.len() > 1 && st.config.profiles.iter().any(|p| p.id == id) && safe_file_name(&id) {
                        // Recoverable: the instance folder goes to the Trash, not straight to deletion.
                        let folder = st.base_dir.join("instances").join(&id);
                        if folder.is_dir() {
                            let name = st.config.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_else(|| id.clone());
                            let clean: String = name.chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').collect();
                            let _ = move_to_trash(&st.base_dir, &folder, &format!("LumosClient {}", clean.trim()));
                        }
                        st.config.profiles.retain(|p| p.id != id);
                        if st.config.active_profile_id == id {
                            st.config.active_profile_id = st.config.profiles[0].id.clone();
                            let ver = st.config.profiles[0].version.clone();
                            if !ver.is_empty() {
                                st.config.selected_version = ver;
                            }
                        }
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                "add_server" => {
                    let address = text("address");
                    let address = address.trim();
                    if !address.is_empty() && address.len() <= 255 && !address.contains(char::is_whitespace) {
                        let name = text("name");
                        let entry = ServerEntry {
                            id: format!("srv-{}", now_millis()),
                            name: if name.trim().is_empty() { address.to_owned() } else { name.trim().to_owned() },
                            address: address.to_owned(),
                        };
                        let mut st = ipc_state.lock().unwrap();
                        st.config.servers.push(entry.clone());
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                        ping_servers_async(ipc_proxy.clone(), vec![entry]);
                    }
                }
                "delete_server" => {
                    let id = text("id");
                    let mut st = ipc_state.lock().unwrap();
                    st.config.servers.retain(|s| s.id != id);
                    st.save_config();
                    push_state(&ipc_proxy, &st);
                }
                "ping_all" => {
                    let servers = ipc_state.lock().unwrap().config.servers.clone();
                    ping_servers_async(ipc_proxy.clone(), servers);
                }
                "refresh_versions" => refresh_catalog_async(ipc_proxy.clone(), ipc_state.clone()),
                "search_modrinth" => {
                    let (target, paths) = {
                        let st = ipc_state.lock().unwrap();
                        (st.target_instance(&text("profileId")), st.paths.clone())
                    };
                    let Some((_, game_dir, selected)) = target else {
                        let seq = v.get("seq").and_then(|x| x.as_u64()).unwrap_or(0);
                        call_js(&ipc_proxy, "renderModResults", json!({"hits": [], "total_hits": 0, "error": GONE_INSTANCE, "seq": seq}));
                        return;
                    };
                    search_modrinth_async(
                        ipc_proxy.clone(),
                        paths,
                        game_dir,
                        text("query"),
                        Some(text("projectType")).filter(|t| !t.is_empty()).unwrap_or_else(|| "mod".into()),
                        text("sortBy"),
                        v.get("offset").and_then(|x| x.as_u64()).unwrap_or(0).min(10_000),
                        selected,
                        v.get("seq").and_then(|x| x.as_u64()).unwrap_or(0),
                    );
                }
                // ── feat/discover: Content Hub / Content Browser (discover.rs, curseforge.rs) ──
                "discover_context" => discover_context(&ipc_proxy, &ipc_state, &text("profileId")),
                "discover_search" => discover_search_async(ipc_proxy.clone(), ipc_state.clone(), v.clone()),
                "discover_tags" => discover_tags_async(ipc_proxy.clone(), ipc_state.clone(), text("source")),
                "discover_project" => discover_project_async(ipc_proxy.clone(), ipc_state.clone(), v.clone()),
                "discover_versions" => discover_versions_async(ipc_proxy.clone(), ipc_state.clone(), v.clone()),
                "discover_install" => discover_install(&ipc_proxy, &ipc_state, &v),
                "discover_updates" => discover_updates_async(ipc_proxy.clone(), ipc_state.clone(), text("profileId")),
                "discover_update_all" => discover_update_all_async(ipc_proxy.clone(), ipc_state.clone(), text("profileId")),
                "discover_suggest" => discover_suggest_async(ipc_proxy.clone(), ipc_state.clone(), text("profileId")),
                // ── end feat/discover ──
                // ── Modpacks (modpack.rs) ──
                "download_modrinth" if text("projectType") == "modpack" => {
                    install_modpack_async(ipc_proxy.clone(), ipc_state.clone(), PackSource::Modrinth(text("slug")));
                }
                "import_modpack" => {
                    let file = text("file");
                    let data = match v.get_mut("data").map(serde_json::Value::take) {
                        Some(serde_json::Value::String(d)) => d,
                        _ => String::new(),
                    };
                    install_modpack_async(ipc_proxy.clone(), ipc_state.clone(), PackSource::File(file, data));
                }
                "download_modrinth" => {
                    let project_type = match text("projectType").as_str() {
                        "mod" => "mod",
                        "shader" => "shader",
                        _ => "resourcepack",
                    }
                    .to_owned();
                    let force = v.get("force").and_then(|f| f.as_bool()).unwrap_or(false);
                    download_modrinth_async(ipc_proxy.clone(), ipc_state.clone(), text("slug"), project_type, text("profileId"), force);
                }
                "check_mods" => check_mods_async(ipc_proxy.clone(), ipc_state.clone(), text("profileId")),
                "boost_instance" => boost_instance_async(ipc_proxy.clone(), ipc_state.clone(), text("profileId")),
                "save_instance_settings" => {
                    // Each field: a value sets this instance's own setting, null goes back to the global one.
                    let id = text("id");
                    let mut st = ipc_state.lock().unwrap();
                    if let Some(p) = st.config.profiles.iter_mut().find(|p| p.id == id) {
                        let num = |k: &str| v.get(k).and_then(|x| x.as_u64());
                        if v.get("memoryMb").is_some() {
                            p.memory_mb = num("memoryMb").map(|m| (m as u32).clamp(1024, 65536));
                        }
                        if v.get("resolutionWidth").is_some() {
                            p.resolution_width = num("resolutionWidth").map(|w| (w as u32).clamp(320, 7680));
                        }
                        if v.get("resolutionHeight").is_some() {
                            p.resolution_height = num("resolutionHeight").map(|h| (h as u32).clamp(240, 4320));
                        }
                        if v.get("fullscreen").is_some() {
                            p.fullscreen = v.get("fullscreen").and_then(|x| x.as_bool());
                        }
                        if v.get("jvmArgs").is_some() {
                            p.jvm_args = v.get("jvmArgs").and_then(|x| x.as_str()).map(|j| j.trim().to_owned());
                        }
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                "save_settings" => {
                    let mut st = ipc_state.lock().unwrap();
                    if let Some(w) = v.get("resolutionWidth").and_then(|x| x.as_u64()) {
                        st.config.resolution_width = (w as u32).clamp(320, 7680);
                    }
                    if let Some(h) = v.get("resolutionHeight").and_then(|x| x.as_u64()) {
                        st.config.resolution_height = (h as u32).clamp(240, 4320);
                    }
                    if let Some(fs) = v.get("fullscreen").and_then(|x| x.as_bool()) {
                        st.config.fullscreen = fs;
                    }
                    if let Some(mem) = v.get("memoryMb").and_then(|x| x.as_u64()) {
                        st.config.memory_mb = (mem as u32).clamp(1024, 65536);
                    }
                    if let Some(jvm) = v.get("jvmArgs").and_then(|x| x.as_str()) {
                        st.config.jvm_args = jvm.to_owned();
                    }
                    if let Some(jh) = v.get("javaHome").and_then(|x| x.as_str()) {
                        st.config.java_home = jh.trim().to_owned();
                    }
                    if let Some(hud) = v.get("lumosHudEnabled").and_then(|x| x.as_bool()) {
                        st.config.lumos_hud_enabled = hud;
                    }
                    if let Some(pad) = v.get("horizonController").and_then(|x| x.as_bool()) {
                        st.config.horizon_controller = pad;
                    }
                    if let Some(a) = v.get("uiAccent").and_then(|x| x.as_str()) {
                        if ["blaze", "amber", "crimson", "violet", "azure", "emerald"].contains(&a) {
                            st.config.ui_accent = a.to_owned();
                        }
                    }
                    if let Some(t) = v.get("uiTheme").and_then(|x| x.as_str()) {
                        if t == "dark" || t == "oled" {
                            st.config.ui_theme = t.to_owned();
                        }
                    }
                    if let Some(m) = v.get("uiMotion").and_then(|x| x.as_bool()) {
                        st.config.ui_motion = m;
                    }
                    if let Some(l) = v.get("homeLayout").and_then(|x| x.as_str()) {
                        if l == "launchpad" || l == "instances" {
                            st.config.ui_home_layout = l.to_owned();
                        }
                    }
                    if let Some(h) = v.get("hideOnLaunch").and_then(|x| x.as_bool()) {
                        st.config.hide_on_launch = h;
                    }
                    // feat/discover: CurseForge API key (empty clears it). Never echoed or logged.
                    if let Some(k) = v.get("curseforgeApiKey").and_then(|x| x.as_str()) {
                        let k = k.trim();
                        if k.len() <= 200 && k.chars().all(|c| c.is_ascii_graphic()) {
                            st.config.curseforge_api_key = k.to_owned();
                        }
                    }
                    st.save_config();
                    push_state(&ipc_proxy, &st);
                }
                "set_version" => {
                    let ver = text("version");
                    let ver = ver.trim();
                    if !ver.is_empty() && ver.len() <= 80 && !ver.contains('/') && !ver.contains('\\') {
                        let mut st = ipc_state.lock().unwrap();
                        st.config.selected_version = ver.to_owned();
                        let active = st.config.active_profile_id.clone();
                        if let Some(p) = st.config.profiles.iter_mut().find(|p| p.id == active) {
                            p.version = ver.to_owned();
                        }
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                "check_resources" => check_resources_async(ipc_proxy.clone(), ipc_state.clone()),
                "prepare_horizon" => {
                    let proxy = ipc_proxy.clone();
                    let shared = ipc_state.clone();
                    let (paths, output) = {
                        let mut st = shared.lock().unwrap();
                        if st.task.is_some() || st.active_child.is_some() {
                            call_js(&proxy, "onTask", json!({"error": "Wait for the current game or download to finish."}));
                            return;
                        }
                        st.task = Some(("Getting Horizon ready".into(), -1.0));
                        push_state(&proxy, &st);
                        (st.paths.clone(), st.horizon_output())
                    };
                    std::thread::spawn(move || {
                        let rep = TaskReporter::new(&proxy, &shared);
                        let result = run_guarded(|| prepare_horizon(&shared, &rep, &paths, &output));
                        {
                            let mut st = lock(&shared);
                            st.task = None;
                            push_state(&proxy, &st);
                        }
                        match result {
                            Ok(()) => call_js(&proxy, "onTask", json!({"done": true, "message": "Horizon 26.3 is ready to play."})),
                            Err(e) => {
                                push_log(&proxy, &format!("[LumosClient] error: {e}"));
                                call_js(&proxy, "onTask", json!({"error": e}));
                            }
                        }
                    });
                }
                // feat/updater
                "check_update" | "install_update" | "repair_instance" => updater_ipc(action, &v, &ipc_proxy, &ipc_state),
                "launch_singleplayer" => launch_game(&ipc_proxy, &ipc_state, None),
                "launch_multiplayer" => {
                    let addr = text("address");
                    let addr = addr.trim();
                    if !addr.is_empty() && addr.len() <= 255 && !addr.contains(char::is_whitespace) {
                        launch_game(&ipc_proxy, &ipc_state, Some(addr.to_owned()));
                    }
                }
                "stop_process" => {
                    crash_help::note_user_stop(); // feat/updater
                    let mut st = ipc_state.lock().unwrap();
                    if let Some(child) = st.active_child.as_mut() {
                        let _ = child.kill();
                    }
                }
                "ely_login" => {
                    let (login, password) = (text("login"), text("password"));
                    let totp = Some(text("totp")).filter(|t| !t.trim().is_empty());
                    if login.trim().is_empty() || password.is_empty() {
                        call_js(&ipc_proxy, "onAccount", json!({"error": "Enter your Ely.by username or email and password."}));
                    } else {
                        ely_login_async(ipc_proxy.clone(), ipc_state.clone(), login.trim().to_owned(), password, totp);
                    }
                }
                "ely_logout" => {
                    let session = {
                        let mut st = ipc_state.lock().unwrap();
                        st.config.account_kind = "offline".into();
                        let s = st.config.ely_session.take();
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                        s
                    };
                    if let Some(s) = session {
                        std::thread::spawn(move || elyby::invalidate(&s));
                    }
                    call_js(&ipc_proxy, "onSkin", json!({"name": null}));
                }
                "use_account" => {
                    let kind = text("kind");
                    let mut st = ipc_state.lock().unwrap();
                    if kind == "offline" || (kind == "elyby" && st.config.ely_session.is_some()) {
                        st.config.account_kind = kind;
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                        drop(st);
                        refresh_skin_async(ipc_proxy.clone(), ipc_state.clone());
                    }
                }
                "refresh_skin" => refresh_skin_async(ipc_proxy.clone(), ipc_state.clone()),
                // ── Lumos capes (begin) ──
                "capes_load" => capes_load_async(ipc_proxy.clone(), ipc_state.clone()),
                "capes_select" => {
                    let cape = Some(text("cape")).filter(|c| !c.is_empty() && c != "none");
                    capes_select_async(ipc_proxy.clone(), ipc_state.clone(), cape);
                }
                "capes_enabled" => {
                    let mut st = ipc_state.lock().unwrap();
                    if let Some(on) = v.get("enabled").and_then(|x| x.as_bool()) {
                        st.config.lumos_capes_enabled = on;
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                // ── Lumos capes (end) ──
                "open_ely" => {
                    let path = text("path");
                    let url = if path.is_empty() { elyby::CATALOG_URL.to_owned() } else { path };
                    if allowed_ely_url(&url) {
                        let _ = ipc_proxy.send_event(UserEvent::OpenBrowser(url));
                    }
                }
                _ => {}
            }
        });
    let webview = attach_webview(builder, &window).expect("Failed to build WebView");

    // In-app Ely.by browser windows (skin catalog / account pages). Their sign-in on the Ely.by
    // website is kept in LumosClient's own folder, so the catalog's one-click ✓ (wear this skin)
    // works every time after the first sign-in.
    let mut browsers: HashMap<WindowId, (Window, WebView)> = HashMap::new();
    let mut ely_context = WebContext::new(Some(state.lock().unwrap().base_dir.join("webview")));

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, window_id, .. } => {
                if window_id == window.id() {
                    *control_flow = ControlFlow::Exit;
                } else if browsers.remove(&window_id).is_some() {
                    // The player may have changed their skin on Ely.by.
                    refresh_skin_async(proxy.clone(), state.clone());
                }
            }
            Event::UserEvent(UserEvent::EvalJs(js)) => {
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(UserEvent::OpenBrowser(url)) => {
                if let Some((w, _)) = browsers.values().next() {
                    w.set_focus();
                }
                if browsers.is_empty() {
                    let built = WindowBuilder::new()
                        .with_title("Ely.by · LumosClient")
                        .with_inner_size(tao::dpi::LogicalSize::new(1100.0, 760.0))
                        .build(target);
                    if let Ok(w) = built {
                        let builder = WebViewBuilder::with_web_context(&mut ely_context)
                            .with_initialization_script(ELY_GUIDE_JS)
                            .with_url(&url);
                        let built = attach_webview(builder, &w);
                        if let Ok(wv) = built {
                            browsers.insert(w.id(), (w, wv));
                        }
                    }
                } else if let Some((_, wv)) = browsers.values().next() {
                    let _ = wv.load_url(&url);
                }
            }
            Event::UserEvent(UserEvent::PingResult { id, ping }) => {
                let mut st = state.lock().unwrap();
                st.pings.insert(id, ping);
                let _ = webview.evaluate_script(&format!("window.renderState({});", st.state_json()));
            }
            Event::UserEvent(UserEvent::ResourcesChecked(ok)) => {
                let mut st = state.lock().unwrap();
                st.resources_ready = ok;
                let _ = webview.evaluate_script(&format!("window.renderState({});", st.state_json()));
            }
            Event::UserEvent(UserEvent::SetWindowVisible(visible)) => {
                window.set_visible(visible);
                if visible {
                    window.set_focus();
                }
            }
            Event::UserEvent(UserEvent::ProcessExited(code)) => {
                let crash_ctx = crash_context(&state); // feat/updater
                let mut ended = None;
                {
                    let mut st = state.lock().unwrap();
                    if let Some((pid, started)) = st.session.take() {
                        ended = Some(pid.clone());
                        let secs = started.elapsed().as_secs();
                        if let Some(p) = st.config.profiles.iter_mut().find(|p| p.id == pid) {
                            p.playtime_secs += secs;
                        }
                        st.save_config();
                    }
                    if !window.is_visible() {
                        window.set_visible(true);
                        window.set_focus();
                    }
                    let _ = webview.evaluate_script(&format!("window.renderState({});", st.state_json()));
                }
                let _ = webview.evaluate_script(&format!("window.onGameExit && window.onGameExit({});", json!({"code": code})));
                explain_crash_async(proxy.clone(), code, crash_ctx); // feat/updater
                check_resources_async(proxy.clone(), state.clone());
                if let Some(pid) = ended {
                    apply_ingame_requests(proxy.clone(), state.clone(), pid);
                }
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod horizon_boost_tests {
    use super::*;
    #[test]
    fn boost_sets_culling_and_keeps_a_backup() {
        let dir = std::env::temp_dir().join(format!("lumos-boost-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("horizon-graphics.txt"), "preset:balanced\nentityCulling:off\nsky:true\n").unwrap();
        let n = boost_horizon_graphics(&dir).unwrap();
        let t = fs::read_to_string(dir.join("horizon-graphics.txt")).unwrap();
        assert!(n >= 2 && t.contains("entityCulling:async") && t.contains("sky:true") && t.contains("preset:custom"));
        assert!(fs::read_to_string(dir.join("horizon-graphics.txt.before-lumos-boost")).unwrap().contains("entityCulling:off"));
        assert_eq!(boost_horizon_graphics(&dir).unwrap(), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_settings_override_only_what_they_set() {
        let mut cfg: LauncherConfig = serde_json::from_str(
            r#"{"playerName":"P","minecraftDir":"","javaHome":"","activeProfileId":"a","selectedVersion":"1.21.11",
                "resolutionWidth":1280,"resolutionHeight":720,"memoryMb":4096,"jvmArgs":"-Dglobal","profiles":[]}"#,
        )
        .unwrap();
        let p: Profile = serde_json::from_str(r#"{"id":"a","name":"A","memoryMb":8192,"fullscreen":true}"#).unwrap();
        p.apply_to(&mut cfg);
        assert_eq!((cfg.memory_mb, cfg.fullscreen, cfg.resolution_width, cfg.jvm_args.as_str()), (8192, true, 1280, "-Dglobal"));
        // Unset overrides aren't written back out.
        let saved = serde_json::to_value(Profile { id: "b".into(), ..Default::default() }).unwrap();
        assert!(saved.get("memoryMb").is_none() && saved.get("jvmArgs").is_none());
    }

    #[test]
    fn lumoshud_goes_into_the_fabric_versions_it_is_built_for() {
        assert!(hud_supported("Fabric 1.21.11") && hud_supported("Fabric 1.21.9") && hud_supported("Fabric 1.21.6"));
        assert!(!hud_supported("Fabric 1.21.5") && !hud_supported("Fabric 1.21") && !hud_supported("Fabric 1.21.4"));
        assert!(!hud_supported("1.21.11") && !hud_supported("Fabric 1.20.1") && !hud_supported("Horizon 26.3 (Rust)"));
        // Every supported version has its jar in the build (lumoshud/gradle.properties).
        let props = include_str!("../lumoshud/gradle.properties");
        let line = props.lines().find_map(|l| l.strip_prefix("mc_versions=")).unwrap();
        assert_eq!(line.split(',').map(str::trim).collect::<Vec<_>>(), HUD_VERSIONS);
    }

    #[test]
    fn lumoshud_sync_keeps_only_the_instance_versions_jar() {
        let dir = std::env::temp_dir().join(format!("lumos-hudsync-{}", now_millis()));
        let mods = dir.join("mods");
        fs::create_dir_all(&mods).unwrap();
        for f in ["lumoshud-1.21.9.jar", "lumoshud-1.21.11.jar", "sodium.jar"] {
            fs::write(mods.join(f), b"x").unwrap();
        }
        sync_lumos_hud(&dir, "Fabric 1.21.10", true);
        assert!(!mods.join("lumoshud-1.21.9.jar").exists() && !mods.join("lumoshud-1.21.11.jar").exists());
        assert!(mods.join("sodium.jar").exists());
        assert_eq!(mods.join("lumoshud-1.21.10.jar").exists(), bundled_hud("1.21.10").is_some());
        sync_lumos_hud(&dir, "Fabric 1.21.10", false);
        assert!(!mods.join("lumoshud-1.21.10.jar").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn controller_choice_keeps_other_controller_settings() {
        let dir = std::env::temp_dir().join(format!("lumos-pad-{}", now_millis()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("horizon-controller.txt"), "vibration:false\nenabled:true\nlookSensitivity:0.7\n").unwrap();
        set_horizon_controller(&dir, false);
        let t = fs::read_to_string(dir.join("horizon-controller.txt")).unwrap();
        assert_eq!(t, "enabled:false\nvibration:false\nlookSensitivity:0.7\n");
        set_horizon_controller(&dir, true);
        assert!(fs::read_to_string(dir.join("horizon-controller.txt")).unwrap().starts_with("enabled:true\n"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_names_cannot_escape_the_folder_on_any_os() {
        for ok in ["sodium-fabric-0.6.jar", "My Pack (v2).zip", "a.jar.disabled", "con-tent.jar"] {
            assert!(safe_file_name(ok), "{ok}");
        }
        for bad in ["", ".", "..", "../x.jar", "a/b.jar", "a\\b.jar", "C:x.jar", "x:stream.jar", "NUL", "con.jar", "Com1.zip", "x.jar.", "x.jar ", "a\u{0}b", "a?b.jar"] {
            assert!(!safe_file_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn requests_for_a_deleted_instance_go_nowhere() {
        let base = std::env::temp_dir().join(format!("lumos-target-{}", now_millis()));
        let cfg: LauncherConfig = serde_json::from_str(
            r#"{"playerName":"P","minecraftDir":"","javaHome":"","activeProfileId":"a","selectedVersion":"1.21.11",
                "resolutionWidth":1280,"resolutionHeight":720,
                "profiles":[{"id":"a","name":"A","world":"","lastPlayed":0,"playtimeSecs":0,"version":"1.21.11"},
                            {"id":"b","name":"B","world":"","lastPlayed":0,"playtimeSecs":0,"version":"Fabric 1.20.1"}]}"#,
        )
        .expect("config parses");
        let st = AppState::for_tests(base.clone(), cfg);
        assert_eq!(st.target_instance("").map(|t| t.0).as_deref(), Some("a"));
        let (id, dir, ver) = st.target_instance("b").expect("b exists");
        assert_eq!((id.as_str(), ver.as_str()), ("b", "Fabric 1.20.1"));
        assert_eq!(dir, base.join("instances").join("b"));
        assert!(st.target_instance("deleted").is_none());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn imported_files_land_in_the_named_content_folder() {
        use base64::Engine as _;
        let dir = std::env::temp_dir().join(format!("lumos-import-{}", now_millis()));
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let jar = b64(b"PK\x03\x04 pretend jar");
        assert!(import_content(&dir, "mods", "cool-mod.jar", &jar, false).is_ok());
        assert!(dir.join("mods/cool-mod.jar").is_file());
        // A disabled copy of the same mod is replaced, not kept next to it.
        fs::write(dir.join("mods/other.jar.disabled"), b"PK").unwrap();
        assert!(import_content(&dir, "mods", "other.jar", &jar, false).is_ok());
        assert!(!dir.join("mods/other.jar.disabled").exists());
        assert!(import_content(&dir, "resourcepacks", "pack.zip", &jar, false).is_ok());
        assert!(dir.join("resourcepacks/pack.zip").is_file());
        // Wrong type, not an archive, or a path instead of a name: refused.
        assert!(import_content(&dir, "mods", "pack.zip", &jar, false).is_err());
        assert!(import_content(&dir, "mods", "fake.jar", &b64(b"hello"), false).is_err());
        assert!(import_content(&dir, "mods", "../evil.jar", &jar, false).is_err());
        assert!(import_content(&dir, "saves", "x.jar", &jar, false).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_settings_carry_over_with_new_names() {
        let old = r#"{"playerName":"P","minecraftDir":"","javaHome":"","activeProfileId":"default","selectedVersion":"1.21.11",
            "resolutionWidth":1280,"resolutionHeight":720,"profiles":[],"emberHudEnabled":false,"uiAccent":"ember",
            "servers":[{"id":"s","name":"Local Ember Server","address":"127.0.0.1:25565"},{"id":"t","name":"Mine","address":"x"}]}"#;
        let mut cfg: LauncherConfig = serde_json::from_str(old).expect("old config still parses");
        assert!(!cfg.lumos_hud_enabled);
        migrate_legacy_names(&mut cfg);
        assert_eq!(cfg.ui_accent, "blaze");
        assert_eq!(cfg.servers[0].name, "Local Server");
        assert_eq!(cfg.servers[1].name, "Mine");
        let saved = serde_json::to_value(&cfg).unwrap();
        assert_eq!(saved["lumosHudEnabled"], false);
        assert!(saved.get("emberHudEnabled").is_none());
    }

    #[test]
    fn log4j_xml_becomes_text() {
        let mut x = Log4jText::default();
        assert_eq!(x.feed("plain line"), vec!["plain line"]);
        assert!(x.feed(r#"  <log4j:Event logger="a" timestamp="1" level="WARN" thread="Render thread">"#).is_empty());
        assert!(x.feed("    <log4j:Message><![CDATA[Something odd]]></log4j:Message>").is_empty());
        assert_eq!(x.feed("  </log4j:Event>"), vec!["[Render thread/WARN] Something odd"]);
        let one = r#"<log4j:Event level="INFO" thread="main"><log4j:Message><![CDATA[Hello]]></log4j:Message></log4j:Event>"#;
        assert_eq!(x.feed(one), vec!["[main] Hello"]);
    }

    #[test]
    fn ping_targets() {
        assert_eq!(ping_target("play.example.net"), "play.example.net:25565");
        assert_eq!(ping_target("1.2.3.4:25570"), "1.2.3.4:25570");
        assert_eq!(ping_target("::1"), "[::1]:25565");
        assert_eq!(ping_target("[::1]:25566"), "[::1]:25566");
        assert_eq!(ping_target("[::1]"), "[::1]:25565");
    }

    #[test]
    fn modrinth_versions() {
        let paths = installer::Paths::new(&std::env::temp_dir().join("lumos-mcv-test"), None);
        assert_eq!(mc_version_for(&paths, "Fabric 1.21.11"), "1.21.11");
        assert_eq!(mc_version_for(&paths, "1.21.11"), "1.21.11");
        assert_eq!(mc_version_for(&paths, HORIZON_ID), "26.3");
        assert_eq!(mc_version_from_name("Forge 1.8.9"), "1.8.9");
        assert!(is_fabric_version("Fabric 1.21.11"));
        assert!(is_fabric_version("fabric-loader-0.20.1-1.21.11"));
        assert!(!is_fabric_version("1.21.11"));
    }

    #[test]
    fn loader_requirements() {
        assert!(satisfies_loader_req(">=0.15.0", (0, 20, 1)));
        assert!(!satisfies_loader_req(">=0.21.0", (0, 20, 1)));
        assert!(satisfies_loader_req(">=0.15 <0.21", (0, 20, 1)));
    }

    #[test]
    fn ely_urls_only() {
        assert!(allowed_ely_url("https://ely.by/skins"));
        assert!(allowed_ely_url("https://account.ely.by/"));
        assert!(!allowed_ely_url("http://ely.by/skins"));
        assert!(!allowed_ely_url("https://ely.by.evil.com/"));
        assert!(!allowed_ely_url("https://evil.com/?x=ely.by"));
        assert!(!allowed_ely_url("https://ely.by@evil.com/"));
    }

    #[test]
    fn names_and_ids() {
        assert!(valid_player_name("Steve_01"));
        assert!(!valid_player_name("ab"));
        assert!(!valid_player_name("has space"));
        assert_eq!(offline_uuid_hex("Steve").len(), 32);
        assert!(valid_icon_key("grass") && !valid_icon_key("../x"));
    }
}
