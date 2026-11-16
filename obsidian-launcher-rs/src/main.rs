use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Profile {
    id: String,
    name: String,
    world: String,
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
    player_name: String,
    minecraft_dir: String,
    java_home: String,
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
    profiles: Vec<Profile>,
    servers: Vec<ServerEntry>,
}

fn default_version() -> String {
    "Horizon 26.3 (Rust)".into()
}
fn default_width() -> u32 { 1280 }
fn default_height() -> u32 { 720 }
fn default_memory() -> u32 { 8192 }
fn default_true() -> bool { true }

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
    ProcessExited,
}

struct AppState {
    base_dir: PathBuf,
    config: LauncherConfig,
    pings: HashMap<String, PingState>,
    resources_ready: bool,
    local_versions: Vec<String>,
    active_child: Option<Child>,
}

impl AppState {
    fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let base_dir = home.join("EmberClient");
        for sub in [
            "instances/default",
            "mods",
            "resourcepacks",
            "shaderpacks",
            "resources/26.3",
            "logs",
        ] {
            let _ = fs::create_dir_all(base_dir.join(sub));
        }

        let cfg_path = base_dir.join("launcher.json");
        let default_mc = home
            .join("Library/Application Support/minecraft")
            .to_string_lossy()
            .into_owned();
        let default_java = "/opt/homebrew/opt/openjdk@25/libexec/openjdk.jdk/Contents/Home".to_owned();

        let mut config = fs::read_to_string(&cfg_path)
            .ok()
            .and_then(|s| serde_json::from_str::<LauncherConfig>(&s).ok())
            .unwrap_or_else(|| LauncherConfig {
                player_name: "EmberPlayer".into(),
                minecraft_dir: default_mc,
                java_home: default_java,
                active_profile_id: "default".into(),
                selected_version: default_version(),
                resolution_width: default_width(),
                resolution_height: default_height(),
                fullscreen: false,
                memory_mb: default_memory(),
                jvm_args: String::new(),
                ember_hud_enabled: true,
                profiles: vec![Profile {
                    id: "default".into(),
                    name: "Default Survival".into(),
                    world: "New World".into(),
                }],
                servers: vec![ServerEntry {
                    id: "local".into(),
                    name: "Local Ember Server".into(),
                    address: "127.0.0.1:25565".into(),
                }],
            });

        let local_versions = discover_local_versions(Path::new(&config.minecraft_dir));
        if config.selected_version == "Horizon 26.3 (Rust)" && !local_versions.is_empty() {
            if let Some(preferred) = local_versions
                .iter()
                .find(|v| v.as_str() == "1.21.11" || v.as_str() == "Fabric 1.21.11")
                .or_else(|| local_versions.first())
            {
                config.selected_version = preferred.clone();
            }
        }

        let state = Self {
            base_dir,
            config,
            pings: HashMap::new(),
            resources_ready: false,
            local_versions,
            active_child: None,
        };
        state.save_config();
        state
    }

    fn save_config(&self) {
        if let Ok(text) = serde_json::to_string_pretty(&self.config) {
            let _ = fs::write(self.base_dir.join("launcher.json"), text);
        }
    }

    fn active_game_dir(&self) -> PathBuf {
        let id = if self.config.active_profile_id.is_empty() {
            "default"
        } else {
            &self.config.active_profile_id
        };
        let dir = self.base_dir.join("instances").join(id);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn resources_dir(&self) -> PathBuf {
        self.base_dir.join("resources/26.3")
    }

    fn state_json(&self) -> String {
        let mut versions = vec!["Horizon 26.3 (Rust)".to_owned()];
        versions.extend(self.local_versions.clone());
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
            "versions": versions,
            "profiles": self.config.profiles,
            "servers": self.config.servers,
            "pings": self.pings,
            "resourcesReady": self.resources_ready,
            "running": self.active_child.is_some(),
        })
        .to_string()
    }
}

fn resolve_binary(name: &str) -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(macos_dir) = exe.parent() {
            let bundled = macos_dir.join("../Resources/bin").join(name);
            if bundled.is_file() {
                return Some(bundled);
            }
        }
    }
    let rel = PathBuf::from("/Users/aashrith/.gemini/antigravity/scratch/horizon/target/release").join(name);
    if rel.is_file() {
        return Some(rel);
    }
    let dev = PathBuf::from("/Users/aashrith/.gemini/antigravity/scratch/horizon/target/debug").join(name);
    if dev.is_file() {
        return Some(dev);
    }
    None
}

fn resolve_resources_root() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(macos_dir) = exe.parent() {
            let bundled = macos_dir.join("../Resources");
            if bundled.join("research-lock.json").is_file() {
                return bundled;
            }
        }
    }
    PathBuf::from("/Users/aashrith/.gemini/antigravity/scratch/horizon")
}

fn discover_local_versions(mc_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(mc_dir.join("versions")) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.join(format!("{name}.json")).is_file() && path.join(format!("{name}.jar")).is_file() {
            out.push(name);
        }
    }
    out.sort_by(|a, b| b.cmp(a));
    out
}

fn pick_java_bin(major: u64, fallback_java_home: &str) -> String {
    let candidates: Vec<PathBuf> = if major <= 8 {
        vec![
            PathBuf::from("/Library/Internet Plug-Ins/JavaAppletPlugin.plugin/Contents/Home/bin/java"),
            PathBuf::from("/Library/Java/JavaVirtualMachines/jdk-21.jdk/Contents/Home/bin/java"),
        ]
    } else if major >= 25 {
        vec![
            PathBuf::from(fallback_java_home).join("bin/java"),
            PathBuf::from("/opt/homebrew/opt/openjdk@25/libexec/openjdk.jdk/Contents/Home/bin/java"),
            PathBuf::from("/Library/Java/JavaVirtualMachines/jdk-21.jdk/Contents/Home/bin/java"),
        ]
    } else {
        vec![
            PathBuf::from("/Library/Java/JavaVirtualMachines/jdk-21.jdk/Contents/Home/bin/java"),
            PathBuf::from(fallback_java_home).join("bin/java"),
        ]
    };
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "java".into())
}

fn lib_rules_allow_macos(rules: Option<&serde_json::Value>) -> bool {
    let Some(arr) = rules.and_then(|r| r.as_array()) else {
        return true;
    };
    let mut allowed = false;
    for rule in arr {
        let action = rule.get("action").and_then(|a| a.as_str()).unwrap_or("allow");
        let matches_os = match rule.get("os") {
            None => true,
            Some(os) => {
                let name_ok = os
                    .get("name")
                    .and_then(|n| n.as_str())
                    .is_none_or(|n| n == "osx");
                let arch_ok = os
                    .get("arch")
                    .and_then(|a| a.as_str())
                    .is_none_or(|a| a == "arm64" || a == "aarch64");
                name_ok && arch_ok
            }
        };
        if matches_os {
            allowed = action == "allow";
        }
    }
    allowed
}

fn maven_to_path(coord: &str) -> Option<String> {
    let mut parts = coord.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    let classifier = parts.next();
    let suffix = match classifier {
        Some(c) => format!("-{c}"),
        None => String::new(),
    };
    Some(format!(
        "{}/{}/{}/{}-{}{}.jar",
        group.replace('.', "/"),
        artifact,
        version,
        artifact,
        version,
        suffix
    ))
}

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

fn resolve_bundled_hud() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(macos_dir) = exe.parent() {
            let bundled = macos_dir.join("../Resources/mods/emberhud-1.21.11.jar");
            if bundled.is_file() {
                return Some(bundled);
            }
        }
    }
    let dev = PathBuf::from("/Users/aashrith/.gemini/antigravity/scratch/horizon/obsidian-launcher-rs/emberhud/emberhud-1.21.11.jar");
    dev.is_file().then_some(dev)
}

fn sync_ember_hud(game_dir: &Path, version: &str, enabled: bool) {
    let mods_dir = game_dir.join("mods");
    let _ = fs::create_dir_all(&mods_dir);
    let dest = mods_dir.join("emberhud-1.21.11.jar");
    if enabled && version.contains("Fabric") && version.contains("1.21.11") {
        if let Some(src) = resolve_bundled_hud() {
            let _ = fs::copy(src, dest);
        }
    } else if !enabled && dest.is_file() {
        let _ = fs::remove_file(dest);
    }
}

fn launch_java_version(
    proxy: EventLoopProxy<UserEvent>,
    shared: Arc<Mutex<AppState>>,
    version: &str,
    mc_dir: &Path,
    game_dir: &Path,
    player_name: &str,
    server_addr: Option<&str>,
    fallback_java_home: &str,
    memory_mb: u32,
    jvm_args: &str,
    width: u32,
    height: u32,
    fullscreen: bool,
    ember_hud_enabled: bool,
) {
    sync_ember_hud(game_dir, version, ember_hud_enabled);
    let ver_dir = mc_dir.join("versions").join(version);
    let json_path = ver_dir.join(format!("{version}.json"));
    let jar_path = ver_dir.join(format!("{version}.jar"));
    let Ok(raw) = fs::read_to_string(&json_path) else {
        push_log(&proxy, &format!("[EmberClient] Missing version JSON: {}", json_path.display()));
        return;
    };
    let Ok(meta) = serde_json::from_str::<serde_json::Value>(&raw) else {
        push_log(&proxy, "[EmberClient] Failed to parse version JSON.");
        return;
    };

    let main_class = meta.get("mainClass").and_then(|m| m.as_str()).unwrap_or("net.minecraft.client.main.Main");
    let asset_index = meta
        .get("assetIndex")
        .and_then(|a| a.get("id"))
        .and_then(|i| i.as_str())
        .or_else(|| meta.get("assets").and_then(|a| a.as_str()))
        .unwrap_or("legacy");
    let java_major = meta
        .get("javaVersion")
        .and_then(|j| j.get("majorVersion"))
        .and_then(|m| m.as_u64())
        .unwrap_or(21);

    let libs_root = mc_dir.join("libraries");
    let mut cp_entries: Vec<String> = Vec::new();
    if let Some(libs) = meta.get("libraries").and_then(|l| l.as_array()) {
        for lib in libs {
            if !lib_rules_allow_macos(lib.get("rules")) {
                continue;
            }
            let rel = lib
                .get("downloads")
                .and_then(|d| d.get("artifact"))
                .and_then(|a| a.get("path"))
                .and_then(|p| p.as_str())
                .map(str::to_owned)
                .or_else(|| lib.get("name").and_then(|n| n.as_str()).and_then(maven_to_path));
            if let Some(r) = rel {
                let full = libs_root.join(r);
                if full.is_file() {
                    cp_entries.push(full.to_string_lossy().into_owned());
                }
            }
        }
    }
    cp_entries.push(jar_path.to_string_lossy().into_owned());
    let classpath = cp_entries.join(":");

    let java_bin = pick_java_bin(java_major, fallback_java_home);
    let natives_dir = ver_dir.join("natives");
    let assets_dir = mc_dir.join("assets");
    let uuid = offline_uuid_hex(player_name);

    let mut args = vec![
        "-XstartOnFirstThread".to_owned(),
        format!("-Xmx{}M", memory_mb.clamp(512, 32768)),
    ];
    for extra in jvm_args.split_whitespace() {
        args.push(extra.to_owned());
    }
    args.extend([
        format!("-Djava.library.path={}", natives_dir.display()),
        "-cp".to_owned(),
        classpath,
        main_class.to_owned(),
        "--username".to_owned(),
        player_name.to_owned(),
        "--version".to_owned(),
        version.to_owned(),
        "--gameDir".to_owned(),
        game_dir.to_string_lossy().into_owned(),
        "--assetsDir".to_owned(),
        assets_dir.to_string_lossy().into_owned(),
        "--assetIndex".to_owned(),
        asset_index.to_owned(),
        "--uuid".to_owned(),
        uuid,
        "--accessToken".to_owned(),
        "0".to_owned(),
        "--userType".to_owned(),
        "legacy".to_owned(),
        "--versionType".to_owned(),
        "EmberClient".to_owned(),
        "--width".to_owned(),
        width.max(640).to_string(),
        "--height".to_owned(),
        height.max(480).to_string(),
    ]);
    if fullscreen {
        args.push("--fullscreen".to_owned());
    }
    if let Some(addr) = server_addr {
        args.push("--quickPlayMultiplayer".to_owned());
        args.push(addr.to_owned());
    }

    push_log(&proxy, &format!("[EmberClient] Launching {version} with {java_bin}..."));
    spawn_streaming_exe(proxy, shared, PathBuf::from(java_bin), args, game_dir.to_path_buf(), None, None);
}

fn push_state(proxy: &EventLoopProxy<UserEvent>, state: &AppState) {
    let js = format!("window.renderState({});", state.state_json());
    let _ = proxy.send_event(UserEvent::EvalJs(js));
}

fn push_log(proxy: &EventLoopProxy<UserEvent>, line: &str) {
    if let Ok(encoded) = serde_json::to_string(line) {
        let _ = proxy.send_event(UserEvent::EvalJs(format!("window.appendLog({encoded});")));
    }
}

fn check_resources_async(proxy: EventLoopProxy<UserEvent>, resources_dir: PathBuf) {
    std::thread::spawn(move || {
        let root = resolve_resources_root();
        let ok = resolve_binary("horizon-prepare")
            .and_then(|bin| {
                Command::new(bin)
                    .args(["--check", "--root"])
                    .arg(&root)
                    .arg("--output")
                    .arg(&resources_dir)
                    .output()
                    .ok()
            })
            .is_some_and(|out| out.status.success());
        let _ = proxy.send_event(UserEvent::ResourcesChecked(ok));
    });
}

fn ping_servers_async(proxy: EventLoopProxy<UserEvent>, servers: Vec<ServerEntry>) {
    for srv in servers {
        let p = proxy.clone();
        let _ = p.send_event(UserEvent::PingResult {
            id: srv.id.clone(),
            ping: PingState::Checking("checking"),
        });
        std::thread::spawn(move || {
            let addr = if srv.address.contains(':') {
                srv.address.clone()
            } else {
                format!("{}:25565", srv.address)
            };
            let start = Instant::now();
            let result = addr
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.next())
                .and_then(|sa| TcpStream::connect_timeout(&sa, Duration::from_secs(2)).ok());
            let ping = match result {
                Some(_) => PingState::Online {
                    online: true,
                    ms: start.elapsed().as_millis().max(1),
                },
                None => PingState::Offline { online: false },
            };
            let _ = p.send_event(UserEvent::PingResult { id: srv.id, ping });
        });
    }
}

fn spawn_streaming(
    proxy: EventLoopProxy<UserEvent>,
    shared: Arc<Mutex<AppState>>,
    bin_name: &str,
    args: Vec<String>,
    cwd: PathBuf,
    java_home: String,
    horizon_home: PathBuf,
) {
    let Some(bin) = resolve_binary(bin_name) else {
        push_log(&proxy, &format!("[Horizon] Binary not found: {bin_name}"));
        return;
    };
    let jh = (!java_home.trim().is_empty()).then_some(java_home);
    spawn_streaming_exe(proxy, shared, bin, args, cwd, jh, Some(horizon_home));
}

fn spawn_streaming_exe(
    proxy: EventLoopProxy<UserEvent>,
    shared: Arc<Mutex<AppState>>,
    exe: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    java_home: Option<String>,
    horizon_home: Option<PathBuf>,
) {
    let mut cmd = Command::new(&exe);
    cmd.args(&args)
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(hh) = horizon_home {
        cmd.env("HORIZON_HOME", hh);
    }
    if let Some(jh) = java_home {
        cmd.env("JAVA_HOME", jh);
    }

    match cmd.spawn() {
        Ok(mut child) => {
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            {
                let mut st = shared.lock().unwrap();
                st.active_child = Some(child);
                push_state(&proxy, &st);
            }

            if let Some(out) = stdout {
                let p = proxy.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        push_log(&p, &line);
                    }
                });
            }
            if let Some(err) = stderr {
                let p = proxy.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(err).lines().map_while(Result::ok) {
                        push_log(&p, &line);
                    }
                });
            }

            let p = proxy.clone();
            let s = shared.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    let done = {
                        let mut st = s.lock().unwrap();
                        if let Some(c) = st.active_child.as_mut() {
                            match c.try_wait() {
                                Ok(Some(status)) => {
                                    push_log(&p, &format!("[Horizon] Process exited: {status}"));
                                    st.active_child = None;
                                    true
                                }
                                Ok(None) => false,
                                Err(_) => {
                                    st.active_child = None;
                                    true
                                }
                            }
                        } else {
                            true
                        }
                    };
                    if done {
                        let _ = p.send_event(UserEvent::ProcessExited);
                        break;
                    }
                }
            });
        }
        Err(e) => {
            push_log(&proxy, &format!("[EmberClient] Failed to start {}: {e}", exe.display()));
        }
    }
}

fn search_modrinth_async(
    proxy: EventLoopProxy<UserEvent>,
    query: String,
    project_type: String,
    sort_by: String,
    offset: u64,
    mc_version: String,
) {
    std::thread::spawn(move || {
        let mut facet_parts = vec![format!("[\"project_type:{project_type}\"]")];
        if project_type == "mod" {
            facet_parts.push("[\"categories:fabric\"]".to_owned());
        }
        if !mc_version.is_empty() && project_type == "mod" {
            facet_parts.push(format!("[\"versions:{mc_version}\"]"));
        }
        let facets = format!("[{}]", facet_parts.join(","));
        let index = match sort_by.as_str() {
            "newest" | "updated" | "follows" | "relevance" => sort_by.as_str(),
            _ => "downloads",
        };
        let url = format!(
            "https://api.modrinth.com/v2/search?query={}&facets={}&index={}&offset={}&limit=20",
            url_encode(&query),
            url_encode(&facets),
            index,
            offset
        );
        let resp = ureq::get(&url)
            .set("User-Agent", "EmberClient/0.2.0")
            .call()
            .ok()
            .and_then(|r| r.into_json::<serde_json::Value>().ok())
            .unwrap_or_else(|| json!({ "hits": [], "total_hits": 0, "offset": 0, "limit": 20 }));
        let js = format!("window.renderModResults({resp});");
        let _ = proxy.send_event(UserEvent::EvalJs(js));
    });
}

fn push_install_status(proxy: &EventLoopProxy<UserEvent>, slug: &str, state: &str, msg: &str) {
    if let (Ok(s), Ok(st), Ok(m)) = (
        serde_json::to_string(slug),
        serde_json::to_string(state),
        serde_json::to_string(msg),
    ) {
        let _ = proxy.send_event(UserEvent::EvalJs(format!(
            "window.onInstallStatus({s}, {st}, {m});"
        )));
    }
}

fn extract_mc_version(selected: &str) -> String {
    if selected.contains("26.3") {
        return "1.21.11".into();
    }
    selected
        .split_whitespace()
        .last()
        .unwrap_or("1.21.11")
        .to_owned()
}

fn detect_fabric_loader_version(mc_dir: &Path, selected: &str) -> Option<(u64, u64, u64)> {
    let json_path = mc_dir.join("versions").join(selected).join(format!("{selected}.json"));
    let raw = fs::read_to_string(json_path).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&raw).ok()?;
    for lib in meta.get("libraries")?.as_array()? {
        let name = lib.get("name")?.as_str()?;
        if let Some(ver) = name.strip_prefix("net.fabricmc:fabric-loader:") {
            return parse_semver(ver);
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
            if installed < v { return false; }
        } else if let Some(v) = p.strip_prefix('>').and_then(parse_semver) {
            if installed <= v { return false; }
        } else if let Some(v) = p.strip_prefix("<=").and_then(parse_semver) {
            if installed > v { return false; }
        } else if let Some(v) = p.strip_prefix('<').and_then(parse_semver) {
            if installed >= v { return false; }
        }
    }
    true
}

fn jar_matches_fabric_loader(jar_path: &Path, installed_loader: Option<(u64, u64, u64)>) -> bool {
    let Some(installed) = installed_loader else { return true; };
    let Ok(out) = Command::new("unzip").arg("-p").arg(jar_path).arg("fabric.mod.json").output() else {
        return true;
    };
    if !out.status.success() { return true; }
    let Ok(meta) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return true; };
    if let Some(req) = meta.get("depends").and_then(|d| d.get("fabricloader")).and_then(|v| v.as_str()) {
        return satisfies_loader_req(req, installed);
    }
    true
}

fn install_modrinth_project(
    proxy: &EventLoopProxy<UserEvent>,
    game_dir: &Path,
    project_id_or_slug: &str,
    project_type: &str,
    mc_ver: &str,
    installed_loader: Option<(u64, u64, u64)>,
    depth: u8,
) -> Result<String, String> {
    if depth > 2 {
        return Ok(String::new());
    }
    let compat_url = if project_type == "mod" {
        format!(
            "https://api.modrinth.com/v2/project/{project_id_or_slug}/version?game_versions=%5B%22{mc_ver}%22%5D&loaders=%5B%22fabric%22%5D"
        )
    } else {
        format!("https://api.modrinth.com/v2/project/{project_id_or_slug}/version?game_versions=%5B%22{mc_ver}%22%5D")
    };
    let mut versions = ureq::get(&compat_url)
        .set("User-Agent", "EmberClient/0.2.0")
        .call()
        .ok()
        .and_then(|r| r.into_json::<serde_json::Value>().ok());

    if versions.as_ref().and_then(|v| v.as_array()).is_none_or(|a| a.is_empty()) {
        let fallback_url = format!("https://api.modrinth.com/v2/project/{project_id_or_slug}/version");
        versions = ureq::get(&fallback_url)
            .set("User-Agent", "EmberClient/0.2.0")
            .call()
            .ok()
            .and_then(|r| r.into_json::<serde_json::Value>().ok());
    }

    let Some(mut candidates) = versions.and_then(|v| v.as_array().cloned()) else {
        return Err("Network error fetching versions".into());
    };
    // Prefer release builds first, then beta/alpha, keeping newest-first order within each tier
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
    let _ = fs::create_dir_all(&dest_dir);

    for ver in &candidates {
        let Some(file_obj) = ver
            .get("files")
            .and_then(|f| f.as_array())
            .and_then(|arr| arr.iter().find(|f| f.get("primary").and_then(|p| p.as_bool()) == Some(true)).or_else(|| arr.first()))
        else { continue };

        let Some(dl_url) = file_obj.get("url").and_then(|u| u.as_str()) else { continue };
        let Some(filename) = file_obj.get("filename").and_then(|f| f.as_str()) else { continue };
        let safe_name = Path::new(filename).file_name().unwrap_or_default();
        let tmp_dest = dest_dir.join(format!(".tmp_{}", safe_name.to_string_lossy()));
        let final_dest = dest_dir.join(safe_name);

        let Ok(resp) = ureq::get(dl_url).set("User-Agent", "EmberClient/0.2.0").call() else { continue };
        let mut reader = resp.into_reader();
        if let Ok(mut out) = fs::File::create(&tmp_dest) {
            if std::io::copy(&mut reader, &mut out).is_err() {
                let _ = fs::remove_file(&tmp_dest);
                continue;
            }
        } else {
            continue;
        }

        if project_type == "mod" && !jar_matches_fabric_loader(&tmp_dest, installed_loader) {
            push_log(proxy, &format!("[Modrinth] Skipped {filename} (requires newer Fabric Loader)"));
            let _ = fs::remove_file(&tmp_dest);
            continue;
        }

        let _ = fs::rename(&tmp_dest, &final_dest);
        push_log(proxy, &format!("[Modrinth] Installed {filename} -> {folder}/"));

        // Install required dependencies (e.g. Fabric API)
        if project_type == "mod" {
            if let Some(deps) = ver.get("dependencies").and_then(|d| d.as_array()) {
                for dep in deps {
                    if dep.get("dependency_type").and_then(|t| t.as_str()) == Some("required") {
                        if let Some(dep_id) = dep.get("project_id").and_then(|id| id.as_str()) {
                            let _ = install_modrinth_project(
                                proxy,
                                game_dir,
                                dep_id,
                                "mod",
                                mc_ver,
                                installed_loader,
                                depth + 1,
                            );
                        }
                    }
                }
            }
        }
        return Ok(filename.to_owned());
    }

    Err(format!("No compatible version found for MC {mc_ver}"))
}

fn download_modrinth_async(
    proxy: EventLoopProxy<UserEvent>,
    game_dir: PathBuf,
    mc_dir: PathBuf,
    selected_version: String,
    slug: String,
    project_type: String,
) {
    std::thread::spawn(move || {
        let mc_ver = extract_mc_version(&selected_version);
        let loader_ver = detect_fabric_loader_version(&mc_dir, &selected_version);
        push_install_status(&proxy, &slug, "installing", &format!("Checking compatibility for {slug}..."));
        match install_modrinth_project(&proxy, &game_dir, &slug, &project_type, &mc_ver, loader_ver, 0) {
            Ok(filename) => {
                push_install_status(&proxy, &slug, "installed", &format!("Installed {filename} (compatible with {mc_ver})"));
            }
            Err(err) => {
                push_install_status(&proxy, &slug, "error", &err);
            }
        }
    });
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn main() {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("EmberClient")
        .with_inner_size(tao::dpi::LogicalSize::new(1100.0, 690.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(900.0, 560.0))
        .build(&event_loop)
        .expect("Failed to create window");

    let state = Arc::new(Mutex::new(AppState::new()));
    let ipc_state = state.clone();
    let ipc_proxy = proxy.clone();

    let html = include_str!("ui.html")
        .replace(
            "{{LOGO_DATA_URI}}",
            &format!(
                "data:image/png;base64,{}",
                base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    include_bytes!("../assets/emberclient_logo_128.png"),
                )
            ),
        )
        .replace(
            "{{PANORAMA_DATA_URI}}",
            &format!(
                "data:image/jpeg;base64,{}",
                base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    include_bytes!("../assets/panorama.jpg"),
                )
            ),
        );
    let webview = WebViewBuilder::new()
        .with_html(html)
        .with_ipc_handler(move |req| {
            let body = req.body();
            let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else { return };
            let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("");

            match action {
                "init" => {
                    let (res_dir, servers) = {
                        let st = ipc_state.lock().unwrap();
                        push_state(&ipc_proxy, &st);
                        (st.resources_dir(), st.config.servers.clone())
                    };
                    check_resources_async(ipc_proxy.clone(), res_dir);
                    ping_servers_async(ipc_proxy.clone(), servers);
                }
                "set_player_name" => {
                    if let Some(name) = v.get("name").and_then(|x| x.as_str()) {
                        let mut st = ipc_state.lock().unwrap();
                        st.config.player_name = name.to_owned();
                        st.save_config();
                    }
                }
                "open_folder" => {
                    let dir = ipc_state.lock().unwrap().base_dir.clone();
                    let _ = Command::new("open").arg(dir).spawn();
                }
                "open_url" => {
                    if let Some(url) = v.get("url").and_then(|x| x.as_str()) {
                        if url.starts_with("https://") {
                            let _ = Command::new("open").arg(url).spawn();
                        }
                    }
                }
                "add_profile" => {
                    let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("").trim();
                    let world = v.get("world").and_then(|x| x.as_str()).unwrap_or("New World").trim();
                    if !name.is_empty() {
                        let mut st = ipc_state.lock().unwrap();
                        let id = format!("prof-{}", Instant::now().elapsed().as_nanos());
                        let _ = fs::create_dir_all(st.base_dir.join("instances").join(&id));
                        st.config.profiles.push(Profile {
                            id: id.clone(),
                            name: name.to_owned(),
                            world: world.to_owned(),
                        });
                        st.config.active_profile_id = id;
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                "select_profile" => {
                    if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                        let mut st = ipc_state.lock().unwrap();
                        st.config.active_profile_id = id.to_owned();
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                "delete_profile" => {
                    if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                        let mut st = ipc_state.lock().unwrap();
                        if st.config.profiles.len() > 1 {
                            st.config.profiles.retain(|p| p.id != id);
                            if st.config.active_profile_id == id {
                                st.config.active_profile_id = st.config.profiles[0].id.clone();
                            }
                            st.save_config();
                            push_state(&ipc_proxy, &st);
                        }
                    }
                }
                "add_server" => {
                    let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("");
                    let address = v.get("address").and_then(|x| x.as_str()).unwrap_or("").trim();
                    if !address.is_empty() {
                        let entry = ServerEntry {
                            id: format!("srv-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                            name: name.to_owned(),
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
                    if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                        let mut st = ipc_state.lock().unwrap();
                        st.config.servers.retain(|s| s.id != id);
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                "ping_all" => {
                    let servers = ipc_state.lock().unwrap().config.servers.clone();
                    ping_servers_async(ipc_proxy.clone(), servers);
                }
                "search_modrinth" => {
                    let query = v.get("query").and_then(|x| x.as_str()).unwrap_or("").to_owned();
                    let project_type = v.get("projectType").and_then(|x| x.as_str()).unwrap_or("mod").to_owned();
                    let sort_by = v.get("sortBy").and_then(|x| x.as_str()).unwrap_or("downloads").to_owned();
                    let offset = v.get("offset").and_then(|x| x.as_u64()).unwrap_or(0);
                    let mc_version = {
                        let st = ipc_state.lock().unwrap();
                        extract_mc_version(&st.config.selected_version)
                    };
                    search_modrinth_async(ipc_proxy.clone(), query, project_type, sort_by, offset, mc_version);
                }
                "download_modrinth" => {
                    let slug = v.get("slug").and_then(|x| x.as_str()).unwrap_or("").to_owned();
                    let project_type = v.get("projectType").and_then(|x| x.as_str()).unwrap_or("resourcepack").to_owned();
                    let (game_dir, mc_dir, selected_version) = {
                        let st = ipc_state.lock().unwrap();
                        (st.active_game_dir(), PathBuf::from(&st.config.minecraft_dir), st.config.selected_version.clone())
                    };
                    download_modrinth_async(ipc_proxy.clone(), game_dir, mc_dir, selected_version, slug, project_type);
                }
                "save_settings" => {
                    let mut st = ipc_state.lock().unwrap();
                    if let Some(w) = v.get("resolutionWidth").and_then(|x| x.as_u64()) {
                        st.config.resolution_width = w as u32;
                    }
                    if let Some(h) = v.get("resolutionHeight").and_then(|x| x.as_u64()) {
                        st.config.resolution_height = h as u32;
                    }
                    if let Some(fs) = v.get("fullscreen").and_then(|x| x.as_bool()) {
                        st.config.fullscreen = fs;
                    }
                    if let Some(mem) = v.get("memoryMb").and_then(|x| x.as_u64()) {
                        st.config.memory_mb = mem as u32;
                    }
                    if let Some(jvm) = v.get("jvmArgs").and_then(|x| x.as_str()) {
                        st.config.jvm_args = jvm.to_owned();
                    }
                    if let Some(jh) = v.get("javaHome").and_then(|x| x.as_str()) {
                        st.config.java_home = jh.to_owned();
                    }
                    if let Some(hud) = v.get("emberHudEnabled").and_then(|x| x.as_bool()) {
                        st.config.ember_hud_enabled = hud;
                    }
                    st.save_config();
                    push_state(&ipc_proxy, &st);
                }
                "set_version" => {
                    if let Some(ver) = v.get("version").and_then(|x| x.as_str()) {
                        let mut st = ipc_state.lock().unwrap();
                        st.config.selected_version = ver.to_owned();
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                    }
                }
                "check_resources" => {
                    let res_dir = ipc_state.lock().unwrap().resources_dir();
                    check_resources_async(ipc_proxy.clone(), res_dir);
                }
                "run_prepare" => {
                    let mc_dir = v.get("minecraftDir").and_then(|x| x.as_str()).unwrap_or("").to_owned();
                    let srv_jar = v.get("serverJar").and_then(|x| x.as_str()).unwrap_or("").to_owned();
                    let java_home = v.get("javaHome").and_then(|x| x.as_str()).unwrap_or("").to_owned();
                    let (base, res_dir) = {
                        let mut st = ipc_state.lock().unwrap();
                        st.config.minecraft_dir = mc_dir.clone();
                        st.config.java_home = java_home.clone();
                        st.local_versions = discover_local_versions(Path::new(&mc_dir));
                        st.save_config();
                        push_state(&ipc_proxy, &st);
                        (st.base_dir.clone(), st.resources_dir())
                    };
                    let root = resolve_resources_root();
                    let mut args = vec![
                        "--prepare".into(),
                        "--root".into(),
                        root.to_string_lossy().into_owned(),
                        "--minecraft-dir".into(),
                        mc_dir,
                        "--output".into(),
                        res_dir.to_string_lossy().into_owned(),
                    ];
                    if !srv_jar.trim().is_empty() {
                        args.push("--server-jar".into());
                        args.push(srv_jar);
                    }
                    if !java_home.trim().is_empty() {
                        args.push("--java-home".into());
                        args.push(java_home.clone());
                    }
                    push_log(&ipc_proxy, "[Horizon] Starting 26.3 resource preparation...");
                    spawn_streaming(
                        ipc_proxy.clone(),
                        ipc_state.clone(),
                        "horizon-prepare",
                        args,
                        base.clone(),
                        java_home,
                        base,
                    );
                }
                "launch_singleplayer" => {
                    let world = v.get("world").and_then(|x| x.as_str()).unwrap_or("New World");
                    let seed = v.get("seed").and_then(|x| x.as_str()).unwrap_or("").trim();
                    let creative = v.get("creative").and_then(|x| x.as_bool()).unwrap_or(false);
                    let rd = v.get("renderDistance").and_then(|x| x.as_u64()).unwrap_or(12);
                    let name = v.get("playerName").and_then(|x| x.as_str()).unwrap_or("EmberPlayer");
                    let (base, game_dir, res_dir, java_home, mc_dir, selected_version, cfg_w, cfg_h, cfg_fs, cfg_mem, cfg_jvm, cfg_hud) = {
                        let mut st = ipc_state.lock().unwrap();
                        st.config.player_name = name.to_owned();
                        st.save_config();
                        (
                            st.base_dir.clone(),
                            st.active_game_dir(),
                            st.resources_dir(),
                            st.config.java_home.clone(),
                            st.config.minecraft_dir.clone(),
                            st.config.selected_version.clone(),
                            st.config.resolution_width,
                            st.config.resolution_height,
                            st.config.fullscreen,
                            st.config.memory_mb,
                            st.config.jvm_args.clone(),
                            st.config.ember_hud_enabled,
                        )
                    };
                    if selected_version == "Horizon 26.3 (Rust)" {
                        let mut args = vec![
                            "--resources".into(), res_dir.to_string_lossy().into_owned(),
                            "--game-dir".into(), game_dir.to_string_lossy().into_owned(),
                            "--world".into(), world.to_owned(),
                            "--render-distance".into(), rd.to_string(),
                            "--width".into(), cfg_w.to_string(),
                            "--height".into(), cfg_h.to_string(),
                            "--name".into(), name.to_owned(),
                        ];
                        if cfg_fs {
                            args.push("--fullscreen".into());
                        }
                        if !seed.is_empty() {
                            args.push("--seed".into());
                            args.push(seed.to_owned());
                        }
                        if creative {
                            args.push("--creative".into());
                        }
                        push_log(&ipc_proxy, &format!("[Horizon] Launching Singleplayer ({world}) as {name}..."));
                        spawn_streaming(ipc_proxy.clone(), ipc_state.clone(), "horizon-client", args, game_dir, java_home, base);
                    } else {
                        launch_java_version(
                            ipc_proxy.clone(),
                            ipc_state.clone(),
                            &selected_version,
                            Path::new(&mc_dir),
                            &game_dir,
                            name,
                            None,
                            &java_home,
                            cfg_mem,
                            &cfg_jvm,
                            cfg_w,
                            cfg_h,
                            cfg_fs,
                            cfg_hud,
                        );
                    }
                }
                "launch_multiplayer" => {
                    let addr = v.get("address").and_then(|x| x.as_str()).unwrap_or("").trim();
                    let rd = v.get("renderDistance").and_then(|x| x.as_u64()).unwrap_or(12);
                    let name = v.get("playerName").and_then(|x| x.as_str()).unwrap_or("EmberPlayer");
                    if addr.is_empty() { return; }
                    let (base, game_dir, res_dir, java_home, mc_dir, selected_version, cfg_w, cfg_h, cfg_fs, cfg_mem, cfg_jvm, cfg_hud) = {
                        let mut st = ipc_state.lock().unwrap();
                        st.config.player_name = name.to_owned();
                        st.save_config();
                        (
                            st.base_dir.clone(),
                            st.active_game_dir(),
                            st.resources_dir(),
                            st.config.java_home.clone(),
                            st.config.minecraft_dir.clone(),
                            st.config.selected_version.clone(),
                            st.config.resolution_width,
                            st.config.resolution_height,
                            st.config.fullscreen,
                            st.config.memory_mb,
                            st.config.jvm_args.clone(),
                            st.config.ember_hud_enabled,
                        )
                    };
                    if selected_version == "Horizon 26.3 (Rust)" {
                        let mut args = vec![
                            "--resources".into(), res_dir.to_string_lossy().into_owned(),
                            "--game-dir".into(), game_dir.to_string_lossy().into_owned(),
                            "--server".into(), addr.to_owned(),
                            "--render-distance".into(), rd.to_string(),
                            "--width".into(), cfg_w.to_string(),
                            "--height".into(), cfg_h.to_string(),
                            "--name".into(), name.to_owned(),
                        ];
                        if cfg_fs {
                            args.push("--fullscreen".into());
                        }
                        push_log(&ipc_proxy, &format!("[Horizon] Connecting to {addr} as {name}..."));
                        spawn_streaming(ipc_proxy.clone(), ipc_state.clone(), "horizon-client", args, game_dir, java_home, base);
                    } else {
                        launch_java_version(
                            ipc_proxy.clone(),
                            ipc_state.clone(),
                            &selected_version,
                            Path::new(&mc_dir),
                            &game_dir,
                            name,
                            Some(addr),
                            &java_home,
                            cfg_mem,
                            &cfg_jvm,
                            cfg_w,
                            cfg_h,
                            cfg_fs,
                            cfg_hud,
                        );
                    }
                }
                "stop_process" => {
                    let mut st = ipc_state.lock().unwrap();
                    if let Some(child) = st.active_child.as_mut() {
                        let _ = child.kill();
                    }
                }
                _ => {}
            }
        })
        .build(&window)
        .expect("Failed to build WebView");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::EvalJs(js)) => {
                let _ = webview.evaluate_script(&js);
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
            Event::UserEvent(UserEvent::ProcessExited) => {
                let res_dir = {
                    let st = state.lock().unwrap();
                    let _ = webview.evaluate_script(&format!("window.renderState({});", st.state_json()));
                    st.resources_dir()
                };
                check_resources_async(proxy.clone(), res_dir);
            }
            _ => {}
        }
    });
}
