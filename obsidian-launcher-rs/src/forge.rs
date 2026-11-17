//! NeoForge and Forge.
//!
//! Which Minecraft versions each loader supports comes from NeoForge's Maven metadata and
//! Forge's promotions list (cached in `<root>/cache`). An install runs the loader's official
//! installer jar headlessly (`--installClient <game folder>`) with the Mojang Java runtime the
//! Minecraft version uses, into LumosClient's own game folder (never the official launcher's).
//! The installer's version JSON is then saved under LumosClient's own id ("NeoForge 1.21.1" /
//! "Forge 1.20.1") so the normal resolve/launch path runs it:
//!   * `downloads` is copied from the vanilla version, so the client jar lives at
//!     `versions/<id>/<id>.jar`. The loaders' `-DignoreList=…,${version_name}.jar` then keeps
//!     the unpatched jar off the module layer, exactly like the official launcher's copy.
//!   * `lumosLoader` records the exact loader version that was installed.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

use crate::installer::{self, Paths, Report};
use crate::net;

pub const NEOFORGE_PREFIX: &str = "NeoForge ";
pub const FORGE_PREFIX: &str = "Forge ";
const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const FORGE_PROMOS: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const NEOFORGE_CACHE: &str = "neoforge-maven-metadata.xml";
const FORGE_CACHE: &str = "forge-promotions.json";
/// How long the loader lists are trusted before asking again.
const INDEX_TTL: Duration = Duration::from_secs(3 * 3600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    NeoForge,
    Forge,
}

impl Kind {
    pub fn prefix(self) -> &'static str {
        match self {
            Kind::NeoForge => NEOFORGE_PREFIX,
            Kind::Forge => FORGE_PREFIX,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::NeoForge => "NeoForge",
            Kind::Forge => "Forge",
        }
    }
    /// Modrinth's loader key (also the catalog `kind`).
    pub fn key(self) -> &'static str {
        match self {
            Kind::NeoForge => "neoforge",
            Kind::Forge => "forge",
        }
    }
    /// (installer jar URL, file name).
    fn installer_url(self, mc: &str, loader: &str) -> (String, String) {
        match self {
            Kind::NeoForge => {
                let file = format!("neoforge-{loader}-installer.jar");
                (format!("{NEOFORGE_MAVEN}/{loader}/{file}"), file)
            }
            Kind::Forge => {
                let file = format!("forge-{mc}-{loader}-installer.jar");
                (format!("{FORGE_MAVEN}/{mc}-{loader}/{file}"), file)
            }
        }
    }
}

/// "NeoForge 1.21.1" → (NeoForge, "1.21.1"); "Forge 1.20.1" → (Forge, "1.20.1").
pub fn split_id(id: &str) -> Option<(Kind, &str)> {
    let t = id.trim();
    for kind in [Kind::NeoForge, Kind::Forge] {
        if let Some(mc) = t.strip_prefix(kind.prefix()).map(str::trim).filter(|m| !m.is_empty()) {
            return Some((kind, mc));
        }
    }
    None
}

// ───────────────────────── Loader versions ─────────────────────────

/// Minecraft version → loader version to install, per loader.
#[derive(Clone, Debug, Default)]
pub struct LoaderIndex {
    pub neoforge: HashMap<String, String>,
    pub forge: HashMap<String, String>,
}

impl LoaderIndex {
    pub fn get(&self, kind: Kind, mc: &str) -> Option<&String> {
        match kind {
            Kind::NeoForge => self.neoforge.get(mc),
            Kind::Forge => self.forge.get(mc),
        }
    }
}

fn numbers(v: &str) -> Option<Vec<u64>> {
    v.split('.').map(|p| p.parse::<u64>().ok()).collect()
}

/// The Minecraft version a NeoForge version is for, and whether it is a stable build.
/// 21.1.77 → 1.21.1, 21.0.10 → 1.21, 20.4.237 → 1.20.4, 21.1.5-beta → (1.21.1, beta),
/// 26.1.2.127 → 26.1.2, 26.3.0.110 → 26.3. Alphas, snapshots and April-fools builds → None.
pub fn neoforge_mc(version: &str) -> Option<(String, bool)> {
    let (core, stable) = match version.split_once('-') {
        None if !version.contains('+') => (version, true),
        Some((core, "beta")) => (core, false),
        _ => return None,
    };
    let n = numbers(core)?;
    match n.as_slice() {
        [major, minor, patch, _build] if *major >= 26 => {
            Some((if *patch == 0 { format!("{major}.{minor}") } else { format!("{major}.{minor}.{patch}") }, stable))
        }
        [major, minor, _build] if (20..26).contains(major) => {
            Some((if *minor == 0 { format!("1.{major}") } else { format!("1.{major}.{minor}") }, stable))
        }
        _ => None,
    }
}

/// Newest stable NeoForge per Minecraft version (newest beta where there's no stable one).
pub fn parse_neoforge_metadata(xml: &str) -> HashMap<String, String> {
    let mut best: HashMap<String, (bool, Vec<u64>, String)> = HashMap::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        rest = &rest[start + 9..];
        let Some(end) = rest.find("</version>") else { break };
        let v = rest[..end].trim();
        rest = &rest[end..];
        let Some((mc, stable)) = neoforge_mc(v) else { continue };
        let Some(nums) = numbers(v.split('-').next().unwrap_or(v)) else { continue };
        let better = match best.get(&mc) {
            None => true,
            Some((s, n, _)) => (stable, &nums) > (*s, n),
        };
        if better {
            best.insert(mc, (stable, nums, v.to_owned()));
        }
    }
    best.into_iter().map(|(mc, (_, _, v))| (mc, v)).collect()
}

/// Forge supports 1.12.2 and every version from 1.13 with installers that install headlessly.
fn forge_mc_supported(mc: &str) -> bool {
    let Some(n) = numbers(mc) else { return false };
    match n.as_slice() {
        [1, 12, 2] => true,
        [1, minor, ..] => *minor >= 13,
        [major, ..] => *major >= 26,
        _ => false,
    }
}

/// NeoForge exists for 1.20.2 and newer (1.20.1's "NeoForge" is a Forge fork, not offered).
fn neoforge_mc_supported(mc: &str) -> bool {
    let Some(n) = numbers(mc) else { return false };
    match n.as_slice() {
        [1, 20, patch] => *patch >= 2,
        [1, minor, ..] => *minor >= 21,
        [major, ..] => *major >= 26,
        _ => false,
    }
}

/// Whether "<Loader> <mc>" names a version LumosClient can install (other names stay plain ids).
pub fn spec_supported(kind: Kind, mc: &str) -> bool {
    match kind {
        Kind::NeoForge => neoforge_mc_supported(mc),
        Kind::Forge => forge_mc_supported(mc),
    }
}

/// Recommended Forge per Minecraft version (latest where none is recommended).
pub fn parse_forge_promos(v: &Value) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let Some(promos) = v.get("promos").and_then(Value::as_object) else { return out };
    for (key, ver) in promos {
        let Some(ver) = ver.as_str().filter(|s| numbers(s).is_some()) else { continue };
        if let Some(mc) = key.strip_suffix("-recommended").filter(|m| forge_mc_supported(m)) {
            out.insert(mc.to_owned(), ver.to_owned());
        }
    }
    for (key, ver) in promos {
        let Some(ver) = ver.as_str().filter(|s| numbers(s).is_some()) else { continue };
        if let Some(mc) = key.strip_suffix("-latest").filter(|m| forge_mc_supported(m)) {
            out.entry(mc.to_owned()).or_insert_with(|| ver.to_owned());
        }
    }
    out
}

fn fresh(p: &Path) -> bool {
    fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < INDEX_TTL)
}

/// The loader lists from the cache only (no network): for building the catalog quickly.
pub fn cached_index(paths: &Paths) -> LoaderIndex {
    let neoforge = fs::read_to_string(paths.cache.join(NEOFORGE_CACHE)).map(|x| parse_neoforge_metadata(&x)).unwrap_or_default();
    let forge = fs::read_to_string(paths.cache.join(FORGE_CACHE))
        .ok()
        .and_then(|x| serde_json::from_str::<Value>(&x).ok())
        .map(|v| parse_forge_promos(&v))
        .unwrap_or_default();
    LoaderIndex { neoforge, forge }
}

/// Refreshes the cached loader lists when they're older than a few hours (offline: keeps the
/// old copies), then returns them.
pub fn refresh_index(paths: &Paths) -> LoaderIndex {
    let neo = paths.cache.join(NEOFORGE_CACHE);
    if !fresh(&neo) {
        if let Ok(bytes) = net::get_bytes(&format!("{NEOFORGE_MAVEN}/maven-metadata.xml"), 8 << 20) {
            if String::from_utf8_lossy(&bytes).contains("<versions>") {
                let _ = write_atomic(&neo, &bytes);
            }
        }
    }
    let forge = paths.cache.join(FORGE_CACHE);
    if !fresh(&forge) {
        if let Ok(v) = net::get_json(FORGE_PROMOS) {
            if v.get("promos").is_some() {
                let _ = write_atomic(&forge, v.to_string().as_bytes());
            }
        }
    }
    cached_index(paths)
}

fn write_atomic(p: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("part");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, p).map_err(|e| e.to_string())
}

/// Catalog entries (id, kind) this Minecraft release gets for the loaders that support it.
pub fn catalog_variants(idx: &LoaderIndex, mc: &str) -> Vec<(String, &'static str)> {
    [Kind::NeoForge, Kind::Forge]
        .into_iter()
        .filter(|k| idx.get(*k, mc).is_some())
        .map(|k| (format!("{}{mc}", k.prefix()), k.key()))
        .collect()
}

// ───────────────────────── Install ─────────────────────────

/// The version id the installer will write (`install_profile.json` → `version`).
fn installer_version_id(jar: &Path) -> Result<String, String> {
    let file = fs::File::open(jar).map_err(|e| format!("Couldn't open {}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("The installer is damaged: {e}"))?;
    let mut raw = String::new();
    zip.by_name("install_profile.json")
        .map_err(|_| "The installer has no install profile.".to_owned())?
        .read_to_string(&mut raw)
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("The installer's profile is damaged: {e}"))?;
    // New installers: "version"; the 1.12.2-era format nests it under versionInfo.id.
    v.get("version")
        .and_then(Value::as_str)
        .or_else(|| v.pointer("/versionInfo/id").and_then(Value::as_str))
        .map(str::to_owned)
        .filter(|id| installer::safe_id(id))
        .ok_or_else(|| "The installer doesn't say which version it installs.".to_owned())
}

/// The installers refuse to run without a launcher profile file in the target folder.
fn ensure_launcher_profiles(game: &Path) -> Result<(), String> {
    let p = game.join("launcher_profiles.json");
    let ok = fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v.is_object());
    if ok {
        return Ok(());
    }
    write_atomic(&p, json!({"profiles": {}, "version": 3}).to_string().as_bytes())
}

/// Runs the installer jar; returns its output tail on failure.
fn run_installer(java: &Path, jar: &Path, game: &Path, work: &Path, label: &str, report: Report) -> Result<(), String> {
    fs::create_dir_all(work).map_err(|e| format!("Couldn't create {}: {e}", work.display()))?;
    let mut cmd = Command::new(java);
    cmd.arg("-Djava.awt.headless=true")
        .arg("-jar")
        .arg(jar)
        .arg("--installClient")
        .arg(game)
        .current_dir(work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    installer::no_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start the installer: {e}"))?;
    let stderr = child.stderr.take();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut e) = stderr {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let mut tail: Vec<String> = Vec::new();
    let mut log = String::new();
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            log.push_str(&line);
            log.push('\n');
            let l = line.trim();
            if l.starts_with("Processor:") || l.starts_with("Downloading") || l.starts_with("Extracting") || l.starts_with("Considering") {
                report(label, -1.0);
            }
            tail.push(line);
            if tail.len() > 12 {
                tail.remove(0);
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let err = err_thread.join().unwrap_or_default();
    log.push_str(&err);
    let _ = fs::write(work.join("last-install.log"), &log);
    if status.success() && !log.contains("There was an error during installation") {
        return Ok(());
    }
    let mut detail: Vec<&str> = tail.iter().map(String::as_str).filter(|l| !l.trim().is_empty()).collect();
    detail.extend(err.lines().filter(|l| !l.trim().is_empty()).take(4));
    Err(format!("The installer stopped ({status}). Last lines:\n{}", detail.join("\n")))
}

fn copy_if_missing(from: &Path, to: &Path, size: Option<u64>) {
    let ok = |p: &Path| fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0 && size.is_none_or(|s| m.len() == s));
    if ok(to) || !ok(from) {
        return;
    }
    if let Some(parent) = to.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = to.with_extension("jar.part");
    if fs::copy(from, &tmp).is_ok() {
        let _ = fs::rename(&tmp, to);
    }
}

/// Installs the recommended NeoForge/Forge for `mc` (unless LumosClient already has it) and
/// returns the version id to launch.
pub fn install_json(paths: &Paths, kind: Kind, mc: &str, report: Report) -> Result<String, String> {
    let id = format!("{}{mc}", kind.prefix());
    if !installer::safe_id(&id) {
        return Err(format!("{} isn't available for Minecraft {mc}.", kind.name()));
    }
    let ours = installer::version_json_in(&paths.game, &id);
    if ours.is_file() && installer::read_json(&ours).is_ok() {
        return Ok(id);
    }
    report(&format!("Finding {} for Minecraft {mc}", kind.name()), -1.0);
    let idx = refresh_index(paths);
    let loader = idx
        .get(kind, mc)
        .cloned()
        .ok_or_else(|| format!("{} isn't available for Minecraft {mc} (or its download list couldn't be reached).", kind.name()))?;

    // Vanilla first: its Java runs the installer, and its jar becomes this version's jar.
    let vanilla_path = installer::install_vanilla_json(paths, mc)?;
    let vanilla_json = installer::read_json(&vanilla_path)?;
    let vanilla = installer::resolve(paths, mc)?;
    let (component, _) = installer::java_component(&vanilla);
    let java = installer::ensure_java(paths, &component, installer::needs_intel_java(&vanilla), report)?;

    let (url, file) = kind.installer_url(mc, &loader);
    let jar = paths.cache.join("loader-installers").join(&file);
    let sha1 = net::get_bytes(&format!("{url}.sha1"), 256)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).trim().chars().take(40).collect::<String>())
        .filter(|h| h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit()));
    report(&format!("Downloading {} {loader}", kind.name()), -1.0);
    net::download_verified(&url, &jar, sha1.as_deref(), None)?;
    let installer_id = installer_version_id(&jar)?;

    ensure_launcher_profiles(&paths.game)?;
    let label = format!("Installing {} {loader} (takes a minute)", kind.name());
    report(&label, -1.0);
    run_installer(&java, &jar, &paths.game, &paths.cache.join("loader-installers"), &label, report)?;

    let produced = installer::version_json_in(&paths.game, &installer_id);
    let mut v = installer::read_json(&produced).map_err(|_| format!("The {} installer finished but wrote no version.", kind.name()))?;
    let parent = v.get("inheritsFrom").and_then(Value::as_str).unwrap_or(mc).to_owned();
    v["id"] = Value::String(id.clone());
    v["inheritsFrom"] = Value::String(parent);
    if let Some(dl) = vanilla_json.get("downloads") {
        v["downloads"] = dl.clone();
    }
    v["lumosLoader"] = json!({"type": kind.key(), "version": loader, "installerId": installer_id});
    // The client jar under this id (see the module docs); copied so it isn't downloaded twice.
    let size = vanilla_json.pointer("/downloads/client/size").and_then(Value::as_u64);
    let dest_jar = paths.game.join("versions").join(&id).join(format!("{id}.jar"));
    for base in [Some(paths.game.clone()), paths.official.clone()].into_iter().flatten() {
        copy_if_missing(&base.join("versions").join(mc).join(format!("{mc}.jar")), &dest_jar, size);
    }
    installer::write_json_atomic(&ours, &v)?;
    // The installer's own copy would show up as a second, unnamed version in the picker.
    if installer_id != id {
        let _ = fs::remove_dir_all(paths.game.join("versions").join(&installer_id));
    }
    Ok(id)
}

// ───────────────────────── Loader of an instance ─────────────────────────

/// The mod loader a version runs ("fabric", "quilt", "neoforge", "forge"), or None for vanilla.
pub fn instance_loader(paths: &Paths, selected: &str) -> Option<&'static str> {
    let s = selected.trim();
    if s.starts_with(installer::FABRIC_PREFIX) {
        return Some("fabric");
    }
    if let Some((kind, _)) = split_id(s) {
        return Some(kind.key());
    }
    // Installed versions (ours or the official launcher's): look at what they load.
    let mut cur = s.to_owned();
    for _ in 0..8 {
        let Some(json) = installer::find_version_json(paths, &cur).and_then(|p| installer::read_json(&p).ok()) else { break };
        if let Some(found) = loader_in_json(&json) {
            return Some(found);
        }
        match json.get("inheritsFrom").and_then(Value::as_str) {
            Some(p) if p != cur => cur = p.to_owned(),
            _ => break,
        }
    }
    let lower = s.to_lowercase();
    ["neoforge", "quilt", "fabric", "forge"].into_iter().find(|l| lower.contains(l))
}

fn loader_in_json(json: &Value) -> Option<&'static str> {
    match json.pointer("/lumosLoader/type").and_then(Value::as_str) {
        Some("neoforge") => return Some("neoforge"),
        Some("forge") => return Some("forge"),
        _ => {}
    }
    let libs = json.get("libraries").and_then(Value::as_array)?;
    let names: Vec<&str> = libs.iter().filter_map(|l| l.get("name").and_then(Value::as_str)).collect();
    let has = |prefix: &str| names.iter().any(|n| n.starts_with(prefix));
    if has("org.quiltmc:quilt-loader:") {
        Some("quilt")
    } else if has("net.fabricmc:fabric-loader:") {
        Some("fabric")
    } else if has("net.neoforged:neoforge:") || has("net.neoforged.fancymodloader:") {
        Some("neoforge")
    } else if has("net.minecraftforge:forge:") || has("net.minecraftforge:fmlloader:") {
        Some("forge")
    } else {
        None
    }
}

/// Modrinth `loaders` a mod may declare to run on this loader.
pub fn compatible_modrinth_loaders(loader: &str) -> Vec<&'static str> {
    match loader {
        "quilt" => vec!["quilt", "fabric"],
        "neoforge" => vec!["neoforge"],
        "forge" => vec!["forge"],
        _ => vec!["fabric"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_versions_map_to_minecraft() {
        assert_eq!(neoforge_mc("21.1.77"), Some(("1.21.1".into(), true)));
        assert_eq!(neoforge_mc("21.0.10-beta"), Some(("1.21".into(), false)));
        assert_eq!(neoforge_mc("20.4.237"), Some(("1.20.4".into(), true)));
        assert_eq!(neoforge_mc("21.11.3"), Some(("1.21.11".into(), true)));
        assert_eq!(neoforge_mc("26.1.2.127"), Some(("26.1.2".into(), true)));
        assert_eq!(neoforge_mc("26.3.0.110"), Some(("26.3".into(), true)));
        assert_eq!(neoforge_mc("26.1.0.0-alpha.1+snapshot-1"), None);
        assert_eq!(neoforge_mc("0.25w14craftmine.3-beta"), None);
    }

    #[test]
    fn newest_stable_neoforge_wins() {
        let xml = "<metadata><versioning><versions>\
            <version>20.2.3-beta</version><version>21.1.9</version><version>21.1.10</version>\
            <version>21.1.100-beta</version><version>21.1.2</version><version>26.3.0.110</version>\
            <version>26.3.0.99</version></versions></versioning></metadata>";
        let m = parse_neoforge_metadata(xml);
        assert_eq!(m.get("1.21.1").map(String::as_str), Some("21.1.10"));
        assert_eq!(m.get("1.20.2").map(String::as_str), Some("20.2.3-beta"));
        assert_eq!(m.get("26.3").map(String::as_str), Some("26.3.0.110"));
    }

    #[test]
    fn forge_prefers_recommended() {
        let v = json!({"promos": {
            "1.20.1-latest": "47.4.29", "1.20.1-recommended": "47.4.28",
            "1.21.9-latest": "59.0.5", "1.7.10-latest": "10.13.4.1614", "1.12.2-latest": "14.23.5.2864"
        }});
        let m = parse_forge_promos(&v);
        assert_eq!(m.get("1.20.1").map(String::as_str), Some("47.4.28"));
        assert_eq!(m.get("1.21.9").map(String::as_str), Some("59.0.5"));
        assert_eq!(m.get("1.12.2").map(String::as_str), Some("14.23.5.2864"));
        assert!(!m.contains_key("1.7.10"));
    }

    #[test]
    fn ids_and_installer_urls() {
        assert_eq!(split_id("NeoForge 1.21.1"), Some((Kind::NeoForge, "1.21.1")));
        assert_eq!(split_id("Forge 1.20.1"), Some((Kind::Forge, "1.20.1")));
        assert_eq!(split_id("Fabric 1.20.1"), None);
        assert_eq!(split_id("Forge "), None);
        assert_eq!(
            Kind::NeoForge.installer_url("1.21.1", "21.1.77").0,
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.77/neoforge-21.1.77-installer.jar"
        );
        assert_eq!(
            Kind::Forge.installer_url("1.20.1", "47.4.28").0,
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.4.28/forge-1.20.1-47.4.28-installer.jar"
        );
    }

    #[test]
    fn loaders_are_read_from_ids_and_jsons() {
        let root = std::env::temp_dir().join(format!("lumos-forge-loader-{}", std::process::id()));
        let paths = Paths::new(&root, None);
        assert_eq!(instance_loader(&paths, "NeoForge 1.21.1"), Some("neoforge"));
        assert_eq!(instance_loader(&paths, "Forge 1.20.1"), Some("forge"));
        assert_eq!(instance_loader(&paths, "Fabric 1.21.1"), Some("fabric"));
        assert_eq!(instance_loader(&paths, "1.21.1"), None);
        let id = "1.20.1-forge-47.4.10";
        let p = installer::version_json_in(&paths.game, id);
        installer::write_json_atomic(&p, &json!({"id": id, "libraries": [{"name": "net.minecraftforge:fmlloader:1.20.1-47.4.10"}]})).unwrap();
        assert_eq!(instance_loader(&paths, id), Some("forge"));
        let q = "quilt-loader-0.26.0-1.21.1";
        installer::write_json_atomic(&installer::version_json_in(&paths.game, q), &json!({"id": q, "libraries": [{"name": "org.quiltmc:quilt-loader:0.26.0"}]})).unwrap();
        assert_eq!(instance_loader(&paths, q), Some("quilt"));
        assert_eq!(compatible_modrinth_loaders("quilt"), vec!["quilt", "fabric"]);
        let _ = fs::remove_dir_all(&root);
    }

    /// Live: installs `spec` (vanilla, Java, the loader's installer run) into a temp root and
    /// checks the launch command it would run. Large download. Run with --ignored.
    fn live_install(spec: &str, root_name: &str) {
        let root = std::env::temp_dir().join(root_name);
        let paths = Paths::new(&root, None);
        let report = |m: &str, f: f64| eprintln!("{m} {f:.2}");
        let (r, prep, java) = installer::install(&paths, spec, &report).expect("install");
        assert!(java.is_file());
        assert_eq!(r.id, spec);
        assert_eq!(r.jar_id, spec, "the client jar must be named after the version (ignoreList)");
        assert!(prep.classpath.iter().all(|p| p.is_file()), "missing classpath entries");
        assert!(prep.classpath.iter().all(|p| p.starts_with(&paths.game)), "everything in LumosClient's own folder");
        assert_eq!(r.json["mainClass"], "cpw.mods.bootstraplauncher.BootstrapLauncher");
        let saved = installer::read_json(&installer::version_json_in(&paths.game, spec)).unwrap();
        let loader = saved.pointer("/lumosLoader/version").and_then(Value::as_str).expect("loader version recorded").to_owned();
        eprintln!("installed {spec} with loader {loader}");
        // The installer's own version folder is gone, so the picker shows one entry.
        let installer_id = saved.pointer("/lumosLoader/installerId").and_then(Value::as_str).unwrap();
        assert!(!paths.game.join("versions").join(installer_id).exists());
        assert!(catalog_variants(&cached_index(&paths), saved.get("inheritsFrom").and_then(Value::as_str).unwrap_or("")).iter().any(|(id, _)| id == spec));

        let game_dir = root.join("instance");
        let auth = installer::Auth { name: "LumosTest".into(), uuid: "0123456789abcdef0123456789abcdef".into(), access_token: "0".into(), user_type: "legacy".into(), javaagent: None };
        let o = installer::LaunchOpts { memory_mb: 2048, extra_jvm: vec![], width: 854, height: 480, fullscreen: false, server: None, launcher_version: "test".into() };
        let args = installer::build_args(&r, &prep, &game_dir, &auth, &o);
        assert!(!args.iter().any(|a| a.contains("${")), "unresolved placeholder: {args:?}");
        let ignore = args.iter().find(|a| a.starts_with("-DignoreList=")).expect("ignoreList");
        assert!(ignore.contains(&format!("{spec}.jar")));
        let jar_name = format!("{spec}.jar");
        assert!(prep.classpath.last().unwrap().file_name().unwrap().to_string_lossy() == jar_name);
        let mp = &args[args.iter().position(|a| a == "-p").expect("module path") + 1];
        for p in mp.split(installer::classpath_separator()) {
            assert!(Path::new(p).is_file(), "module path entry missing: {p}");
        }
        let main_at = args.iter().position(|a| a == "cpw.mods.bootstraplauncher.BootstrapLauncher").unwrap();
        assert!(args[..main_at].iter().any(|a| a == "-cp"));
        assert!(args[main_at..].iter().any(|a| a == "--launchTarget"));
        // A second install is instant and changes nothing.
        let again = installer::install(&paths, spec, &|_, _| {}).expect("reinstall");
        assert_eq!(again.0.jar_id, spec);
    }

    #[test]
    #[ignore]
    fn live_install_neoforge() {
        live_install("NeoForge 1.21.1", "lumos-live-neoforge");
    }

    #[test]
    #[ignore]
    fn live_install_forge() {
        live_install("Forge 1.20.1", "lumos-live-forge");
    }

    #[test]
    fn catalog_variants_follow_the_index() {
        let mut idx = LoaderIndex::default();
        idx.neoforge.insert("1.21.1".into(), "21.1.77".into());
        idx.forge.insert("1.21.1".into(), "52.1.20".into());
        idx.forge.insert("1.20.1".into(), "47.4.28".into());
        assert_eq!(catalog_variants(&idx, "1.21.1"), vec![("NeoForge 1.21.1".to_owned(), "neoforge"), ("Forge 1.21.1".to_owned(), "forge")]);
        assert_eq!(catalog_variants(&idx, "1.20.1"), vec![("Forge 1.20.1".to_owned(), "forge")]);
        assert!(catalog_variants(&idx, "1.8.9").is_empty());
        assert!(spec_supported(Kind::NeoForge, "1.20.2") && spec_supported(Kind::NeoForge, "26.3"));
        assert!(!spec_supported(Kind::NeoForge, "1.20.1") && !spec_supported(Kind::Forge, "1.8.9"));
        assert!(spec_supported(Kind::Forge, "1.12.2") && spec_supported(Kind::Forge, "1.20.1"));
    }
}
