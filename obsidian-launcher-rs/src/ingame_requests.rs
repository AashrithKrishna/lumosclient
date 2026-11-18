//! What the in-game Discover (lumos-mods/lumos_discover) asks Lumos to do between runs. Native
//! mods are compiled into the game, so the game can't add or remove one itself: it writes
//! `lumos-requests.json` beside `options.txt`,
//! `{"install": [ids], "remove": [ids], "restart": true}`, and closes. When the game Lumos
//! started exits, Lumos takes the file, applies it with the same code as Discover's buttons
//! (`native_builds::install`/`remove`, the dependency rules of `mod_plan`) and, when asked,
//! starts the same instance again.
//!
//! Only registry ids get through (`[a-z0-9_]{1,64}`, never a path), only native mods are
//! installed this way, and a removal another switched-on mod needs is refused.

use crate::{horizon_mods, mod_plan, native_builds};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

pub const FILE: &str = "lumos-requests.json";
/// More than any player asks for between two runs.
const MAX_IDS: usize = 64;
/// Larger than any real request file.
const MAX_BYTES: u64 = 64 << 10;

#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
pub struct Requests {
    #[serde(default)]
    pub install: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
    #[serde(default)]
    pub restart: bool,
}

/// Horizon's mod id rule: what every id in the file must be.
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl Requests {
    /// Parses the file's text, keeping only valid ids, each once, at most `MAX_IDS` of each.
    pub fn parse(text: &str) -> Option<Requests> {
        let raw: Requests = serde_json::from_str(text).ok()?;
        let clean = |ids: Vec<String>| {
            let mut out: Vec<String> = Vec::new();
            for id in ids {
                if valid_id(&id) && !out.contains(&id) && out.len() < MAX_IDS {
                    out.push(id);
                }
            }
            out
        };
        Some(Requests { install: clean(raw.install), remove: clean(raw.remove), restart: raw.restart })
    }

    pub fn is_empty(&self) -> bool {
        self.install.is_empty() && self.remove.is_empty() && !self.restart
    }
}

/// Reads and deletes an instance's request file. `None` when there is none or it can't be read
/// (an unreadable one is deleted all the same, so it never blocks a later run).
pub fn take(game_dir: &Path) -> Option<Requests> {
    let path = game_dir.join(FILE);
    let meta = fs::symlink_metadata(&path).ok()?;
    let text = (meta.is_file() && meta.len() <= MAX_BYTES).then(|| fs::read_to_string(&path).ok()).flatten();
    let _ = fs::remove_file(&path);
    Requests::parse(&text?).filter(|r| !r.is_empty())
}

/// What applying requests needs from the instance and the registry (mocked in tests).
pub trait Ops {
    /// The registry (or Lumos's own list) has `id` as a native mod.
    fn is_native(&self, id: &str) -> bool;
    /// The native mods this instance has.
    fn native_ids(&self) -> Vec<String>;
    /// A content mod with this id is in the instance's `horizon-mods/`.
    fn has_content(&self, id: &str) -> bool;
    /// The switched-on content mods that need `id` (a required dependency).
    fn needed_by(&self, id: &str) -> Vec<String>;
    fn install_native(&mut self, id: &str) -> Result<String, String>;
    fn remove_native(&mut self, id: &str) -> Result<(), String>;
    fn remove_content(&mut self, id: &str) -> Result<(), String>;
}

/// Applies `req`: removals first (so swapping one mod for another works), then installs. Each
/// outcome is (went well, a line for the player).
pub fn apply(req: &Requests, ops: &mut dyn Ops) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    for id in &req.remove {
        let native = ops.native_ids().contains(id);
        let content = !native && ops.has_content(id);
        if !native && !content {
            continue;
        }
        let needs = ops.needed_by(id);
        if !needs.is_empty() {
            out.push((false, format!("{id} stays: {} needs it.", needs.join(", "))));
            continue;
        }
        let done = if native { ops.remove_native(id) } else { ops.remove_content(id) };
        out.push(match done {
            Ok(()) => (true, format!("Removed {id}")),
            Err(e) => (false, format!("{id} wasn't removed: {e}")),
        });
    }
    for id in &req.install {
        if !ops.is_native(id) {
            out.push((false, format!("{id} isn't a native mod in Horizon's registry, so the game's request was ignored.")));
            continue;
        }
        if ops.native_ids().contains(id) {
            continue;
        }
        out.push(match ops.install_native(id) {
            Ok(msg) => (true, msg),
            Err(e) => (false, format!("{id} wasn't installed: {e}")),
        });
    }
    out
}

/// The real instance: the cloud builds, `horizon-mods/` and `horizon-mods.txt`.
pub struct Live<'a> {
    pub base_dir: PathBuf,
    pub game_dir: PathBuf,
    pub report: &'a dyn Fn(&str),
    /// Moves a removed file somewhere it can be got back from (the launcher's trash).
    pub trash: &'a dyn Fn(&Path, &str) -> Result<(), String>,
}

impl Ops for Live<'_> {
    fn is_native(&self, id: &str) -> bool {
        horizon_mods::is_native(id)
    }
    fn native_ids(&self) -> Vec<String> {
        native_builds::installed_ids(&self.game_dir)
    }
    fn has_content(&self, id: &str) -> bool {
        mod_plan::scan_horizon(&self.game_dir).iter().any(|m| m.manifest.id == id)
    }
    fn needed_by(&self, id: &str) -> Vec<String> {
        mod_plan::scan_horizon(&self.game_dir)
            .into_iter()
            .filter(|m| m.enabled && m.manifest.id != id && m.manifest.deps.iter().any(|d| d.id == id && !d.optional))
            .map(|m| m.manifest.id)
            .collect()
    }
    fn install_native(&mut self, id: &str) -> Result<String, String> {
        native_builds::install(&self.base_dir, &self.game_dir, id, self.report)
    }
    fn remove_native(&mut self, id: &str) -> Result<(), String> {
        native_builds::remove(&self.game_dir, id)
    }
    fn remove_content(&mut self, id: &str) -> Result<(), String> {
        let dir = self.game_dir.join(horizon_mods::MODS_DIR);
        for m in mod_plan::scan_horizon(&self.game_dir).into_iter().filter(|m| m.manifest.id == id) {
            (self.trash)(&dir.join(&m.file), &m.file)?;
        }
        horizon_mods::forget(&self.game_dir, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Mock {
        registry_native: Vec<&'static str>,
        natives: Vec<String>,
        content: Vec<(&'static str, bool, Vec<&'static str>)>,
        log: Vec<String>,
        fail_install: bool,
    }

    impl Ops for Mock {
        fn is_native(&self, id: &str) -> bool {
            self.registry_native.contains(&id)
        }
        fn native_ids(&self) -> Vec<String> {
            self.natives.clone()
        }
        fn has_content(&self, id: &str) -> bool {
            self.content.iter().any(|c| c.0 == id)
        }
        fn needed_by(&self, id: &str) -> Vec<String> {
            self.content.iter().filter(|c| c.1 && c.2.contains(&id)).map(|c| c.0.to_owned()).collect()
        }
        fn install_native(&mut self, id: &str) -> Result<String, String> {
            if self.fail_install {
                return Err("no build".into());
            }
            self.natives.push(id.into());
            self.log.push(format!("install {id}"));
            Ok(format!("Installed {id} (native)"))
        }
        fn remove_native(&mut self, id: &str) -> Result<(), String> {
            self.natives.retain(|n| n != id);
            self.log.push(format!("remove {id}"));
            Ok(())
        }
        fn remove_content(&mut self, id: &str) -> Result<(), String> {
            self.content.retain(|c| c.0 != id);
            self.log.push(format!("delete {id}"));
            Ok(())
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumos-requests-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_registry_ids_get_through() {
        let r = Requests::parse(r#"{"install": ["zoom", "../../etc/passwd", "a/b", "Zoom", "zoom", ""], "remove": ["x.json"], "restart": true}"#).unwrap();
        assert_eq!(r.install, vec!["zoom"]);
        assert!(r.remove.is_empty() && r.restart);
        assert!(Requests::parse("not json").is_none());
        assert!(Requests::parse(r#"{"install": "zoom"}"#).is_none(), "a list is a list");
        let many = format!(r#"{{"install": [{}]}}"#, (0..200).map(|i| format!("\"m{i}\"")).collect::<Vec<_>>().join(","));
        assert_eq!(Requests::parse(&many).unwrap().install.len(), MAX_IDS);
    }

    #[test]
    fn take_reads_once_and_deletes() {
        let dir = temp("take");
        assert!(take(&dir).is_none());
        fs::write(dir.join(FILE), r#"{"install": ["zoom"], "remove": [], "restart": true}"#).unwrap();
        let r = take(&dir).unwrap();
        assert_eq!((r.install, r.restart), (vec!["zoom".to_owned()], true));
        assert!(!dir.join(FILE).exists());
        assert!(take(&dir).is_none(), "applied once");
        // A broken or empty file is dropped, not kept for ever.
        fs::write(dir.join(FILE), "{").unwrap();
        assert!(take(&dir).is_none() && !dir.join(FILE).exists());
        fs::write(dir.join(FILE), r#"{"install": [], "remove": [], "restart": false}"#).unwrap();
        assert!(take(&dir).is_none() && !dir.join(FILE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn removes_then_installs_natives() {
        let mut ops = Mock { registry_native: vec!["zoom", "minimap"], natives: vec!["minimap".into()], ..Mock::default() };
        let req = Requests { install: vec!["zoom".into()], remove: vec!["minimap".into()], restart: true };
        let out = apply(&req, &mut ops);
        assert_eq!(ops.log, vec!["remove minimap", "install zoom"]);
        assert!(out.iter().all(|(ok, _)| *ok), "{out:?}");
        assert_eq!(ops.natives, vec!["zoom"]);
        // Asking again changes nothing.
        let again = apply(&Requests { install: vec!["zoom".into()], ..Requests::default() }, &mut ops);
        assert!(again.is_empty() && ops.log.len() == 2);
    }

    #[test]
    fn content_mods_are_never_installed_this_way() {
        let mut ops = Mock { registry_native: vec!["zoom"], ..Mock::default() };
        let out = apply(&Requests { install: vec!["ruby".into()], ..Requests::default() }, &mut ops);
        assert_eq!(out.len(), 1);
        assert!(!out[0].0 && ops.log.is_empty());
    }

    #[test]
    fn a_needed_mod_stays() {
        let mut ops = Mock {
            registry_native: vec!["corelib"],
            natives: vec!["corelib".into()],
            content: vec![("ruby", true, vec!["corelib"]), ("old", false, vec!["gems"]), ("gems", true, vec![])],
            ..Mock::default()
        };
        let req = Requests { remove: vec!["corelib".into(), "gems".into(), "ghost".into()], ..Requests::default() };
        let out = apply(&req, &mut ops);
        // corelib: ruby (on) needs it. gems: only a switched-off mod needs it. ghost: not there.
        assert_eq!(ops.log, vec!["delete gems"]);
        assert_eq!(out.len(), 2);
        assert!(!out[0].0 && out[0].1.contains("ruby"));
        assert!(out[1].0);
    }

    #[test]
    fn failures_are_reported() {
        let mut ops = Mock { registry_native: vec!["zoom"], fail_install: true, ..Mock::default() };
        let out = apply(&Requests { install: vec!["zoom".into()], ..Requests::default() }, &mut ops);
        assert_eq!(out, vec![(false, "zoom wasn't installed: no build".to_owned())]);
    }

    #[test]
    fn live_removes_a_content_mod_to_the_trash() {
        let dir = temp("live");
        let mods = dir.join(horizon_mods::MODS_DIR);
        fs::create_dir_all(&mods).unwrap();
        let hmod = |id: &str| {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            z.start_file("horizon-mod.toml", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut z, format!("id = \"{id}\"\nversion = \"1.0.0\"\n").as_bytes()).unwrap();
            z.finish().unwrap().into_inner()
        };
        fs::write(mods.join("ruby-1.0.hmod"), hmod("ruby")).unwrap();
        fs::write(mods.join("keep.hmod"), hmod("keep")).unwrap();
        horizon_mods::set_enabled(&dir, "ruby", false).unwrap();
        let trashed = std::cell::RefCell::new(Vec::new());
        let trash = |p: &Path, label: &str| {
            trashed.borrow_mut().push(label.to_owned());
            fs::remove_file(p).map_err(|e| e.to_string())
        };
        let mut live = Live { base_dir: dir.clone(), game_dir: dir.clone(), report: &|_| {}, trash: &trash };
        assert!(live.has_content("ruby") && !live.has_content("nothing"));
        let out = apply(&Requests { remove: vec!["ruby".into()], ..Requests::default() }, &mut live);
        assert_eq!(out, vec![(true, "Removed ruby".to_owned())]);
        assert_eq!(*trashed.borrow(), vec!["ruby-1.0.hmod"]);
        assert!(!mods.join("ruby-1.0.hmod").exists() && mods.join("keep.hmod").exists());
        assert!(!fs::read_to_string(dir.join("horizon-mods.txt")).unwrap_or_default().contains("mod.ruby"));
        let _ = fs::remove_dir_all(&dir);
    }
}
