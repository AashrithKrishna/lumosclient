//! LumosClient launcher: a tao + wry desktop app. The UI (ui.html) talks to this
//! backend over `window.ipc.postMessage(JSON)`; the backend answers by evaluating
//! `window.renderState(...)`, `window.onTask(...)` and friends in the page.

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
use wry::{WebView, WebViewBuilder};

mod elyby;
mod installer;
mod net;

const HORIZON_ID: &str = "Horizon 26.3 (Rust)";
const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ───────────────────────── Config ─────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    #[serde(default = "default_true")]
    ember_hud_enabled: bool,
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
    "ember".into()
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
    [dir.join("../Resources"), dir.join("resources")].into_iter().find(|p| p.is_dir())
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

fn bundled_hud() -> Option<PathBuf> {
    bundled_file("mods/emberhud-1.21.11.jar", "emberhud/emberhud-1.21.11.jar")
}

fn bundled_authlib() -> Option<PathBuf> {
    bundled_file(
        &format!("authlib-injector-{}.jar", elyby::AUTHLIB_VERSION),
        &format!("vendor/authlib-injector-{}.jar", elyby::AUTHLIB_VERSION),
    )
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

/// Content folders inside an instance that the UI is allowed to list/modify.
fn content_folder(kind: &str) -> Option<&'static str> {
    match kind {
        "mods" => Some("mods"),
        "resourcepacks" => Some("resourcepacks"),
        "shaderpacks" => Some("shaderpacks"),
        _ => None,
    }
}

/// Rejects anything that is not a plain file name (no separators, no `..`).
fn safe_file_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
}

fn list_content_json(game_dir: &Path, kind: &str) -> String {
    let mut items: Vec<serde_json::Value> = Vec::new();
    if let Some(folder) = content_folder(kind) {
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
                items.push(json!({
                    "file": name,
                    "enabled": !name.ends_with(".disabled"),
                    "isDir": meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                    "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
                    "modified": modified,
                }));
            }
        }
    }
    items.sort_by_key(|a| a["file"].as_str().unwrap_or("").to_lowercase());
    json!({ "kind": kind, "items": items }).to_string()
}

fn push_content(proxy: &EventLoopProxy<UserEvent>, game_dir: &Path, kind: &str) {
    let js = format!("window.renderContent && window.renderContent({});", list_content_json(game_dir, kind));
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
    /// Set by launches so `spawn_streaming_exe` can tell a game apart from tools.
    pending_launch: bool,
    /// Profile id + start time of the running game session (for playtime).
    session: Option<(String, Instant)>,
    /// Horizon start command kept for one automatic retry: the engine has an intermittent
    /// startup race (exit 101 within seconds) that a second start doesn't hit.
    horizon_retry: Option<HorizonRetry>,
    /// A launch/install in progress: (what's happening, fraction or -1).
    task: Option<(String, f64)>,
}

impl AppState {
    fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let base_dir = home.join("LumosClient");
        // Renamed from EmberClient: carry the player's instances, settings and downloads over.
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
                ember_hud_enabled: true,
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
                }],
                servers: vec![],
            });
        // Older builds pinned a Homebrew Java; LumosClient now downloads the right Java itself.
        if config.java_home.contains("/opt/homebrew/opt/openjdk") {
            config.java_home.clear();
        }
        if config.ely_client_token.is_empty() {
            config.ely_client_token = elyby::new_client_token();
        }
        if config.profiles.is_empty() {
            config.profiles.push(Profile {
                id: "default".into(),
                name: "Survival".into(),
                world: String::new(),
                last_played: 0,
                playtime_secs: 0,
                icon: String::new(),
                version: config.selected_version.clone(),
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
            pending_launch: false,
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
            "emberHudEnabled": self.config.ember_hud_enabled,
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
            shared.lock().unwrap().pending_launch = false;
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
        if std::mem::take(&mut st.pending_launch) {
            let pid = st.config.active_profile_id.clone();
            if let Some(p) = st.config.profiles.iter_mut().find(|p| p.id == pid) {
                p.last_played = now_millis();
            }
            st.session = Some((pid, Instant::now()));
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
        LaunchSnapshot {
            config: st.config.clone(),
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
                st.pending_launch = false;
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
        Ok(installer::Auth {
            name: fresh.name.clone(),
            uuid: fresh.uuid.clone(),
            access_token: fresh.access_token.clone(),
            user_type: "mojang".into(),
            javaagent: Some(elyby::javaagent_arg(&jar)),
        })
    } else {
        let name = cfg.player_name.trim().to_owned();
        if !valid_player_name(&name) {
            return Err("Set a username first: 3–16 letters, numbers or _.".into());
        }
        Ok(installer::Auth { uuid: offline_uuid_hex(&name), name, access_token: "0".into(), user_type: "legacy".into(), javaagent: None })
    }
}

fn sync_ember_hud(game_dir: &Path, version: &str, enabled: bool) {
    let mods_dir = game_dir.join("mods");
    let dest = mods_dir.join("emberhud-1.21.11.jar");
    let wants = enabled && version.contains("Fabric") && version.contains("1.21.11");
    if wants {
        if let Some(src) = bundled_hud() {
            let _ = fs::create_dir_all(&mods_dir);
            let _ = fs::copy(src, dest);
        }
    } else if dest.is_file() {
        let _ = fs::remove_file(dest);
    }
}

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
    sync_ember_hud(&snap.game_dir, &cfg.selected_version, cfg.ember_hud_enabled);
    let custom = (!cfg.java_home.trim().is_empty())
        .then(|| installer::java_binary_in_home(Path::new(cfg.java_home.trim())))
        .filter(|p| p.is_file());
    let java = custom.unwrap_or(managed_java);
    let opts = installer::LaunchOpts {
        memory_mb: cfg.memory_mb,
        extra_jvm: cfg.jvm_args.split_whitespace().map(str::to_owned).collect(),
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
    shared.lock().unwrap().pending_launch = true;
    spawn_streaming_exe(proxy, shared, &java, &args, &snap.game_dir, &[], None)
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
    prepare_horizon(shared, rep, &snap.paths, &snap.horizon_output)?;
    let client = resolve_binary("horizon-client").ok_or("This LumosClient build is missing the Horizon engine. Reinstall LumosClient.")?;
    let session = json!({"name": auth.name, "uuid": auth.uuid, "access_token": auth.access_token}).to_string();
    let mut args: Vec<String> = vec![
        "--resources".into(),
        snap.horizon_output.to_string_lossy().into_owned(),
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
    shared.lock().unwrap().pending_launch = true;
    {
        let mut st = lock(shared);
        st.pending_launch = true;
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
    if let installer::Spec::Fabric(mc) = installer::parse_spec(selected) {
        return mc;
    }
    if let Some(base) = installer::base_game_version(paths, selected) {
        return base;
    }
    mc_version_from_name(selected)
}

fn mc_version_from_name(selected: &str) -> String {
    match installer::parse_spec(selected) {
        installer::Spec::Fabric(mc) => mc,
        installer::Spec::Id(id) => {
            let first_numeric = id.split_whitespace().rev().find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()));
            first_numeric.unwrap_or(&id).to_owned()
        }
    }
}

fn is_fabric_version(selected: &str) -> bool {
    selected.starts_with(installer::FABRIC_PREFIX) || selected.to_lowercase().contains("fabric")
}

fn search_modrinth_async(proxy: EventLoopProxy<UserEvent>, paths: installer::Paths, query: String, project_type: String, sort_by: String, offset: u64, selected: String, seq: u64) {
    std::thread::spawn(move || {
        let project_type = match project_type.as_str() {
            "mod" | "resourcepack" | "shader" => project_type,
            _ => "mod".to_owned(),
        };
        if selected == HORIZON_ID && project_type != "resourcepack" {
            let msg = if project_type == "mod" {
                "Horizon 26.3 runs the Rust engine, which can't load Java mods. Switch this instance to a Fabric version to use mods."
            } else {
                "Shader packs need a Java version with Iris. Switch this instance to a Fabric version to use shaders."
            };
            call_js(&proxy, "renderModResults", json!({"hits": [], "total_hits": 0, "notice": msg, "seq": seq}));
            return;
        }
        let mc = mc_version_for(&paths, &selected);
        let mut facets = vec![format!("[\"project_type:{project_type}\"]")];
        if project_type == "mod" {
            facets.push("[\"categories:fabric\"]".to_owned());
        }
        if project_type != "shader" {
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
        payload["seq"] = json!(seq);
        call_js(&proxy, "renderModResults", payload);
    });
}

fn push_install_status(proxy: &EventLoopProxy<UserEvent>, slug: &str, state: &str, msg: &str) {
    if let (Ok(s), Ok(st), Ok(m)) = (serde_json::to_string(slug), serde_json::to_string(state), serde_json::to_string(msg)) {
        let _ = proxy.send_event(UserEvent::EvalJs(format!("window.onInstallStatus({s}, {st}, {m});")));
    }
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

fn install_modrinth_project(
    proxy: &EventLoopProxy<UserEvent>,
    game_dir: &Path,
    project: &str,
    project_type: &str,
    mc_ver: &str,
    installed_loader: Option<(u64, u64, u64)>,
    depth: u8,
) -> Result<String, String> {
    let mut existing: Option<HashMap<String, String>> = None;
    install_modrinth_inner(proxy, game_dir, project, project_type, mc_ver, installed_loader, depth, &mut existing)
}

#[allow(clippy::too_many_arguments)]
fn install_modrinth_inner(
    proxy: &EventLoopProxy<UserEvent>,
    game_dir: &Path,
    project: &str,
    project_type: &str,
    mc_ver: &str,
    installed_loader: Option<(u64, u64, u64)>,
    depth: u8,
    existing: &mut Option<HashMap<String, String>>,
) -> Result<String, String> {
    if depth > 3 {
        return Ok(String::new());
    }
    if project.is_empty() || !project.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("That Modrinth project name isn't valid.".into());
    }
    let game_versions = url_encode(&format!("[\"{mc_ver}\"]"));
    let compat_url = if project_type == "mod" {
        format!(
            "https://api.modrinth.com/v2/project/{project}/version?game_versions={game_versions}&loaders={}",
            url_encode("[\"fabric\"]")
        )
    } else {
        format!("https://api.modrinth.com/v2/project/{project}/version?game_versions={game_versions}")
    };
    let versions = net::get_json(&compat_url)?;
    let mut candidates = versions.as_array().cloned().unwrap_or_default();
    if candidates.is_empty() && project_type == "shader" {
        // Shader packs rarely tag game versions; any release works with Iris.
        candidates = net::get_json(&format!("https://api.modrinth.com/v2/project/{project}/version"))?.as_array().cloned().unwrap_or_default();
    }
    if candidates.is_empty() {
        return Err(format!("No version of this works with Minecraft {mc_ver}."));
    }
    // Prefer releases, then betas, then alphas, keeping newest-first order within each.
    candidates.sort_by_key(|v| match v.get("version_type").and_then(|t| t.as_str()) {
        Some("release") => 0,
        Some("beta") => 1,
        _ => 2,
    });
    let folder = match project_type {
        "resourcepack" => "resourcepacks",
        "shader" => "shaderpacks",
        _ => "mods",
    };
    let dest_dir = game_dir.join(folder);
    fs::create_dir_all(&dest_dir).map_err(|e| format!("Couldn't create {}: {e}", dest_dir.display()))?;
    let known = existing.get_or_insert_with(|| installed_projects(&dest_dir));
    let project_id = candidates.first().and_then(|v| v.get("project_id")).and_then(|p| p.as_str()).unwrap_or("").to_owned();
    let old_file = known.get(&project_id).cloned();
    // A dependency that's already there is left alone (whatever version the player has).
    if depth > 0 {
        if let Some(f) = &old_file {
            return Ok(format!("{f} (already installed)"));
        }
    }

    for ver in &candidates {
        let Some(file_obj) = ver
            .get("files")
            .and_then(|f| f.as_array())
            .and_then(|arr| arr.iter().find(|f| f.get("primary").and_then(|p| p.as_bool()) == Some(true)).or_else(|| arr.first()))
        else {
            continue;
        };
        let Some(dl_url) = file_obj.get("url").and_then(|u| u.as_str()) else { continue };
        let Some(filename) = file_obj.get("filename").and_then(|f| f.as_str()) else { continue };
        let safe_name = Path::new(filename).file_name().unwrap_or_default().to_string_lossy().into_owned();
        if !safe_file_name(&safe_name) {
            continue;
        }
        let sha1 = file_obj.pointer("/hashes/sha1").and_then(|h| h.as_str());
        let size = file_obj.get("size").and_then(|s| s.as_u64());
        let tmp_dest = dest_dir.join(format!(".tmp_{}_{}_{safe_name}", std::process::id(), now_millis()));
        let final_dest = dest_dir.join(&safe_name);
        if let Err(e) = net::download_verified(dl_url, &tmp_dest, sha1, size) {
            push_log(proxy, &format!("[Modrinth] {e}"));
            continue;
        }
        if project_type == "mod" && !jar_matches_fabric_loader(&tmp_dest, installed_loader) {
            push_log(proxy, &format!("[Modrinth] Skipped {filename} (needs a newer Fabric Loader)"));
            let _ = fs::remove_file(&tmp_dest);
            continue;
        }
        fs::rename(&tmp_dest, &final_dest).map_err(|e| format!("Couldn't save {safe_name}: {e}"))?;
        // Replace the older copy of the same project so the game never loads it twice.
        if let Some(old) = old_file.as_deref().filter(|o| *o != safe_name && safe_file_name(o)) {
            let _ = fs::remove_file(dest_dir.join(old));
            push_log(proxy, &format!("[Modrinth] Replaced {old}"));
        }
        if let Some(k) = existing.as_mut() {
            k.insert(project_id.clone(), safe_name.clone());
        }
        push_log(proxy, &format!("[Modrinth] Installed {filename} -> {folder}/"));

        // Required dependencies (e.g. Fabric API)
        if project_type == "mod" {
            for dep in ver.get("dependencies").and_then(|d| d.as_array()).into_iter().flatten() {
                if dep.get("dependency_type").and_then(|t| t.as_str()) == Some("required") {
                    if let Some(dep_id) = dep.get("project_id").and_then(|id| id.as_str()) {
                        match install_modrinth_inner(proxy, game_dir, dep_id, "mod", mc_ver, installed_loader, depth + 1, existing) {
                            Ok(f) if !f.is_empty() => push_log(proxy, &format!("[Modrinth] Added required dependency {f}")),
                            Ok(_) => {}
                            Err(e) => push_log(proxy, &format!("[Modrinth] A required dependency couldn't be installed: {e}")),
                        }
                    }
                }
            }
        }
        return Ok(filename.to_owned());
    }
    Err(format!("No compatible file for Minecraft {mc_ver} could be installed."))
}

fn download_modrinth_async(proxy: EventLoopProxy<UserEvent>, shared: Arc<Mutex<AppState>>, slug: String, project_type: String) {
    std::thread::spawn(move || {
        let (game_dir, paths, selected) = {
            let st = shared.lock().unwrap();
            (st.active_game_dir(), st.paths.clone(), st.config.selected_version.clone())
        };
        if project_type == "mod" && !is_fabric_version(&selected) {
            let mc = mc_version_for(&paths, &selected);
            let hint = if selected == HORIZON_ID {
                "Horizon 26.3 can't load Java mods. Switch this instance to a Fabric version first.".to_owned()
            } else {
                format!("Mods need Fabric. Switch this instance to \"Fabric {mc}\" first (version menu next to Launch).")
            };
            push_install_status(&proxy, &slug, "error", &hint);
            return;
        }
        let mc_ver = mc_version_for(&paths, &selected);
        let loader = detect_fabric_loader_version(&paths, &selected);
        push_install_status(&proxy, &slug, "installing", &format!("Checking compatibility for {slug}..."));
        match install_modrinth_project(&proxy, &game_dir, &slug, &project_type, &mc_ver, loader, 0) {
            Ok(filename) => push_install_status(&proxy, &slug, "installed", &format!("Installed {filename} (Minecraft {mc_ver})")),
            Err(err) => push_install_status(&proxy, &slug, "error", &err),
        }
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
fn allowed_ely_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.contains('@') && !host.contains(':') && (host == "ely.by" || host.ends_with(".ely.by"))
}

// ───────────────────────── Main ─────────────────────────

fn main() {
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
        .replace("{{LOGO_DATA_URI}}", &format!("data:image/png;base64,{}", b64(include_bytes!("../assets/emberclient_logo_128.png"))))
        .replace("{{PANORAMA_DATA_URI}}", &format!("data:image/jpeg;base64,{}", b64(include_bytes!("../assets/panorama.jpg"))));

    let webview = WebViewBuilder::new()
        .with_html(html)
        .with_ipc_handler(move |req| {
            let body = req.body();
            let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else { return };
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
                        match target.as_str() {
                            "instance" => st.active_game_dir(),
                            "mods" | "resourcepacks" | "shaderpacks" | "saves" | "screenshots" | "logs" => st.active_game_dir().join(&target),
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
                "list_content" => {
                    let game_dir = ipc_state.lock().unwrap().active_game_dir();
                    push_content(&ipc_proxy, &game_dir, &text("kind"));
                }
                "toggle_content" => {
                    let (kind, file) = (text("kind"), text("file"));
                    let enable = v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true);
                    if let (Some(folder), true) = (content_folder(&kind), safe_file_name(&file)) {
                        let game_dir = ipc_state.lock().unwrap().active_game_dir();
                        let dir = game_dir.join(folder);
                        let target = if enable {
                            file.strip_suffix(".disabled").unwrap_or(&file).to_owned()
                        } else if file.ends_with(".disabled") {
                            file.clone()
                        } else {
                            format!("{file}.disabled")
                        };
                        if target != file {
                            if let Err(e) = fs::rename(dir.join(&file), dir.join(&target)) {
                                push_log(&ipc_proxy, &format!("[LumosClient] Could not rename {file}: {e}"));
                            }
                        }
                        push_content(&ipc_proxy, &game_dir, &kind);
                    }
                }
                "trash_content" => {
                    let (kind, file) = (text("kind"), text("file"));
                    if let (Some(folder), true) = (content_folder(&kind), safe_file_name(&file)) {
                        let game_dir = ipc_state.lock().unwrap().active_game_dir();
                        let src = game_dir.join(folder).join(&file);
                        // Recoverable: the macOS Trash, else LumosClient's own trash folder.
                        let trash = if cfg!(target_os = "macos") {
                            dirs::home_dir().unwrap_or_default().join(".Trash")
                        } else {
                            ipc_state.lock().unwrap().base_dir.join("trash")
                        };
                        let _ = fs::create_dir_all(&trash);
                        let mut dest = trash.join(&file);
                        if dest.exists() {
                            dest = trash.join(format!("{}-{file}", now_millis()));
                        }
                        match fs::rename(&src, &dest) {
                            Ok(_) => call_js(&ipc_proxy, "onNotice", json!({"message": format!("Moved {file} to the Trash"), "kind": "info"})),
                            Err(e) => call_js(&ipc_proxy, "onNotice", json!({"message": format!("Couldn't move {file} to the Trash: {e}"), "kind": "err"})),
                        }
                        push_content(&ipc_proxy, &game_dir, &kind);
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
                        });
                        st.config.active_profile_id = id;
                        st.config.selected_version = version;
                        st.save_config();
                        push_state(&ipc_proxy, &st);
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
                            let trash = if cfg!(target_os = "macos") {
                                dirs::home_dir().unwrap_or_default().join(".Trash")
                            } else {
                                st.base_dir.join("trash")
                            };
                            let _ = fs::create_dir_all(&trash);
                            let name = st.config.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_else(|| id.clone());
                            let clean: String = name.chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').collect();
                            let dest = trash.join(format!("LumosClient {} {}", clean.trim(), now_millis()));
                            let _ = fs::rename(&folder, &dest);
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
                    let (selected, paths) = {
                        let st = ipc_state.lock().unwrap();
                        (st.config.selected_version.clone(), st.paths.clone())
                    };
                    search_modrinth_async(
                        ipc_proxy.clone(),
                        paths,
                        text("query"),
                        Some(text("projectType")).filter(|t| !t.is_empty()).unwrap_or_else(|| "mod".into()),
                        text("sortBy"),
                        v.get("offset").and_then(|x| x.as_u64()).unwrap_or(0).min(10_000),
                        selected,
                        v.get("seq").and_then(|x| x.as_u64()).unwrap_or(0),
                    );
                }
                "download_modrinth" => {
                    let project_type = match text("projectType").as_str() {
                        "mod" => "mod",
                        "shader" => "shader",
                        _ => "resourcepack",
                    }
                    .to_owned();
                    download_modrinth_async(ipc_proxy.clone(), ipc_state.clone(), text("slug"), project_type);
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
                    if let Some(hud) = v.get("emberHudEnabled").and_then(|x| x.as_bool()) {
                        st.config.ember_hud_enabled = hud;
                    }
                    if let Some(a) = v.get("uiAccent").and_then(|x| x.as_str()) {
                        if ["ember", "amber", "crimson", "violet", "azure", "emerald"].contains(&a) {
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
                "launch_singleplayer" => launch_game(&ipc_proxy, &ipc_state, None),
                "launch_multiplayer" => {
                    let addr = text("address");
                    let addr = addr.trim();
                    if !addr.is_empty() && addr.len() <= 255 && !addr.contains(char::is_whitespace) {
                        launch_game(&ipc_proxy, &ipc_state, Some(addr.to_owned()));
                    }
                }
                "stop_process" => {
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
                "open_ely" => {
                    let path = text("path");
                    let url = if path.is_empty() { elyby::CATALOG_URL.to_owned() } else { path };
                    if allowed_ely_url(&url) {
                        let _ = ipc_proxy.send_event(UserEvent::OpenBrowser(url));
                    }
                }
                _ => {}
            }
        })
        .build(&window)
        .expect("Failed to build WebView");

    // In-app Ely.by browser windows (skin catalog / account pages).
    let mut browsers: HashMap<WindowId, (Window, WebView)> = HashMap::new();

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
                        if let Ok(wv) = WebViewBuilder::new().with_url(&url).build(&w) {
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
                {
                    let mut st = state.lock().unwrap();
                    if let Some((pid, started)) = st.session.take() {
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
                check_resources_async(proxy.clone(), state.clone());
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let paths = installer::Paths::new(&std::env::temp_dir().join("ember-mcv-test"), None);
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
