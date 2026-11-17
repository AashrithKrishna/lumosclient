//! "Repair instance": checks every file a version needs against Mojang's / Fabric's sha1,
//! deletes the damaged ones (only in LumosClient's own folder; the official launcher's is
//! read-only) and leftover partial downloads, so the installer downloads them again.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::installer::{self, Paths, Report};
use crate::net;

/// What a repair found and fixed.
#[derive(Debug, Default)]
pub struct Findings {
    /// Short sentences for the player ("Replaced 3 damaged libraries").
    pub fixed: Vec<String>,
    pub checked: usize,
}

/// One file the version expects.
#[derive(Debug, Clone, PartialEq)]
pub struct Expected {
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub group: &'static str,
}

fn s(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_owned)
}

fn safe_rel(rel: &str) -> bool {
    installer::safe_rel(rel)
}

/// Every downloadable file of a resolved version JSON, inside `game` (our own folder).
pub fn expected_files(game: &Path, json: &Value, jar_id: &str) -> Vec<Expected> {
    let mut out = Vec::new();
    if let Some(d) = json.pointer("/downloads/client") {
        out.push(Expected {
            path: game.join("versions").join(jar_id).join(format!("{jar_id}.jar")),
            sha1: s(d.get("sha1")),
            size: d.get("size").and_then(Value::as_u64),
            group: "game jar",
        });
    }
    let f = installer::Features::default();
    for lib in json.get("libraries").and_then(Value::as_array).into_iter().flatten() {
        if !installer::rules_allow(lib.get("rules"), &f) {
            continue;
        }
        let name = lib.get("name").and_then(Value::as_str).unwrap_or_default();
        match lib.get("downloads") {
            Some(dl) => {
                if let Some(a) = dl.get("artifact") {
                    if let Some(rel) = s(a.get("path")).or_else(|| installer::maven_path(name)) {
                        out.push(Expected { path: game.join("libraries").join(&rel), sha1: s(a.get("sha1")), size: a.get("size").and_then(Value::as_u64), group: "libraries" });
                    }
                }
                // Native classifiers (old versions): any present copy is checked.
                for c in dl.get("classifiers").and_then(Value::as_object).into_iter().flat_map(|m| m.values()) {
                    if let Some(rel) = s(c.get("path")) {
                        out.push(Expected { path: game.join("libraries").join(&rel), sha1: s(c.get("sha1")), size: c.get("size").and_then(Value::as_u64), group: "libraries" });
                    }
                }
            }
            None => {
                if let Some(rel) = installer::maven_path(name) {
                    out.push(Expected { path: game.join("libraries").join(&rel), sha1: s(lib.get("sha1")), size: lib.get("size").and_then(Value::as_u64), group: "libraries" });
                }
            }
        }
    }
    if let Some(file) = json.pointer("/logging/client/file") {
        if let Some(id) = file.get("id").and_then(Value::as_str).filter(|i| safe_rel(i) && !i.contains('/')) {
            out.push(Expected { path: game.join("assets").join("log_configs").join(id), sha1: s(file.get("sha1")), size: file.get("size").and_then(Value::as_u64), group: "game files" });
        }
    }
    if let Some(idx) = json.get("assetIndex") {
        if let Some(id) = idx.get("id").and_then(Value::as_str).filter(|i| safe_rel(i) && !i.contains('/')) {
            out.push(Expected {
                path: game.join("assets").join("indexes").join(format!("{id}.json")),
                sha1: s(idx.get("sha1")),
                size: idx.get("size").and_then(Value::as_u64),
                group: "game files",
            });
        }
    }
    out.retain(|e| e.path.starts_with(game) && e.path.components().all(|c| !matches!(c, std::path::Component::ParentDir)));
    out
}

/// Asset objects listed in an asset index (hash-named, so the name is the sha1).
pub fn expected_assets(game: &Path, index: &Value) -> Vec<Expected> {
    index
        .get("objects")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|m| m.values())
        .filter_map(|o| {
            let h = o.get("hash")?.as_str()?;
            (h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| Expected {
                path: game.join("assets").join("objects").join(&h[..2]).join(h),
                sha1: Some(h.to_ascii_lowercase()),
                size: o.get("size").and_then(Value::as_u64),
                group: "sounds and textures",
            })
        })
        .collect()
}

/// Deletes present files that fail their check; returns how many per group.
pub fn remove_damaged(files: &[Expected], report: Report, label: &str) -> Vec<(&'static str, usize)> {
    let mut bad: Vec<(&'static str, usize)> = Vec::new();
    let total = files.len().max(1);
    for (i, f) in files.iter().enumerate() {
        if i % 64 == 0 {
            report(label, i as f64 / total as f64);
        }
        if !f.path.is_file() || net::file_is_valid(&f.path, f.sha1.as_deref(), f.size) {
            continue;
        }
        if fs::remove_file(&f.path).is_ok() {
            match bad.iter_mut().find(|(g, _)| *g == f.group) {
                Some((_, n)) => *n += 1,
                None => bad.push((f.group, 1)),
            }
        }
    }
    bad
}

/// Removes leftover partial downloads (`*.part`) under `dirs`; returns how many.
pub fn remove_part_files(dirs: &[PathBuf]) -> usize {
    fn walk(dir: &Path, n: &mut usize, depth: u32) {
        if depth > 12 {
            return;
        }
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                walk(&p, n, depth + 1);
            } else if ft.is_file() && p.extension().is_some_and(|x| x == "part") && fs::remove_file(&p).is_ok() {
                *n += 1;
            }
        }
    }
    let mut n = 0;
    for d in dirs {
        walk(d, &mut n, 0);
    }
    n
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Checks a version's files and its Java runtime; damaged ones are deleted so that a
/// following `installer::install` downloads them again.
pub fn verify_version(paths: &Paths, spec: &str, report: Report) -> Findings {
    let mut found = Findings::default();
    let spec = spec.trim();
    report("Checking game files", -1.0);
    let resolved = match installer::resolve(paths, spec) {
        Ok(r) => r,
        Err(_) => {
            // A damaged version file in our own folder: remove it so it's fetched again.
            let own = paths.game.join("versions").join(spec).join(format!("{spec}.json"));
            if installer::safe_rel(spec) && !spec.contains('/') && own.is_file() && fs::read_to_string(&own).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_none() {
                let _ = fs::remove_file(&own);
                found.fixed.push("Replaced a damaged version file".into());
            }
            return found;
        }
    };
    let mut files = expected_files(&paths.game, &resolved.json, &resolved.jar_id);
    let index_path = files.iter().find(|f| f.path.parent().is_some_and(|p| p.ends_with("indexes"))).map(|f| f.path.clone());
    // A damaged index is deleted below; its objects are checked when it's readable.
    if let Some(ip) = &index_path {
        let index_ok = files.iter().find(|f| &f.path == ip).is_some_and(|f| net::file_is_valid(&f.path, f.sha1.as_deref(), f.size));
        if index_ok {
            if let Some(index) = fs::read_to_string(ip).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                files.extend(expected_assets(&paths.game, &index));
            }
        }
    }
    found.checked += files.iter().filter(|f| f.path.is_file()).count();
    for (group, n) in remove_damaged(&files, report, "Checking game files") {
        found.fixed.push(match group {
            "game jar" => "Replaced the damaged game jar".to_owned(),
            g => format!("Replaced {} ({g})", plural(n, "damaged file", "damaged files")),
        });
    }
    let (component, _) = installer::java_component(&resolved);
    let (checked, bad, reset) = verify_java(paths, &component);
    found.checked += checked;
    if bad > 0 {
        found.fixed.push(format!("Replaced {} of Java", plural(bad, "damaged file", "damaged files")));
    } else if reset {
        found.fixed.push("Re-checked Java".into());
    }
    found
}

/// Checks installed Java runtimes of `component` against the file lists cached when they
/// were installed. Returns (files checked, damaged files removed, whether the runtime is
/// marked for a full re-check by the installer).
pub fn verify_java(paths: &Paths, component: &str) -> (usize, usize, bool) {
    let (mut checked, mut bad, mut reset) = (0, 0, false);
    let Ok(rd) = fs::read_dir(&paths.runtime) else { return (0, 0, false) };
    for e in rd.flatten() {
        let dir = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(platform) = name.strip_prefix(&format!("{component}-")) else { continue };
        let manifest = paths.cache.join(format!("java-{component}-{platform}.json"));
        let parsed = fs::read_to_string(&manifest).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let mut damaged = 0;
        match parsed.as_ref().and_then(|m| m.get("files")).and_then(Value::as_object) {
            Some(files) => {
                for (rel, info) in files {
                    if info.get("type").and_then(Value::as_str) != Some("file") || !installer::safe_rel(rel) {
                        continue;
                    }
                    let Some(raw) = info.pointer("/downloads/raw") else { continue };
                    let p = dir.join(rel);
                    checked += 1;
                    if !net::file_is_valid(&p, raw.get("sha1").and_then(Value::as_str), raw.get("size").and_then(Value::as_u64)) {
                        if p.is_file() {
                            let _ = fs::remove_file(&p);
                        }
                        damaged += 1;
                    }
                }
            }
            // No file list to check against: let the installer re-check everything.
            None => damaged = usize::MAX,
        }
        if damaged > 0 {
            for marker in [".lumos-runtime.json", ".ember-runtime.json"] {
                let _ = fs::remove_file(dir.join(marker));
            }
            reset = true;
            if damaged != usize::MAX {
                bad += damaged;
            }
        }
    }
    (checked, bad, reset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lumos-repair-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    const HELLO_SHA1: &str = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";

    #[test]
    fn lists_jar_libraries_logging_and_index() {
        let game = PathBuf::from("/g");
        let j = json!({
            "downloads": {"client": {"sha1": "abc", "size": 5}},
            "libraries": [
                {"name": "a:b:1", "downloads": {"artifact": {"path": "a/b/1/b-1.jar", "sha1": "x", "size": 1}}},
                {"name": "c:d:2", "url": "https://maven.fabricmc.net/", "sha1": "y"},
                {"name": "e:f:3", "downloads": {"artifact": {"path": "e/f.jar"}}, "rules": [{"action": "allow", "os": {"name": "no-such-os"}}]},
                {"name": "evil:x:1", "downloads": {"artifact": {"path": "../../etc/passwd"}}}
            ],
            "logging": {"client": {"file": {"id": "client-1.12.xml", "sha1": "z", "size": 3}}},
            "assetIndex": {"id": "17", "sha1": "w", "size": 9}
        });
        let files = expected_files(&game, &j, "1.21.11");
        let paths: Vec<String> = files.iter().map(|f| f.path.to_string_lossy().into_owned()).collect();
        assert_eq!(
            paths,
            vec![
                "/g/versions/1.21.11/1.21.11.jar",
                "/g/libraries/a/b/1/b-1.jar",
                "/g/libraries/c/d/2/d-2.jar",
                "/g/assets/log_configs/client-1.12.xml",
                "/g/assets/indexes/17.json"
            ]
        );
        assert_eq!(files[2].sha1.as_deref(), Some("y"));
        let idx = json!({"objects": {"a": {"hash": "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d", "size": 5}, "bad": {"hash": "../x"}}});
        let assets = expected_assets(&game, &idx);
        assert_eq!(assets.len(), 1);
        assert!(assets[0].path.ends_with("objects/aa/aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"));
    }

    #[test]
    fn removes_damaged_files_and_parts() {
        let d = temp("damaged");
        let good = d.join("good");
        let bad = d.join("bad");
        fs::write(&good, "hello").unwrap();
        fs::write(&bad, "hellx").unwrap();
        let files = vec![
            Expected { path: good.clone(), sha1: Some(HELLO_SHA1.into()), size: Some(5), group: "libraries" },
            Expected { path: bad.clone(), sha1: Some(HELLO_SHA1.into()), size: Some(5), group: "libraries" },
            Expected { path: d.join("missing"), sha1: Some(HELLO_SHA1.into()), size: None, group: "libraries" },
        ];
        let removed = remove_damaged(&files, &|_, _| {}, "x");
        assert_eq!(removed, vec![("libraries", 1)]);
        assert!(good.is_file() && !bad.exists());

        fs::create_dir_all(d.join("a/b")).unwrap();
        fs::write(d.join("a/b/.lib.jar.part"), "x").unwrap();
        fs::write(d.join("a/keep.jar"), "x").unwrap();
        assert_eq!(remove_part_files(&[d.clone(), d.join("nope")]), 1);
        assert!(d.join("a/keep.jar").is_file());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn java_runtime_check_resets_the_marker() {
        let root = temp("java");
        let paths = Paths::new(&root, None);
        let rt = paths.runtime.join("java-runtime-delta-mac-os-arm64");
        fs::create_dir_all(rt.join("bin")).unwrap();
        fs::write(rt.join("bin/java"), "hellx").unwrap();
        fs::write(rt.join(".lumos-runtime.json"), "{}").unwrap();
        let manifest = json!({"files": {
            "bin": {"type": "directory"},
            "bin/java": {"type": "file", "downloads": {"raw": {"sha1": HELLO_SHA1, "size": 5}}}
        }});
        fs::write(paths.cache.join("java-java-runtime-delta-mac-os-arm64.json"), manifest.to_string()).unwrap();
        assert_eq!(verify_java(&paths, "java-runtime-delta"), (1, 1, true));
        assert!(!rt.join(".lumos-runtime.json").exists() && !rt.join("bin/java").exists());
        // Healthy runtime: nothing to do.
        fs::write(rt.join("bin/java"), "hello").unwrap();
        fs::write(rt.join(".lumos-runtime.json"), "{}").unwrap();
        assert_eq!(verify_java(&paths, "java-runtime-delta"), (1, 0, false));
        assert!(rt.join(".lumos-runtime.json").exists());
        // Other components are left alone.
        assert_eq!(verify_java(&paths, "jre-legacy"), (0, 0, false));
        let _ = fs::remove_dir_all(&root);
    }
}
