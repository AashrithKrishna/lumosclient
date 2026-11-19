//! Instances other launchers keep, found on disk without changing anything there, and copied
//! into new Lumos instances. The formats, paths and decisions are in IMPORT-PLAN.md.
//!
//! Read-only on the source: files there are only ever opened for reading. Symlinks are followed
//! only when they lead back inside the other launcher's folder or the instance's game folder.
//! Everything is written into a brand-new instance folder, with files created `create_new`.

use crate::sqlite_ro;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Official,
    Prism,
    MultiMc,
    Modrinth,
    CurseForge,
    SkLauncher,
    AtLauncher,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Official => "Minecraft Launcher",
            Source::Prism => "Prism Launcher",
            Source::MultiMc => "MultiMC",
            Source::Modrinth => "Modrinth App",
            Source::CurseForge => "CurseForge",
            Source::SkLauncher => "SKLauncher",
            Source::AtLauncher => "ATLauncher",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loader {
    Vanilla,
    Fabric,
    Forge,
    NeoForge,
    Quilt,
    OptiFine,
    LiteLoader,
    Other(String),
}

impl Loader {
    fn name(&self) -> &str {
        match self {
            Loader::Vanilla => "Vanilla",
            Loader::Fabric => "Fabric",
            Loader::Forge => "Forge",
            Loader::NeoForge => "NeoForge",
            Loader::Quilt => "Quilt",
            Loader::OptiFine => "OptiFine",
            Loader::LiteLoader => "LiteLoader",
            Loader::Other(s) => s,
        }
    }

    /// A loader as launchers name it (`fabric`, `Forge`, `neoforge`, `quilt`, `vanilla`…).
    fn parse(s: &str) -> Loader {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "vanilla" | "minecraft" | "none" => Loader::Vanilla,
            "fabric" | "fabric-loader" => Loader::Fabric,
            "forge" => Loader::Forge,
            "neoforge" | "neoforged" => Loader::NeoForge,
            "quilt" | "quilt-loader" => Loader::Quilt,
            "optifine" => Loader::OptiFine,
            "liteloader" => Loader::LiteLoader,
            _ => Loader::Other(s.trim().chars().take(24).collect()),
        }
    }
}

/// Minecraft version tokens some launchers use instead of an id.
const LATEST_RELEASE: &str = "latest-release";
const LATEST_SNAPSHOT: &str = "latest-snapshot";

/// One instance found in another launcher.
#[derive(Clone, Debug)]
pub struct Found {
    pub source: Source,
    pub name: String,
    /// Minecraft version id, [`LATEST_RELEASE`] / [`LATEST_SNAPSHOT`], or unknown.
    pub mc: Option<String>,
    pub loader: Loader,
    pub loader_version: Option<String>,
    /// The folder with saves/, mods/, options.txt…
    pub game_dir: PathBuf,
    /// Folders symlinks may lead into: the other launcher's own folder(s) and the game folder.
    pub roots: Vec<PathBuf>,
    /// The game folder is the launcher's own folder (the official launcher's default).
    pub shared: bool,
}

/// What an import of a [`Found`] makes.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The Lumos version spec of the new instance; `None`: it can't be imported.
    pub target: Option<String>,
    pub horizon: bool,
    pub copy_mods: bool,
    /// The target isn't what the instance plays (a loader Lumos can't install): only worlds,
    /// packs and settings come over. Unticked in the list until the player ticks it.
    pub fallback: bool,
    pub notes: Vec<String>,
}

// ───────────────────────── Version mapping ─────────────────────────

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// The Lumos version a found instance becomes (IMPORT-PLAN.md, "Mapping to a Lumos version").
/// `latest_release` is the newest release in Lumos's version list, when it has loaded.
pub fn plan(f: &Found, mods: usize, latest_release: Option<&str>) -> Plan {
    let mut p = Plan { target: None, horizon: false, copy_mods: false, fallback: false, notes: Vec::new() };
    let mc = match f.mc.as_deref() {
        None => {
            p.notes.push(format!("{} doesn't say which Minecraft version this plays, so it can't be imported.", f.source.label()));
            return p;
        }
        Some(LATEST_SNAPSHOT) => {
            p.notes.push("Plays the latest snapshot. Lumos doesn't offer snapshots, and snapshot worlds can't be opened in a release.".into());
            return p;
        }
        Some(LATEST_RELEASE) => match latest_release {
            Some(v) => v.to_owned(),
            None => {
                p.notes.push("Plays the latest release, and Lumos's version list hasn't loaded yet. Try again in a moment.".into());
                return p;
            }
        },
        Some(v) => v.to_owned(),
    };
    if !crate::installer::safe_id(&mc) {
        p.notes.push("Its Minecraft version has a name Lumos can't use.".into());
        return p;
    }
    let mods_skipped = |why: &str| if mods > 0 { format!("{} skipped: {why}", plural(mods, "mod")) } else { String::new() };
    if mc == crate::HORIZON_MC {
        p.target = Some(crate::HORIZON_ID.to_owned());
        p.horizon = true;
        p.notes.push("Becomes a Horizon 26.3 instance: worlds, resource packs, shader packs and settings come over.".into());
        let s = mods_skipped("Java mods don't run in Horizon (it has its own mods in Discover)");
        if !s.is_empty() {
            p.notes.push(s + ".");
        }
        return p;
    }
    let lv = f.loader_version.as_deref().map(|v| format!(" {v}")).unwrap_or_default();
    let fallback = |p: &mut Plan, spec: String, why: String| {
        p.target = Some(spec.clone());
        p.fallback = true;
        let s = mods_skipped("they need that loader");
        p.notes.push(format!("{why} Only worlds, packs and settings can come over, into {spec}.{}", if s.is_empty() { String::new() } else { format!(" {s}.") }));
    };
    use crate::forge::{spec_supported, Kind};
    match &f.loader {
        Loader::Vanilla => p.target = Some(mc.clone()),
        Loader::Fabric if crate::installer::fabric_supports(&mc) => {
            p.target = Some(format!("{}{mc}", crate::installer::FABRIC_PREFIX));
            p.copy_mods = true;
        }
        Loader::Forge if spec_supported(Kind::Forge, &mc) => {
            p.target = Some(format!("{}{mc}", crate::forge::FORGE_PREFIX));
            p.copy_mods = true;
        }
        Loader::NeoForge if spec_supported(Kind::NeoForge, &mc) => {
            p.target = Some(format!("{}{mc}", crate::forge::NEOFORGE_PREFIX));
            p.copy_mods = true;
        }
        Loader::Fabric | Loader::Forge | Loader::NeoForge => {
            let why = format!("Lumos can't install {}{lv} for Minecraft {mc}.", f.loader.name());
            fallback(&mut p, mc.clone(), why);
        }
        Loader::Quilt if crate::installer::fabric_supports(&mc) => {
            fallback(&mut p, format!("{}{mc}", crate::installer::FABRIC_PREFIX), "Lumos can't install Quilt.".into());
            if mods > 0 {
                p.notes.push("Many Quilt mods have Fabric versions in Discover.".into());
            }
        }
        other => {
            let why = format!("Lumos can't install {}.", other.name());
            fallback(&mut p, mc.clone(), why);
        }
    }
    if p.copy_mods && mods > 0 {
        p.notes.push(format!("Lumos sets up its own {} build for {mc}{}, so check the mods start.", f.loader.name(), if lv.is_empty() { String::new() } else { format!(" (this instance used{lv})") }));
    }
    p
}

// ───────────────────────── Finding instances ─────────────────────────

/// Where to look: the system folders (tests use temp folders).
pub struct Roots {
    /// `~/Library/Application Support` (macOS), `%APPDATA%` (Windows).
    pub data: PathBuf,
    pub home: PathBuf,
    pub documents: Option<PathBuf>,
    pub windows: bool,
}

impl Roots {
    pub fn system() -> Option<Roots> {
        Some(Roots { data: dirs::data_dir()?, home: dirs::home_dir()?, documents: dirs::document_dir(), windows: cfg!(windows) })
    }

    fn official(&self) -> PathBuf {
        self.data.join(if self.windows { ".minecraft" } else { "minecraft" })
    }

    /// Each launcher and the folders it may keep its instances in.
    pub fn places(&self) -> Vec<(Source, PathBuf)> {
        let mut v = vec![
            (Source::Official, self.official()),
            (Source::Prism, self.data.join("PrismLauncher")),
            (Source::Modrinth, self.data.join("ModrinthApp")),
            (Source::Modrinth, self.data.join("com.modrinth.theseus")),
            (Source::SkLauncher, self.data.join("sklauncher")),
            (Source::SkLauncher, self.official().join("sklauncher")),
            (Source::AtLauncher, self.data.join("ATLauncher")),
        ];
        if self.windows {
            v.push((Source::CurseForge, self.home.join("curseforge").join("minecraft")));
        } else {
            v.push((Source::MultiMc, PathBuf::from("/Applications/MultiMC.app/Data")));
            v.push((Source::MultiMc, self.home.join("Applications/MultiMC.app/Data")));
            v.push((Source::MultiMc, self.data.join("MultiMC")));
            let docs = self.documents.clone().unwrap_or_else(|| self.home.join("Documents"));
            v.push((Source::CurseForge, docs.join("curseforge").join("minecraft")));
        }
        v
    }
}

/// Everything found: instances, and the launcher folders that exist.
pub struct Scan {
    pub found: Vec<Found>,
    pub launchers: Vec<(Source, PathBuf)>,
}

pub fn scan(roots: &Roots) -> Scan {
    let mut out = Scan { found: Vec::new(), launchers: Vec::new() };
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for (source, dir) in roots.places() {
        if !dir.is_dir() || !seen.insert(fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
            continue;
        }
        let found = match source {
            Source::Official => official(&dir),
            Source::Prism | Source::MultiMc => prism(&dir, source),
            Source::Modrinth => modrinth(&dir),
            Source::CurseForge => curseforge(&dir),
            Source::SkLauncher => sklauncher(&dir),
            Source::AtLauncher => atlauncher(&dir),
        };
        // SKLauncher's legacy folder inside .minecraft only counts when it has instances.
        if source == Source::SkLauncher && found.is_empty() && !dir.join("instances.json").is_file() {
            continue;
        }
        out.launchers.push((source, dir));
        out.found.extend(found.into_iter().filter(|f| has_content(&f.game_dir)));
    }
    out
}

fn read_json(path: &Path) -> Option<Value> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 16 << 20 {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// One plain path component (a folder name taken from metadata).
fn plain_name(s: &str) -> bool {
    let p = Path::new(s);
    !s.is_empty() && p.components().count() == 1 && matches!(p.components().next(), Some(Component::Normal(_)))
}

fn folder_name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

// ── Official launcher ──

/// The Minecraft version and loader of an official-launcher version id.
fn official_version(root: &Path, id: &str) -> (Option<String>, Loader, Option<String>) {
    let json = if plain_name(id) { read_json(&root.join("versions").join(id).join(format!("{id}.json"))) } else { None };
    let lower = id.to_ascii_lowercase();
    let libs: Vec<String> = json
        .as_ref()
        .and_then(|j| j.get("libraries"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|l| str_of(l, "name").map(str::to_owned)).collect())
        .unwrap_or_default();
    let lib = |prefix: &str| libs.iter().find_map(|n| n.strip_prefix(prefix).map(|v| v.split(':').next().unwrap_or("").to_owned()));
    let inherits = json.as_ref().and_then(|j| str_of(j, "inheritsFrom")).map(str::to_owned);
    // Loader from the libraries first (TLauncher-style ids say little), then the id.
    let (loader, lv) = if let Some(v) = lib("net.fabricmc:fabric-loader:") {
        (Loader::Fabric, Some(v))
    } else if let Some(v) = lib("org.quiltmc:quilt-loader:") {
        (Loader::Quilt, Some(v))
    } else if let Some(v) = lib("net.neoforged:neoforge:").or_else(|| lib("net.neoforged.fancymodloader:loader:").map(|_| String::new())) {
        (Loader::NeoForge, Some(v).filter(|v| !v.is_empty()))
    } else if let Some(v) = lib("net.minecraftforge:forge:").or_else(|| lib("net.minecraftforge:fmlloader:")) {
        (Loader::Forge, Some(v))
    } else if let Some(v) = lib("optifine:OptiFine:") {
        (Loader::OptiFine, Some(v))
    } else if let Some(rest) = lower.strip_prefix("fabric-loader-") {
        (Loader::Fabric, rest.split('-').next().map(str::to_owned))
    } else if let Some(rest) = lower.strip_prefix("quilt-loader-") {
        (Loader::Quilt, rest.split('-').next().map(str::to_owned))
    } else if let Some(rest) = lower.strip_prefix("neoforge-") {
        (Loader::NeoForge, Some(rest.to_owned()))
    } else if let Some((_, v)) = lower.split_once("-forge-").or_else(|| lower.split_once("-forge")) {
        (Loader::Forge, Some(v.to_owned()).filter(|v| !v.is_empty()))
    } else if lower.contains("optifine") {
        (Loader::OptiFine, None)
    } else if lower.contains("forge") {
        (Loader::Forge, None)
    } else if lower.contains("fabric") {
        (Loader::Fabric, None)
    } else {
        (Loader::Vanilla, None)
    };
    let mc = match (&loader, inherits) {
        (_, Some(parent)) => Some(parent),
        (Loader::Vanilla, None) => Some(id.to_owned()),
        // Fabric/Quilt ids end in the game version: fabric-loader-0.16.9-1.21.1.
        (Loader::Fabric | Loader::Quilt, None) if lower.contains("-loader-") => id.rsplit('-').next().map(str::to_owned),
        // 1.20.1-forge-47.3.0
        (Loader::Forge, None) if lower.contains("-forge") => id.split('-').next().map(str::to_owned),
        _ => None,
    };
    (mc, loader, lv)
}

pub fn official(root: &Path) -> Vec<Found> {
    let Some(j) = read_json(&root.join("launcher_profiles.json")) else { return Vec::new() };
    let Some(profiles) = j.get("profiles").and_then(Value::as_object) else { return Vec::new() };
    let mut out = Vec::new();
    for p in profiles.values() {
        let kind = str_of(p, "type").unwrap_or("custom");
        let (mc, loader, lv) = match kind {
            "latest-release" => (Some(LATEST_RELEASE.to_owned()), Loader::Vanilla, None),
            "latest-snapshot" => (Some(LATEST_SNAPSHOT.to_owned()), Loader::Vanilla, None),
            _ => match str_of(p, "lastVersionId") {
                Some(id) => official_version(root, id),
                None => continue,
            },
        };
        let name = match (str_of(p, "name"), kind) {
            (Some(n), _) => n.to_owned(),
            (None, "latest-release") => "Latest release".into(),
            (None, "latest-snapshot") => "Latest snapshot".into(),
            (None, _) => str_of(p, "lastVersionId").unwrap_or("Profile").to_owned(),
        };
        // A profile whose own folder is gone (or relative, which the launcher reads against
        // its working folder) isn't guessed at: it is left out.
        let game = match str_of(p, "gameDir").map(PathBuf::from) {
            Some(g) if g.is_absolute() && g.is_dir() => Some(g),
            Some(_) => continue,
            None => None,
        };
        let shared = game.as_ref().map_or(true, |g| fs::canonicalize(g).ok() == fs::canonicalize(root).ok());
        let game_dir = game.unwrap_or_else(|| root.to_path_buf());
        out.push(Found { source: Source::Official, name, mc, loader, loader_version: lv, roots: vec![root.to_path_buf(), game_dir.clone()], game_dir, shared });
    }
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

// ── Prism Launcher / MultiMC ──

/// `key=value` of an INI file, outside any section or in `[General]`; Prism quotes values with
/// special characters.
fn ini_value(text: &str, key: &str) -> Option<String> {
    let mut section = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = s.to_owned();
            continue;
        }
        if !(section.is_empty() || section == "General") {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                let v = match v.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
                    Some(q) => q.replace("\\\"", "\"").replace("\\\\", "\\"),
                    None => v.to_owned(),
                };
                return Some(v);
            }
        }
    }
    None
}

pub fn prism_instance(dir: &Path, source: Source, root: &Path) -> Option<Found> {
    let pack = read_json(&dir.join("mmc-pack.json"))?;
    let comps: Vec<&Value> = pack.get("components")?.as_array()?.iter().filter(|c| !c.get("disabled").and_then(Value::as_bool).unwrap_or(false)).collect();
    let version = |c: &Value| str_of(c, "version").or_else(|| str_of(c, "cachedVersion")).map(str::to_owned);
    let mc = comps.iter().find(|c| str_of(c, "uid") == Some("net.minecraft")).and_then(|c| version(c));
    let mut loader = (Loader::Vanilla, None);
    for c in &comps {
        let l = match str_of(c, "uid") {
            Some("net.fabricmc.fabric-loader") => Loader::Fabric,
            Some("org.quiltmc.quilt-loader") => Loader::Quilt,
            Some("net.minecraftforge") => Loader::Forge,
            Some("net.neoforged") => Loader::NeoForge,
            Some("com.mumfrey.liteloader") => Loader::LiteLoader,
            _ => continue,
        };
        loader = (l, version(c));
        break;
    }
    let (mc_dir, dot) = (dir.join("minecraft"), dir.join(".minecraft"));
    let game_dir = if dot.is_dir() && !mc_dir.exists() { dot } else { mc_dir };
    let name = fs::read_to_string(dir.join("instance.cfg")).ok().and_then(|t| ini_value(&t, "name")).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| folder_name(dir));
    Some(Found { source, name, mc, loader: loader.0, loader_version: loader.1, roots: vec![root.to_path_buf(), dir.to_path_buf()], game_dir, shared: false })
}

pub fn prism(root: &Path, source: Source) -> Vec<Found> {
    let cfg = ["prismlauncher.cfg", "multimc.cfg"].iter().find_map(|c| fs::read_to_string(root.join(c)).ok());
    let inst = cfg.and_then(|t| ini_value(&t, "InstanceDir")).filter(|s| !s.is_empty()).unwrap_or_else(|| "instances".into());
    let inst = if Path::new(&inst).is_absolute() { PathBuf::from(inst) } else { root.join(inst) };
    let mut out: Vec<Found> = subdirs(&inst).iter().filter_map(|d| prism_instance(d, source, root)).collect();
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

// ── Modrinth App ──

/// (name, game version, loader, loader version) per profile folder name, from `app.db`.
fn modrinth_db(root: &Path) -> (Option<PathBuf>, HashMap<String, (String, String, String, Option<String>)>) {
    let mut out = HashMap::new();
    let Ok(db) = sqlite_ro::Db::open(&root.join("app.db")) else { return (None, out) };
    let text = |m: &HashMap<String, sqlite_ro::Value>, k: &str| m.get(k).and_then(|v| v.as_str()).map(str::to_owned);
    let custom = db.table("settings").ok().and_then(|rows| rows.first().and_then(|r| text(r, "custom_dir"))).map(PathBuf::from).filter(|p| p.is_absolute() && p.is_dir());
    if db.has_table("instances") && db.has_table("instance_content_sets") {
        let sets: HashMap<String, HashMap<String, sqlite_ro::Value>> =
            db.table("instance_content_sets").unwrap_or_default().into_iter().filter_map(|r| Some((text(&r, "id")?, r))).collect();
        for r in db.table("instances").unwrap_or_default() {
            let (Some(path), Some(name)) = (text(&r, "path"), text(&r, "name")) else { continue };
            let Some(set) = text(&r, "applied_content_set_id").and_then(|id| sets.get(&id)) else { continue };
            let (Some(gv), Some(l)) = (text(set, "game_version"), text(set, "loader")) else { continue };
            out.insert(path, (name, gv, l, text(set, "loader_version")));
        }
    } else if db.has_table("profiles") {
        for r in db.table("profiles").unwrap_or_default() {
            let (Some(path), Some(name), Some(gv), Some(l)) = (text(&r, "path"), text(&r, "name"), text(&r, "game_version"), text(&r, "mod_loader")) else { continue };
            out.insert(path, (name, gv, l, text(&r, "mod_loader_version")));
        }
    }
    (custom, out)
}

pub fn modrinth(root: &Path) -> Vec<Found> {
    let (custom, rows) = modrinth_db(root);
    let config = custom.clone().unwrap_or_else(|| root.to_path_buf());
    let mut roots = vec![root.to_path_buf()];
    if let Some(c) = custom {
        roots.push(c);
    }
    let mut out = Vec::new();
    for dir in subdirs(&config.join("profiles")) {
        let folder = folder_name(&dir);
        let (name, mc, loader, lv) = if let Some((n, gv, l, lv)) = rows.get(&folder) {
            (n.clone(), Some(gv.clone()), Loader::parse(l), lv.clone())
        } else if let Some(meta) = read_json(&dir.join("profile.json")).and_then(|j| j.get("metadata").cloned()) {
            let lv = meta.get("loader_version").and_then(|v| str_of(v, "id")).map(str::to_owned);
            (str_of(&meta, "name").unwrap_or(&folder).to_owned(), str_of(&meta, "game_version").map(str::to_owned), Loader::parse(str_of(&meta, "loader").unwrap_or("vanilla")), lv)
        } else {
            (folder.clone(), None, Loader::Vanilla, None)
        };
        let mut r = roots.clone();
        r.push(dir.clone());
        out.push(Found { source: Source::Modrinth, name, mc, loader, loader_version: lv.filter(|v| !v.is_empty()), roots: r, game_dir: dir, shared: false });
    }
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

// ── CurseForge ──

pub fn curseforge_instance(dir: &Path, root: &Path) -> Option<Found> {
    let (mc, id, name) = if let Some(j) = read_json(&dir.join("minecraftinstance.json")) {
        let id = j.get("baseModLoader").and_then(|b| str_of(b, "name")).unwrap_or("").to_owned();
        (str_of(&j, "gameVersion").map(str::to_owned), id, str_of(&j, "name").map(str::to_owned))
    } else {
        let j = read_json(&dir.join("manifest.json"))?;
        let m = j.get("minecraft")?;
        let loaders: Vec<&Value> = m.get("modLoaders").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
        let primary = loaders.iter().find(|l| l.get("primary").and_then(Value::as_bool) == Some(true)).or(loaders.first());
        (str_of(m, "version").map(str::to_owned), primary.and_then(|l| str_of(l, "id")).unwrap_or("").to_owned(), str_of(&j, "name").map(str::to_owned))
    };
    let (kind, rest) = id.split_once('-').unwrap_or((id.as_str(), ""));
    let loader = if id.is_empty() { Loader::Vanilla } else { Loader::parse(kind) };
    // Fabric and Quilt ids end in the Minecraft version: fabric-0.16.14-1.21.5.
    let lv = mc.as_deref().and_then(|m| rest.strip_suffix(m).and_then(|r| r.strip_suffix('-'))).unwrap_or(rest);
    Some(Found {
        source: Source::CurseForge,
        name: name.unwrap_or_else(|| folder_name(dir)),
        mc,
        loader,
        loader_version: Some(lv.to_owned()).filter(|v| !v.is_empty()),
        roots: vec![root.to_path_buf(), dir.to_path_buf()],
        game_dir: dir.to_path_buf(),
        shared: false,
    })
}

pub fn curseforge(root: &Path) -> Vec<Found> {
    let mut out: Vec<Found> = subdirs(&root.join("Instances")).iter().filter_map(|d| curseforge_instance(d, root)).collect();
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

// ── SKLauncher ──

pub fn sklauncher(root: &Path) -> Vec<Found> {
    let Some(j) = read_json(&root.join("instances.json")) else { return Vec::new() };
    let Some(list) = j.get("instances").and_then(Value::as_array) else { return Vec::new() };
    let mut out = Vec::new();
    for i in list {
        let Some(id) = str_of(i, "id") else { continue };
        let dir = str_of(i, "directory").map(PathBuf::from).filter(|d| d.is_absolute() && d.is_dir());
        let game_dir = match dir {
            Some(d) => d,
            None if plain_name(id) => root.join("instances").join(id),
            None => continue,
        };
        let mc = str_of(i, "minecraftVersion").map(str::to_owned);
        out.push(Found {
            source: Source::SkLauncher,
            name: str_of(i, "name").unwrap_or(id).to_owned(),
            mc,
            loader: Loader::parse(str_of(i, "gameType").unwrap_or("vanilla")),
            loader_version: str_of(i, "loaderVersion").map(str::to_owned),
            roots: vec![root.to_path_buf(), game_dir.clone()],
            game_dir,
            shared: false,
        });
    }
    out
}

// ── ATLauncher ──

pub fn atlauncher(root: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    for dir in subdirs(&root.join("instances")) {
        let Some(j) = read_json(&dir.join("instance.json")) else { continue };
        let Some(l) = j.get("launcher").filter(|l| l.is_object()) else { continue };
        let lv = l.get("loaderVersion");
        out.push(Found {
            source: Source::AtLauncher,
            name: str_of(l, "name").map(str::to_owned).unwrap_or_else(|| folder_name(&dir)),
            mc: str_of(&j, "id").map(str::to_owned),
            loader: lv.and_then(|v| str_of(v, "type")).map(Loader::parse).unwrap_or(Loader::Vanilla),
            loader_version: lv.and_then(|v| str_of(v, "version")).map(str::to_owned),
            roots: vec![root.to_path_buf(), dir.clone()],
            game_dir: dir,
            shared: false,
        });
    }
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

// ───────────────────────── Counting ─────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub mods: usize,
    pub worlds: usize,
    pub resourcepacks: usize,
    pub shaderpacks: usize,
}

fn is_mod_file(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".jar") || l.ends_with(".jar.disabled")
}

fn pack_entry(p: &Path) -> bool {
    let n = folder_name(p);
    !n.starts_with('.') && (p.is_dir() || n.to_ascii_lowercase().ends_with(".zip"))
}

pub fn counts(game: &Path) -> Counts {
    let list = |d: &str| fs::read_dir(game.join(d)).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    Counts {
        mods: list("mods").iter().filter(|p| p.is_file() && is_mod_file(&folder_name(p))).count(),
        worlds: list("saves").iter().filter(|p| p.join("level.dat").is_file()).count(),
        resourcepacks: list("resourcepacks").iter().filter(|p| pack_entry(p)).count(),
        shaderpacks: list("shaderpacks").iter().filter(|p| pack_entry(p)).count(),
    }
}

/// Whether an instance has anything an import would bring over.
fn has_content(game: &Path) -> bool {
    let c = counts(game);
    c != Counts::default() || ["options.txt", "servers.dat"].iter().any(|f| game.join(f).is_file())
}

// ───────────────────────── Copying ─────────────────────────

/// One file or folder to create in the new instance.
#[derive(Debug)]
pub struct Item {
    src: PathBuf,
    rel: PathBuf,
    dir: bool,
}

#[derive(Debug, Default)]
pub struct Collected {
    pub items: Vec<Item>,
    pub bytes: u64,
    /// Links that lead outside the other launcher's folders, not followed.
    pub outside_links: usize,
}

/// What of `f`'s game folder an import with `plan` copies (IMPORT-PLAN.md, "What is copied").
pub fn collect(f: &Found, plan: &Plan) -> Collected {
    let roots: Vec<PathBuf> = f.roots.iter().chain(std::iter::once(&f.game_dir)).filter_map(|r| fs::canonicalize(r).ok()).collect();
    let mut c = Collected::default();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut tops: Vec<&str> = vec!["saves", "resourcepacks", "shaderpacks", "screenshots", "options.txt", "servers.dat"];
    if !plan.horizon {
        tops.push("schematics");
        if plan.copy_mods {
            tops.push("config");
        }
    }
    for top in tops {
        walk(&f.game_dir.join(top), PathBuf::from(top), &roots, &mut visited, &mut c, 0, None);
    }
    if plan.copy_mods {
        walk(&f.game_dir.join("mods"), PathBuf::from("mods"), &roots, &mut visited, &mut c, 0, Some(is_mod_file));
    }
    c
}

/// Where `p` really is, if it may be read: itself when it is no link, else its target when that
/// lies inside one of `roots`. `None` when missing; `Some(None)` for a link leading outside.
fn resolve(p: &Path, roots: &[PathBuf]) -> Option<Option<(PathBuf, fs::Metadata)>> {
    let lm = fs::symlink_metadata(p).ok()?;
    if !lm.file_type().is_symlink() {
        return Some(Some((p.to_path_buf(), lm)));
    }
    let target = match fs::canonicalize(p) {
        Ok(t) => t,
        Err(_) => return Some(None),
    };
    if !roots.iter().any(|r| target.starts_with(r)) {
        return Some(None);
    }
    let m = fs::metadata(&target).ok()?;
    Some(Some((target, m)))
}

fn walk(src: &Path, rel: PathBuf, roots: &[PathBuf], visited: &mut HashSet<PathBuf>, c: &mut Collected, depth: usize, files_only: Option<fn(&str) -> bool>) {
    let Some(r) = resolve(src, roots) else { return };
    let Some((real, meta)) = r else {
        c.outside_links += 1;
        return;
    };
    if meta.is_dir() {
        if depth > 64 || !visited.insert(fs::canonicalize(&real).unwrap_or_else(|_| real.clone())) {
            return;
        }
        c.items.push(Item { src: real.clone(), rel: rel.clone(), dir: true });
        let mut entries: Vec<_> = fs::read_dir(&real).into_iter().flatten().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name();
            let n = name.to_string_lossy();
            if n == ".DS_Store" {
                continue;
            }
            if let Some(ok) = files_only {
                // Only top-level files that pass (mods/*.jar); no subfolders.
                match resolve(&e.path(), roots) {
                    Some(None) => {
                        if ok(&n) {
                            c.outside_links += 1;
                        }
                        continue;
                    }
                    Some(Some((_, m))) if m.is_file() && ok(&n) => {}
                    _ => continue,
                }
            }
            walk(&e.path(), rel.join(&name), roots, visited, c, depth + 1, None);
        }
    } else if meta.is_file() {
        c.bytes += meta.len();
        c.items.push(Item { src: real, rel, dir: false });
    }
}

/// Stops a running copy (between chunks).
pub static CANCEL: AtomicBool = AtomicBool::new(false);

/// Copies `items` into the new folder `dest`, which must not exist yet. `progress` gets the
/// bytes copied so far. On any error (or cancel) the caller removes `dest`.
pub fn copy_into(items: &[Item], dest: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64)) -> Result<(), String> {
    fs::create_dir(dest).map_err(|e| format!("Couldn't create {}: {e}", dest.display()))?;
    let mut done: u64 = 0;
    let mut buf = vec![0u8; 1 << 20];
    for it in items {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        // Every path is `dest` plus plain names read from the source folder.
        if !it.rel.components().all(|c| matches!(c, Component::Normal(_))) {
            continue;
        }
        let to = dest.join(&it.rel);
        if it.dir {
            fs::create_dir_all(&to).map_err(|e| format!("Couldn't create {}: {e}", to.display()))?;
            continue;
        }
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
        }
        let mut src = fs::File::open(&it.src).map_err(|e| format!("Couldn't read {}: {e}", it.src.display()))?;
        let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&to).map_err(|e| format!("Couldn't write {}: {e}", to.display()))?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            let n = src.read(&mut buf).map_err(|e| format!("Couldn't read {}: {e}", it.src.display()))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| format!("Couldn't write {}: {e}", to.display()))?;
            done += n as u64;
            progress(done);
        }
        // Keep the file's time (worlds list by last played, packs by date).
        if let Ok(t) = src.metadata().and_then(|m| m.modified()) {
            let _ = out.set_modified(t);
        }
    }
    Ok(())
}

// ───────────────────────── The list the UI shows ─────────────────────────

/// The last scan, so imports name instances by key and never by a path from the page.
static LAST: Mutex<(u64, Vec<Found>)> = Mutex::new((0, Vec::new()));

fn loader_label(f: &Found) -> String {
    match (&f.loader, &f.loader_version) {
        (Loader::Vanilla, _) => "Vanilla".into(),
        (l, Some(v)) => format!("{} {v}", l.name()),
        (l, None) => l.name().to_owned(),
    }
}

fn target_label(t: &str) -> String {
    if t == crate::HORIZON_ID {
        "Horizon 26.3".into()
    } else {
        t.to_owned()
    }
}

/// Scans and remembers the result: the JSON for the page's `onImportScan`.
pub fn scan_json(roots: &Roots, latest_release: Option<&str>) -> Value {
    let s = scan(roots);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    last.0 += 1;
    let generation = last.0;
    let items: Vec<Value> = s
        .found
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let c = counts(&f.game_dir);
            let p = plan(f, c.mods, latest_release);
            let mut notes = p.notes.clone();
            if f.shared {
                notes.push(format!("Uses {}'s main folder: only its worlds, packs and settings are copied.", f.source.label()));
            }
            json!({
                "key": format!("{generation}:{i}"),
                "source": f.source.label(),
                "name": f.name,
                "mc": f.mc.as_deref().map(|m| match m { LATEST_RELEASE => "Latest release", LATEST_SNAPSHOT => "Latest snapshot", v => v }),
                "loader": loader_label(f),
                "target": p.target.as_deref(),
                "targetLabel": p.target.as_deref().map(target_label),
                "horizon": p.horizon,
                "copyMods": p.copy_mods,
                "fallback": p.fallback,
                "mods": c.mods,
                "worlds": c.worlds,
                "resourcepacks": c.resourcepacks,
                "shaderpacks": c.shaderpacks,
                "notes": notes,
                "path": f.game_dir.to_string_lossy(),
            })
        })
        .collect();
    let launchers: Vec<Value> = s.launchers.iter().map(|(src, d)| json!({"name": src.label(), "path": d.to_string_lossy()})).collect();
    last.1 = s.found;
    json!({"items": items, "launchers": launchers})
}

/// The found instances the keys name (from the latest scan only).
pub fn take(keys: &[String]) -> Vec<Found> {
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = Vec::new();
    for k in keys {
        let Some((g, i)) = k.split_once(':') else { continue };
        if g.parse::<u64>().ok() != Some(last.0) {
            continue;
        }
        if let Some(f) = i.parse::<usize>().ok().and_then(|i| last.1.get(i)) {
            out.push(f.clone());
        }
    }
    out
}

/// What one finished import brought over, for the summary.
pub fn summary(c: Counts, plan: &Plan, outside_links: usize) -> (Vec<String>, Vec<String>) {
    let mut copied = Vec::new();
    let mut skipped = Vec::new();
    if c.worlds > 0 {
        copied.push(plural(c.worlds, "world"));
    }
    if c.resourcepacks > 0 {
        copied.push(plural(c.resourcepacks, "resource pack"));
    }
    if c.shaderpacks > 0 {
        copied.push(plural(c.shaderpacks, "shader pack"));
    }
    if plan.copy_mods && c.mods > 0 {
        copied.push(plural(c.mods, "mod"));
    }
    if !plan.copy_mods && c.mods > 0 {
        skipped.push(format!(
            "{} ({})",
            plural(c.mods, "mod"),
            if plan.horizon { "Java mods don't run in Horizon" } else { "they need a loader Lumos can't install here" }
        ));
    }
    if outside_links > 0 {
        skipped.push(format!("{} that point outside the other launcher's folder", plural(outside_links, "link")));
    }
    (copied, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(name: &str) -> Tmp {
        let d = std::env::temp_dir().join(format!("lumos-import-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }
    fn write(p: &Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }
    /// A small game folder: 2 mods (one off), 1 world, packs, settings, junk.
    fn game(dir: &Path) {
        write(&dir.join("mods/a.jar"), "A");
        write(&dir.join("mods/b.jar.disabled"), "B");
        write(&dir.join("mods/readme.txt"), "x");
        write(&dir.join("mods/sub/c.jar"), "C");
        write(&dir.join("saves/World/level.dat"), "lvl");
        write(&dir.join("saves/World/region/r.0.0.mca"), &"r".repeat(5000));
        write(&dir.join("saves/notaworld/x.txt"), "x");
        write(&dir.join("resourcepacks/P.zip"), "p");
        write(&dir.join("resourcepacks/Folder Pack/pack.mcmeta"), "{}");
        write(&dir.join("shaderpacks/S.zip"), "s");
        write(&dir.join("shaderpacks/S.zip.txt"), "settings");
        write(&dir.join("config/a.json"), "{}");
        write(&dir.join("schematics/house.litematic"), "h");
        write(&dir.join("screenshots/1.png"), "png");
        write(&dir.join("options.txt"), "fov:1\n");
        write(&dir.join("servers.dat"), "srv");
        write(&dir.join("logs/latest.log"), "log");
    }
    fn found(dir: &Path, loader: Loader, mc: &str) -> Found {
        Found { source: Source::Prism, name: "T".into(), mc: Some(mc.into()), loader, loader_version: None, game_dir: dir.to_path_buf(), roots: vec![dir.to_path_buf()], shared: false }
    }

    #[test]
    fn mapping() {
        let d = PathBuf::from("/x");
        let p = |l: Loader, mc: &str| plan(&found(&d, l, mc), 3, Some("1.21.11"));
        let fab = p(Loader::Fabric, "1.21.11");
        assert_eq!((fab.target.as_deref(), fab.copy_mods, fab.fallback), (Some("Fabric 1.21.11"), true, false));
        let hz = p(Loader::Fabric, "26.3");
        assert_eq!((hz.target.as_deref(), hz.horizon, hz.copy_mods), (Some(crate::HORIZON_ID), true, false));
        assert!(hz.notes.iter().any(|n| n.contains("3 mods skipped")));
        let v = p(Loader::Vanilla, "1.8.9");
        assert_eq!((v.target.as_deref(), v.fallback), (Some("1.8.9"), false));
        let f189 = p(Loader::Forge, "1.8.9");
        assert_eq!((f189.target.as_deref(), f189.copy_mods, f189.fallback), (Some("1.8.9"), false, true));
        assert_eq!(p(Loader::Forge, "1.20.1").target.as_deref(), Some("Forge 1.20.1"));
        assert_eq!(p(Loader::NeoForge, "1.21.1").target.as_deref(), Some("NeoForge 1.21.1"));
        assert_eq!(p(Loader::NeoForge, "1.20.1").target.as_deref(), Some("1.20.1"));
        let q = p(Loader::Quilt, "1.20.1");
        assert_eq!((q.target.as_deref(), q.copy_mods, q.fallback), (Some("Fabric 1.20.1"), false, true));
        let o = p(Loader::OptiFine, "1.21.4");
        assert_eq!((o.target.as_deref(), o.fallback), (Some("1.21.4"), true));
        assert_eq!(p(Loader::Fabric, "1.12.2").target.as_deref(), Some("1.12.2"));
        assert_eq!(p(Loader::Vanilla, LATEST_RELEASE).target.as_deref(), Some("1.21.11"));
        assert_eq!(plan(&found(&d, Loader::Vanilla, LATEST_RELEASE), 0, None).target, None);
        assert_eq!(p(Loader::Vanilla, LATEST_SNAPSHOT).target, None);
        let mut unknown = found(&d, Loader::Vanilla, "x");
        unknown.mc = None;
        assert_eq!(plan(&unknown, 0, None).target, None);
        assert_eq!(p(Loader::Vanilla, "../evil").target, None);
    }

    #[test]
    fn official_profiles() {
        let t = tmp("official");
        let root = &t.0;
        game(root);
        write(&root.join("versions/fabric-loader-0.16.9-1.21.1/fabric-loader-0.16.9-1.21.1.json"), r#"{"inheritsFrom":"1.21.1","libraries":[{"name":"net.fabricmc:fabric-loader:0.16.9"}]}"#);
        write(&root.join("versions/Fabric 1.21.11/Fabric 1.21.11.json"), r#"{"inheritsFrom":"1.21.11","libraries":[{"name":"net.fabricmc:fabric-loader:0.18.1"},{"name":"org.ow2.asm:asm:9"}]}"#);
        write(&root.join("versions/1.20.1-forge-47.3.0/1.20.1-forge-47.3.0.json"), r#"{"inheritsFrom":"1.20.1","libraries":[{"name":"net.minecraftforge:forge:1.20.1-47.3.0"}]}"#);
        write(&root.join("versions/OptiFine 1.21.4/OptiFine 1.21.4.json"), r#"{"inheritsFrom":"1.21.4","libraries":[{"name":"optifine:OptiFine:1.21.4_HD_U_J3"}]}"#);
        let other = root.join("custom-dir");
        game(&other);
        let profiles = format!(
            r#"{{"profiles":{{
              "a":{{"name":"","type":"latest-release"}},
              "b":{{"name":"","type":"latest-snapshot"}},
              "c":{{"name":"Fab","type":"custom","lastVersionId":"fabric-loader-0.16.9-1.21.1","gameDir":{dir:?}}},
              "d":{{"name":"TL","type":"custom","lastVersionId":"Fabric 1.21.11"}},
              "e":{{"name":"Forge","type":"custom","lastVersionId":"1.20.1-forge-47.3.0"}},
              "f":{{"name":"OF","type":"custom","lastVersionId":"OptiFine 1.21.4"}},
              "g":{{"name":"Old","type":"custom","lastVersionId":"1.8.9"}},
              "h":{{"name":"Bad","type":"custom"}},
              "i":{{"name":"Gone","type":"custom","lastVersionId":"1.21.1","gameDir":"/nowhere/at/all"}}
            }}}}"#,
            dir = other.to_string_lossy()
        );
        write(&root.join("launcher_profiles.json"), &profiles);
        let f = official(root);
        let by = |n: &str| f.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("{n}"));
        assert_eq!(f.len(), 7);
        assert_eq!(by("Latest release").mc.as_deref(), Some(LATEST_RELEASE));
        assert!(by("Latest release").shared);
        let fab = by("Fab");
        assert_eq!((fab.mc.as_deref(), &fab.loader, fab.loader_version.as_deref(), fab.shared), (Some("1.21.1"), &Loader::Fabric, Some("0.16.9"), false));
        assert_eq!(fab.game_dir, other);
        assert_eq!((by("TL").mc.as_deref(), &by("TL").loader), (Some("1.21.11"), &Loader::Fabric));
        assert_eq!((by("Forge").mc.as_deref(), &by("Forge").loader), (Some("1.20.1"), &Loader::Forge));
        assert_eq!((by("OF").mc.as_deref(), &by("OF").loader), (Some("1.21.4"), &Loader::OptiFine));
        assert_eq!((by("Old").mc.as_deref(), &by("Old").loader), (Some("1.8.9"), &Loader::Vanilla));
        // Ids without a JSON still say what they are.
        assert_eq!(official_version(root, "quilt-loader-0.27.1-1.21.1").0.as_deref(), Some("1.21.1"));
        assert_eq!(official_version(root, "1.12.2-forge-14.23.5.2860").0.as_deref(), Some("1.12.2"));
        assert_eq!(official_version(root, "neoforge-21.1.77").1, Loader::NeoForge);
    }

    #[test]
    fn prism_instances() {
        let t = tmp("prism");
        let root = &t.0;
        write(&root.join("prismlauncher.cfg"), "[General]\nInstanceDir=insts\n");
        let a = root.join("insts/a");
        write(&a.join("instance.cfg"), "[General]\nInstanceType=OneSix\nname=\"My, Pack\"\n");
        write(&a.join("mmc-pack.json"), r#"{"components":[{"uid":"net.minecraft","version":"1.21.1"},{"uid":"net.minecraftforge","version":"1","disabled":true},{"uid":"net.fabricmc.fabric-loader","version":"0.16.9"}],"formatVersion":1}"#);
        game(&a.join(".minecraft"));
        let b = root.join("insts/b");
        write(&b.join("instance.cfg"), "name=Quilty\n");
        write(&b.join("mmc-pack.json"), r#"{"components":[{"uid":"net.minecraft","cachedVersion":"1.20.1"},{"uid":"org.quiltmc.quilt-loader","version":"0.27"}]}"#);
        game(&b.join("minecraft"));
        game(&b.join(".minecraft"));
        write(&root.join("insts/notone/readme.txt"), "x");
        let f = prism(root, Source::Prism);
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].name.as_str(), f[0].mc.as_deref(), &f[0].loader), ("My, Pack", Some("1.21.1"), &Loader::Fabric));
        assert!(f[0].game_dir.ends_with(".minecraft"));
        assert_eq!((f[1].mc.as_deref(), &f[1].loader), (Some("1.20.1"), &Loader::Quilt));
        assert!(f[1].game_dir.ends_with("minecraft") && !f[1].game_dir.ends_with(".minecraft"));
    }

    #[test]
    fn curseforge_instances() {
        let t = tmp("cf");
        let root = &t.0;
        let a = root.join("Instances/All the Mods");
        write(&a.join("minecraftinstance.json"), "\u{feff}{\"name\":\"ATM\",\"gameVersion\":\"1.20.1\",\"baseModLoader\":{\"name\":\"forge-47.2.0\",\"type\":1}}");
        game(&a);
        let b = root.join("Instances/Fab");
        write(&b.join("minecraftinstance.json"), r#"{"name":"Fab","gameVersion":"1.21.5","baseModLoader":{"name":"fabric-0.16.14-1.21.5"}}"#);
        let c = root.join("Instances/Vanilla");
        write(&c.join("minecraftinstance.json"), r#"{"name":"Van","gameVersion":"1.21.4","baseModLoader":null}"#);
        let d = root.join("Instances/Manifest");
        write(&d.join("manifest.json"), r#"{"name":"Man","minecraft":{"version":"1.21.1","modLoaders":[{"id":"neoforge-21.1.77","primary":true}]}}"#);
        let f = curseforge(root);
        let by = |n: &str| f.iter().find(|x| x.name == n).unwrap();
        assert_eq!((by("ATM").mc.as_deref(), &by("ATM").loader, by("ATM").loader_version.as_deref()), (Some("1.20.1"), &Loader::Forge, Some("47.2.0")));
        assert_eq!((&by("Fab").loader, by("Fab").loader_version.as_deref()), (&Loader::Fabric, Some("0.16.14")));
        assert_eq!(by("Van").loader, Loader::Vanilla);
        assert_eq!((by("Man").mc.as_deref(), &by("Man").loader), (Some("1.21.1"), &Loader::NeoForge));
    }

    #[test]
    fn sklauncher_instances() {
        let t = tmp("sk");
        let root = &t.0;
        let inst = root.join("instances/26-3");
        game(&inst);
        fs::create_dir_all(root.join("instances/latest-release")).unwrap();
        // The shape of the real instances.json on the developer's Mac.
        let j = json!({"instances": [
            {"id":"latest-release","name":"Latest Release","type":"latest-release","gameType":"vanilla","minecraftVersion":"latest-release","directory": root.join("instances/latest-release")},
            {"id":"26-3","name":"26.3","type":"custom","versionId":"fabric-loader-0.20.1-26.3","gameType":"fabric","minecraftVersion":"26.3","loaderVersion":"0.20.1","directory": inst},
            {"id":"forge-1-8-9","name":"Forge 1.8.9","type":"custom","gameType":"forge","minecraftVersion":"1.8.9","loaderVersion":"11.15.1.2318","directory": "/nowhere/forge-1-8-9"},
            {"id":"../x","name":"Bad","gameType":"vanilla","minecraftVersion":"1.21"}
        ]});
        write(&root.join("instances.json"), &j.to_string());
        let f = sklauncher(root);
        assert_eq!(f.len(), 3);
        assert_eq!((f[1].mc.as_deref(), &f[1].loader, f[1].loader_version.as_deref()), (Some("26.3"), &Loader::Fabric, Some("0.20.1")));
        // A directory that isn't there falls back to instances/<id>.
        assert_eq!(f[2].game_dir, root.join("instances/forge-1-8-9"));
        let p = plan(&f[1], counts(&f[1].game_dir).mods, None);
        assert!(p.horizon && !p.copy_mods);
        // The empty latest-release instance isn't offered.
        assert!(!has_content(&root.join("instances/latest-release")));
        assert!(has_content(&inst));
    }

    #[test]
    fn atlauncher_instances() {
        let t = tmp("atl");
        let root = &t.0;
        let a = root.join("instances/Pack");
        write(&a.join("instance.json"), r#"{"id":"1.20.1","launcher":{"name":"Big Pack","loaderVersion":{"type":"Forge","version":"47.2.0"}}}"#);
        game(&a);
        write(&root.join("instances/Junk/instance.json"), r#"{"id":"1.20.1"}"#);
        let f = atlauncher(root);
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].name.as_str(), &f[0].loader, f[0].loader_version.as_deref()), ("Big Pack", &Loader::Forge, Some("47.2.0")));
    }

    #[test]
    fn modrinth_both_schemas_and_profile_json() {
        let t = tmp("mr");
        let root = &t.0;
        for p in ["new-a", "old-b", "legacy", "unknown"] {
            game(&root.join("profiles").join(p));
        }
        write(&root.join("profiles/legacy/profile.json"), r#"{"metadata":{"name":"Legacy","game_version":"1.19.2","loader":"quilt","loader_version":{"id":"0.17"}}}"#);
        let script = format!(
            r#"
import sqlite3
db = sqlite3.connect({db:?})
db.execute("CREATE TABLE settings (id INTEGER NOT NULL CHECK (id = 0), custom_dir TEXT NULL, PRIMARY KEY (id))")
db.execute("INSERT INTO settings VALUES (0, NULL)")
db.execute("CREATE TABLE instances (id TEXT NOT NULL, path TEXT NOT NULL, applied_content_set_id TEXT NULL, install_stage TEXT NOT NULL, launcher_feature_version TEXT NOT NULL, update_channel TEXT NOT NULL DEFAULT 'release', name TEXT NOT NULL, icon_path TEXT NULL, created INTEGER NOT NULL, modified INTEGER NOT NULL, last_played INTEGER NULL, submitted_time_played INTEGER NOT NULL DEFAULT 0, recent_time_played INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (id), UNIQUE (path))")
db.execute("CREATE TABLE instance_content_sets (id TEXT NOT NULL, instance_id TEXT NOT NULL, name TEXT NOT NULL, source_kind TEXT NOT NULL, status TEXT NOT NULL, game_version TEXT NOT NULL, protocol_version INTEGER NULL, loader TEXT NOT NULL, loader_version TEXT NULL, created INTEGER NOT NULL, modified INTEGER NOT NULL, PRIMARY KEY (id))")
db.execute("INSERT INTO instances (id, path, applied_content_set_id, install_stage, launcher_feature_version, name, created, modified) VALUES ('i1','new-a','s1','installed','1','Fabulous',0,0)")
db.execute("INSERT INTO instance_content_sets VALUES ('s1','i1','x','local','ok','1.21.1',NULL,'fabric','0.16.9',0,0)")
db.commit()
db.close()
db = sqlite3.connect({old:?})
db.execute("CREATE TABLE profiles (path TEXT NOT NULL, install_stage TEXT NOT NULL, name TEXT NOT NULL, icon_path TEXT NULL, game_version TEXT NOT NULL, mod_loader TEXT NOT NULL, mod_loader_version TEXT NULL, PRIMARY KEY (path))")
db.execute("INSERT INTO profiles VALUES ('old-b','installed','Old Neo',NULL,'1.21.1','neoforge','21.1.77')")
db.commit()
db.close()
"#,
            db = root.join("app.db").to_string_lossy(),
            old = root.join("old.db").to_string_lossy(),
        );
        if !crate::sqlite_ro::tests::python(&script) {
            return;
        }
        let f = modrinth(root);
        let by = |n: &str| f.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("{n}: {:?}", f.iter().map(|x| &x.name).collect::<Vec<_>>()));
        assert_eq!((by("Fabulous").mc.as_deref(), &by("Fabulous").loader, by("Fabulous").loader_version.as_deref()), (Some("1.21.1"), &Loader::Fabric, Some("0.16.9")));
        assert_eq!((by("Legacy").mc.as_deref(), &by("Legacy").loader), (Some("1.19.2"), &Loader::Quilt));
        assert_eq!(by("unknown").mc, None);
        // The schema before 2026-06 (`profiles` table).
        fs::rename(root.join("app.db"), root.join("new.db")).unwrap();
        fs::rename(root.join("old.db"), root.join("app.db")).unwrap();
        let f = modrinth(root);
        let old = f.iter().find(|x| x.name == "Old Neo").unwrap();
        assert_eq!((old.mc.as_deref(), &old.loader), (Some("1.21.1"), &Loader::NeoForge));
    }

    fn rels(c: &Collected) -> Vec<String> {
        let mut v: Vec<String> = c.items.iter().filter(|i| !i.dir).map(|i| i.rel.to_string_lossy().replace('\\', "/")).collect();
        v.sort();
        v
    }

    #[test]
    fn collect_java_with_mods_and_without() {
        let t = tmp("collect");
        game(&t.0);
        let f = found(&t.0, Loader::Fabric, "1.21.11");
        let p = plan(&f, 2, None);
        let c = collect(&f, &p);
        let r = rels(&c);
        assert!(r.contains(&"mods/a.jar".into()) && r.contains(&"mods/b.jar.disabled".into()));
        assert!(!r.iter().any(|x| x.contains("readme.txt") || x.contains("mods/sub")));
        assert!(r.contains(&"config/a.json".into()) && r.contains(&"schematics/house.litematic".into()));
        assert!(r.contains(&"saves/World/region/r.0.0.mca".into()) && r.contains(&"options.txt".into()) && r.contains(&"servers.dat".into()));
        assert!(!r.iter().any(|x| x.starts_with("logs")));
        assert_eq!(counts(&t.0), Counts { mods: 2, worlds: 1, resourcepacks: 2, shaderpacks: 1 });
        // Forge 1.8.9: no mods, no config.
        let f2 = found(&t.0, Loader::Forge, "1.8.9");
        let r2 = rels(&collect(&f2, &plan(&f2, 2, None)));
        assert!(!r2.iter().any(|x| x.starts_with("mods/") || x.starts_with("config/")));
        assert!(r2.contains(&"saves/World/level.dat".into()));
    }

    #[test]
    fn collect_horizon_skips_jars_config_schematics() {
        let t = tmp("hz");
        game(&t.0);
        let f = found(&t.0, Loader::Fabric, "26.3");
        let r = rels(&collect(&f, &plan(&f, 2, None)));
        assert!(!r.iter().any(|x| x.starts_with("mods/") || x.starts_with("config/") || x.starts_with("schematics/")));
        for want in ["saves/World/level.dat", "resourcepacks/P.zip", "resourcepacks/Folder Pack/pack.mcmeta", "shaderpacks/S.zip", "options.txt", "servers.dat", "screenshots/1.png"] {
            assert!(r.contains(&want.to_owned()), "{want}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_inside_followed_outside_skipped() {
        use std::os::unix::fs::symlink;
        let t = tmp("links");
        let root = t.0.join("launcher");
        let inst = root.join("profiles/a");
        game(&inst);
        let secret = t.0.join("outside");
        write(&secret.join("secret.txt"), "nope");
        write(&root.join("store/x.jar"), "store");
        write(&root.join("synced/options.txt"), "synced:1\n");
        fs::remove_file(inst.join("options.txt")).unwrap();
        symlink(root.join("synced/options.txt"), inst.join("options.txt")).unwrap();
        symlink(root.join("store/x.jar"), inst.join("mods/x.jar")).unwrap();
        symlink(secret.join("secret.txt"), inst.join("resourcepacks/evil.zip")).unwrap();
        symlink(&secret, inst.join("saves/Outside")).unwrap();
        symlink(secret.join("secret.txt"), inst.join("mods/evil.jar")).unwrap();
        // A loop back into the world itself is walked once.
        symlink(inst.join("saves/World"), inst.join("saves/World/loop")).unwrap();
        let mut f = found(&inst, Loader::Fabric, "1.21.11");
        f.roots = vec![root.clone()];
        let c = collect(&f, &plan(&f, 3, None));
        let r = rels(&c);
        assert!(r.contains(&"options.txt".into()) && r.contains(&"mods/x.jar".into()));
        assert!(!r.iter().any(|x| x.contains("evil") || x.contains("Outside") || x.contains("secret")));
        assert_eq!(c.outside_links, 3);
        let dest = t.0.join("dest");
        copy_into(&c.items, &dest, &AtomicBool::new(false), &|_| {}).unwrap();
        assert_eq!(fs::read_to_string(dest.join("options.txt")).unwrap(), "synced:1\n");
        assert!(!fs::symlink_metadata(dest.join("options.txt")).unwrap().file_type().is_symlink());
        // Nothing in the source changed.
        assert!(fs::symlink_metadata(inst.join("options.txt")).unwrap().file_type().is_symlink());
    }

    #[test]
    fn copy_never_overwrites_and_cancels() {
        let t = tmp("copy");
        let src = t.0.join("src");
        game(&src);
        let f = found(&src, Loader::Fabric, "1.21.11");
        let c = collect(&f, &plan(&f, 2, None));
        let dest = t.0.join("dest");
        fs::create_dir_all(&dest).unwrap();
        assert!(copy_into(&c.items, &dest, &AtomicBool::new(false), &|_| {}).is_err(), "an existing folder is never written into");
        fs::remove_dir_all(&dest).unwrap();
        let total = std::cell::Cell::new(0);
        copy_into(&c.items, &dest, &AtomicBool::new(false), &|n| total.set(n)).unwrap();
        assert_eq!(total.get(), c.bytes);
        assert_eq!(fs::read_to_string(dest.join("saves/World/region/r.0.0.mca")).unwrap().len(), 5000);
        let dest2 = t.0.join("dest2");
        assert_eq!(copy_into(&c.items, &dest2, &AtomicBool::new(true), &|_| {}), Err("cancelled".into()));
        // The source is untouched.
        assert_eq!(fs::read_to_string(src.join("options.txt")).unwrap(), "fov:1\n");
    }

    /// Lists (read-only) what this computer's launchers hold. Run with --ignored --nocapture.
    #[test]
    #[ignore]
    fn scan_this_computer() {
        let roots = Roots::system().unwrap();
        let j = scan_json(&roots, Some("1.21.11"));
        println!("{}", serde_json::to_string_pretty(&j).unwrap());
        for f in take(&j["items"].as_array().unwrap().iter().map(|i| i["key"].as_str().unwrap().to_owned()).collect::<Vec<_>>()) {
            let c = collect(&f, &plan(&f, counts(&f.game_dir).mods, Some("1.21.11")));
            println!("{}: {} files, {} MB, {} outside links", f.name, c.items.iter().filter(|i| !i.dir).count(), c.bytes >> 20, c.outside_links);
        }
    }

    #[test]
    fn keys_name_only_the_latest_scan() {
        let t = tmp("keys");
        let roots = Roots { data: t.0.join("data"), home: t.0.join("home"), documents: Some(t.0.join("docs")), windows: false };
        let sk = roots.data.join("sklauncher");
        game(&sk.join("instances/a"));
        write(&sk.join("instances.json"), &json!({"instances":[{"id":"a","name":"A","gameType":"fabric","minecraftVersion":"1.21.11","directory": sk.join("instances/a")}]}).to_string());
        let j = scan_json(&roots, None);
        let items = j["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["target"], "Fabric 1.21.11");
        assert_eq!(items[0]["mods"], 2);
        let key = items[0]["key"].as_str().unwrap().to_owned();
        assert_eq!(take(&[key.clone()]).len(), 1);
        let _ = scan_json(&roots, None);
        assert!(take(&[key]).is_empty(), "a key from an older scan names nothing");
        assert!(take(&["x".into(), "1:99".into()]).is_empty());
    }
}
