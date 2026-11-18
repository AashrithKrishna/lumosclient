//! Self-contained game installer and launch-command builder.
//!
//! Everything the game needs is downloaded into the launcher's own folders, with
//! integrity checks, so the player never has to touch files:
//!   <root>/game/versions/<id>/<id>.json + .jar   version metadata and client jars
//!   <root>/game/libraries/…                       Maven-layout libraries
//!   <root>/game/assets/{indexes,objects}/…        sounds, languages, textures
//!   <root>/runtime/<component>/…                  Mojang's Java runtimes
//!   <root>/horizon/                               writable Horizon preparer root
//! Versions the official launcher already installed are reused read-only.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::net;

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const JAVA_RUNTIME_INDEX: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
pub const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
pub const ASSET_BASE: &str = "https://resources.download.minecraft.net";
pub const MOJANG_LIBRARIES: &str = "https://libraries.minecraft.net/";
const DOWNLOAD_THREADS: usize = 8;
pub const FABRIC_PREFIX: &str = "Fabric ";

/// Progress callback: (what is happening, fraction 0..=1, or negative when unknown).
pub type Report<'a> = &'a (dyn Fn(&str, f64) + Sync);

// ───────────────────────── Paths ─────────────────────────

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub game: PathBuf,
    pub runtime: PathBuf,
    pub cache: PathBuf,
    /// The official launcher's folder, read-only, when it exists.
    pub official: Option<PathBuf>,
}

impl Paths {
    pub fn new(root: &Path, official: Option<PathBuf>) -> Self {
        let game = root.join("game");
        let p = Self {
            root: root.to_path_buf(),
            runtime: root.join("runtime"),
            cache: root.join("cache"),
            official: official.filter(|o| o.join("versions").is_dir() && *o != game),
            game,
        };
        for d in [&p.game, &p.runtime, &p.cache] {
            let _ = fs::create_dir_all(d);
        }
        p
    }
    /// Same folders, but never reading the official launcher's.
    pub fn own_only(&self) -> Self {
        Self { official: None, ..self.clone() }
    }
    /// Folders that may hold version JSONs / libraries / assets: ours first.
    fn bases(&self) -> Vec<&Path> {
        let mut v = vec![self.game.as_path()];
        if let Some(o) = &self.official {
            v.push(o.as_path());
        }
        v
    }
    pub fn horizon_root(&self) -> PathBuf {
        self.root.join("horizon")
    }
}

/// The official launcher's game folder for this OS, when it exists.
pub fn official_minecraft_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let p = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/minecraft")
    } else if cfg!(windows) {
        dirs::data_dir()?.join(".minecraft")
    } else {
        home.join(".minecraft")
    };
    p.join("versions").is_dir().then_some(p)
}

// ───────────────────────── Platform & rules ─────────────────────────

fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "osx"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

fn os_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "x86"
    }
}

pub fn classpath_separator() -> &'static str {
    if cfg!(windows) {
        ";"
    } else {
        ":"
    }
}

#[derive(Clone, Debug, Default)]
pub struct Features {
    pub custom_resolution: bool,
    pub quick_play_multiplayer: bool,
}

fn arch_matches(a: &str) -> bool {
    match a {
        "x86" => os_arch() == "x86",
        "arm64" | "aarch64" => os_arch() == "arm64",
        "x86_64" | "amd64" => os_arch() == "x86_64",
        _ => false,
    }
}

fn rule_matches(rule: &Value, f: &Features) -> bool {
    if let Some(os) = rule.get("os") {
        if let Some(n) = os.get("name").and_then(Value::as_str) {
            if n != os_name() {
                return false;
            }
        }
        if let Some(a) = os.get("arch").and_then(Value::as_str) {
            if !arch_matches(a) {
                return false;
            }
        }
    }
    if let Some(feats) = rule.get("features").and_then(Value::as_object) {
        for (k, v) in feats {
            let want = v.as_bool().unwrap_or(false);
            let have = match k.as_str() {
                "has_custom_resolution" => f.custom_resolution,
                "is_quick_play_multiplayer" => f.quick_play_multiplayer,
                _ => false, // demo user, quick play singleplayer/path/realms: never
            };
            if have != want {
                return false;
            }
        }
    }
    true
}

/// Mojang rule semantics: no rules = allowed; otherwise the last matching rule decides.
pub fn rules_allow(rules: Option<&Value>, f: &Features) -> bool {
    let Some(arr) = rules.and_then(Value::as_array) else {
        return true;
    };
    let mut allowed = false;
    for rule in arr {
        if rule_matches(rule, f) {
            allowed = rule.get("action").and_then(Value::as_str).unwrap_or("allow") == "allow";
        }
    }
    allowed
}

/// `group:artifact:version[:classifier][@ext]` → Maven relative path.
pub fn maven_path(coord: &str) -> Option<String> {
    let (coord, ext) = match coord.split_once('@') {
        Some((c, e)) => (c, e),
        None => (coord, "jar"),
    };
    let mut parts = coord.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    let classifier = parts.next();
    if group.is_empty() || artifact.is_empty() || version.is_empty() {
        return None;
    }
    let file = match classifier {
        Some(c) => format!("{artifact}-{version}-{c}.{ext}"),
        None => format!("{artifact}-{version}.{ext}"),
    };
    Some(format!("{}/{artifact}/{version}/{file}", group.replace('.', "/")))
}

/// Library identity without version, so a child version can override its parent's copy.
fn library_key(name: &str) -> String {
    let parts: Vec<&str> = name.split('@').next().unwrap_or(name).split(':').collect();
    match parts.len() {
        0..=2 => name.to_owned(),
        3 => format!("{}:{}", parts[0], parts[1]),
        _ => format!("{}:{}:{}", parts[0], parts[1], parts[3..].join(":")),
    }
}

// ───────────────────────── Version specs ─────────────────────────

/// How the UI names a version.
#[derive(Clone, Debug, PartialEq)]
pub enum Spec {
    /// A version JSON id (vanilla release/snapshot, or any installed id).
    Id(String),
    /// "Fabric <mc>": latest stable Fabric Loader on that Minecraft version.
    Fabric(String),
    // ── NeoForge / Forge (forge.rs) ──
    /// "NeoForge <mc>": latest stable NeoForge on that Minecraft version.
    NeoForge(String),
    /// "Forge <mc>": recommended (else latest) Forge on that Minecraft version.
    Forge(String),
}

pub fn parse_spec(s: &str) -> Spec {
    let t = s.trim();
    // ── NeoForge / Forge (forge.rs) ──
    match crate::forge::split_id(t) {
        Some((crate::forge::Kind::NeoForge, mc)) if crate::forge::spec_supported(crate::forge::Kind::NeoForge, mc) => {
            return Spec::NeoForge(mc.to_owned())
        }
        Some((crate::forge::Kind::Forge, mc)) if crate::forge::spec_supported(crate::forge::Kind::Forge, mc) => return Spec::Forge(mc.to_owned()),
        _ => {}
    }
    match t.strip_prefix(FABRIC_PREFIX) {
        Some(mc) if !mc.trim().is_empty() => Spec::Fabric(mc.trim().to_owned()),
        _ => Spec::Id(t.to_owned()),
    }
}

/// Version folder names must be a single, plain path component.
pub(crate) fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains('\0')
        && !id.chars().any(|c| c.is_control() || matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
        && !id.ends_with('.')
        && !id.ends_with(' ')
        && id != "."
        && id != ".."
}

/// A relative path made only of normal components (no `..`, no root, no drive prefix).
pub fn safe_rel(rel: &str) -> bool {
    use std::path::Component;
    let p = Path::new(rel);
    !rel.is_empty() && !rel.contains('\0') && p.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Lexically resolves `..`/`.` and reports whether `path` stays inside `base`.
fn stays_within(base: &Path, path: &Path) -> bool {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                if !out.pop() {
                    return false;
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out.starts_with(base)
}

pub(crate) fn version_json_in(base: &Path, id: &str) -> PathBuf {
    base.join("versions").join(id).join(format!("{id}.json"))
}

pub(crate) fn find_version_json(paths: &Paths, id: &str) -> Option<PathBuf> {
    if !safe_id(id) {
        return None;
    }
    paths.bases().into_iter().map(|b| version_json_in(b, id)).find(|p| p.is_file())
}

pub(crate) fn read_json(p: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(p).map_err(|e| format!("Couldn't read {}: {e}", p.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("{} is damaged ({e}).", p.display()))
}

pub(crate) fn write_json_atomic(p: &Path, v: &Value) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
    }
    let tmp = p.with_extension("json.part");
    let text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    fs::write(&tmp, text).map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, p).map_err(|e| format!("Couldn't save {}: {e}", p.display()))
}

// ───────────────────────── Manifest & catalog ─────────────────────────

/// Mojang's version manifest, refreshed at most every 10 minutes, cached for offline use.
pub fn manifest(paths: &Paths) -> Result<Value, String> {
    let cached = paths.cache.join("version_manifest_v2.json");
    let fresh = fs::metadata(&cached)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(600));
    if fresh {
        if let Ok(v) = read_json(&cached) {
            return Ok(v);
        }
    }
    match net::get_json(MANIFEST_URL) {
        Ok(v) if v.get("versions").and_then(Value::as_array).is_some() => {
            let _ = write_json_atomic(&cached, &v);
            Ok(v)
        }
        Ok(_) => read_json(&cached).map_err(|_| "Mojang's version list was unreadable.".to_owned()),
        Err(e) => read_json(&cached).map_err(|_| e),
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    /// "vanilla" | "fabric" | "installed"
    pub kind: String,
    pub installed: bool,
}

/// Fabric supports Minecraft 1.14 and newer (and every year-numbered release).
fn fabric_supports(mc: &str) -> bool {
    let mut it = mc.split('.').filter_map(|p| p.parse::<u32>().ok());
    match (it.next(), it.next()) {
        (Some(1), Some(minor)) => minor >= 14,
        (Some(major), _) => major >= 2,
        _ => false,
    }
}

/// Every installed version, plus every release (and its Fabric variant) that can be installed.
pub fn catalog(paths: &Paths, manifest: Option<&Value>) -> Vec<CatalogEntry> {
    let mut installed: Vec<String> = Vec::new();
    for base in paths.bases() {
        if let Ok(rd) = fs::read_dir(base.join("versions")) {
            for e in rd.flatten() {
                let id = e.file_name().to_string_lossy().into_owned();
                // "Fabric <mc>" means LumosClient's own profile; other launchers' copies don't count.
                let foreign_fabric = (id.starts_with(FABRIC_PREFIX) || matches!(parse_spec(&id), Spec::NeoForge(_) | Spec::Forge(_))) && base != paths.game.as_path();
                if safe_id(&id) && !foreign_fabric && version_json_in(base, &id).is_file() && !installed.contains(&id) {
                    installed.push(id);
                }
            }
        }
    }
    let releases: Vec<String> = manifest
        .and_then(|m| m.get("versions"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|v| v.get("type").and_then(Value::as_str) == Some("release"))
                .filter_map(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut out: Vec<CatalogEntry> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let loader_index = crate::forge::cached_index(paths);
    for mc in &releases {
        if fabric_supports(mc) {
            let id = format!("{FABRIC_PREFIX}{mc}");
            out.push(CatalogEntry { installed: installed.contains(&id), id: id.clone(), kind: "fabric".into() });
            seen.insert(id);
        }
        // ── NeoForge / Forge (forge.rs) ──
        for (id, kind) in crate::forge::catalog_variants(&loader_index, mc) {
            out.push(CatalogEntry { installed: installed.contains(&id), id: id.clone(), kind: kind.into() });
            seen.insert(id);
        }
        out.push(CatalogEntry { installed: installed.contains(mc), id: mc.clone(), kind: "vanilla".into() });
        seen.insert(mc.clone());
    }
    // Installed versions the manifest doesn't know (Forge, other launchers' Fabric, snapshots…)
    let mut extra: Vec<String> = installed.into_iter().filter(|i| !seen.contains(i)).collect();
    extra.sort();
    out.extend(extra.into_iter().map(|id| CatalogEntry { id, kind: "installed".into(), installed: true }));
    out
}

// ───────────────────────── Install version metadata ─────────────────────────

pub(crate) fn install_vanilla_json(paths: &Paths, id: &str) -> Result<PathBuf, String> {
    if let Some(found) = find_version_json(paths, id) {
        if read_json(&found).is_ok() {
            return Ok(found);
        }
    }
    let m = manifest(paths)?;
    let entry = m
        .get("versions")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|v| v.get("id").and_then(Value::as_str) == Some(id)))
        .ok_or_else(|| format!("Minecraft {id} isn't a version Mojang offers."))?;
    let url = entry.get("url").and_then(Value::as_str).ok_or("Mojang's version list is missing a link.")?;
    let dest = version_json_in(&paths.game, id);
    net::download_verified(url, &dest, entry.get("sha1").and_then(Value::as_str), None)?;
    read_json(&dest)?;
    Ok(dest)
}

fn install_fabric_json(paths: &Paths, mc: &str) -> Result<String, String> {
    let id = format!("{FABRIC_PREFIX}{mc}");
    if !safe_id(&id) || !fabric_supports(mc) {
        return Err(format!("Fabric isn't available for Minecraft {mc}."));
    }
    install_vanilla_json(paths, mc)?;
    let ours = version_json_in(&paths.game, &id);
    if ours.is_file() && read_json(&ours).is_ok() {
        return Ok(id);
    }
    let loaders = net::get_json(&format!("{FABRIC_META}/versions/loader/{mc}"))?;
    let loader = loaders
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|l| l.pointer("/loader/stable").and_then(Value::as_bool) == Some(true))
                .or_else(|| a.first())
        })
        .and_then(|l| l.pointer("/loader/version").and_then(Value::as_str))
        .ok_or_else(|| format!("Fabric doesn't support Minecraft {mc} yet."))?
        .to_owned();
    let mut profile = net::get_json(&format!("{FABRIC_META}/versions/loader/{mc}/{loader}/profile/json"))?;
    if profile.get("mainClass").and_then(Value::as_str).is_none() {
        return Err("Fabric sent an incomplete profile.".into());
    }
    profile["id"] = Value::String(id.clone());
    profile["inheritsFrom"] = Value::String(mc.to_owned());
    profile["lumosFabricLoader"] = Value::String(loader);
    write_json_atomic(&version_json_in(&paths.game, &id), &profile)?;
    Ok(id)
}

// ───────────────────────── Resolve (inheritance) ─────────────────────────

#[derive(Clone, Debug)]
pub struct Resolved {
    pub id: String,
    /// Merged JSON (child over parent).
    pub json: Value,
    /// Which id's client jar the game runs.
    pub jar_id: String,
}

fn merge(child: &Value, parent: &Value) -> Value {
    let mut out = parent.clone();
    let (Some(obj), Some(cobj)) = (out.as_object_mut(), child.as_object()) else {
        return child.clone();
    };
    for (k, v) in cobj {
        match k.as_str() {
            "inheritsFrom" => {}
            "libraries" => {
                let child_libs = v.as_array().cloned().unwrap_or_default();
                let child_keys: HashSet<String> = child_libs
                    .iter()
                    .filter_map(|l| l.get("name").and_then(Value::as_str).map(library_key))
                    .collect();
                let mut libs = child_libs;
                // A parent entry is dropped only when the child provides that library itself
                // (vanilla lists some libraries twice on purpose: per-OS variants, natives).
                for lib in parent.get("libraries").and_then(Value::as_array).cloned().unwrap_or_default() {
                    let key = lib.get("name").and_then(Value::as_str).map(library_key).unwrap_or_default();
                    if key.is_empty() || !child_keys.contains(&key) {
                        libs.push(lib);
                    }
                }
                obj.insert(k.clone(), Value::Array(libs));
            }
            "arguments" => {
                let mut args = parent.get("arguments").cloned().unwrap_or_else(|| serde_json::json!({}));
                for side in ["game", "jvm"] {
                    let mut list = args.get(side).and_then(Value::as_array).cloned().unwrap_or_default();
                    list.extend(v.get(side).and_then(Value::as_array).cloned().unwrap_or_default());
                    args[side] = Value::Array(list);
                }
                obj.insert(k.clone(), args);
            }
            _ => {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    out
}

/// The Minecraft version at the root of a version's inheritance chain (e.g. "1.21.1" for
/// "fabric-loader-0.16.10-1.21.1"), read from the installed JSONs without downloading.
pub fn base_game_version(paths: &Paths, id: &str) -> Option<String> {
    let mut cur = id.to_owned();
    for _ in 0..8 {
        let p = find_version_json(paths, &cur)?;
        let json = read_json(&p).ok()?;
        match json.get("inheritsFrom").and_then(Value::as_str) {
            Some(parent) if parent != cur => cur = parent.to_owned(),
            _ => return Some(cur),
        }
    }
    Some(cur)
}

/// Loads a version and everything it inherits from, installing missing parents.
pub fn resolve(paths: &Paths, id: &str) -> Result<Resolved, String> {
    let mut chain: Vec<(String, Value)> = Vec::new();
    let mut cur = id.to_owned();
    for _ in 0..8 {
        let p = match find_version_json(paths, &cur) {
            Some(found) => found,
            None => install_vanilla_json(paths, &cur)?,
        };
        let json = read_json(&p)?;
        let parent = json.get("inheritsFrom").and_then(Value::as_str).map(str::to_owned);
        chain.push((cur.clone(), json));
        match parent {
            Some(par) if !chain.iter().any(|(c, _)| *c == par) => cur = par,
            _ => break,
        }
    }
    // The jar comes from the version that has a client download (usually the vanilla parent),
    // else from whichever version folder actually has a jar.
    let jar_id = chain
        .iter()
        .find(|(_, j)| j.pointer("/downloads/client").is_some())
        .map(|(i, _)| i.clone())
        .or_else(|| {
            chain
                .iter()
                .find(|(i, _)| paths.bases().iter().any(|b| b.join("versions").join(i).join(format!("{i}.jar")).is_file()))
                .map(|(i, _)| i.clone())
        })
        .unwrap_or_else(|| id.to_owned());
    let mut merged = chain.last().map(|(_, j)| j.clone()).unwrap_or(Value::Null);
    for (_, j) in chain.iter().rev().skip(1) {
        merged = merge(j, &merged);
    }
    merged["id"] = Value::String(id.to_owned());
    Ok(Resolved { id: id.to_owned(), json: merged, jar_id })
}

// ───────────────────────── Downloads ─────────────────────────

struct Job {
    url: String,
    dest: PathBuf,
    sha1: Option<String>,
    size: Option<u64>,
}

/// Downloads in parallel with progress (by bytes when sizes are known).
fn run_jobs(jobs: Vec<Job>, label: &str, report: Report) -> Result<(), String> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let jobs: Vec<Job> = jobs.into_iter().filter(|j| !j.url.is_empty() && seen.insert(j.dest.clone())).collect();
    if jobs.is_empty() {
        return Ok(());
    }
    let total_bytes: u64 = jobs.iter().map(|j| j.size.unwrap_or(0)).sum();
    let total_count = jobs.len();
    let queue = Mutex::new(jobs.into_iter().collect::<VecDeque<_>>());
    let done_bytes = AtomicU64::new(0);
    let done_count = AtomicUsize::new(0);
    let error: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|s| {
        let workers: Vec<_> = (0..DOWNLOAD_THREADS.min(total_count))
            .map(|_| {
                s.spawn(|| loop {
                    if error.lock().unwrap().is_some() {
                        return;
                    }
                    let Some(job) = queue.lock().unwrap().pop_front() else { return };
                    if let Err(e) = net::download_verified(&job.url, &job.dest, job.sha1.as_deref(), job.size) {
                        error.lock().unwrap().get_or_insert(e);
                        return;
                    }
                    done_bytes.fetch_add(job.size.unwrap_or(0), Ordering::Relaxed);
                    done_count.fetch_add(1, Ordering::Relaxed);
                })
            })
            .collect();
        let mut last = Instant::now() - Duration::from_secs(1);
        while !workers.iter().all(|w| w.is_finished()) {
            if last.elapsed() >= Duration::from_millis(150) {
                last = Instant::now();
                let bytes = done_bytes.load(Ordering::Relaxed);
                let count = done_count.load(Ordering::Relaxed);
                let (frac, text) = if total_bytes > 0 {
                    let mb = |b: u64| b as f64 / 1_048_576.0;
                    (bytes as f64 / total_bytes as f64, format!("{label} · {:.0} of {:.0} MB", mb(bytes), mb(total_bytes)))
                } else {
                    (count as f64 / total_count as f64, format!("{label} · {count} of {total_count}"))
                };
                report(&text, frac.clamp(0.0, 1.0));
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    });
    if let Some(e) = error.lock().unwrap().take() {
        return Err(e);
    }
    report(label, 1.0);
    Ok(())
}

/// An existing, non-empty file with the expected size (sha1 was checked when downloaded).
fn present(p: &Path, size: Option<u64>) -> bool {
    fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0 && size.is_none_or(|s| m.len() == s))
}

fn find_existing(paths: &Paths, sub: &str, rel: &str, size: Option<u64>) -> Option<PathBuf> {
    paths.bases().into_iter().map(|b| b.join(sub).join(rel)).find(|p| present(p, size))
}

/// Native classifier for legacy (LWJGL 2) libraries on this OS.
fn native_classifier(lib: &Value) -> Option<String> {
    let key = lib.get("natives")?.get(os_name())?.as_str()?;
    let bits = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
    Some(key.replace("${arch}", bits))
}

fn s(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_owned)
}

/// Where a library file lives, or a job to fetch it into our folder.
fn locate(paths: &Paths, rel: &str, url: Option<String>, sha1: Option<String>, size: Option<u64>, jobs: &mut Vec<Job>) -> Result<PathBuf, String> {
    if !safe_rel(rel) {
        return Err(format!("A version file lists an unsafe library path ({rel})."));
    }
    if let Some(p) = find_existing(paths, "libraries", rel, size) {
        return Ok(p);
    }
    let dest = paths.game.join("libraries").join(rel);
    if let Some(url) = url {
        jobs.push(Job { url, dest: dest.clone(), sha1, size });
    }
    Ok(dest)
}

pub struct Prepared {
    pub classpath: Vec<PathBuf>,
    pub natives_dir: PathBuf,
    pub assets_root: PathBuf,
    pub asset_index: String,
    /// For pre-1.7 "virtual" asset layouts.
    pub game_assets: PathBuf,
    pub logging_arg: Option<String>,
}

/// Makes sure every file the version needs is present, downloading what's missing.
pub fn ensure_files(paths: &Paths, r: &Resolved, report: Report) -> Result<Prepared, String> {
    let f = Features::default();
    let j = &r.json;
    report("Checking game files", -1.0);
    let mut jobs: Vec<Job> = Vec::new();

    // Client jar
    let jar_rel = format!("{0}/{0}.jar", r.jar_id);
    let jar_dl = j.pointer("/downloads/client");
    let jar_size = jar_dl.and_then(|d| d.get("size")).and_then(Value::as_u64);
    let jar_path = match find_existing(paths, "versions", &jar_rel, jar_size) {
        Some(p) => p,
        None => {
            let d = jar_dl.ok_or_else(|| format!("{} has no game jar to download.", r.jar_id))?;
            let dest = paths.game.join("versions").join(&jar_rel);
            jobs.push(Job {
                url: s(d.get("url")).unwrap_or_default(),
                dest: dest.clone(),
                sha1: s(d.get("sha1")),
                size: jar_size,
            });
            dest
        }
    };

    // Libraries
    let mut classpath: Vec<PathBuf> = Vec::new();
    let mut seen_cp: HashSet<PathBuf> = HashSet::new();
    let mut natives: Vec<PathBuf> = Vec::new();
    for lib in j.get("libraries").and_then(Value::as_array).cloned().unwrap_or_default() {
        if !rules_allow(lib.get("rules"), &f) {
            continue;
        }
        let name = lib.get("name").and_then(Value::as_str).unwrap_or_default();
        let main = match lib.get("downloads") {
            // Mojang style: an artifact (absent on old native-only entries).
            Some(dl) => dl.get("artifact").and_then(|a| {
                let rel = s(a.get("path")).or_else(|| maven_path(name))?;
                Some((rel, s(a.get("url")), s(a.get("sha1")), a.get("size").and_then(Value::as_u64)))
            }),
            // Maven style (Fabric): coordinate + repository URL.
            None => maven_path(name).map(|rel| {
                let base = lib.get("url").and_then(Value::as_str).unwrap_or(MOJANG_LIBRARIES);
                let url = format!("{}/{rel}", base.trim_end_matches('/'));
                (rel, Some(url), s(lib.get("sha1")), lib.get("size").and_then(Value::as_u64))
            }),
        };
        if let Some((rel, url, sha1, size)) = main {
            let path = locate(paths, &rel, url, sha1, size, &mut jobs)?;
            if seen_cp.insert(path.clone()) {
                classpath.push(path);
            }
        }
        if let Some(cls) = native_classifier(&lib) {
            if let Some(nat) = lib.pointer(&format!("/downloads/classifiers/{cls}")) {
                if let Some(rel) = s(nat.get("path")) {
                    let path = locate(paths, &rel, s(nat.get("url")), s(nat.get("sha1")), nat.get("size").and_then(Value::as_u64), &mut jobs)?;
                    natives.push(path);
                }
            }
        }
    }
    classpath.push(jar_path);

    // Logging config (Mojang's patched log4j configs, important for old versions)
    let mut logging_arg = None;
    if let Some(log) = j.pointer("/logging/client") {
        if let (Some(arg), Some(file)) = (log.get("argument").and_then(Value::as_str), log.get("file")) {
            if let Some(fid) = file.get("id").and_then(Value::as_str).filter(|i| safe_id(i)) {
                let dest = paths.game.join("assets").join("log_configs").join(fid);
                let size = file.get("size").and_then(Value::as_u64);
                if !present(&dest, size) {
                    jobs.push(Job { url: s(file.get("url")).unwrap_or_default(), dest: dest.clone(), sha1: s(file.get("sha1")), size });
                }
                logging_arg = Some(arg.replace("${path}", &dest.to_string_lossy()));
            }
        }
    }
    run_jobs(jobs, "Downloading game files", report)?;
    for p in &classpath {
        if !p.is_file() {
            return Err(format!("A game file couldn't be downloaded: {}", p.display()));
        }
    }

    // Natives: extracted for legacy versions; modern ones unpack themselves.
    let natives_dir = paths.game.join("natives").join(&r.id);
    for sub in ["", "java", "jna", "lwjgl", "netty"] {
        fs::create_dir_all(natives_dir.join(sub)).map_err(|e| format!("Couldn't create {}: {e}", natives_dir.display()))?;
    }
    for jar in &natives {
        extract_natives(jar, &natives_dir)?;
    }

    let (assets_root, asset_index, game_assets) = ensure_assets(paths, j, report)?;
    Ok(Prepared { classpath, natives_dir, assets_root, asset_index, game_assets, logging_arg })
}

fn extract_natives(jar: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(jar).map_err(|e| format!("Couldn't open {}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{} is damaged: {e}", jar.display()))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { continue };
        if entry.is_dir() || rel.starts_with("META-INF") {
            continue;
        }
        let out = dest.join(&rel);
        if fs::metadata(&out).is_ok_and(|m| m.is_file() && m.len() == entry.size()) {
            continue;
        }
        if let Some(parent) = out.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        fs::write(&out, buf).map_err(|e| format!("Couldn't write {}: {e}", out.display()))?;
    }
    Ok(())
}

fn ensure_assets(paths: &Paths, j: &Value, report: Report) -> Result<(PathBuf, String, PathBuf), String> {
    let idx = j.get("assetIndex").ok_or("This version doesn't list its assets.")?;
    let id = idx.get("id").and_then(Value::as_str).ok_or("This version's asset list has no name.")?.to_owned();
    if !safe_id(&id) {
        return Err("This version's asset list has an invalid name.".into());
    }
    let rel = format!("indexes/{id}.json");
    let size = idx.get("size").and_then(Value::as_u64);
    let own = paths.game.join("assets");
    if !present(&own.join(&rel), size) {
        net::download_verified(
            idx.get("url").and_then(Value::as_str).unwrap_or_default(),
            &own.join(&rel),
            idx.get("sha1").and_then(Value::as_str),
            size,
        )?;
    }
    let index = read_json(&own.join(&rel))?;
    let objects = index.get("objects").and_then(Value::as_object).cloned().unwrap_or_default();
    let complete_in = |assets: &Path| {
        objects.values().all(|o| {
            let h = o.get("hash").and_then(Value::as_str).unwrap_or("");
            h.len() == 40 && present(&assets.join("objects").join(&h[..2]).join(h), o.get("size").and_then(Value::as_u64))
        })
    };
    // Reuse the official launcher's store only when it already has everything (read-only);
    // otherwise everything missing goes into ours.
    let root = match paths.official.as_ref().map(|o| o.join("assets")) {
        Some(off) if !complete_in(&own) && present(&off.join(&rel), size) && complete_in(&off) => off,
        _ => own.clone(),
    };
    let virtual_layout = index.get("virtual").and_then(Value::as_bool) == Some(true)
        || index.get("map_to_resources").and_then(Value::as_bool) == Some(true);
    let mut jobs = Vec::new();
    for obj in objects.values() {
        let Some(hash) = obj.get("hash").and_then(Value::as_str) else { continue };
        if hash.len() != 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let size = obj.get("size").and_then(Value::as_u64);
        let dest = root.join("objects").join(&hash[..2]).join(hash);
        if !present(&dest, size) {
            jobs.push(Job { url: format!("{ASSET_BASE}/{}/{hash}", &hash[..2]), dest, sha1: Some(hash.to_owned()), size });
        }
    }
    run_jobs(jobs, "Downloading sounds and textures", report)?;
    let mut game_assets = root.clone();
    if virtual_layout {
        game_assets = own.join("virtual").join(&id);
        for (name, obj) in &objects {
            let Some(hash) = obj.get("hash").and_then(Value::as_str) else { continue };
            if !safe_rel(name) || hash.len() != 40 {
                continue;
            }
            let dest = game_assets.join(name);
            if !dest.is_file() {
                if let Some(parent) = dest.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::copy(root.join("objects").join(&hash[..2]).join(hash), &dest);
            }
        }
    }
    Ok((root, id, game_assets))
}

// ───────────────────────── Java ─────────────────────────

fn runtime_platforms(component: &str, intel_only_natives: bool) -> Vec<&'static str> {
    if cfg!(target_os = "macos") {
        if os_arch() == "arm64" {
            // Mojang ships no Apple-silicon Java 8, and versions before 1.19 only have Intel
            // macOS natives: both run on the Intel Java under Rosetta, like the official launcher.
            if component == "jre-legacy" || intel_only_natives {
                vec!["mac-os"]
            } else {
                vec!["mac-os-arm64", "mac-os"]
            }
        } else {
            vec!["mac-os"]
        }
    } else if cfg!(windows) {
        match os_arch() {
            "arm64" => vec!["windows-arm64", "windows-x64"],
            "x86" => vec!["windows-x86"],
            _ => vec!["windows-x64"],
        }
    } else if os_arch() == "x86" {
        vec!["linux-i386"]
    } else {
        vec!["linux"]
    }
}

pub fn java_binary(runtime_dir: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        runtime_dir.join("jre.bundle/Contents/Home/bin/java")
    } else if cfg!(windows) {
        runtime_dir.join("bin/javaw.exe")
    } else {
        runtime_dir.join("bin/java")
    }
}

/// The java binary inside a JAVA_HOME the player chose themselves.
pub fn java_binary_in_home(home: &Path) -> PathBuf {
    home.join("bin").join(if cfg!(windows) { "javaw.exe" } else { "java" })
}

/// JAVA_HOME for a java binary (…/bin/java → …).
pub fn java_home_of(java: &Path) -> PathBuf {
    java.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default()
}

/// True when a version only ships Intel macOS natives (LWJGL before 3.3, i.e. Minecraft < 1.19).
pub fn needs_intel_java(r: &Resolved) -> bool {
    if !(cfg!(target_os = "macos") && os_arch() == "arm64") {
        return false;
    }
    let libs = r.json.get("libraries").and_then(Value::as_array).cloned().unwrap_or_default();
    let has_arm = libs.iter().any(|l| l.get("name").and_then(Value::as_str).is_some_and(|n| n.contains("natives-macos-arm64")));
    let has_mac_natives = libs.iter().any(|l| {
        l.get("name").and_then(Value::as_str).is_some_and(|n| n.contains("natives-macos") || n.contains("natives-osx"))
            || l.pointer("/natives/osx").is_some()
    });
    has_mac_natives && !has_arm
}

/// Installs (or reuses) Mojang's Java runtime `component`; returns the `java` binary.
pub fn ensure_java(paths: &Paths, component: &str, intel_only_natives: bool, report: Report) -> Result<PathBuf, String> {
    if !safe_id(component) {
        return Err("Unknown Java runtime.".into());
    }
    let platforms = runtime_platforms(component, intel_only_natives);
    // An already-installed runtime for a usable platform needs no network.
    for p in &platforms {
        let dir = paths.runtime.join(format!("{component}-{p}"));
        let java = java_binary(&dir);
        // ".ember-runtime.json" is the marker written by builds from before the rename.
        if (dir.join(".lumos-runtime.json").is_file() || dir.join(".ember-runtime.json").is_file()) && java.is_file() {
            return Ok(java);
        }
    }
    report("Getting Java", -1.0);
    let index = net::get_json(JAVA_RUNTIME_INDEX)?;
    let (platform, entry) = platforms
        .into_iter()
        .find_map(|p| {
            index
                .get(p)
                .and_then(|x| x.get(component))
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .map(|e| (p, e.clone()))
        })
        .ok_or_else(|| format!("Mojang has no {component} Java for this computer."))?;
    let man_url = entry.pointer("/manifest/url").and_then(Value::as_str).ok_or("Java listing is missing a link.")?;
    let man_sha1 = entry.pointer("/manifest/sha1").and_then(Value::as_str);
    let dir = paths.runtime.join(format!("{component}-{platform}"));
    let marker = dir.join(".lumos-runtime.json");
    let java = java_binary(&dir);
    let man_path = paths.cache.join(format!("java-{component}-{platform}.json"));
    net::download_verified(man_url, &man_path, man_sha1, None)?;
    let manifest = read_json(&man_path)?;
    let files = manifest.get("files").and_then(Value::as_object).ok_or("Java's file list is unreadable.")?;
    let mut jobs = Vec::new();
    let mut executables = Vec::new();
    let mut links = Vec::new();
    for (rel, info) in files {
        if !safe_rel(rel) {
            continue;
        }
        let dest = dir.join(rel);
        match info.get("type").and_then(Value::as_str) {
            Some("directory") => {
                fs::create_dir_all(&dest).map_err(|e| format!("Couldn't create {}: {e}", dest.display()))?;
            }
            Some("file") => {
                let Some(raw) = info.pointer("/downloads/raw") else { continue };
                let size = raw.get("size").and_then(Value::as_u64);
                let sha1 = s(raw.get("sha1"));
                if !net::file_is_valid(&dest, sha1.as_deref(), size) {
                    jobs.push(Job { url: s(raw.get("url")).unwrap_or_default(), dest: dest.clone(), sha1, size });
                }
                if info.get("executable").and_then(Value::as_bool) == Some(true) {
                    executables.push(dest);
                }
            }
            Some("link") => {
                if let Some(t) = info.get("target").and_then(Value::as_str) {
                    // Relative targets like ../java.base/LICENSE are normal; escaping the runtime isn't.
                    let resolved = dest.parent().map(|p| p.join(t)).unwrap_or_default();
                    if !t.starts_with('/') && stays_within(&dir, &resolved) {
                        links.push((dest, t.to_owned()));
                    }
                }
            }
            _ => {}
        }
    }
    run_jobs(jobs, "Downloading Java", report)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for e in &executables {
            let _ = fs::set_permissions(e, fs::Permissions::from_mode(0o755));
        }
        for (link, target) in &links {
            if target.starts_with('/') {
                continue;
            }
            if let Some(parent) = link.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if fs::symlink_metadata(link).is_ok() {
                let _ = fs::remove_file(link);
            }
            let _ = std::os::unix::fs::symlink(target, link);
        }
    }
    #[cfg(not(unix))]
    let _ = (&executables, &links);
    if !java.is_file() {
        return Err("Java downloaded, but the java program is missing.".into());
    }
    let info = serde_json::json!({
        "component": component, "platform": platform,
        "version": entry.pointer("/version/name"), "manifest": man_sha1,
    });
    write_json_atomic(&marker, &info)?;
    Ok(java)
}

/// (Mojang runtime component, Java major) a version asks for.
pub fn java_component(r: &Resolved) -> (String, u64) {
    let major = r.json.pointer("/javaVersion/majorVersion").and_then(Value::as_u64).unwrap_or(8);
    let comp = s(r.json.pointer("/javaVersion/component")).unwrap_or_else(|| "jre-legacy".to_owned());
    (comp, major)
}

// ───────────────────────── Launch command ─────────────────────────

#[derive(Clone, Debug)]
pub struct Auth {
    pub name: String,
    /// 32 hex, no dashes.
    pub uuid: String,
    pub access_token: String,
    /// "legacy" offline, "mojang" for Ely.by.
    pub user_type: String,
    /// e.g. "-javaagent:/…/authlib-injector.jar=ely.by"
    pub javaagent: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LaunchOpts {
    pub memory_mb: u32,
    pub extra_jvm: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub server: Option<String>,
    pub launcher_version: String,
}

fn substitute(s: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find('}') {
            Some(end) => {
                let key = &rest[start + 2..start + 2 + end];
                match vars.get(key) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + 3 + end]),
                }
                rest = &rest[start + 3 + end..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn collect_args(list: Option<&Value>, f: &Features, vars: &HashMap<&str, String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in list.and_then(Value::as_array).cloned().unwrap_or_default() {
        match &item {
            Value::String(s) => out.push(substitute(s, vars)),
            Value::Object(_) => {
                if !rules_allow(item.get("rules"), f) {
                    continue;
                }
                match item.get("value") {
                    Some(Value::String(s)) => out.push(substitute(s, vars)),
                    Some(Value::Array(a)) => out.extend(a.iter().filter_map(Value::as_str).map(|s| substitute(s, vars))),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

fn unresolved(s: &str) -> bool {
    s.is_empty() || s.contains("${")
}

/// Drops `--flag value` pairs whose value is empty or an unresolved placeholder, and any
/// other argument that still has an unresolved placeholder.
fn drop_empty_pairs(args: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a.starts_with("--") && args.get(i + 1).is_some_and(|n| !n.starts_with("--") && unresolved(n)) {
            i += 2;
            continue;
        }
        if unresolved(a) {
            i += 1;
            continue;
        }
        out.push(a.clone());
        i += 1;
    }
    out
}

fn split_server(addr: &str) -> (String, String) {
    let a = addr.trim();
    if let Some(rest) = a.strip_prefix('[') {
        if let Some((host, port)) = rest.split_once("]:") {
            return (host.to_owned(), port.to_owned());
        }
        return (rest.trim_end_matches(']').to_owned(), "25565".into());
    }
    match a.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') && !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h.to_owned(), p.to_owned()),
        _ => (a.to_owned(), "25565".into()),
    }
}

/// Full argument list (after the java binary) to start this version.
pub fn build_args(r: &Resolved, prep: &Prepared, game_dir: &Path, auth: &Auth, o: &LaunchOpts) -> Vec<String> {
    let j = &r.json;
    let has_new_args = j.get("arguments").is_some();
    let supports_quick_play = j
        .pointer("/arguments/game")
        .is_some_and(|g| g.to_string().contains("is_quick_play_multiplayer"));
    let features = Features {
        custom_resolution: !o.fullscreen,
        quick_play_multiplayer: o.server.is_some() && supports_quick_play,
    };
    let cp: Vec<String> = prep.classpath.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let classpath = cp.join(classpath_separator());
    let library_dir = prep
        .classpath
        .first()
        .and_then(|p| p.ancestors().find(|a| a.file_name().is_some_and(|n| n == "libraries")))
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", auth.name.clone());
    vars.insert("version_name", r.id.clone());
    vars.insert("game_directory", game_dir.to_string_lossy().into_owned());
    vars.insert("assets_root", prep.assets_root.to_string_lossy().into_owned());
    vars.insert("game_assets", prep.game_assets.to_string_lossy().into_owned());
    vars.insert("assets_index_name", prep.asset_index.clone());
    vars.insert("auth_uuid", auth.uuid.clone());
    vars.insert("auth_access_token", auth.access_token.clone());
    vars.insert("auth_session", format!("token:{}:{}", auth.access_token, auth.uuid));
    vars.insert("user_type", auth.user_type.clone());
    vars.insert("user_properties", "{}".into());
    vars.insert("clientid", String::new());
    vars.insert("auth_xuid", String::new());
    vars.insert("version_type", "LumosClient".into());
    vars.insert("natives_directory", prep.natives_dir.to_string_lossy().into_owned());
    vars.insert("launcher_name", "LumosClient".into());
    vars.insert("launcher_version", o.launcher_version.clone());
    vars.insert("classpath", classpath.clone());
    vars.insert("classpath_separator", classpath_separator().into());
    vars.insert("library_directory", library_dir);
    vars.insert("resolution_width", o.width.max(640).to_string());
    vars.insert("resolution_height", o.height.max(480).to_string());
    if let Some(sv) = &o.server {
        vars.insert("quickPlayMultiplayer", sv.trim().to_owned());
    }

    let mut args: Vec<String> = Vec::new();
    if let Some(agent) = &auth.javaagent {
        // The agent can bring its own JVM properties after it, one per line.
        args.extend(agent.lines().filter(|l| !l.trim().is_empty()).map(str::to_owned));
    }
    args.push(format!("-Xmx{}M", o.memory_mb.clamp(512, 65536)));
    args.push(format!("-Xms{}M", (o.memory_mb / 4).clamp(256, 2048)));
    args.extend(o.extra_jvm.iter().filter(|a| !a.trim().is_empty()).cloned());
    if let Some(l) = &prep.logging_arg {
        args.push(l.clone());
    }
    if has_new_args {
        args.extend(collect_args(j.pointer("/arguments/jvm"), &features, &vars));
    } else {
        if cfg!(target_os = "macos") {
            args.push("-XstartOnFirstThread".into());
        }
        args.push(format!("-Djava.library.path={}", prep.natives_dir.display()));
        args.push("-Dminecraft.launcher.brand=LumosClient".into());
        args.push("-cp".into());
        args.push(classpath);
    }
    args.push(j.get("mainClass").and_then(Value::as_str).unwrap_or("net.minecraft.client.main.Main").to_owned());
    let mut game = if has_new_args {
        collect_args(j.pointer("/arguments/game"), &features, &vars)
    } else {
        j.get("minecraftArguments")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .split_whitespace()
            .map(|a| substitute(a, &vars))
            .collect()
    };
    if !has_new_args && !o.fullscreen {
        game.extend(["--width".into(), vars["resolution_width"].clone(), "--height".into(), vars["resolution_height"].clone()]);
    }
    if o.fullscreen {
        game.push("--fullscreen".into());
    }
    if let Some(sv) = &o.server {
        if !supports_quick_play {
            let (host, port) = split_server(sv);
            game.extend(["--server".into(), host, "--port".into(), port]);
        }
    }
    args.extend(game);
    drop_empty_pairs(args)
}

// ───────────────────────── One-call install ─────────────────────────

/// Installs the named version and its Java, returning what's needed to launch it.
pub fn install(paths: &Paths, spec: &str, report: Report) -> Result<(Resolved, Prepared, PathBuf), String> {
    let spec = spec.trim();
    let is_fabric = matches!(parse_spec(spec), Spec::Fabric(_) | Spec::NeoForge(_) | Spec::Forge(_));
    let installed = if is_fabric { version_json_in(&paths.game, spec).is_file() } else { find_version_json(paths, spec).is_some() };
    let id = if installed {
        spec.to_owned()
    } else {
        match parse_spec(spec) {
            Spec::Fabric(mc) => {
                report(&format!("Setting up Fabric for Minecraft {mc}"), -1.0);
                install_fabric_json(paths, &mc)?
            }
            // ── NeoForge / Forge (forge.rs) ──
            Spec::NeoForge(mc) => crate::forge::install_json(paths, crate::forge::Kind::NeoForge, &mc, report)?,
            Spec::Forge(mc) => crate::forge::install_json(paths, crate::forge::Kind::Forge, &mc, report)?,
            Spec::Id(id) => {
                if !safe_id(&id) {
                    return Err("That version name isn't valid.".into());
                }
                report(&format!("Getting Minecraft {id}"), -1.0);
                install_vanilla_json(paths, &id)?;
                id
            }
        }
    };
    let resolved = resolve(paths, &id)?;
    let prepared = ensure_files(paths, &resolved, report)?;
    let (component, _) = java_component(&resolved);
    let java = ensure_java(paths, &component, needs_intel_java(&resolved), report)?;
    Ok((resolved, prepared, java))
}

// ───────────────────────── Horizon ─────────────────────────

/// The Minecraft version the bundled preparer is pinned to (`--launch-api`).
/// What a Horizon preparer understands. Native-mod builds come from older Horizon commits
/// whose preparer has fewer options, so every option is checked against its own `--help`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareCaps {
    pub output: bool,
    pub java_home: bool,
    pub complete: bool,
    pub progress: bool,
    pub check: bool,
    pub launch_api: bool,
    pub sounds: bool,
}

impl PrepareCaps {
    pub fn from_help(help: &str) -> Self {
        let has = |f: &str| help.contains(f);
        PrepareCaps {
            output: has("--output"),
            java_home: has("--java-home"),
            complete: has("--complete"),
            progress: has("--progress"),
            check: has("--check"),
            launch_api: has("--launch-api"),
            sounds: has("horizon-prepare sounds"),
        }
    }
}

pub fn prepare_caps(prepare: &Path) -> PrepareCaps {
    let out = no_console(&mut Command::new(prepare)).arg("--help").stdin(Stdio::null()).output();
    out.map(|o| PrepareCaps::from_help(&format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))))
        .unwrap_or_default()
}

/// Marker Lumos writes once a preparer without `--check` finished.
const PREPARED_STAMP: &str = ".lumos-prepared";

pub fn horizon_minecraft(prepare: &Path, root: &Path) -> Result<String, String> {
    if !prepare_caps(prepare).launch_api {
        // Older preparers: the version is the one research-lock.json pins.
        let lock = read_json(&root.join("research-lock.json"))?;
        return s(lock.pointer("/minecraft/version")).ok_or_else(|| "research-lock.json names no Minecraft version.".to_owned());
    }
    let out = no_console(&mut Command::new(prepare))
        .arg("--launch-api")
        .arg("--root")
        .arg(root)
        .current_dir(root)
        .output()
        .map_err(|e| format!("Couldn't start the Horizon preparer: {e}"))?;
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|_| "The Horizon preparer gave an unreadable answer.".to_owned())?;
    s(v.get("minecraft")).ok_or_else(|| "The Horizon preparer didn't name its Minecraft version.".to_owned())
}

/// Copies the bundled Horizon support files (pins, exporter sources, compiled exporters)
/// into a writable root; skipped when the marker says they're current.
pub fn sync_horizon_root(bundle: &Path, dest: &Path) -> Result<(), String> {
    let lock = bundle.join("research-lock.json");
    if !lock.is_file() || !bundle.join("exporters/manifest.json").is_file() {
        return Err("This LumosClient build is missing Horizon's support files. Reinstall LumosClient.".into());
    }
    let stamp = format!(
        "{}|{}",
        net::sha1_file(&lock).unwrap_or_default(),
        net::sha1_file(&bundle.join("exporters/manifest.json")).unwrap_or_default()
    );
    let marker = dest.join(".lumos-sync");
    if fs::read_to_string(&marker).is_ok_and(|m| m == stamp) {
        return Ok(());
    }
    copy_tree(bundle, dest)?;
    fs::write(&marker, stamp).map_err(|e| format!("Couldn't write {}: {e}", marker.display()))
}

/// Keeps helper programs from opening a console window on Windows (no-op elsewhere).
pub fn no_console(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("Couldn't create {}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("Couldn't read {}: {e}", from.display()))?.flatten() {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else if net::sha1_file(&dst) != net::sha1_file(&src) {
            fs::copy(&src, &dst).map_err(|e| format!("Couldn't copy {}: {e}", src.display()))?;
        }
    }
    Ok(())
}

/// `horizon-prepare --check`: true when the prepared resources are complete and current.
pub fn horizon_check(prepare: &Path, root: &Path, output: &Path) -> bool {
    let caps = prepare_caps(prepare);
    if !caps.check {
        return output.join(PREPARED_STAMP).is_file();
    }
    if !caps.output {
        return no_console(&mut Command::new(prepare))
            .current_dir(root)
            .arg("--check")
            .arg("--root")
            .arg(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|st| st.success());
    }
    no_console(&mut Command::new(prepare))
        .current_dir(root)
        .arg("--check")
        .arg("--root")
        .arg(root)
        .arg("--output")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|st| st.success())
}

/// Where a preparer puts its resources: `wanted` when it takes `--output`, else its own
/// `<root>/.local/horizon-client/<version>`.
pub fn horizon_output(prepare: &Path, root: &Path, wanted: &Path) -> Result<PathBuf, String> {
    if prepare_caps(prepare).output {
        return Ok(wanted.to_path_buf());
    }
    Ok(root.join(".local").join("horizon-client").join(horizon_minecraft(prepare, root)?))
}

/// Downloads Minecraft's files for the pinned version, its Java and the server jar into our
/// own folder, then runs the preparer (with machine-readable progress when it has it).
/// Returns the resources folder to pass to the game.
pub fn ensure_horizon(
    paths: &Paths,
    prepare: &Path,
    root: &Path,
    output: &Path,
    report: Report,
    log: &(dyn Fn(&str) + Sync),
) -> Result<PathBuf, String> {
    let caps = prepare_caps(prepare);
    let output = &horizon_output(prepare, root, output)?;
    if horizon_check(prepare, root, output) {
        return Ok(output.clone());
    }
    let mc = horizon_minecraft(prepare, root)?;
    // Only our own folder: the preparer is pointed at it with --minecraft-dir.
    let own = paths.own_only();
    report(&format!("Getting Minecraft {mc} for Horizon"), -1.0);
    let (resolved, _prep, java) = install(&own, &mc, report)?;
    let server = resolved.json.pointer("/downloads/server").ok_or("Mojang lists no server jar for this version.")?;
    let server_jar = paths.cache.join("horizon").join(format!("server-{mc}.jar"));
    report("Downloading the server files Horizon needs", -1.0);
    net::download_verified(
        server.get("url").and_then(Value::as_str).unwrap_or_default(),
        &server_jar,
        server.get("sha1").and_then(Value::as_str),
        server.get("size").and_then(Value::as_u64),
    )?;
    let java_home = java_home_of(&java);
    report("Preparing Horizon (the first time takes a few minutes)", 0.0);
    let mut cmd = Command::new(prepare);
    no_console(&mut cmd)
        .arg("--prepare")
        .arg("--server-jar")
        .arg(&server_jar)
        .arg("--minecraft-dir")
        .arg(&own.game)
        .arg("--root")
        .arg(root);
    if caps.output {
        cmd.arg("--output").arg(output);
    }
    if caps.java_home {
        cmd.arg("--java-home").arg(&java_home);
    }
    if caps.complete {
        cmd.arg("--complete");
    }
    if caps.progress {
        cmd.arg("--progress").arg("json");
    }
    let mut child = cmd
        .env("JAVA_HOME", &java_home)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start the Horizon preparer: {e}"))?;
    let err_tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
    let err_thread = child.stderr.take().map(|e| {
        let tail = err_tail.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(e).lines().map_while(Result::ok) {
                let mut t = tail.lock().unwrap();
                t.push_back(line);
                if t.len() > 20 {
                    t.pop_front();
                }
            }
        })
    });
    let mut last_error: Option<String> = None;
    let mut message = String::from("Preparing Horizon");
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            match serde_json::from_str::<Value>(&line) {
                Ok(v) => {
                    if let Some(m) = v.get("message").and_then(Value::as_str) {
                        log(&format!("[Horizon] {m}"));
                        message = m.chars().take(90).collect();
                        report(&message, -1.0);
                    }
                    if let Some(f) = v.get("fraction").and_then(Value::as_f64) {
                        report(&message, f.clamp(0.0, 1.0));
                    }
                    if let Some(e) = v.get("error").and_then(Value::as_str) {
                        last_error = Some(e.to_owned());
                    }
                }
                Err(_) => log(&format!("[Horizon] {line}")),
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if let Some(t) = err_thread {
        let _ = t.join();
    }
    for l in err_tail.lock().unwrap().iter() {
        log(&format!("[Horizon] {l}"));
    }
    if !status.success() {
        let detail = last_error
            .or_else(|| err_tail.lock().unwrap().back().cloned())
            .unwrap_or_else(|| format!("exit status {status}"));
        return Err(format!("Horizon couldn't prepare its files: {detail}"));
    }
    // Preparers without --complete copy the sounds with their own command (optional: the game
    // only goes silent without them).
    if !caps.complete && caps.sounds {
        report("Copying Horizon's sounds", -1.0);
        let _ = no_console(&mut Command::new(prepare))
            .arg("sounds")
            .arg("--minecraft-dir")
            .arg(&own.game)
            .arg("--root")
            .arg(root)
            .env("JAVA_HOME", &java_home)
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    if !caps.check {
        let _ = fs::create_dir_all(output);
        let _ = fs::write(output.join(PREPARED_STAMP), mc.as_bytes());
    }
    Ok(output.clone())
}

// ───────────────────────── Tests ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preparer_options_are_read_from_its_help() {
        let old = "Usage: horizon-prepare [--prepare] [--server-jar PATH] [--minecraft-dir PATH] [--root PATH]\n       horizon-prepare sounds [--minecraft-dir PATH] [--root PATH]";
        let c = PrepareCaps::from_help(old);
        assert!(!c.output && !c.java_home && !c.complete && !c.progress && !c.check && !c.launch_api && c.sounds);
        let new = "[--output PATH] [--java-home PATH] [--complete] [--progress json] --check --launch-api horizon-prepare sounds";
        let c = PrepareCaps::from_help(new);
        assert!(c.output && c.java_home && c.complete && c.progress && c.check && c.launch_api);
    }

    #[test]
    fn maven_paths() {
        assert_eq!(maven_path("org.ow2.asm:asm:9.10.1").unwrap(), "org/ow2/asm/asm/9.10.1/asm-9.10.1.jar");
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.3:natives-macos-arm64").unwrap(),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-macos-arm64.jar"
        );
        assert_eq!(maven_path("a.b:c:1@zip").unwrap(), "a/b/c/1/c-1.zip");
        assert!(maven_path("broken").is_none());
    }

    #[test]
    fn library_keys_ignore_version_keep_classifier() {
        assert_eq!(library_key("org.ow2.asm:asm:9.10.1"), "org.ow2.asm:asm");
        assert_eq!(library_key("org.lwjgl:lwjgl:3.3.3:natives-macos"), "org.lwjgl:lwjgl:natives-macos");
    }

    #[test]
    fn rules_follow_last_match() {
        let f = Features::default();
        assert!(rules_allow(None, &f));
        assert!(rules_allow(Some(&json!([{"action":"allow","os":{"name": os_name()}}])), &f));
        assert!(!rules_allow(Some(&json!([{"action":"allow","os":{"name":"nowhere"}}])), &f));
        assert!(!rules_allow(Some(&json!([{"action":"allow"},{"action":"disallow","os":{"name": os_name()}}])), &f));
        let feat = json!([{"action":"allow","features":{"has_custom_resolution":true}}]);
        assert!(!rules_allow(Some(&feat), &f));
        assert!(rules_allow(Some(&feat), &Features { custom_resolution: true, ..Default::default() }));
        assert!(!rules_allow(Some(&json!([{"action":"allow","features":{"is_demo_user":true}}])), &f));
    }

    #[test]
    fn specs_and_ids() {
        assert_eq!(parse_spec("Fabric 1.21.11"), Spec::Fabric("1.21.11".into()));
        assert_eq!(parse_spec("1.21.11"), Spec::Id("1.21.11".into()));
        assert_eq!(parse_spec("Forge 1.8.9"), Spec::Id("Forge 1.8.9".into()));
        assert_eq!(parse_spec("Forge 1.20.1"), Spec::Forge("1.20.1".into()));
        assert_eq!(parse_spec("NeoForge 1.21.1"), Spec::NeoForge("1.21.1".into()));
        assert!(!safe_id("../x"));
        assert!(!safe_id(".."));
        assert!(safe_id("Fabric 1.21.11"));
        assert!(fabric_supports("1.21.11") && fabric_supports("26.3") && fabric_supports("1.14"));
        assert!(!fabric_supports("1.8.9") && !fabric_supports("1.13.2"));
    }

    #[test]
    fn substitution_and_empty_pairs() {
        let mut vars = HashMap::new();
        vars.insert("a", "1".to_owned());
        assert_eq!(substitute("x${a}y${missing}", &vars), "x1y${missing}");
        let args = vec!["--clientId".into(), String::new(), "--xuid".into(), "${auth_xuid}".into(), "--ok".into(), "v".into(), "--flag".into()];
        assert_eq!(drop_empty_pairs(args), vec!["--ok", "v", "--flag"]);
    }

    #[test]
    fn merge_child_over_parent() {
        let parent = json!({"id":"1.21.11","mainClass":"P","libraries":[{"name":"org.ow2.asm:asm:9.0"},{"name":"x:y:1"}],
            "arguments":{"game":["--a"],"jvm":["-Dp"]},"downloads":{"client":{}}});
        let child = json!({"id":"Fabric 1.21.11","inheritsFrom":"1.21.11","mainClass":"C",
            "libraries":[{"name":"org.ow2.asm:asm:9.10.1"}],"arguments":{"game":[],"jvm":["-Dc"]}});
        let m = merge(&child, &parent);
        assert_eq!(m["mainClass"], "C");
        // vanilla's own duplicates (e.g. LWJGL listed twice) survive the merge
        let dup_parent = json!({"libraries":[{"name":"org.lwjgl:lwjgl:3.2.1"},{"name":"org.lwjgl:lwjgl:3.2.1","natives":{}}]});
        let dup = merge(&json!({"libraries":[]}), &dup_parent);
        assert_eq!(dup["libraries"].as_array().unwrap().len(), 2);
        let libs: Vec<&str> = m["libraries"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap()).collect();
        assert_eq!(libs, vec!["org.ow2.asm:asm:9.10.1", "x:y:1"]);
        assert_eq!(m["arguments"]["jvm"], json!(["-Dp", "-Dc"]));
        assert!(m.get("inheritsFrom").is_none());
        assert!(m.pointer("/downloads/client").is_some());
    }

    #[test]
    fn path_safety() {
        assert!(safe_rel("org/ow2/asm/asm/9.0/asm-9.0.jar"));
        assert!(!safe_rel("../evil.jar"));
        assert!(!safe_rel("/etc/passwd"));
        assert!(!safe_rel("a/../../b"));
        assert!(!safe_rel(""));
        let base = Path::new("/r/java");
        assert!(stays_within(base, Path::new("/r/java/legal/java.compiler/../java.base/LICENSE")));
        assert!(!stays_within(base, Path::new("/r/java/legal/../../../etc/x")));
    }

    #[test]
    fn server_addresses() {
        assert_eq!(split_server("play.example.net"), ("play.example.net".into(), "25565".into()));
        assert_eq!(split_server("1.2.3.4:25570"), ("1.2.3.4".into(), "25570".into()));
        assert_eq!(split_server("[::1]:25566"), ("::1".into(), "25566".into()));
        assert_eq!(split_server("::1"), ("::1".into(), "25565".into()));
    }

    fn prep(cp: Vec<&str>) -> Prepared {
        Prepared {
            classpath: cp.into_iter().map(PathBuf::from).collect(),
            natives_dir: PathBuf::from("/n"),
            assets_root: PathBuf::from("/as"),
            asset_index: "29".into(),
            game_assets: PathBuf::from("/as"),
            logging_arg: None,
        }
    }

    #[test]
    fn modern_args_build() {
        let r = Resolved {
            id: "1.21.11".into(),
            jar_id: "1.21.11".into(),
            json: json!({
                "mainClass": "net.minecraft.client.main.Main",
                "arguments": {
                    "game": ["--username","${auth_player_name}","--clientId","${clientid}","--xuid","${auth_xuid}",
                        {"rules":[{"action":"allow","features":{"has_custom_resolution":true}}],"value":["--width","${resolution_width}","--height","${resolution_height}"]},
                        {"rules":[{"action":"allow","features":{"is_quick_play_multiplayer":true}}],"value":["--quickPlayMultiplayer","${quickPlayMultiplayer}"]},
                        {"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"--demo"}],
                    "jvm": [{"rules":[{"action":"allow","os":{"name":"osx"}}],"value":["-XstartOnFirstThread"]},
                        "-Djava.library.path=${natives_directory}","-cp","${classpath}"]
                }
            }),
        };
        let auth = Auth { name: "Steve".into(), uuid: "0".repeat(32), access_token: "t".into(), user_type: "legacy".into(), javaagent: None };
        let o = LaunchOpts { memory_mb: 4096, extra_jvm: vec![], width: 1280, height: 720, fullscreen: false, server: Some("mc.example.net".into()), launcher_version: "0.3.0".into() };
        let a = build_args(&r, &prep(vec!["/l/a.jar", "/v/c.jar"]), Path::new("/g"), &auth, &o);
        let joined = a.join(" ");
        assert!(joined.contains("-Xmx4096M"));
        assert!(joined.contains(&format!("-cp /l/a.jar{}/v/c.jar", classpath_separator())));
        assert!(joined.contains("--username Steve"));
        assert!(joined.contains("--width 1280 --height 720"));
        assert!(joined.contains("--quickPlayMultiplayer mc.example.net"));
        assert!(!joined.contains("--clientId") && !joined.contains("--xuid") && !joined.contains("--demo"));
        assert_eq!(joined.contains("-XstartOnFirstThread"), cfg!(target_os = "macos"));
        let main_pos = a.iter().position(|x| x == "net.minecraft.client.main.Main").unwrap();
        assert!(a.iter().position(|x| x == "-cp").unwrap() < main_pos);
    }

    #[test]
    fn legacy_args_build() {
        let r = Resolved {
            id: "1.8.9".into(),
            jar_id: "1.8.9".into(),
            json: json!({"mainClass":"net.minecraft.client.main.Main",
                "minecraftArguments":"--username ${auth_player_name} --userProperties ${user_properties} --userType ${user_type}"}),
        };
        let mut p = prep(vec!["/c.jar"]);
        p.logging_arg = Some("-Dlog4j.configurationFile=/x.xml".into());
        let auth = Auth { name: "Alex".into(), uuid: "1".repeat(32), access_token: "0".into(), user_type: "legacy".into(), javaagent: Some("-javaagent:/a.jar=ely.by".into()) };
        let o = LaunchOpts { memory_mb: 2048, extra_jvm: vec!["-XX:+UseG1GC".into()], width: 854, height: 480, fullscreen: false, server: Some("1.2.3.4:25570".into()), launcher_version: "0.3.0".into() };
        let a = build_args(&r, &p, Path::new("/g"), &auth, &o);
        assert_eq!(a[0], "-javaagent:/a.jar=ely.by");
        let joined = a.join(" ");
        assert!(joined.contains("-XX:+UseG1GC"));
        assert!(joined.contains("-Dlog4j.configurationFile=/x.xml"));
        assert!(joined.contains("-Djava.library.path=/n"));
        assert!(joined.contains("--userProperties {}"));
        assert!(joined.contains("--server 1.2.3.4 --port 25570"));
        assert!(joined.contains("--width 854 --height 480"));
    }

    #[test]
    fn catalog_lists_fabric_and_vanilla() {
        let root = std::env::temp_dir().join(format!("lumos-cat-{}", std::process::id()));
        let paths = Paths::new(&root, None);
        let m = json!({"versions":[{"id":"26.3","type":"release"},{"id":"26.4-snapshot-8","type":"snapshot"},{"id":"1.8.9","type":"release"}]});
        let c = catalog(&paths, Some(&m));
        let ids: Vec<&str> = c.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["Fabric 26.3", "26.3", "1.8.9"]);
        let _ = fs::remove_dir_all(&root);
    }

    /// Live: the whole first-run Horizon setup from a built app's bundled support files
    /// (LUMOS_BUNDLE=<.../Lumos.app/Contents/Resources>). Large download. Run with --ignored.
    #[test]
    #[ignore]
    fn live_horizon_setup() {
        let bundle = PathBuf::from(std::env::var("LUMOS_BUNDLE").expect("set LUMOS_BUNDLE"));
        let root = std::env::temp_dir().join("lumos-live-horizon");
        let paths = Paths::new(&root, None);
        let hroot = paths.horizon_root();
        sync_horizon_root(&bundle.join("horizon-root"), &hroot).expect("sync");
        let prepare = bundle.join("bin/horizon-prepare");
        let output = root.join("resources/26.3");
        let report = |m: &str, f: f64| eprintln!("[{f:.2}] {m}");
        let log = |l: &str| eprintln!("{l}");
        ensure_horizon(&paths, &prepare, &hroot, &output, &report, &log).expect("horizon setup");
        assert!(horizon_check(&prepare, &hroot, &output), "resources incomplete after setup");
    }

    /// Live: starts the game installed by `live_install_fabric` for ~25 s and checks it boots
    /// (Fabric loads, the window opens) without crashing. Run with --ignored after the install test.
    #[test]
    #[ignore]
    fn live_launch_fabric() {
        let root = std::env::temp_dir().join("lumos-live-install");
        let paths = Paths::new(&root, None);
        let report = |_: &str, _: f64| {};
        let (r, prep, java) = install(&paths, "Fabric 1.21.11", &report).expect("install");
        let game_dir = root.join("instance");
        fs::create_dir_all(&game_dir).unwrap();
        let auth = Auth { name: "LumosTest".into(), uuid: "0123456789abcdef0123456789abcdef".into(), access_token: "0".into(), user_type: "legacy".into(), javaagent: None };
        let o = LaunchOpts { memory_mb: 2048, extra_jvm: vec![], width: 854, height: 480, fullscreen: false, server: None, launcher_version: "test".into() };
        let args = build_args(&r, &prep, &game_dir, &auth, &o);
        let log_path = root.join("launch.log");
        let log = fs::File::create(&log_path).unwrap();
        let mut child = Command::new(&java).args(&args).current_dir(&game_dir)
            .stdout(log.try_clone().unwrap()).stderr(log).spawn().expect("spawn");
        let started = Instant::now();
        let mut exited = None;
        while started.elapsed() < Duration::from_secs(25) {
            if let Some(st) = child.try_wait().unwrap() { exited = Some(st); break; }
            std::thread::sleep(Duration::from_millis(500));
        }
        let _ = child.kill();
        let _ = child.wait();
        let out = fs::read_to_string(&log_path).unwrap_or_default();
        eprintln!("---- game log (tail) ----\n{}", out.lines().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
        assert!(exited.is_none(), "game exited early: {exited:?}");
        assert!(out.contains("Loading Minecraft") || out.contains("Fabric Loader") || out.contains("Setting user"), "no boot markers in log");
    }

    /// Live: plays a copy of a world with only LumosHUD installed and checks the HUD hooked in and
    /// drew. Needs LUMOS_TEST_WORLD (a Fabric 1.21.11 world folder) and LUMOS_HUD_JAR. Run with --ignored.
    #[test]
    #[ignore]
    fn live_lumoshud_in_world() {
        let world = PathBuf::from(std::env::var("LUMOS_TEST_WORLD").expect("set LUMOS_TEST_WORLD"));
        let hud = PathBuf::from(std::env::var("LUMOS_HUD_JAR").expect("set LUMOS_HUD_JAR"));
        let root = std::env::temp_dir().join("lumos-live-install");
        let paths = Paths::new(&root, None);
        let (r, prep, java) = install(&paths, "Fabric 1.21.11", &|_: &str, _: f64| {}).expect("install");
        let game_dir = root.join("hud-instance");
        let _ = fs::remove_dir_all(&game_dir);
        fs::create_dir_all(game_dir.join("mods")).unwrap();
        fs::copy(&hud, game_dir.join("mods/lumoshud-1.21.11.jar")).unwrap();
        let status = Command::new("cp").arg("-R").arg(&world).arg(game_dir.join("saves/HudTest")).status();
        if status.map(|s| !s.success()).unwrap_or(true) {
            fs::create_dir_all(game_dir.join("saves")).unwrap();
            assert!(Command::new("cp").arg("-R").arg(&world).arg(game_dir.join("saves/HudTest")).status().unwrap().success());
        }
        let _ = fs::remove_file(game_dir.join("saves/HudTest/session.lock"));
        let auth = Auth { name: "LumosTest".into(), uuid: "0123456789abcdef0123456789abcdef".into(), access_token: "0".into(), user_type: "legacy".into(), javaagent: None };
        let o = LaunchOpts { memory_mb: 3072, extra_jvm: vec![], width: 854, height: 480, fullscreen: false, server: None, launcher_version: "test".into() };
        let mut args = build_args(&r, &prep, &game_dir, &auth, &o);
        args.extend(["--quickPlaySingleplayer".to_owned(), "HudTest".to_owned()]);
        let log_path = root.join("hud.log");
        let log = fs::File::create(&log_path).unwrap();
        let mut child = Command::new(&java).args(&args).current_dir(&game_dir)
            .stdout(log.try_clone().unwrap()).stderr(log).spawn().expect("spawn");
        let started = Instant::now();
        let mut out = String::new();
        while started.elapsed() < Duration::from_secs(150) {
            out = fs::read_to_string(&log_path).unwrap_or_default();
            if out.contains("[LumosHUD] Ready") || out.contains("[LumosHUD] Can't") || out.contains("[LumosHUD] Drawing failed") {
                std::thread::sleep(Duration::from_secs(4));
                out = fs::read_to_string(&log_path).unwrap_or_default();
                break;
            }
            if child.try_wait().unwrap().is_some() { break; }
            std::thread::sleep(Duration::from_millis(500));
        }
        let _ = child.kill();
        let _ = child.wait();
        for l in out.lines().filter(|l| l.contains("LumosHUD") || l.contains("lumoshud") || l.contains("Mixin") && l.contains("WARN")) {
            eprintln!("{l}");
        }
        assert!(!out.contains("was not found lumoshud"), "mixin target missing");
        assert!(out.contains("[LumosHUD] Ready"), "HUD never drew");
        assert!(!out.contains("[LumosHUD] Drawing failed") && !out.contains("[LumosHUD] Can't"), "HUD error");
    }

    /// Live: installs vanilla 1.21.11 + Fabric + Java into a temp root (large download). Run with --ignored.
    #[test]
    #[ignore]
    fn live_install_fabric() {
        let root = std::env::temp_dir().join("lumos-live-install");
        let paths = Paths::new(&root, None);
        let report = |m: &str, f: f64| eprintln!("{m} {f:.2}");
        let (r, prep, java) = install(&paths, "Fabric 1.21.11", &report).expect("install");
        assert!(java.is_file());
        assert!(prep.classpath.iter().all(|p| p.is_file()), "missing classpath entries");
        assert_eq!(r.jar_id, "1.21.11");
        assert_eq!(r.json["mainClass"], "net.fabricmc.loader.impl.launch.knot.KnotClient");
        let out = Command::new(&java).arg("-version").output().unwrap();
        assert!(out.status.success());
    }
}
