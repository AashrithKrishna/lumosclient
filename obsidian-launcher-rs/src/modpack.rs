//! Modrinth modpacks (`.mrpack`).
//!
//! An `.mrpack` is a zip with `modrinth.index.json` (the Minecraft version, the loader, and
//! every file to download with its hashes) plus `overrides/` and `client-overrides/` folders
//! copied over the instance. Every download is checked against its sha1 and sha512, comes only
//! from the hosts Modrinth allows, and may only land inside the instance folder.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use serde_json::Value;
use sha2::{Digest, Sha512};

use crate::installer::{self, Report};
use crate::net;

const INDEX_FILE: &str = "modrinth.index.json";
const THREADS: usize = 6;
/// The biggest single override file unpacked (a world or a resource pack can be large).
const MAX_OVERRIDE_BYTES: u64 = 1 << 30;

#[derive(Clone, Debug, PartialEq)]
pub struct PackFile {
    pub path: String,
    pub urls: Vec<String>,
    pub sha1: String,
    pub sha512: String,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PackIndex {
    pub name: String,
    pub version_id: String,
    pub mc: String,
    /// (dependency key, version): "fabric-loader", "quilt-loader", "neoforge" or "forge".
    pub loader: Option<(String, String)>,
    /// Files the client needs (server-only ones are left out).
    pub files: Vec<PackFile>,
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Hosts Modrinth allows modpack downloads from (https only).
pub fn allowed_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
    if host.contains('@') || host.contains(':') {
        return false;
    }
    matches!(host.as_str(), "cdn.modrinth.com" | "github.com" | "raw.githubusercontent.com" | "gitlab.com")
}

/// Where a pack-relative path lands inside `dir`, or None when it would leave it.
pub fn target_in(dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.replace('\\', "/");
    if rel.starts_with('/') || !installer::safe_rel(&rel) || rel.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
        return None;
    }
    // Windows drive letters and alternate streams ("C:", "file:stream").
    if rel.contains(':') {
        return None;
    }
    Some(dir.join(rel))
}

/// Reads `modrinth.index.json`.
pub fn parse_index(v: &Value) -> Result<PackIndex, String> {
    if v.get("formatVersion").and_then(Value::as_u64) != Some(1) {
        return Err("This modpack uses a format LumosClient doesn't know.".into());
    }
    if v.get("game").and_then(Value::as_str) != Some("minecraft") {
        return Err("This isn't a Minecraft modpack.".into());
    }
    let deps = v.get("dependencies").and_then(Value::as_object).ok_or("The modpack doesn't say which Minecraft it needs.")?;
    let mc = deps.get("minecraft").and_then(Value::as_str).ok_or("The modpack doesn't say which Minecraft it needs.")?.to_owned();
    if !installer::safe_id(&mc) {
        return Err("The modpack names an invalid Minecraft version.".into());
    }
    let mut loader = None;
    for key in ["neoforge", "forge", "fabric-loader", "quilt-loader"] {
        if let Some(ver) = deps.get(key).and_then(Value::as_str) {
            loader = Some((key.to_owned(), ver.to_owned()));
            break;
        }
    }
    let mut files = Vec::new();
    for f in v.get("files").and_then(Value::as_array).into_iter().flatten() {
        if f.pointer("/env/client").and_then(Value::as_str) == Some("unsupported") {
            continue;
        }
        let path = f.get("path").and_then(Value::as_str).unwrap_or_default().to_owned();
        let sha1 = f.pointer("/hashes/sha1").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
        let sha512 = f.pointer("/hashes/sha512").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
        let urls: Vec<String> = f.get("downloads").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
        if target_in(Path::new("instance"), &path).is_none() {
            return Err(format!("The modpack lists a file outside the instance ({path})."));
        }
        if !is_hex(&sha1, 40) || !is_hex(&sha512, 128) {
            return Err(format!("The modpack lists {path} without proper checksums."));
        }
        if urls.is_empty() {
            return Err(format!("The modpack lists {path} without a download."));
        }
        files.push(PackFile { path, urls, sha1, sha512, size: f.get("fileSize").and_then(Value::as_u64) });
    }
    Ok(PackIndex {
        name: v.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Modpack").to_owned(),
        version_id: v.get("versionId").and_then(Value::as_str).unwrap_or_default().to_owned(),
        mc,
        loader,
        files,
    })
}

/// The LumosClient version (as the picker names it) the pack runs on.
pub fn version_spec(idx: &PackIndex) -> Result<String, String> {
    match idx.loader.as_ref().map(|(k, _)| k.as_str()) {
        None => Ok(idx.mc.clone()),
        Some("fabric-loader") => Ok(format!("{}{}", installer::FABRIC_PREFIX, idx.mc)),
        Some("neoforge") => Ok(format!("{}{}", crate::forge::NEOFORGE_PREFIX, idx.mc)),
        Some("forge") => Ok(format!("{}{}", crate::forge::FORGE_PREFIX, idx.mc)),
        Some(_) => Err("Quilt modpacks aren't supported yet. Fabric, NeoForge and Forge packs are.".into()),
    }
}

pub fn read_index(mrpack: &Path) -> Result<PackIndex, String> {
    let file = fs::File::open(mrpack).map_err(|e| format!("Couldn't open the modpack: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "That file isn't a modpack (.mrpack).".to_owned())?;
    let mut raw = String::new();
    zip.by_name(INDEX_FILE)
        .map_err(|_| "That file isn't a Modrinth modpack (no modrinth.index.json).".to_owned())?
        .take(32 << 20)
        .read_to_string(&mut raw)
        .map_err(|e| format!("Couldn't read the modpack: {e}"))?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("The modpack's file list is damaged: {e}"))?;
    parse_index(&v)
}

fn sha512_file(p: &Path) -> Option<String> {
    let mut f = fs::File::open(p).ok()?;
    let mut h = Sha512::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn fetch_one(f: &PackFile, dest: &Path) -> Result<(), String> {
    if sha512_file(dest).as_deref() == Some(f.sha512.as_str()) {
        return Ok(());
    }
    let tmp = dest.with_file_name(format!(".{}.lumos-part", dest.file_name().and_then(|n| n.to_str()).unwrap_or("file")));
    let mut last = format!("No allowed download for {}.", f.path);
    for url in f.urls.iter().filter(|u| allowed_url(u)) {
        let _ = fs::remove_file(&tmp);
        match net::download_verified(url, &tmp, Some(&f.sha1), f.size) {
            Ok(()) => {
                if sha512_file(&tmp).as_deref() != Some(f.sha512.as_str()) {
                    let _ = fs::remove_file(&tmp);
                    last = format!("{} didn't match its checksum.", f.path);
                    continue;
                }
                return fs::rename(&tmp, dest).map_err(|e| {
                    let _ = fs::remove_file(&tmp);
                    format!("Couldn't save {}: {e}", f.path)
                });
            }
            Err(e) => last = e,
        }
    }
    let _ = fs::remove_file(&tmp);
    Err(last)
}

/// Downloads every file of the pack into `dir` (in parallel, with progress).
pub fn download_files(idx: &PackIndex, dir: &Path, report: Report) -> Result<(), String> {
    let mut seen = HashSet::new();
    let mut jobs: Vec<(&PackFile, PathBuf)> = Vec::new();
    for f in &idx.files {
        let dest = target_in(dir, &f.path).ok_or_else(|| format!("The modpack lists a file outside the instance ({}).", f.path))?;
        if seen.insert(dest.clone()) {
            jobs.push((f, dest));
        }
    }
    for (_, dest) in &jobs {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
        }
    }
    let total = jobs.len().max(1);
    let total_bytes: u64 = jobs.iter().map(|(f, _)| f.size.unwrap_or(0)).sum();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let done_bytes = AtomicU64::new(0);
    let error: Mutex<Option<String>> = Mutex::new(None);
    let label = format!("Downloading {} files for {}", jobs.len(), idx.name);
    report(&label, 0.0);
    std::thread::scope(|s| {
        for _ in 0..THREADS.min(jobs.len()) {
            s.spawn(|| loop {
                if error.lock().unwrap().is_some() {
                    return;
                }
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some((f, dest)) = jobs.get(i) else { return };
                if let Err(e) = fetch_one(f, dest) {
                    error.lock().unwrap().get_or_insert(e);
                    return;
                }
                let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                let b = done_bytes.fetch_add(f.size.unwrap_or(0), Ordering::SeqCst) + f.size.unwrap_or(0);
                let frac = if total_bytes > 0 { b as f64 / total_bytes as f64 } else { n as f64 / total as f64 };
                report(&label, frac.min(0.999));
            });
        }
    });
    if let Some(e) = error.into_inner().unwrap() {
        return Err(e);
    }
    report(&label, 1.0);
    Ok(())
}

/// Copies `overrides/` then `client-overrides/` from the pack over `dir`. Returns how many files.
pub fn extract_overrides(mrpack: &Path, dir: &Path) -> Result<usize, String> {
    let file = fs::File::open(mrpack).map_err(|e| format!("Couldn't open the modpack: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("The modpack is damaged: {e}"))?;
    let mut count = 0;
    for prefix in ["overrides/", "client-overrides/"] {
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| format!("The modpack is damaged: {e}"))?;
            let name = entry.name().replace('\\', "/");
            let Some(rel) = name.strip_prefix(prefix) else { continue };
            if rel.is_empty() || entry.is_dir() {
                continue;
            }
            if entry.enclosed_name().is_none() {
                return Err(format!("The modpack has a file outside the instance ({name})."));
            }
            let dest = target_in(dir, rel).ok_or_else(|| format!("The modpack has a file outside the instance ({name})."))?;
            if entry.size() > MAX_OVERRIDE_BYTES {
                return Err(format!("{rel} in the modpack is too big."));
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
            }
            let tmp = dest.with_file_name(format!(".{}.lumos-part", dest.file_name().and_then(|n| n.to_str()).unwrap_or("file")));
            let mut out = fs::File::create(&tmp).map_err(|e| format!("Couldn't write {rel}: {e}"))?;
            let copied = std::io::copy(&mut (&mut entry).take(MAX_OVERRIDE_BYTES + 1), &mut out).map_err(|e| format!("Couldn't unpack {rel}: {e}"))?;
            out.flush().map_err(|e| e.to_string())?;
            drop(out);
            if copied > MAX_OVERRIDE_BYTES {
                let _ = fs::remove_file(&tmp);
                return Err(format!("{rel} in the modpack is too big."));
            }
            fs::rename(&tmp, &dest).map_err(|e| format!("Couldn't save {rel}: {e}"))?;
            count += 1;
        }
    }
    Ok(count)
}

/// Fills an instance folder from a pack: downloads, then overrides.
pub fn install(mrpack: &Path, idx: &PackIndex, dir: &Path, report: Report) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    download_files(idx, dir, report)?;
    report(&format!("Unpacking {}", idx.name), -1.0);
    extract_overrides(mrpack, dir)?;
    Ok(())
}

/// Downloads the newest release (else newest) `.mrpack` of a Modrinth modpack into `dest_dir`.
pub fn download_from_modrinth(project: &str, dest_dir: &Path, report: Report) -> Result<PathBuf, String> {
    if project.is_empty() || project.len() > 64 || !project.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("That Modrinth project name isn't valid.".into());
    }
    report("Finding the modpack on Modrinth", -1.0);
    let versions = net::get_json(&format!("https://api.modrinth.com/v2/project/{project}/version"))?;
    let list = versions.as_array().cloned().unwrap_or_default();
    let pick = list
        .iter()
        .find(|v| v.get("version_type").and_then(Value::as_str) == Some("release"))
        .or_else(|| list.first())
        .ok_or("This modpack has no downloads.")?;
    let files = pick.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
    let file = files
        .iter()
        .filter(|f| f.get("filename").and_then(Value::as_str).is_some_and(|n| n.ends_with(".mrpack")))
        .find(|f| f.get("primary").and_then(Value::as_bool) == Some(true))
        .or_else(|| files.iter().find(|f| f.get("filename").and_then(Value::as_str).is_some_and(|n| n.ends_with(".mrpack"))))
        .ok_or("This modpack has no .mrpack file to install.")?;
    let url = file.get("url").and_then(Value::as_str).filter(|u| allowed_url(u)).ok_or("The modpack's download link isn't from Modrinth.")?;
    let dest = dest_dir.join(format!("{project}.mrpack"));
    report(&format!("Downloading {}", pick.get("name").and_then(Value::as_str).unwrap_or(project)), -1.0);
    net::download_verified(url, &dest, file.pointer("/hashes/sha1").and_then(Value::as_str), file.get("size").and_then(Value::as_u64))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(path: &str, client: &str) -> Value {
        json!({"path": path, "hashes": {"sha1": "a".repeat(40), "sha512": "b".repeat(128)},
               "env": {"client": client, "server": "required"},
               "downloads": ["https://cdn.modrinth.com/data/x/versions/y/f.jar"], "fileSize": 10})
    }

    #[test]
    fn index_reads_versions_loaders_and_client_files() {
        let v = json!({"formatVersion": 1, "game": "minecraft", "versionId": "1.0", "name": "Pack",
            "dependencies": {"minecraft": "1.21.1", "neoforge": "21.1.77"},
            "files": [file("mods/a.jar", "required"), file("mods/server.jar", "unsupported"), file("config/b.toml", "optional")]});
        let idx = parse_index(&v).unwrap();
        assert_eq!(idx.mc, "1.21.1");
        assert_eq!(idx.loader, Some(("neoforge".into(), "21.1.77".into())));
        assert_eq!(idx.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), vec!["mods/a.jar", "config/b.toml"]);
        assert_eq!(version_spec(&idx).unwrap(), "NeoForge 1.21.1");
        let mut fab = idx.clone();
        fab.loader = Some(("fabric-loader".into(), "0.16.10".into()));
        assert_eq!(version_spec(&fab).unwrap(), "Fabric 1.21.1");
        fab.loader = Some(("forge".into(), "52.1.20".into()));
        assert_eq!(version_spec(&fab).unwrap(), "Forge 1.21.1");
        fab.loader = None;
        assert_eq!(version_spec(&fab).unwrap(), "1.21.1");
        fab.loader = Some(("quilt-loader".into(), "0.26".into()));
        assert!(version_spec(&fab).is_err());
    }

    #[test]
    fn unsafe_paths_and_hosts_are_refused() {
        for bad in ["../evil.jar", "/etc/passwd", "mods/../../x", "C:/x.jar", "mods\\..\\..\\x", "", "mods//a.jar", "./a"] {
            let v = json!({"formatVersion": 1, "game": "minecraft", "dependencies": {"minecraft": "1.21.1"}, "files": [file(bad, "required")]});
            assert!(parse_index(&v).is_err(), "{bad} should be refused");
        }
        assert!(target_in(Path::new("/i"), "mods/a.jar").is_some());
        assert!(allowed_url("https://cdn.modrinth.com/data/a/b.jar"));
        assert!(allowed_url("https://github.com/x/y/releases/download/v/a.jar"));
        assert!(!allowed_url("http://cdn.modrinth.com/a.jar"));
        assert!(!allowed_url("https://evil.example/a.jar"));
        assert!(!allowed_url("https://cdn.modrinth.com.evil.example/a.jar"));
        assert!(!allowed_url("https://user@cdn.modrinth.com/a.jar"));
        let mut bad_hash = file("mods/a.jar", "required");
        bad_hash["hashes"]["sha512"] = json!("zz");
        let v = json!({"formatVersion": 1, "game": "minecraft", "dependencies": {"minecraft": "1.21.1"}, "files": [bad_hash]});
        assert!(parse_index(&v).is_err());
    }

    /// Live: downloads a real Modrinth modpack and installs its files into a temp instance.
    /// Run with --ignored.
    #[test]
    #[ignore]
    fn live_modrinth_modpack() {
        let root = std::env::temp_dir().join("lumos-live-mrpack");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let report = |m: &str, f: f64| eprintln!("{m} {f:.2}");
        let pack = download_from_modrinth("fabulously-optimized", &root, &report).expect("download");
        let idx = read_index(&pack).expect("index");
        let spec = version_spec(&idx).expect("spec");
        eprintln!("{} {} -> {spec} ({} files)", idx.name, idx.version_id, idx.files.len());
        assert!(spec.starts_with("Fabric "));
        let inst = root.join("instance");
        install(&pack, &idx, &inst, &report).expect("install");
        for f in &idx.files {
            let p = inst.join(&f.path);
            assert_eq!(sha512_file(&p).as_deref(), Some(f.sha512.as_str()), "{}", f.path);
        }
        assert!(fs::read_dir(inst.join("mods")).unwrap().count() >= 5);
        assert!(inst.join("config").is_dir(), "overrides unpacked");
        // Importing again only re-checks files.
        install(&pack, &idx, &inst, &|_, _| {}).expect("reinstall");
    }

    #[test]
    fn overrides_unpack_inside_the_instance_only() {
        let root = std::env::temp_dir().join(format!("lumos-mrpack-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let pack = root.join("p.mrpack");
        {
            let mut z = zip::ZipWriter::new(fs::File::create(&pack).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file(INDEX_FILE, o).unwrap();
            z.write_all(json!({"formatVersion": 1, "game": "minecraft", "name": "T", "dependencies": {"minecraft": "1.20.1", "forge": "47.4.0"}, "files": []}).to_string().as_bytes()).unwrap();
            z.start_file("overrides/config/a.txt", o).unwrap();
            z.write_all(b"base").unwrap();
            z.start_file("client-overrides/config/a.txt", o).unwrap();
            z.write_all(b"client").unwrap();
            z.start_file("overrides/options.txt", o).unwrap();
            z.write_all(b"opt").unwrap();
            z.finish().unwrap();
        }
        let idx = read_index(&pack).unwrap();
        assert_eq!(version_spec(&idx).unwrap(), "Forge 1.20.1");
        let inst = root.join("inst");
        install(&pack, &idx, &inst, &|_, _| {}).unwrap();
        assert_eq!(fs::read_to_string(inst.join("config/a.txt")).unwrap(), "client");
        assert_eq!(fs::read_to_string(inst.join("options.txt")).unwrap(), "opt");

        // A hostile entry name stops the import before anything is written outside.
        let evil = root.join("evil.mrpack");
        {
            let mut z = zip::ZipWriter::new(fs::File::create(&evil).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("overrides/../../escaped.txt", o).unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        assert!(extract_overrides(&evil, &root.join("inst2")).is_err());
        assert!(!root.join("escaped.txt").exists() && !std::env::temp_dir().join("escaped.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }
}
