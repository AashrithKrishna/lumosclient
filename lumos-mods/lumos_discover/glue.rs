// Lumos Discover: between Horizon's `Game` and the rules in `lumos_discover`.
//
// The registry is read and files are downloaded on a worker with the game's own registry code
// (`horizon_client::mod_registry`, the in-game Browse Mods' path): the index with its revoked
// list, each file checked against it (`horizon_mods::index::check_file`: size, SHA-256, the
// author's minisign signature, the file's own manifest) before it is moved into the instance.
// What needs the game mutably (switching packs, mods, the restart) runs in the client tick.
//
// Modrinth's shader packs (the Shaders tab, after the registry's) are read from Modrinth's public
// API on a worker too, one request at a time and only as the list is scrolled or searched
// (`lumos_discover::Feed`). A pack's file comes only from `cdn.modrinth.com` and goes into
// `shaderpacks/` only with Modrinth's size and SHA-512.
use crate::game::lumos_discover as m;
use crate::game::lumos_discover_keys as keys;
use crate::game::{Game, Screen};
use crate::mod_api::{NativeMod, NativeScreenView};
use horizon_client::hud::HudBatch;
use horizon_client::mod_registry;
use horizon_client::text::Text;
use horizon_client::ui;
use horizon_mods::index::{Index, Kind as RegistryKind, Trust};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha512};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Which native mods Lumos's cloud builds have (`obsidian-launcher-rs/src/native_builds.rs`).
const NATIVE_INDEX: &str = "https://github.com/AashrithKrishna/lumosclient/releases/download/native-builds/native-index.json";
/// Where a download waits for its checks (the in-game Browse Mods' folder).
const DOWNLOADS: &str = "mod-browser/downloads";
const AGENT: &str = "lumos-client/lumos_discover";

/// What a worker finished.
enum Finished {
    Catalogue(Result<(Arc<Index>, Vec<m::Entry>), String>),
    /// A pack or content mod landed in the instance: (id, its kind, its file, signed) or why not.
    Installed(String, m::Kind, Result<(String, bool), String>),
    /// A Modrinth search page: (search key, offset, its hits and total, or why not).
    Search(String, usize, Result<(Vec<m::Entry>, usize), String>),
}

/// Modrinth projects' versions read this session, by project id.
static MODRINTH_VERSIONS: Mutex<BTreeMap<String, Vec<m::MrVersion>>> = Mutex::new(BTreeMap::new());
/// What Discover installed from Modrinth, in its data folder: `<project id> <file>` a line.
const MODRINTH_RECORDS: &str = "modrinth-shaders.txt";

#[derive(Default)]
struct Jobs {
    catalogue: Option<Receiver<Finished>>,
    install: Option<Receiver<Finished>>,
    search: Option<Receiver<Finished>>,
    index: Option<Arc<Index>>,
    /// Files to delete once the game no longer reads them: (path, ticks to wait).
    delete: Vec<(PathBuf, u32)>,
}

thread_local! {
    static JOBS: RefCell<Jobs> = RefCell::new(Jobs::default());
}

fn jobs<R>(f: impl FnOnce(&mut Jobs) -> R) -> R {
    JOBS.with(|j| f(&mut j.borrow_mut()))
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(work) {
        log::warn!("Lumos Discover: {name} did not start: {e}");
    }
}

fn settings(game: &Game<'_>) -> m::Settings {
    m::settings(|key| game.native_setting(m::ID, key))
}

fn open(game: &Game<'_>) -> bool {
    game.native_mod_screen() == Some(m::ID)
}

// ── the catalogue ──

/// The native mods a Lumos cloud build has for this computer; none when it can't be read.
fn cloud_mods() -> BTreeSet<String> {
    let platform = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-arm64"
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        "windows-x64"
    } else {
        return BTreeSet::new();
    };
    let read = || -> Result<serde_json::Value, String> {
        let response = ureq::get(NATIVE_INDEX).set("User-Agent", AGENT).call().map_err(|e| e.to_string())?;
        let mut text = String::new();
        response.into_reader().take(4 << 20).read_to_string(&mut text).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    };
    let index = match read() {
        Ok(v) if v["schema"] == 1 => v,
        Ok(_) => return BTreeSet::new(),
        Err(e) => {
            log::info!("Lumos Discover: Lumos's native builds weren't read: {e}");
            return BTreeSet::new();
        }
    };
    index["builds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["platform"] == platform)
        .flat_map(|b| b["mods"].as_array().cloned().unwrap_or_default())
        .filter_map(|m| m["id"].as_str().map(str::to_owned))
        .filter(|id| m::valid_id(id))
        .collect()
}

/// The registry's projects this game can use, then Lumos's own mods.
fn entries(index: &Index, cloud: &BTreeSet<String>) -> Vec<m::Entry> {
    let level = horizon_mods::api_level::LEVEL;
    let mut out: Vec<m::Entry> = index
        .projects
        .iter()
        .filter(|p| !m::OWN.iter().any(|o| o.0 == p.id))
        .filter_map(|p| {
            let (kind, version) = match p.kind {
                RegistryKind::Mod => (m::Kind::Content, Some(mod_registry::installable(index, p)?)),
                RegistryKind::NativeMod => {
                    let v = index.installable(p, mod_registry::GAME, level);
                    if v.is_none() && !cloud.contains(&p.id) {
                        return None;
                    }
                    (m::Kind::Native, None)
                }
                RegistryKind::ResourcePack => (m::Kind::ResourcePack, Some(index.installable(p, mod_registry::GAME, level)?)),
                RegistryKind::Shader | RegistryKind::ShaderPack => (m::Kind::Shader, Some(index.installable(p, mod_registry::GAME, level)?)),
                _ => return None,
            };
            Some(m::Entry {
                id: p.id.clone(),
                name: p.name.clone(),
                summary: p.summary.clone(),
                author: p.authors.join(", "),
                kind,
                categories: p.categories.clone(),
                tags: p.tags.clone(),
                downloads: p.downloads,
                version: version.map(|v| v.version.clone()).unwrap_or_default(),
                file: version.map(|v| v.file.name.clone()).unwrap_or_default(),
                own: false,
                cloud: cloud.contains(&p.id),
                modrinth: None,
            })
        })
        .collect();
    out.extend(m::own_entries(&|id| cloud.contains(id)));
    out
}

/// Reads the registry again (the kept copy when it can't be reached), on a worker.
fn load_catalogue() {
    if jobs(|j| j.catalogue.is_some()) {
        return;
    }
    m::with(|s| {
        s.ui.loading = true;
        s.ui.error = None;
    });
    let home = Game::game_dir();
    let (send, receive) = channel();
    jobs(|j| j.catalogue = Some(receive));
    spawn("lumos-discover-index", move || {
        let read = mod_registry::load_index(&home, &mod_registry::location()).map(|index| {
            let list = entries(&index, &cloud_mods());
            (Arc::new(index), list)
        });
        let _ = send.send(Finished::Catalogue(read));
    });
}

// ── what the instance has ──

fn requests() -> m::Requests {
    fs::read_to_string(Game::game_dir().join(m::REQUESTS_FILE))
        .map(|t| m::Requests::parse(&t))
        .unwrap_or_default()
}

fn save_requests(r: &m::Requests) {
    let path = Game::game_dir().join(m::REQUESTS_FILE);
    let saved = if r.is_empty() && !r.restart {
        fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
    } else {
        fs::write(&path, r.to_json())
    };
    if let Err(e) = saved {
        log::warn!("Lumos Discover: {}: {e}", path.display());
    }
}

fn file_names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

/// What this instance has of every entry, read again.
fn refresh(game: &Game<'_>) {
    let home = Game::game_dir();
    let native_text = fs::read_to_string(home.join(m::NATIVE_FILE)).unwrap_or_default();
    let instance = m::native_mods(&native_text);
    let asked = requests();
    let packs = file_names(&super::packs::packs_dir());
    let shader_dir = Game::shader_packs_dir();
    let shaders = file_names(&shader_dir);
    let active_shader = match game.settings.horizon.shader_pack.as_str() {
        "" => String::new(),
        picked => horizon_client::shader_pack_list::resolve(&shader_dir, picked)
            .1
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let mods = &game.settings.mods;
    let applied = game.applied_mods();
    // Modrinth's packs: the file recorded at their install, or one named like them.
    let shader_files: Vec<String> = shaders.iter().cloned().collect();
    let records = modrinth_records(game);
    m::with(|s| {
        for e in s.entries.iter_mut() {
            if let Some(slug) = &e.modrinth {
                let recorded = e.id.strip_prefix(m::MODRINTH_PREFIX).and_then(|p| records.get(p)).map(String::as_str);
                e.file = m::installed_file(slug, &e.name, &shader_files, recorded).unwrap_or_default();
            }
        }
    });
    let have: BTreeMap<String, m::Have> = m::with(|s| {
        s.entries
            .iter()
            .map(|e| {
                let id = e.id.as_str();
                let h = match e.kind {
                    m::Kind::Content => {
                        let installed = mods.installed.iter().any(|r| r.id == id && r.native.is_none());
                        let on = mods.enabled.contains(id);
                        let waits = installed && applied.contains(id) != on;
                        m::Have { installed, on, built_in: false, pending: if waits { m::Pending::Restart } else { m::Pending::None } }
                    }
                    m::Kind::Native => {
                        let built_in = mods.installed.iter().any(|r| r.id == id && r.native.is_some());
                        let on = built_in && !mods.disabled.contains(id);
                        let req = asked.pending(id);
                        if built_in {
                            let installed = match req {
                                m::Pending::Install => true,
                                m::Pending::Remove => false,
                                _ => instance.iter().any(|i| i == id) || on,
                            };
                            m::Have { installed, on, built_in, pending: m::Pending::None }
                        } else {
                            m::Have { installed: instance.iter().any(|i| i == id), on: false, built_in, pending: req }
                        }
                    }
                    m::Kind::ResourcePack => {
                        let installed = !e.file.is_empty() && packs.contains(&e.file);
                        let on = installed && game.settings.resource_packs.iter().any(|p| *p == format!("file/{}", e.file));
                        m::Have { installed, on, ..m::Have::default() }
                    }
                    m::Kind::Shader => {
                        let installed = !e.file.is_empty() && shaders.contains(&e.file);
                        m::Have { installed, on: installed && active_shader == e.file, ..m::Have::default() }
                    }
                };
                (e.id.clone(), h)
            })
            .collect()
    });
    m::with(|s| {
        let restart = have.values().any(|h| h.pending != m::Pending::None);
        s.have = have;
        s.ui.restart = restart;
        s.stale = 0;
    });
}

// ── installing ──

/// Downloads entry `id`'s file, checks it against the index and moves it into `folder` under
/// the index's file name. Returns that name and whether the author's signature checked.
fn fetch_into(index: &Index, home: &Path, id: &str, folder: &Path) -> Result<(String, bool), String> {
    let p = index.project(id).ok_or_else(|| format!("{id} isn't in Horizon's registry"))?;
    let v = index
        .installable(p, mod_registry::GAME, horizon_mods::api_level::LEVEL)
        .ok_or_else(|| format!("{} has no version for this game", p.name))?;
    let name = v.file.name.clone();
    // The index checks its names; a name that could leave the folder is refused all the same.
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']) || name.contains("..") {
        return Err(format!("{} has an unsafe file name", p.name));
    }
    let bytes = mod_registry::download(&v.file.url, v.file.size)?;
    let staging = home.join(DOWNLOADS);
    fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    let part = staging.join(format!("{id}.part"));
    fs::write(&part, &bytes).map_err(|e| format!("{}: {e}", part.display()))?;
    let trust = horizon_mods::index::check_file(index, p, v, &part).inspect_err(|_| {
        let _ = fs::remove_file(&part);
    })?;
    fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let target = folder.join(&name);
    fs::rename(&part, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok((name, trust == Trust::Verified))
}

// ── Modrinth ──

fn modrinth_records_path(game: &Game<'_>) -> PathBuf {
    game.native_data_dir(m::ID).join(MODRINTH_RECORDS)
}

fn modrinth_records(game: &Game<'_>) -> BTreeMap<String, String> {
    m::parse_records(&fs::read_to_string(modrinth_records_path(game)).unwrap_or_default())
}

/// Records (`Some(file)`) or forgets (`None`) Modrinth project `project`'s file.
fn modrinth_record(game: &Game<'_>, project: &str, file: Option<&str>) {
    let mut records = modrinth_records(game);
    match file {
        Some(f) => records.insert(project.to_owned(), f.to_owned()),
        None => records.remove(project),
    };
    let path = modrinth_records_path(game);
    let written = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&path, m::records_text(&records)));
    if let Err(e) = written {
        log::warn!("Lumos Discover: {}: {e}", path.display());
    }
}

/// GETs `url` from Modrinth (at most `max` bytes, no redirects followed), with a generic client
/// name and nothing else about the player.
fn modrinth_get(url: &str, max: u64, timeout: u64) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(timeout))
        .build();
    let response = agent.get(url).set("User-Agent", m::MODRINTH_AGENT).call().map_err(|e| match e {
        ureq::Error::Status(429, _) => "too many requests just now, try again shortly".to_owned(),
        ureq::Error::Status(code, _) => format!("Modrinth answered {code}"),
        ureq::Error::Transport(t) => {
            log::info!("Lumos Discover: {url}: {t}");
            "can't be reached (offline?)".to_owned()
        }
    })?;
    if response.status() != 200 {
        return Err(format!("Modrinth answered {}", response.status()));
    }
    let mut body = Vec::new();
    response.into_reader().take(max + 1).read_to_end(&mut body).map_err(|e| format!("the download stopped: {e}"))?;
    if body.len() as u64 > max {
        return Err("Modrinth's answer is larger than expected".into());
    }
    Ok(body)
}

/// Asks Modrinth for search `key`'s hits from `offset`, on a worker.
fn search_modrinth(key: String, offset: usize) {
    let (send, receive) = channel();
    jobs(|j| j.search = Some(receive));
    m::with(|s| s.feed.sent(&key, offset));
    spawn("lumos-discover-modrinth", move || {
        let read = modrinth_get(&m::search_url(&key, offset), 4 << 20, 20)
            .and_then(|b| String::from_utf8(b).map_err(|_| "Modrinth's answer isn't text".to_owned()))
            .and_then(|t| m::parse_search(&t));
        if let Err(e) = &read {
            log::info!("Lumos Discover: Modrinth search {key:?} at {offset}: {e}");
        }
        let _ = send.send(Finished::Search(key, offset, read));
    });
}

/// Installs Modrinth project `project`'s pack into `folder` (on a worker): its versions (kept for
/// the session), the one `pick_version` picks, its primary file from `cdn.modrinth.com` checked
/// against Modrinth's size and SHA-512, under a name that stays in the folder.
fn fetch_modrinth(home: &Path, project: &str, folder: &Path) -> Result<(String, bool), String> {
    let cached = MODRINTH_VERSIONS.lock().ok().and_then(|v| v.get(project).cloned());
    let versions = match cached {
        Some(v) => v,
        None => {
            let url = m::versions_url(project).ok_or("it isn't a Modrinth project")?;
            let text = String::from_utf8(modrinth_get(&url, 16 << 20, 30)?).map_err(|_| "Modrinth's answer isn't text")?;
            let v = m::parse_versions(&text)?;
            if let Ok(mut cache) = MODRINTH_VERSIONS.lock() {
                cache.insert(project.to_owned(), v.clone());
            }
            v
        }
    };
    let v = m::pick_version(&versions, mod_registry::GAME).ok_or("Modrinth has no file of it")?;
    let f = m::primary_file(v).ok_or("Modrinth has no file of it")?;
    if !m::cdn_url_ok(&f.url) {
        return Err("its file isn't on cdn.modrinth.com".into());
    }
    if !m::safe_file_name(&f.name) {
        return Err(format!("its file name {:?} isn't a plain .zip name", f.name));
    }
    if f.size == 0 || f.size > m::MODRINTH_MAX_FILE {
        return Err(format!("its file is {} bytes", f.size));
    }
    log::info!("Lumos Discover: Modrinth {project}: version {} ({}), {}", v.number, v.id, f.url);
    let bytes = modrinth_get(&f.url, f.size, 300)?;
    let sha512 = format!("{:x}", Sha512::digest(&bytes));
    m::check_download(f, &bytes, &sha512)?;
    let staging = home.join(DOWNLOADS);
    fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    let part = staging.join(format!("modrinth-{project}.part"));
    fs::write(&part, &bytes).map_err(|e| format!("{}: {e}", part.display()))?;
    fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let target = folder.join(&f.name);
    if target.parent() != Some(folder) {
        let _ = fs::remove_file(&part);
        return Err("its file name leaves shaderpacks/".into());
    }
    fs::rename(&part, &target).map_err(|e| {
        let _ = fs::remove_file(&part);
        format!("{}: {e}", target.display())
    })?;
    Ok((f.name.clone(), true))
}

fn install(e: &m::Entry) {
    if let Some(project) = e.id.strip_prefix(m::MODRINTH_PREFIX).filter(|_| e.modrinth.is_some()) {
        if jobs(|j| j.install.is_some()) {
            m::with(|s| s.ui.message = Some(("One download at a time: wait for the one running.".into(), false)));
            return;
        }
        let (home, id, project, folder) = (Game::game_dir(), e.id.clone(), project.to_owned(), Game::shader_packs_dir());
        let (send, receive) = channel();
        jobs(|j| j.install = Some(receive));
        m::with(|s| {
            s.ui.busy = Some(id.clone());
            s.ui.message = Some((format!("Downloading {} from Modrinth...", e.name), true));
        });
        spawn("lumos-discover-install", move || {
            let done = fetch_modrinth(&home, &project, &folder);
            let _ = send.send(Finished::Installed(id, m::Kind::Shader, done));
        });
        return;
    }
    let Some(index) = jobs(|j| j.index.clone()) else { return };
    if jobs(|j| j.install.is_some()) {
        m::with(|s| s.ui.message = Some(("One download at a time: wait for the one running.".into(), false)));
        return;
    }
    let (home, id, kind) = (Game::game_dir(), e.id.clone(), e.kind);
    let folder = match kind {
        m::Kind::ResourcePack => super::packs::packs_dir(),
        m::Kind::Shader => Game::shader_packs_dir(),
        _ => PathBuf::new(),
    };
    let (send, receive) = channel();
    jobs(|j| j.install = Some(receive));
    m::with(|s| {
        s.ui.busy = Some(id.clone());
        s.ui.message = Some((format!("Downloading {}...", e.name), true));
    });
    spawn("lumos-discover-install", move || {
        let done = match kind {
            // As Browse Mods installs one: horizon-mods/<id>.hmod, recorded in mod-browser.json.
            m::Kind::Content => mod_registry::install(&index, &home, &id, &mod_registry::download).map(|d| (format!("{}.hmod", d.id), d.verified)),
            _ => fetch_into(&index, &home, &id, &folder),
        };
        let _ = send.send(Finished::Installed(id, kind, done));
    });
}

fn name_of(id: &str) -> String {
    m::with(|s| s.entries.iter().find(|e| e.id == id).map_or(id.to_owned(), |e| e.name.clone()))
}

fn say(text: String, ok: bool) {
    m::with(|s| s.ui.message = Some((text, ok)));
}

fn save(game: &Game<'_>) {
    game.settings.save(&Game::options_path());
}

/// A finished download, put to use: packs apply at once, a content mod switches on (live when
/// it has only assets and data).
fn landed(game: &mut Game<'_>, id: String, kind: m::Kind, done: Result<(String, bool), String>) {
    m::with(|s| s.ui.busy = None);
    let name = name_of(&id);
    let (file, verified) = match done {
        Ok(d) => d,
        Err(why) => {
            log::warn!("Lumos Discover: {id} wasn't installed: {why}");
            say(format!("{name} wasn't installed: {why}"), false);
            return;
        }
    };
    let project = id.strip_prefix(m::MODRINTH_PREFIX).map(str::to_owned);
    if let Some(p) = &project {
        log::info!("Lumos Discover: installed Modrinth {p} ({file}, size and SHA-512 checked)");
        modrinth_record(game, p, Some(&file));
    } else {
        log::info!("Lumos Discover: installed {id} ({file}, {})", if verified { "signed" } else { "unsigned" });
    }
    match kind {
        m::Kind::Content => {
            game.settings.mods.enabled.insert(id.clone());
            game.settings.mods.disabled.remove(&id);
            save(game);
            game.start_mods();
            game.switch_mods();
            if game.applied_mods().contains(&id) {
                say(format!("{name} is installed and on."), true);
            } else {
                say(format!("{name} is installed. It adds blocks or items, so it loads when the game restarts."), true);
            }
        }
        m::Kind::ResourcePack => {
            let pack = format!("file/{file}");
            game.settings.resource_packs.retain(|p| *p != pack);
            game.settings.resource_packs.push(pack);
            save(game);
            game.reload_packs();
            say(format!("{name} is installed and applied."), true);
        }
        m::Kind::Shader => {
            pick_shader(game, &file);
            if project.is_some() {
                say(format!("{name} is installed and on. {}", m::MODRINTH_NOTE), true);
            } else {
                say(format!("{name} is installed and on."), true);
            }
        }
        m::Kind::Native => {}
    }
}

/// Runs shader pack `file` (`""`: none), as Options > Shader Packs does.
fn pick_shader(game: &mut Game<'_>, file: &str) {
    let dir = Game::shader_packs_dir();
    game.settings.horizon.shader_pack = if file.is_empty() {
        String::new()
    } else {
        horizon_client::shader_pack_list::resolve(&dir, file).0
    };
    save(game);
    game.settings.shader_packs = horizon_client::shader_pack_list::scan(&dir);
}

/// The content mod `id`'s files in `horizon-mods/`, and its `mod-browser.json` entry, go.
fn delete_content(id: &str) {
    let home = Game::game_dir();
    for path in horizon_mods::find(&home.join(horizon_client::mod_list::DIR)) {
        let ours = path.is_file() && horizon_mods::Package::read(&path).is_ok_and(|p| p.manifest.id == id);
        if ours && let Err(e) = fs::remove_file(&path) {
            log::warn!("Lumos Discover: {}: {e}", path.display());
        }
    }
    let manifest = home.join(mod_registry::MANIFEST);
    if let Some(mut v) = fs::read_to_string(&manifest).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        && let Some(entries) = v.get_mut("entries").and_then(|e| e.as_array_mut())
    {
        entries.retain(|e| !(e["source"] == "horizon" && e["project"] == id));
        if let Ok(text) = serde_json::to_string_pretty(&v) {
            let _ = fs::write(&manifest, text);
        }
    }
}

/// A row's button.
fn act(game: &mut Game<'_>, i: usize, act: m::Act) {
    let Some(e) = m::with(|s| s.entries.get(i).cloned()) else { return };
    let have = m::with(|s| s.have.get(&e.id).copied().unwrap_or_default());
    let id = e.id.clone();
    let name = e.name.clone();
    match (e.kind, act) {
        (m::Kind::Native, _) => native(game, &e, have, act),
        (_, m::Act::Install) => install(&e),
        (m::Kind::Content, m::Act::Undo) => {}
        (m::Kind::Content, m::Act::On | m::Act::Off | m::Act::Remove) => {
            let on = act == m::Act::On;
            if on {
                game.settings.mods.enabled.insert(id.clone());
                let mut asked = requests();
                asked.undo(&id);
                save_requests(&asked);
            } else {
                game.settings.mods.enabled.remove(&id);
            }
            save(game);
            game.switch_mods();
            let loaded = game.applied_mods().contains(&id);
            if act == m::Act::Remove {
                if loaded {
                    // Its blocks or items are in the running game: Lumos deletes it after.
                    let mut asked = requests();
                    asked.take_out(&id);
                    save_requests(&asked);
                    say(format!("{name} is off. Lumos deletes it when the game closes."), true);
                } else {
                    delete_content(&id);
                    game.start_mods();
                    say(format!("{name} is removed."), true);
                }
            } else if loaded == on {
                say(format!("{name} is {}.", if on { "on" } else { "off" }), true);
            } else {
                say(format!("{name} adds blocks or items: restart to turn it {}.", if on { "on" } else { "off" }), true);
            }
        }
        (m::Kind::ResourcePack, _) => {
            let pack = format!("file/{}", e.file);
            game.settings.resource_packs.retain(|p| *p != pack);
            if act == m::Act::On {
                game.settings.resource_packs.push(pack);
            }
            save(game);
            game.reload_packs();
            if act == m::Act::Remove {
                jobs(|j| j.delete.push((super::packs::packs_dir().join(&e.file), 40)));
                say(format!("{name} is removed."), true);
            } else {
                say(format!("{name} is {}.", if act == m::Act::On { "applied" } else { "off" }), true);
            }
        }
        (m::Kind::Shader, _) => {
            if act == m::Act::On {
                pick_shader(game, &e.file);
            } else if have.on {
                pick_shader(game, "");
            }
            if act == m::Act::Remove {
                if let Some(project) = id.strip_prefix(m::MODRINTH_PREFIX) {
                    modrinth_record(game, project, None);
                }
                jobs(|j| j.delete.push((Game::shader_packs_dir().join(&e.file), 40)));
                say(format!("{name} is removed."), true);
            } else {
                say(format!("{name} is {}.", if act == m::Act::On { "on" } else { "off" }), true);
            }
        }
    }
}

/// Native mods: one compiled into this game switches now (`native_on`); otherwise Lumos adds
/// or takes it out when the game closes (`lumos-requests.json`), and Restart applies it.
fn native(game: &mut Game<'_>, e: &m::Entry, have: m::Have, act: m::Act) {
    if e.id == m::ID {
        return;
    }
    let id = e.id.as_str();
    let mut asked = requests();
    let switch = |game: &mut Game<'_>, on: bool| {
        if on {
            game.settings.mods.disabled.remove(id);
        } else {
            game.settings.mods.disabled.insert(id.to_owned());
        }
        save(game);
        game.sync_native_switches();
    };
    let text = match act {
        m::Act::Install => {
            asked.add(id);
            if have.built_in {
                switch(game, true);
                format!("{} is on. Lumos keeps it in this instance.", e.name)
            } else {
                format!("{} installs when the game restarts.", e.name)
            }
        }
        m::Act::Remove => {
            asked.take_out(id);
            if have.built_in {
                switch(game, false);
                format!("{} is off, and leaves this instance when the game closes.", e.name)
            } else {
                format!("{} is removed when the game restarts.", e.name)
            }
        }
        m::Act::On => {
            if !have.installed {
                asked.add(id);
            }
            switch(game, true);
            format!("{} is on.", e.name)
        }
        m::Act::Off => {
            switch(game, false);
            format!("{} is off.", e.name)
        }
        m::Act::Undo => {
            asked.undo(id);
            format!("{}: change taken back.", e.name)
        }
    };
    save_requests(&asked);
    say(text, true);
}

/// Restart: asks Lumos to apply what waits and start the game again, then closes the game as
/// its window's close button does (the world saved first).
fn restart(game: &mut Game<'_>) {
    let mut asked = requests();
    asked.restart = true;
    save_requests(&asked);
    log::info!("Lumos Discover: closing the game for Lumos to restart it");
    game.native_mod_screen_close(m::ID);
    if let Err(e) = game.begin_closing_save(true) {
        log::warn!("Lumos Discover: the game didn't close: {e}");
    }
}

/// Opens the screen, from the world or from the pause menu.
fn open_screen(game: &mut Game<'_>) {
    if game.screen == Screen::Paused && game.player.is_some() {
        // As the pause menu's Back to Game does, then the screen over the world.
        game.pause_focus = None;
        game.screen = Screen::Playing;
    }
    if !game.native_playing() || game.player.is_none() {
        return;
    }
    if game.native_mod_screen_open(m::ID) {
        m::with(|s| {
            s.ui.typing = true;
            s.ui.message = None;
        });
        refresh(game);
        if jobs(|j| j.index.is_none()) {
            load_catalogue();
        }
    }
}

// ── hooks ──

/// `native_key`: the hotkey opens and closes the screen, in the world or the pause menu. Bound
/// to a key that types (a letter), it doesn't close the screen while the search is typed in:
/// the letter goes into the search (Escape still closes).
pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || !game.native_on(m::ID) || !keys::pressed(game, "open", key, pressed) {
        return;
    }
    if open(game) {
        if typing_into_search(key) {
            return;
        }
        game.native_mod_screen_close(m::ID);
    } else if game.native_playing() || game.screen == Screen::Paused {
        open_screen(game);
    }
}

/// The client tick: what the workers finished, and what the screen's clicks asked for.
pub(super) fn on_tick(game: &mut Game<'_>) {
    if !game.native_on(m::ID) {
        if open(game) {
            game.native_mod_screen_close(m::ID);
        }
        return;
    }
    // Files waiting for the game to stop reading them.
    let reloading = game.pack_reload.is_some() || game.pending_pack_reload;
    let due: Vec<PathBuf> = jobs(|j| {
        for d in &mut j.delete {
            if !reloading {
                d.1 = d.1.saturating_sub(1);
            }
        }
        let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut j.delete).into_iter().partition(|d| d.1 == 0);
        j.delete = wait;
        due.into_iter().map(|d| d.0).collect()
    });
    for path in due {
        if let Err(e) = fs::remove_file(&path) {
            log::warn!("Lumos Discover: {}: {e}", path.display());
        }
    }
    if m::with(|s| std::mem::take(&mut s.open)) {
        open_screen(game);
    }
    let finished = jobs(|j| {
        let mut out = Vec::new();
        for slot in [&mut j.catalogue, &mut j.install, &mut j.search] {
            if let Some(rx) = slot {
                match rx.try_recv() {
                    Ok(f) => {
                        out.push(f);
                        *slot = None;
                    }
                    Err(TryRecvError::Disconnected) => *slot = None,
                    Err(TryRecvError::Empty) => {}
                }
            }
        }
        out
    });
    let mut changed = false;
    for f in finished {
        changed = true;
        match f {
            Finished::Catalogue(Ok((index, list))) => {
                jobs(|j| j.index = Some(index));
                m::with(|s| {
                    s.registry = list;
                    s.ui.loading = false;
                });
            }
            Finished::Catalogue(Err(why)) => {
                log::warn!("Lumos Discover: the registry wasn't read: {why}");
                m::with(|s| {
                    s.ui.loading = false;
                    s.ui.error = Some(why);
                });
            }
            Finished::Installed(id, kind, done) => landed(game, id, kind, done),
            Finished::Search(key, offset, read) => m::with(|s| s.feed.landed(&key, offset, read)),
        }
    }
    // A search worker that ended without an answer.
    if jobs(|j| j.search.is_none())
        && let Some((key, offset)) = m::with(|s| s.feed.busy.clone())
    {
        m::with(|s| s.feed.landed(&key, offset, Err("the search stopped".into())));
    }
    let modrinth = settings(game).modrinth;
    m::with(|s| {
        let query = s.ui.query.clone();
        s.feed.tick(&query);
    });
    if !open(game) {
        if changed {
            refresh(game);
        }
        return;
    }
    let queue = m::with(|s| std::mem::take(&mut s.queue));
    for hit in queue {
        changed = true;
        match hit {
            m::Hit::Tab(tab) => m::with(|s| s.ui.set_tab(tab)),
            m::Hit::Category(c) => m::with(|s| {
                s.ui.category = c;
                s.ui.scroll = 0;
            }),
            m::Hit::Search => m::with(|s| s.ui.typing = true),
            m::Hit::Retry => load_catalogue(),
            m::Hit::Done => {
                game.native_mod_screen_close(m::ID);
                return;
            }
            m::Hit::Restart => {
                restart(game);
                return;
            }
            m::Hit::Row(i, a) => act(game, i, a),
        }
    }
    // Modrinth's next page, when the Shaders tab wants one.
    if modrinth && jobs(|j| j.search.is_none()) {
        let (sw, sh) = game.gui_size();
        let fits = m::rows_fit(&m::layout(sw, sh));
        let settings = settings(game);
        let next = m::with(|s| {
            if s.ui.tab != m::Tab::Shaders {
                return None;
            }
            let have = |id: &str| s.have.get(id).copied().unwrap_or_default();
            let width = |_: &str| 0.0_f32;
            let view = m::View { entries: &s.entries, have: &have, width: &width, settings, tail: None };
            let want_more = s.ui.scroll + fits + 4 >= m::rows(&s.ui, &view).len();
            s.feed.next(&s.ui.query, want_more)
        });
        if let Some((key, offset)) = next {
            search_modrinth(key, offset);
        }
    }
    // After the clicks, which name rows as they were drawn.
    if m::with(|s| s.compose(modrinth)) {
        changed = true;
    }
    // What the instance has is read again after a change, and every two seconds.
    let stale = m::with(|s| {
        s.stale = s.stale.saturating_add(1);
        s.stale
    });
    if changed || stale >= 40 {
        refresh(game);
    }
}

fn paint(game: &Game<'_>, batch: &mut HudBatch, prims: &[m::Prim]) {
    for p in prims {
        match p {
            m::Prim::Fill { rect, argb } => game.native_fill(batch, *rect, *argb),
            m::Prim::Text { text, at, argb, shadow } => game.native_text(batch, text, *at, *argb & 0xFF_FFFF, *shadow),
            m::Prim::Button { rect, label, active, hover } => {
                ui::button(batch, &game.gui.atlas, rect[0], rect[1], rect[2], rect[3], &Text::literal(label.as_str()), *active, *hover)
            }
        }
    }
}

/// The line after the Shaders tab's rows: Modrinth's state, when its packs are listed.
fn tail(s: &m::State, settings: m::Settings) -> Option<(String, u32)> {
    (settings.modrinth && s.ui.tab == m::Tab::Shaders).then(|| s.feed.tail(&s.ui.query))
}

/// The screen's frame: the catalogue, what the instance has, the current search.
fn with_frame<R>(game: &Game<'_>, mouse: [f32; 2], f: impl FnOnce(Vec<m::Prim>, Vec<([f32; 4], m::Hit)>) -> R) -> R {
    let (sw, sh) = game.gui_size();
    let settings = settings(game);
    let width = |t: &str| game.native_text_width(t);
    m::with(|s| {
        let have = |id: &str| s.have.get(id).copied().unwrap_or_default();
        let view = m::View { entries: &s.entries, have: &have, width: &width, settings, tail: tail(s, settings) };
        let (prims, hits) = m::frame(&s.ui, &view, mouse, sw, sh);
        f(prims, hits)
    })
}

fn mine(game: &Game<'_>, screen: &NativeScreenView<'_>) -> bool {
    screen.id == "mod" && open(game) && game.native_on(m::ID)
}

fn on_pause(game: &Game<'_>, screen: &NativeScreenView<'_>) -> bool {
    screen.id == "paused" && game.native_on(m::ID) && game.player.is_some() && settings(game).pause_button
}

/// `native_screen_render_post`: the screen, and the pause menu's Discover button.
pub(super) fn on_render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if on_pause(game, screen) {
        let r = m::pause_button();
        let hover = m::inside(r, screen.mouse);
        ui::button(batch, &game.gui.atlas, r[0], r[1], r[2], r[3], &Text::literal("Discover"), true, hover);
        return;
    }
    if !mine(game, screen) {
        return;
    }
    game.native_draw_menu_background(batch, [0.0, 0.0, batch.width, batch.height], true);
    let prims = with_frame(game, screen.mouse, |prims, _| prims);
    paint(game, batch, &prims);
}

pub(super) fn on_press(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    if on_pause(game, screen) && button == 0 && m::inside(m::pause_button(), screen.mouse) {
        *canceled = true;
        m::with(|s| s.open = true);
        return;
    }
    if !mine(game, screen) {
        return;
    }
    *canceled = true;
    if button != 0 {
        return;
    }
    let hit = with_frame(game, screen.mouse, |_, hits| m::hit(&hits, screen.mouse));
    m::with(|s| {
        s.ui.typing = matches!(hit, Some(m::Hit::Search)) || s.ui.typing && hit.is_none();
        if let Some(h) = hit {
            s.queue.push(h);
        }
    });
}

pub(super) fn on_release(game: &Game<'_>, screen: &NativeScreenView<'_>, canceled: &mut bool) {
    if mine(game, screen) {
        *canceled = true;
    }
}

pub(super) fn on_scroll(game: &Game<'_>, screen: &NativeScreenView<'_>, scroll: [f64; 2], canceled: &mut bool) {
    if !mine(game, screen) || scroll[1] == 0.0 {
        return;
    }
    *canceled = true;
    let (sw, sh) = game.gui_size();
    let l = m::layout(sw, sh);
    let up = scroll[1] > 0.0;
    let settings = settings(game);
    let width = |t: &str| game.native_text_width(t);
    let rows = m::with(|s| {
        let have = |id: &str| s.have.get(id).copied().unwrap_or_default();
        let view = m::View { entries: &s.entries, have: &have, width: &width, settings, tail: tail(s, settings) };
        (m::list_len(&s.ui, &view), m::categories(&s.entries, s.ui.tab).len() + 1)
    });
    m::with(|s| {
        if m::inside(l.cats, screen.mouse) {
            let fits = ((l.cats[3] / m::CAT_H).floor() as usize).max(1);
            let max = rows.1.saturating_sub(fits);
            s.ui.cat_scroll = if up { s.ui.cat_scroll.saturating_sub(2) } else { (s.ui.cat_scroll + 2).min(max) };
        } else {
            s.ui.wheel(up, rows.0, m::rows_fit(&l));
        }
    });
}

/// Keys on the screen: Backspace edits the search; the others stay out of the game (Escape
/// still closes the screen).
pub(super) fn on_screen_key(game: &Game<'_>, screen: &NativeScreenView<'_>, key: &str, canceled: &mut bool) {
    if !mine(game, screen) || key == "key.keyboard.escape" {
        return;
    }
    // The hotkey closes the screen through `native_key` (unless it is typed into the search).
    if keys::bound(game, "open") == key && !typing_into_search(key) {
        return;
    }
    *canceled = true;
    if key == "key.keyboard.backspace" {
        m::with(|s| s.ui.backspace());
    }
}

/// Key `key` is a character of the search: the search box has the keyboard and the key types.
fn typing_into_search(key: &str) -> bool {
    m::types_text(key) && m::with(|s| s.ui.typing)
}

pub(super) fn on_char(game: &Game<'_>, screen: &NativeScreenView<'_>, c: char, canceled: &mut bool) {
    if !mine(game, screen) {
        return;
    }
    *canceled = true;
    m::with(|s| {
        if s.ui.typing {
            s.ui.type_char(c);
        }
    });
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
    for (key, value) in s.stored() {
        game.native_set_setting(m::ID, key, &value);
    }
}
