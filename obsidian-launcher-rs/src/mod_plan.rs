//! Install plans and mod checks.
//!
//! Before a mod is downloaded, Lumos works out an install plan: the version that fits the
//! instance (Minecraft version + loader), every required dependency (followed recursively,
//! once each, each resolved to a version for this instance), what's already installed (by
//! file hash, then by project id), optional dependencies (suggested, never auto-installed), and
//! problems that would stop the game from starting:
//!
//! * mods marked incompatible (either way round) with something installed or being installed,
//! * a required dependency with no version for this Minecraft version + loader,
//! * a mod that needs a newer Fabric Loader than the instance has (found when its jar is
//!   downloaded, before anything is saved),
//! * two copies of one project,
//! * for Horizon content mods, the `horizon-mod.toml` `[dependencies]`, `[incompatible]` and
//!   `[discouraged]` rules (semver requirements) against `horizon-mods/` and Horizon's registry.
//!
//! A plan without blocking problems installs straight away; otherwise the page shows it and
//! the player picks "Install anyway" or "Cancel". The same rules check an instance's existing
//! mods ("Check mods" in the Mods tab).
//!
//! Network access goes through the [`Modrinth`] and [`HorizonRegistry`] traits so the planning
//! logic is tested with canned answers.

use crate::horizon_mods::{self, Fetched};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const API: &str = "https://api.modrinth.com/v2";
/// How deep dependency chains are followed (the old installer stopped at 3).
const MAX_DEPTH: usize = 6;

// ───────────────────────── Modrinth access ─────────────────────────

pub trait Modrinth {
    /// Versions of a project (slug or id), newest first; `mc` None and no loaders = every version.
    fn versions(&self, project: &str, mc: Option<&str>, loaders: &[&str]) -> Result<Vec<Value>, String>;
    /// One version by id.
    fn version(&self, id: &str) -> Result<Value, String>;
    /// Projects by id (for their names).
    fn projects(&self, ids: &[String]) -> Result<Vec<Value>, String>;
    /// sha1 → the version that file belongs to (files Modrinth doesn't know are left out).
    fn version_files(&self, sha1s: &[String]) -> Result<HashMap<String, Value>, String>;
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The real Modrinth API.
pub struct Live;

impl Modrinth for Live {
    fn versions(&self, project: &str, mc: Option<&str>, loaders: &[&str]) -> Result<Vec<Value>, String> {
        if !valid_id(project) {
            return Err("That Modrinth project name isn't valid.".into());
        }
        let mut q = Vec::new();
        if let Some(mc) = mc {
            q.push(format!("game_versions={}", crate::url_encode(&json!([mc]).to_string())));
        }
        if !loaders.is_empty() {
            q.push(format!("loaders={}", crate::url_encode(&json!(loaders).to_string())));
        }
        let mut url = format!("{API}/project/{project}/version");
        if !q.is_empty() {
            url = format!("{url}?{}", q.join("&"));
        }
        Ok(crate::net::get_json(&url)?.as_array().cloned().unwrap_or_default())
    }

    fn version(&self, id: &str) -> Result<Value, String> {
        if !valid_id(id) {
            return Err("That Modrinth version id isn't valid.".into());
        }
        crate::net::get_json(&format!("{API}/version/{id}"))
    }

    fn projects(&self, ids: &[String]) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        for chunk in ids.chunks(100) {
            let url = format!("{API}/projects?ids={}", crate::url_encode(&json!(chunk).to_string()));
            out.extend(crate::net::get_json(&url)?.as_array().cloned().unwrap_or_default());
        }
        Ok(out)
    }

    fn version_files(&self, sha1s: &[String]) -> Result<HashMap<String, Value>, String> {
        if sha1s.is_empty() {
            return Ok(HashMap::new());
        }
        let body = json!({"hashes": sha1s, "algorithm": "sha1"});
        let resp = crate::net::agent()
            .post(&format!("{API}/version_files"))
            .send_json(body)
            .map_err(|e| crate::net::describe_error("api.modrinth.com", &e))?;
        let v: Value = resp.into_json().map_err(|e| format!("api.modrinth.com sent data the launcher couldn't read: {e}"))?;
        Ok(v.as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default())
    }
}

// ───────────────────────── What's installed ─────────────────────────

/// A file in a content folder.
#[derive(Clone, Debug)]
pub struct LocalFile {
    pub file: String,
    pub sha1: String,
    pub enabled: bool,
}

/// Every file in `dir` with its sha1 (`.disabled` files count, as switched off).
pub fn scan_dir(dir: &Path) -> Vec<LocalFile> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().into_owned();
            if file.starts_with('.') || !e.path().is_file() {
                continue;
            }
            if let Some(sha1) = crate::net::sha1_file(&e.path()) {
                out.push(LocalFile { enabled: !file.ends_with(".disabled"), file, sha1 });
            }
        }
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

/// One Modrinth dependency entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Dep {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    /// required, optional, incompatible or embedded.
    pub kind: String,
}

fn deps_of(ver: &Value) -> Vec<Dep> {
    ver.get("dependencies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|d| Dep {
            project_id: d.get("project_id").and_then(Value::as_str).map(str::to_owned),
            version_id: d.get("version_id").and_then(Value::as_str).map(str::to_owned),
            kind: d.get("dependency_type").and_then(Value::as_str).unwrap_or("").to_owned(),
        })
        .collect()
}

/// An installed file, identified on Modrinth by its hash when Modrinth knows it.
#[derive(Clone, Debug, Default)]
pub struct Installed {
    pub file: String,
    pub enabled: bool,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub deps: Vec<Dep>,
}

/// Looks every file up on Modrinth by sha1 (one request).
pub fn identify(api: &dyn Modrinth, files: Vec<LocalFile>) -> Result<Vec<Installed>, String> {
    let hashes: Vec<String> = files.iter().map(|f| f.sha1.clone()).collect();
    let known = api.version_files(&hashes)?;
    Ok(files
        .into_iter()
        .map(|f| {
            let ver = known.get(&f.sha1);
            Installed {
                project_id: ver.and_then(|v| v.get("project_id")).and_then(Value::as_str).map(str::to_owned),
                version_id: ver.and_then(|v| v.get("id")).and_then(Value::as_str).map(str::to_owned),
                deps: ver.map(deps_of).unwrap_or_default(),
                file: f.file,
                enabled: f.enabled,
            }
        })
        .collect())
}

/// Finds the project a dependency names (some name only a version), asking Modrinth once per version.
struct Resolver<'a> {
    api: &'a dyn Modrinth,
    by_version: HashMap<String, Option<String>>,
}

impl<'a> Resolver<'a> {
    fn new(api: &'a dyn Modrinth) -> Self {
        Self { api, by_version: HashMap::new() }
    }
    fn project_of(&mut self, d: &Dep) -> Option<String> {
        if let Some(p) = &d.project_id {
            return Some(p.clone());
        }
        let vid = d.version_id.as_ref()?;
        let api = self.api;
        self.by_version
            .entry(vid.clone())
            .or_insert_with(|| api.version(vid).ok().and_then(|v| v.get("project_id").and_then(Value::as_str).map(str::to_owned)))
            .clone()
    }
}

// ───────────────────────── Plans ─────────────────────────

/// Something that would stop the game from starting (`blocking`) or is worth knowing.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Problem {
    /// incompatible, missing, loader, duplicate, version or discouraged.
    pub kind: &'static str,
    pub message: String,
    pub blocking: bool,
}

/// What the page shows about a plan (names, not ids).
#[derive(Serialize, Clone, Debug, Default)]
pub struct Summary {
    pub title: String,
    /// Required dependencies that will be installed with it.
    pub deps: Vec<String>,
    /// Required dependencies already in the instance.
    pub present: Vec<String>,
    /// Switched-off dependencies that will be switched on.
    pub enable: Vec<String>,
    /// Optional dependencies (suggestions only).
    pub optional: Vec<String>,
    pub problems: Vec<Problem>,
}

impl Summary {
    pub fn blocked(&self) -> bool {
        self.problems.iter().any(|p| p.blocking)
    }

    /// "Installed Sodium Extra + Sodium, Fabric API (Minecraft 1.21.11)." plus what else happened.
    pub fn success_message(&self, suffix: &str, warnings: &[String]) -> String {
        let mut msg = format!("Installed {}", self.title);
        if !self.deps.is_empty() {
            msg.push_str(&format!(" + {}", self.deps.join(", ")));
        }
        msg.push_str(suffix);
        msg.push('.');
        if !self.enable.is_empty() {
            msg.push_str(&format!(" Switched on {} (needed).", self.enable.join(", ")));
        }
        for p in self.problems.iter().filter(|p| !p.blocking) {
            msg.push(' ');
            msg.push_str(&p.message);
        }
        if !self.optional.is_empty() {
            msg.push_str(&format!(" Optional extras: {}.", self.optional.join(", ")));
        }
        for w in warnings {
            msg.push_str(&format!(" {w}."));
        }
        msg
    }
}

/// A downloadable file of one version.
#[derive(Clone, Debug, PartialEq)]
pub struct FileRef {
    pub url: String,
    pub filename: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

fn primary_file(ver: &Value) -> Option<FileRef> {
    let arr = ver.get("files")?.as_array()?;
    let f = arr.iter().find(|f| f.get("primary").and_then(Value::as_bool) == Some(true)).or_else(|| arr.first())?;
    let filename = Path::new(f.get("filename")?.as_str()?).file_name()?.to_string_lossy().into_owned();
    if !crate::safe_file_name(&filename) {
        return None;
    }
    Some(FileRef {
        url: f.get("url")?.as_str()?.to_owned(),
        filename,
        sha1: f.pointer("/hashes/sha1").and_then(Value::as_str).map(str::to_owned),
        size: f.get("size").and_then(Value::as_u64),
    })
}

/// The chosen project: its preferred file first, then fallbacks (older or less stable versions).
#[derive(Clone, Debug)]
pub struct Pick {
    pub project_id: String,
    pub version_id: String,
    pub candidates: Vec<FileRef>,
    /// Installed files of the same project, removed once the new one is saved.
    pub replaces: Vec<String>,
    deps: Vec<Dep>,
}

/// Releases first, then betas, then alphas, newest first within each.
fn pick_from(mut versions: Vec<Value>) -> Option<Pick> {
    versions.sort_by_key(|v| match v.get("version_type").and_then(Value::as_str) {
        Some("release") => 0,
        Some("beta") => 1,
        _ => 2,
    });
    let with_files: Vec<(&Value, FileRef)> = versions.iter().filter_map(|v| primary_file(v).map(|f| (v, f))).collect();
    let (first, _) = with_files.first()?;
    Some(Pick {
        project_id: first.get("project_id").and_then(Value::as_str).unwrap_or("").to_owned(),
        version_id: first.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
        deps: deps_of(first),
        candidates: with_files.iter().map(|(_, f)| f.clone()).collect(),
        replaces: Vec::new(),
    })
}

pub fn loader_name(loader: &str) -> &'static str {
    match loader {
        "quilt" => "Quilt",
        "neoforge" => "NeoForge",
        "forge" => "Forge",
        _ => "Fabric",
    }
}

/// A Modrinth install worked out before anything is downloaded.
#[derive(Clone, Debug)]
pub struct ModrinthPlan {
    pub summary: Summary,
    pub main: Pick,
    pub deps: Vec<Pick>,
    /// `.disabled` files to switch back on.
    pub enable_files: Vec<String>,
    titles: HashMap<String, String>,
}

impl ModrinthPlan {
    pub fn title_of(&self, project_id: &str) -> String {
        self.titles.get(project_id).cloned().unwrap_or_else(|| project_id.to_owned())
    }
}

/// A file name as a readable name ("sodium-fabric-0.6.jar" → "sodium fabric").
fn pretty_file(file: &str) -> String {
    let base = file.trim_end_matches(".disabled");
    let base = base.strip_suffix(".jar").or_else(|| base.strip_suffix(".zip")).unwrap_or(base);
    let words: Vec<&str> = base.split(['-', '_', '+', ' ']).take_while(|w| !w.starts_with(|c: char| c.is_ascii_digit()) && !w.starts_with("mc")).collect();
    if words.is_empty() {
        file.to_owned()
    } else {
        words.join(" ")
    }
}

/// Project id → name, from Modrinth, falling back to installed file names, then the id.
fn titles_for(api: &dyn Modrinth, ids: &HashSet<String>, installed: &[Installed]) -> HashMap<String, String> {
    let mut list: Vec<String> = ids.iter().cloned().collect();
    list.sort();
    let mut out: HashMap<String, String> = HashMap::new();
    for i in installed {
        if let Some(p) = &i.project_id {
            out.entry(p.clone()).or_insert_with(|| pretty_file(&i.file));
        }
    }
    for p in api.projects(&list).unwrap_or_default() {
        if let (Some(id), Some(t)) = (p.get("id").and_then(Value::as_str), p.get("title").and_then(Value::as_str)) {
            out.insert(id.to_owned(), t.to_owned());
        }
    }
    for id in list {
        out.entry(id.clone()).or_insert(id);
    }
    out
}

/// A pair marked incompatible: `owner` lists `target`.
#[derive(PartialEq)]
struct Clash {
    owner: String,
    owner_file: Option<String>,
    target: String,
    target_file: Option<String>,
}

fn clash_message(c: &Clash, t: &HashMap<String, String>) -> String {
    let name = |id: &str, file: &Option<String>| match file {
        Some(f) => format!("{} (installed: {f})", t.get(id).map(String::as_str).unwrap_or(id)),
        None => t.get(id).cloned().unwrap_or_else(|| id.to_owned()),
    };
    format!("{} is marked incompatible with {}. Using both can stop the game from starting.", name(&c.owner, &c.owner_file), name(&c.target, &c.target_file))
}

fn version_matches(rule: &Dep, version_id: Option<&str>) -> bool {
    rule.version_id.is_none() || rule.version_id.as_deref() == version_id
}

/// Works out what installing `project` (slug or id) into an instance means.
pub fn plan_modrinth(api: &dyn Modrinth, project: &str, project_type: &str, mc: &str, loader: &str, installed: &[Installed]) -> Result<ModrinthPlan, String> {
    let is_mod = project_type == "mod";
    let loaders: Vec<&str> = if is_mod { crate::forge::compatible_modrinth_loaders(loader) } else { Vec::new() };
    let mut versions = api.versions(project, Some(mc), &loaders)?;
    if versions.is_empty() && project_type == "shader" {
        // Shader packs rarely tag game versions; any release works with Iris.
        versions = api.versions(project, None, &[])?;
    }
    let for_what = if is_mod { format!("Minecraft {mc} with {}", loader_name(loader)) } else { format!("Minecraft {mc}") };
    let mut main = pick_from(versions).ok_or_else(|| format!("No version of this works with {for_what}."))?;

    let mut by_pid: HashMap<&str, Vec<&Installed>> = HashMap::new();
    for i in installed {
        if let Some(p) = &i.project_id {
            by_pid.entry(p.as_str()).or_default().push(i);
        }
    }
    main.replaces = by_pid.get(main.project_id.as_str()).map(|v| v.iter().map(|i| i.file.clone()).collect()).unwrap_or_default();

    let mut resolver = Resolver::new(api);
    let mut picks: Vec<Pick> = Vec::new();
    let mut seen: HashSet<String> = HashSet::from([main.project_id.clone()]);
    let mut present: Vec<String> = Vec::new();
    let mut enable_files: Vec<String> = Vec::new();
    let mut optional: Vec<String> = Vec::new();
    let mut missing: Vec<(String, String)> = Vec::new(); // (dependency, needed by)
    if is_mod {
        let mut queue: VecDeque<(String, Vec<Dep>, usize)> = VecDeque::from([(main.project_id.clone(), main.deps.clone(), 0)]);
        while let Some((parent, deps, depth)) = queue.pop_front() {
            for d in &deps {
                match d.kind.as_str() {
                    "required" => {
                        let Some(pid) = resolver.project_of(d) else { continue };
                        if !seen.insert(pid.clone()) {
                            continue;
                        }
                        if let Some(files) = by_pid.get(pid.as_str()) {
                            present.push(pid.clone());
                            if !files.iter().any(|f| f.enabled) {
                                enable_files.push(files[0].file.clone());
                            }
                            continue;
                        }
                        match pick_from(api.versions(&pid, Some(mc), &loaders)?) {
                            None => missing.push((pid, parent.clone())),
                            Some(p) => {
                                if depth + 1 < MAX_DEPTH {
                                    queue.push_back((pid, p.deps.clone(), depth + 1));
                                }
                                picks.push(p);
                            }
                        }
                    }
                    "optional" if depth == 0 => {
                        if let Some(pid) = resolver.project_of(d) {
                            if !by_pid.contains_key(pid.as_str()) && !optional.contains(&pid) && pid != main.project_id {
                                optional.push(pid);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // A dependency also suggested as optional by the main mod isn't a suggestion.
    optional.retain(|o| !seen.contains(o));

    // Incompatibilities, both ways, between what's being installed and what's switched on.
    let mut clashes: Vec<Clash> = Vec::new();
    if is_mod {
        let new_mods: Vec<(&str, &str, &[Dep])> = std::iter::once(&main).chain(&picks).map(|p| (p.project_id.as_str(), p.version_id.as_str(), p.deps.as_slice())).collect();
        let active: Vec<&Installed> = installed.iter().filter(|i| i.enabled && i.project_id.is_some() && i.project_id.as_deref() != Some(main.project_id.as_str())).collect();
        let mut push = |c: Clash| {
            let same_pair = |o: &Clash| (o.owner == c.owner && o.target == c.target) || (o.owner == c.target && o.target == c.owner);
            if !clashes.iter().any(same_pair) {
                clashes.push(c);
            }
        };
        for (owner, _, deps) in &new_mods {
            for d in deps.iter().filter(|d| d.kind == "incompatible") {
                let Some(target) = resolver.project_of(d) else { continue };
                for inst in active.iter().filter(|i| i.project_id.as_deref() == Some(target.as_str()) && version_matches(d, i.version_id.as_deref())) {
                    push(Clash { owner: owner.to_string(), owner_file: None, target: target.clone(), target_file: Some(inst.file.clone()) });
                }
                for (other, vid, _) in &new_mods {
                    if *other == target && other != owner && version_matches(d, Some(vid)) {
                        push(Clash { owner: owner.to_string(), owner_file: None, target: target.clone(), target_file: None });
                    }
                }
            }
        }
        for inst in &active {
            for d in inst.deps.iter().filter(|d| d.kind == "incompatible") {
                let Some(target) = resolver.project_of(d) else { continue };
                if let Some((pid, _, _)) = new_mods.iter().find(|(p, v, _)| *p == target && version_matches(d, Some(v))) {
                    push(Clash { owner: inst.project_id.clone().unwrap_or_default(), owner_file: Some(inst.file.clone()), target: pid.to_string(), target_file: None });
                }
            }
        }
    }

    // Two switched-on copies of one project (the one being installed replaces its own copies).
    let mut duplicates: Vec<(String, Vec<String>)> = by_pid
        .iter()
        .filter(|(pid, _)| **pid != main.project_id)
        .map(|(pid, files)| (pid.to_string(), files.iter().filter(|f| f.enabled).map(|f| f.file.clone()).collect::<Vec<_>>()))
        .filter(|(_, files)| files.len() > 1)
        .collect();
    duplicates.sort();

    let mut ids: HashSet<String> = HashSet::from([main.project_id.clone()]);
    ids.extend(picks.iter().map(|p| p.project_id.clone()));
    ids.extend(present.iter().cloned());
    ids.extend(optional.iter().cloned());
    ids.extend(missing.iter().flat_map(|(a, b)| [a.clone(), b.clone()]));
    ids.extend(clashes.iter().flat_map(|c| [c.owner.clone(), c.target.clone()]));
    ids.extend(duplicates.iter().map(|(p, _)| p.clone()));
    ids.retain(|i| !i.is_empty());
    let titles = titles_for(api, &ids, installed);
    let t = |id: &str| titles.get(id).cloned().unwrap_or_else(|| id.to_owned());

    let mut problems = Vec::new();
    for c in &clashes {
        problems.push(Problem { kind: "incompatible", message: clash_message(c, &titles), blocking: true });
    }
    for (dep, by) in &missing {
        problems.push(Problem {
            kind: "missing",
            message: format!("{} needs {}, which has no version for {for_what}, so it can't be installed for this version.", t(by), t(dep)),
            blocking: true,
        });
    }
    for (pid, files) in &duplicates {
        problems.push(Problem {
            kind: "duplicate",
            message: format!("{} is installed {} times ({}). Two copies stop the game from starting: remove all but one.", t(pid), files.len(), files.join(", ")),
            blocking: true,
        });
    }
    let summary = Summary {
        title: t(&main.project_id),
        deps: picks.iter().map(|p| t(&p.project_id)).collect(),
        present: present.iter().map(|p| t(p)).collect(),
        enable: enable_files.clone(),
        optional: optional.iter().map(|p| t(p)).collect(),
        problems,
    };
    Ok(ModrinthPlan { summary, main, deps: picks, enable_files, titles })
}

/// Why a plan didn't install.
#[derive(Debug)]
pub enum ExecError {
    /// A problem found while downloading (a mod needs a newer Fabric Loader): ask the player.
    Blocked(Problem),
    Failed(String),
}

/// What was installed, and what couldn't be (a dependency whose download failed).
#[derive(Debug)]
pub struct Done {
    #[allow(dead_code)] // read by tests
    pub files: Vec<String>,
    pub warnings: Vec<String>,
}

/// Downloads every file of a plan (checking each with `loader_ok` unless `force`), and only
/// when all are in hand saves them, removes the older copies they replace and switches needed
/// dependencies back on.
pub fn execute_modrinth(
    plan: &ModrinthPlan,
    dest_dir: &Path,
    force: bool,
    download: &dyn Fn(&FileRef, &Path) -> Result<(), String>,
    loader_ok: &dyn Fn(&Path) -> bool,
    log: &dyn Fn(&str),
) -> Result<Done, ExecError> {
    fs::create_dir_all(dest_dir).map_err(|e| ExecError::Failed(format!("Couldn't create {}: {e}", dest_dir.display())))?;
    let mut staged: Vec<(&Pick, PathBuf, String)> = Vec::new();
    let cleanup = |staged: &[(&Pick, PathBuf, String)]| {
        for (_, tmp, _) in staged {
            let _ = fs::remove_file(tmp);
        }
    };
    let mut warnings = Vec::new();
    for (i, pick) in std::iter::once(&plan.main).chain(&plan.deps).enumerate() {
        let title = plan.title_of(&pick.project_id);
        let mut too_new = false;
        let mut got = None;
        for c in &pick.candidates {
            let tmp = dest_dir.join(format!(".tmp_{}_{}_{}_{}", std::process::id(), crate::now_millis(), i, c.filename));
            if let Err(e) = download(c, &tmp) {
                log(&e);
                let _ = fs::remove_file(&tmp);
                continue;
            }
            if !force && !loader_ok(&tmp) {
                log(&format!("Skipped {} (needs a newer Fabric Loader)", c.filename));
                let _ = fs::remove_file(&tmp);
                too_new = true;
                continue;
            }
            got = Some((tmp, c.filename.clone()));
            break;
        }
        match got {
            Some((tmp, name)) => staged.push((pick, tmp, name)),
            None if too_new => {
                cleanup(&staged);
                return Err(ExecError::Blocked(Problem {
                    kind: "loader",
                    message: format!(
                        "{title} needs a newer Fabric Loader than this instance has. Reinstall the instance's Fabric version to get the newest loader, or install anyway at your own risk."
                    ),
                    blocking: true,
                }));
            }
            None if i == 0 => {
                cleanup(&staged);
                return Err(ExecError::Failed(format!("No file of {title} could be downloaded. Check your connection and try again.")));
            }
            None => warnings.push(format!("{title} couldn't be downloaded")),
        }
    }
    let mut files = Vec::new();
    for (n, (pick, tmp, name)) in staged.iter().enumerate() {
        if let Err(e) = fs::rename(tmp, dest_dir.join(name)) {
            cleanup(&staged[n..]);
            return Err(ExecError::Failed(format!("Couldn't save {name}: {e}")));
        }
        // The older copies of the same project go, so the game never loads it twice.
        for old in pick.replaces.iter().filter(|o| *o != name && crate::safe_file_name(o)) {
            if fs::remove_file(dest_dir.join(old)).is_ok() {
                log(&format!("Replaced {old}"));
            }
        }
        log(&format!("Installed {name}"));
        files.push(name.clone());
    }
    for f in &plan.enable_files {
        if let Some(on) = f.strip_suffix(".disabled") {
            if !dest_dir.join(on).exists() && fs::rename(dest_dir.join(f), dest_dir.join(on)).is_ok() {
                log(&format!("Switched on {on} (a required dependency)"));
            }
        }
    }
    Ok(Done { files, warnings })
}

// ───────────────────────── Horizon content mods ─────────────────────────

/// A rule in `horizon-mod.toml`: a dependency, or an `[incompatible]`/`[discouraged]` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct HRule {
    pub id: String,
    /// A semver requirement ("*" when left out).
    pub req: String,
    pub optional: bool,
    pub reason: String,
}

/// What Lumos reads from a content mod's manifest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HManifest {
    pub id: String,
    pub version: Option<String>,
    pub deps: Vec<HRule>,
    pub incompatible: Vec<HRule>,
    pub discouraged: Vec<HRule>,
}

/// Parses `horizon-mod.toml` text (rules for the dedicated server only are left out).
pub fn parse_manifest(text: &str) -> Option<HManifest> {
    let v: toml::Value = toml::from_str(text).ok()?;
    let id = v.get("id")?.as_str()?.to_owned();
    let rules = |table: &str| -> Vec<HRule> {
        let Some(t) = v.get(table).and_then(toml::Value::as_table) else { return Vec::new() };
        t.iter()
            .filter_map(|(k, val)| match val {
                toml::Value::String(req) => Some(HRule { id: k.clone(), req: req.clone(), optional: false, reason: String::new() }),
                toml::Value::Table(t) => {
                    if t.get("side").and_then(toml::Value::as_str) == Some("server") {
                        return None;
                    }
                    Some(HRule {
                        id: k.clone(),
                        req: t.get("version").and_then(toml::Value::as_str).unwrap_or("*").to_owned(),
                        optional: t.get("optional").and_then(toml::Value::as_bool).unwrap_or(false),
                        reason: t.get("reason").and_then(toml::Value::as_str).unwrap_or("").to_owned(),
                    })
                }
                _ => None,
            })
            .collect()
    };
    Some(HManifest {
        version: v.get("version").and_then(toml::Value::as_str).map(str::to_owned),
        deps: rules("dependencies"),
        incompatible: rules("incompatible"),
        discouraged: rules("discouraged"),
        id,
    })
}

fn manifest_from_zip<R: Read + std::io::Seek>(r: R) -> Option<HManifest> {
    let mut zip = zip::ZipArchive::new(r).ok()?;
    let mut text = String::new();
    zip.by_name("horizon-mod.toml").ok()?.take(256 * 1024).read_to_string(&mut text).ok()?;
    parse_manifest(&text)
}

pub fn manifest_of_bytes(bytes: &[u8]) -> Option<HManifest> {
    manifest_from_zip(std::io::Cursor::new(bytes))
}

/// A Horizon mod version ("1" means 1.0.0, as Horizon pads it).
fn h_version(v: &str) -> Option<semver::Version> {
    let v = v.trim();
    let padded = match v.matches('.').count() {
        0 => format!("{v}.0.0"),
        1 => format!("{v}.0"),
        _ => v.to_owned(),
    };
    semver::Version::parse(&padded).ok()
}

/// Whether `version` meets `req`; None when either can't be read.
fn req_matches(req: &str, version: Option<&str>) -> Option<bool> {
    let req = req.trim();
    if req == "*" || req.is_empty() {
        return Some(true);
    }
    let r = semver::VersionReq::parse(req).ok()?;
    Some(r.matches(&h_version(version?)?))
}

/// A content mod in the instance's `horizon-mods/` folder.
#[derive(Clone, Debug)]
pub struct HInstalled {
    pub file: String,
    pub manifest: HManifest,
    pub enabled: bool,
}

pub fn scan_horizon(game_dir: &Path) -> Vec<HInstalled> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(game_dir.join(horizon_mods::MODS_DIR)) {
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().into_owned();
            if file.starts_with('.') || !e.path().is_file() {
                continue;
            }
            if let Some(m) = fs::File::open(e.path()).ok().and_then(manifest_from_zip) {
                out.push(HInstalled { enabled: horizon_mods::is_enabled(game_dir, &m.id), manifest: m, file });
            }
        }
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

/// Horizon's registry (mocked in tests).
pub trait HorizonRegistry {
    /// The registry kind and display name of a project, when it's listed.
    fn kind(&self, id: &str) -> Option<(String, String)>;
    /// Downloads and checks a project's current file.
    fn fetch(&self, id: &str) -> Result<Fetched, String>;
}

pub struct LiveHorizon;

impl HorizonRegistry for LiveHorizon {
    fn kind(&self, id: &str) -> Option<(String, String)> {
        horizon_mods::project_kind(id)
    }
    fn fetch(&self, id: &str) -> Result<Fetched, String> {
        horizon_mods::fetch(id)
    }
}

pub struct HorizonPlan {
    pub summary: Summary,
    pub main: Fetched,
    /// Required dependencies to install, already downloaded and checked.
    pub deps: Vec<Fetched>,
    /// Installed but switched-off dependencies to switch on.
    pub enable_ids: Vec<String>,
}

/// A mod in the set the instance would load after the install.
struct Loaded<'a> {
    id: &'a str,
    title: String,
    version: Option<&'a str>,
    manifest: Option<&'a HManifest>,
    new: bool,
}

/// Version, incompatible and discouraged rules between loaded mods. With `only_new`, only
/// rules that involve a mod being installed (the rest is "Check mods" business).
fn horizon_rules(loaded: &[Loaded], only_new: bool) -> Vec<Problem> {
    let mut out = Vec::new();
    for a in loaded {
        let Some(m) = a.manifest else { continue };
        for (b, r, what) in m
            .deps
            .iter()
            .filter(|r| !r.optional)
            .map(|r| (r, "dep"))
            .chain(m.incompatible.iter().map(|r| (r, "incompatible")))
            .chain(m.discouraged.iter().map(|r| (r, "discouraged")))
            .filter_map(|(r, w)| loaded.iter().find(|b| b.id == r.id && b.id != a.id).map(|b| (b, r, w)))
        {
            if only_new && !a.new && !b.new {
                continue;
            }
            let because = if r.reason.is_empty() { String::new() } else { format!(" ({})", r.reason) };
            let ver = b.version.unwrap_or("unknown version");
            match what {
                "dep" if req_matches(&r.req, b.version) == Some(false) => out.push(Problem {
                    kind: "version",
                    message: format!("{} needs {} {}, but {} {ver} would be loaded.", a.title, b.title, r.req, b.title),
                    blocking: true,
                }),
                "incompatible" if req_matches(&r.req, b.version) == Some(true) => out.push(Problem {
                    kind: "incompatible",
                    message: format!("{} doesn't work with {} {ver}{because}. Horizon refuses to start with both.", a.title, b.title),
                    blocking: true,
                }),
                "discouraged" if req_matches(&r.req, b.version) == Some(true) => out.push(Problem {
                    kind: "discouraged",
                    message: format!("{} advises against {} {ver}{because}.", a.title, b.title),
                    blocking: false,
                }),
                _ => {}
            }
        }
    }
    out
}

fn horizon_duplicates(installed: &[HInstalled], except: &HashSet<&str>) -> Vec<Problem> {
    let mut by_id: Vec<(&str, Vec<&str>)> = Vec::new();
    for i in installed {
        match by_id.iter_mut().find(|(id, _)| *id == i.manifest.id) {
            Some((_, files)) => files.push(&i.file),
            None => by_id.push((&i.manifest.id, vec![&i.file])),
        }
    }
    by_id
        .into_iter()
        .filter(|(id, files)| files.len() > 1 && !except.contains(id))
        .map(|(id, files)| Problem {
            kind: "duplicate",
            message: format!("{id} is installed {} times ({}). Two copies stop Horizon from loading mods: remove all but one.", files.len(), files.join(", ")),
            blocking: true,
        })
        .collect()
}

/// Works out installing Horizon registry project `id` into an instance: its manifest's
/// dependencies (fetched from the registry when missing), rules and duplicates.
pub fn plan_horizon(reg: &dyn HorizonRegistry, id: &str, installed: &[HInstalled], native_ids: &[String]) -> Result<HorizonPlan, String> {
    let main = reg.fetch(id)?;
    let mut summary = Summary { title: main.title.clone(), ..Summary::default() };
    if main.kind != "mod" {
        return Ok(HorizonPlan { summary, main, deps: Vec::new(), enable_ids: Vec::new() });
    }
    let main_m = manifest_of_bytes(&main.bytes).ok_or(format!("{} isn't a valid Horizon mod.", main.name))?;
    let title_of = |id: &str| reg.kind(id).map(|(_, t)| t).filter(|t| !t.is_empty()).unwrap_or_else(|| id.to_owned());
    let mut deps: Vec<(Fetched, HManifest)> = Vec::new();
    let mut enable_ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::from([id.to_owned()]);
    let mut queue: VecDeque<(String, HManifest, usize)> = VecDeque::from([(main.title.clone(), main_m.clone(), 0)]);
    while let Some((owner, m, depth)) = queue.pop_front() {
        for r in &m.deps {
            let have: Vec<&HInstalled> = installed.iter().filter(|i| i.manifest.id == r.id).collect();
            let known = !have.is_empty() || native_ids.contains(&r.id);
            if r.optional {
                if depth == 0 && !known && !summary.optional.contains(&title_of(&r.id)) {
                    summary.optional.push(title_of(&r.id));
                }
                continue;
            }
            if !seen.insert(r.id.clone()) {
                continue;
            }
            if known {
                summary.present.push(title_of(&r.id));
                if !have.is_empty() && !have.iter().any(|i| i.enabled) {
                    enable_ids.push(r.id.clone());
                    summary.enable.push(title_of(&r.id));
                }
                continue;
            }
            let missing = |why: String| Problem { kind: "missing", message: format!("{owner} needs {why}, so it can't be installed with it."), blocking: true };
            match reg.kind(&r.id) {
                None => summary.problems.push(missing(format!("\"{}\", which isn't in Horizon's mod registry", r.id))),
                Some((k, t)) if k == "native-mod" => summary.problems.push(Problem {
                    kind: "missing",
                    message: format!("{owner} needs {t}, a native Rust mod Lumos doesn't add automatically. Install {t} from Discover first."),
                    blocking: true,
                }),
                Some((k, t)) if k != "mod" => summary.problems.push(missing(format!("{t}, which isn't a content mod"))),
                Some((_, t)) => match reg.fetch(&r.id) {
                    Ok(f) => match manifest_of_bytes(&f.bytes) {
                        Some(dm) => {
                            if depth + 1 < MAX_DEPTH {
                                queue.push_back((t.clone(), dm.clone(), depth + 1));
                            }
                            summary.deps.push(t);
                            deps.push((f, dm));
                        }
                        None => summary.problems.push(missing(format!("{t}, whose file isn't a valid Horizon mod"))),
                    },
                    Err(e) => summary.problems.push(missing(format!("{t}, which couldn't be downloaded ({e})"))),
                },
            }
        }
    }
    // The set Horizon would load: what's new, plus what's switched on (or about to be).
    let new_ids: HashSet<&str> = std::iter::once(id).chain(deps.iter().map(|(f, _)| f.id.as_str())).collect();
    let mut loaded: Vec<Loaded> = vec![Loaded { id, title: main.title.clone(), version: main_m.version.as_deref(), manifest: Some(&main_m), new: true }];
    for (f, m) in &deps {
        loaded.push(Loaded { id: &f.id, title: f.title.clone(), version: m.version.as_deref(), manifest: Some(m), new: true });
    }
    for i in installed.iter().filter(|i| (i.enabled || enable_ids.contains(&i.manifest.id)) && !new_ids.contains(i.manifest.id.as_str())) {
        if !loaded.iter().any(|l| l.id == i.manifest.id) {
            loaded.push(Loaded { id: &i.manifest.id, title: title_of(&i.manifest.id), version: i.manifest.version.as_deref(), manifest: Some(&i.manifest), new: false });
        }
    }
    for n in native_ids {
        if !loaded.iter().any(|l| l.id == n) {
            loaded.push(Loaded { id: n, title: title_of(n), version: None, manifest: None, new: false });
        }
    }
    summary.problems.extend(horizon_rules(&loaded, true));
    summary.problems.extend(horizon_duplicates(installed, &new_ids));
    let deps = deps.into_iter().map(|(f, _)| f).collect();
    Ok(HorizonPlan { summary, main, deps, enable_ids })
}

/// Saves a Horizon plan: dependencies first, then the mod; switched-off dependencies go on.
pub fn execute_horizon(plan: &HorizonPlan, game_dir: &Path) -> Result<(), String> {
    for f in plan.deps.iter().chain(std::iter::once(&plan.main)) {
        horizon_mods::save(game_dir, f)?;
    }
    for id in &plan.enable_ids {
        horizon_mods::set_enabled(game_dir, id, true)?;
    }
    Ok(())
}

// ───────────────────────── Checking installed mods ─────────────────────────

/// A required dependency nothing provides.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MissingDep {
    /// Modrinth project id or Horizon mod id (what an install request takes).
    pub id: String,
    pub title: String,
    #[serde(rename = "neededBy")]
    pub needed_by: Vec<String>,
    /// There's a version for this instance (or it's in Horizon's registry).
    pub installable: bool,
    /// It's installed but switched off: this file can be switched on instead.
    #[serde(rename = "disabledFile")]
    pub disabled_file: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Report {
    pub checked: usize,
    /// Files Modrinth doesn't know (their dependencies can't be checked).
    pub unknown: Vec<String>,
    pub missing: Vec<MissingDep>,
    pub conflicts: Vec<String>,
    pub duplicates: Vec<String>,
    pub warnings: Vec<String>,
}

/// Checks a Modrinth instance's switched-on mods against each other.
pub fn check_modrinth(api: &dyn Modrinth, installed: &[Installed], mc: &str, loader: &str) -> Result<Report, String> {
    let mut resolver = Resolver::new(api);
    let active: Vec<&Installed> = installed.iter().filter(|i| i.enabled && i.project_id.is_some()).collect();
    let active_ids: HashSet<&str> = active.iter().filter_map(|i| i.project_id.as_deref()).collect();
    let mut missing: Vec<(String, Vec<String>)> = Vec::new(); // (dependency, needed by)
    let mut clashes: Vec<Clash> = Vec::new();
    for a in &active {
        let owner = a.project_id.clone().unwrap_or_default();
        for d in &a.deps {
            let Some(target) = resolver.project_of(d) else { continue };
            match d.kind.as_str() {
                "required" if !active_ids.contains(target.as_str()) => match missing.iter_mut().find(|(m, _)| *m == target) {
                    Some((_, by)) => by.push(owner.clone()),
                    None => missing.push((target, vec![owner.clone()])),
                },
                "incompatible" => {
                    for b in active.iter().filter(|b| b.project_id.as_deref() == Some(target.as_str()) && version_matches(d, b.version_id.as_deref())) {
                        let c = Clash { owner: owner.clone(), owner_file: Some(a.file.clone()), target: target.clone(), target_file: Some(b.file.clone()) };
                        if !clashes.iter().any(|o| (o.owner == c.owner && o.target == c.target) || (o.owner == c.target && o.target == c.owner)) {
                            clashes.push(c);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut dup: Vec<(String, Vec<String>)> = Vec::new();
    for a in &active {
        let pid = a.project_id.clone().unwrap_or_default();
        match dup.iter_mut().find(|(p, _)| *p == pid) {
            Some((_, files)) => files.push(a.file.clone()),
            None => dup.push((pid, vec![a.file.clone()])),
        }
    }
    dup.retain(|(_, f)| f.len() > 1);
    let mut ids: HashSet<String> = missing.iter().flat_map(|(m, by)| std::iter::once(m.clone()).chain(by.iter().cloned())).collect();
    ids.extend(clashes.iter().flat_map(|c| [c.owner.clone(), c.target.clone()]));
    ids.extend(dup.iter().map(|(p, _)| p.clone()));
    let titles = titles_for(api, &ids, installed);
    let t = |id: &str| titles.get(id).cloned().unwrap_or_else(|| id.to_owned());
    let loaders = crate::forge::compatible_modrinth_loaders(loader);
    let mut report = Report {
        checked: installed.len(),
        unknown: installed.iter().filter(|i| i.project_id.is_none()).map(|i| i.file.clone()).collect(),
        conflicts: clashes.iter().map(|c| clash_message(c, &titles)).collect(),
        duplicates: dup.iter().map(|(p, f)| format!("{} is installed {} times ({}). Remove all but one.", t(p), f.len(), f.join(", "))).collect(),
        ..Report::default()
    };
    for (dep, by) in missing {
        let disabled_file = installed.iter().find(|i| i.project_id.as_deref() == Some(dep.as_str()) && !i.enabled).map(|i| i.file.clone());
        let installable = disabled_file.is_none() && api.versions(&dep, Some(mc), &loaders).map(|v| pick_from(v).is_some()).unwrap_or(false);
        report.missing.push(MissingDep { title: t(&dep), needed_by: by.iter().map(|b| t(b)).collect(), id: dep, installable, disabled_file });
    }
    Ok(report)
}

/// Checks a Horizon instance's content mods (and native mods it has) against each other.
pub fn check_horizon(reg: &dyn HorizonRegistry, installed: &[HInstalled], native_ids: &[String]) -> Report {
    let title_of = |id: &str| reg.kind(id).map(|(_, t)| t).filter(|t| !t.is_empty()).unwrap_or_else(|| id.to_owned());
    let active: Vec<&HInstalled> = installed.iter().filter(|i| i.enabled).collect();
    let mut report = Report { checked: installed.len(), ..Report::default() };
    for a in &active {
        for r in a.manifest.deps.iter().filter(|r| !r.optional) {
            if active.iter().any(|b| b.manifest.id == r.id) || native_ids.contains(&r.id) {
                continue;
            }
            let owner = title_of(&a.manifest.id);
            if let Some(m) = report.missing.iter_mut().find(|m| m.id == r.id) {
                m.needed_by.push(owner);
                continue;
            }
            let disabled_file = installed.iter().find(|i| i.manifest.id == r.id && !i.enabled).map(|i| i.file.clone());
            let installable = disabled_file.is_none() && reg.kind(&r.id).is_some_and(|(k, _)| k == "mod");
            report.missing.push(MissingDep { id: r.id.clone(), title: title_of(&r.id), needed_by: vec![owner], installable, disabled_file });
        }
    }
    let mut loaded: Vec<Loaded> = Vec::new();
    for i in &active {
        if !loaded.iter().any(|l| l.id == i.manifest.id) {
            loaded.push(Loaded { id: &i.manifest.id, title: title_of(&i.manifest.id), version: i.manifest.version.as_deref(), manifest: Some(&i.manifest), new: true });
        }
    }
    for n in native_ids {
        loaded.push(Loaded { id: n, title: title_of(n), version: None, manifest: None, new: true });
    }
    for p in horizon_rules(&loaded, false) {
        if p.blocking { report.conflicts.push(p.message) } else { report.warnings.push(p.message) }
    }
    let active_owned: Vec<HInstalled> = active.into_iter().cloned().collect();
    report.duplicates = horizon_duplicates(&active_owned, &HashSet::new()).into_iter().map(|p| p.message).collect();
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Write;

    /// Canned Modrinth: versions per (project, mc), versions by id, projects, hashes.
    #[derive(Default)]
    struct Mock {
        versions: HashMap<String, Vec<Value>>,
        by_id: HashMap<String, Value>,
        titles: HashMap<String, String>,
        hashes: HashMap<String, Value>,
        asked: RefCell<Vec<String>>,
    }

    impl Mock {
        /// A project with one version per (mc, loaders, deps) given.
        fn project(&mut self, id: &str, title: &str, versions: &[(&str, &[&str], Value)]) {
            self.titles.insert(id.into(), title.into());
            for (n, (mc, loaders, deps)) in versions.iter().enumerate() {
                let v = ver(id, &format!("{id}-v{n}"), mc, loaders, deps.clone());
                self.by_id.insert(format!("{id}-v{n}"), v.clone());
                self.versions.entry(id.into()).or_default().push(v);
            }
        }
    }

    fn ver(pid: &str, vid: &str, mc: &str, loaders: &[&str], deps: Value) -> Value {
        json!({"id": vid, "project_id": pid, "version_type": "release", "game_versions": [mc], "loaders": loaders, "dependencies": deps,
               "files": [{"url": format!("https://cdn.modrinth.com/data/{pid}/{vid}.jar"), "filename": format!("{vid}.jar"), "primary": true,
                          "hashes": {"sha1": format!("sha-{vid}")}, "size": 10}]})
    }

    fn dep(pid: &str, kind: &str) -> Value {
        json!({"project_id": pid, "version_id": null, "dependency_type": kind})
    }

    impl Modrinth for Mock {
        fn versions(&self, project: &str, mc: Option<&str>, loaders: &[&str]) -> Result<Vec<Value>, String> {
            self.asked.borrow_mut().push(project.to_owned());
            let all = self.versions.get(project).cloned().unwrap_or_default();
            Ok(all
                .into_iter()
                .filter(|v| mc.is_none_or(|mc| v["game_versions"].as_array().unwrap().iter().any(|g| g == mc)))
                .filter(|v| loaders.is_empty() || v["loaders"].as_array().unwrap().iter().any(|l| loaders.contains(&l.as_str().unwrap())))
                .collect())
        }
        fn version(&self, id: &str) -> Result<Value, String> {
            self.by_id.get(id).cloned().ok_or("404".into())
        }
        fn projects(&self, ids: &[String]) -> Result<Vec<Value>, String> {
            Ok(ids.iter().filter_map(|i| self.titles.get(i).map(|t| json!({"id": i, "title": t}))).collect())
        }
        fn version_files(&self, sha1s: &[String]) -> Result<HashMap<String, Value>, String> {
            Ok(sha1s.iter().filter_map(|h| self.hashes.get(h).map(|v| (h.clone(), v.clone()))).collect())
        }
    }

    fn installed(pid: &str, vid: &str, file: &str, enabled: bool, deps: Value) -> Installed {
        Installed {
            file: file.into(),
            enabled,
            project_id: Some(pid.into()),
            version_id: Some(vid.into()),
            deps: deps_of(&json!({"dependencies": deps})),
        }
    }

    fn world() -> Mock {
        let mut m = Mock::default();
        let fab: &[&str] = &["fabric"];
        m.project("sodium-extra", "Sodium Extra", &[("1.21.11", fab, json!([dep("sodium", "required"), dep("reeses", "optional"), dep("optifine-ish", "incompatible")]))]);
        m.project("sodium", "Sodium", &[("1.21.11", fab, json!([dep("fabric-api", "required")]))]);
        m.project("fabric-api", "Fabric API", &[("1.21.11", fab, json!([]))]);
        m.project("reeses", "Reese's Sodium Options", &[("1.21.11", fab, json!([]))]);
        m.project("optifine-ish", "OptiFabric", &[("1.21.11", fab, json!([]))]);
        m.project("old-lib", "Old Lib", &[("1.20.1", fab, json!([]))]);
        m.project("needs-old", "Needs Old", &[("1.21.11", fab, json!([dep("old-lib", "required")]))]);
        m.project("hater", "Hater", &[("1.21.11", fab, json!([dep("sodium", "incompatible")]))]);
        m.project("cloth", "Cloth Config", &[("1.21.11", &["fabric", "neoforge"], json!([]))]);
        m.project("neo-mod", "Neo Mod", &[("1.21.1", &["neoforge"], json!([dep("cloth", "required")]))]);
        m.project("quilt-mod", "Quilt Mod", &[("1.21.11", &["quilt"], json!([dep("fabric-api", "required")]))]);
        m
    }

    #[test]
    fn dependencies_are_followed_and_named() {
        let m = world();
        let plan = plan_modrinth(&m, "sodium-extra", "mod", "1.21.11", "fabric", &[]).unwrap();
        assert_eq!(plan.summary.title, "Sodium Extra");
        assert_eq!(plan.summary.deps, vec!["Sodium", "Fabric API"]);
        assert_eq!(plan.summary.optional, vec!["Reese's Sodium Options"]);
        assert!(!plan.summary.blocked(), "{:?}", plan.summary.problems);
        assert_eq!(plan.main.candidates[0].filename, "sodium-extra-v0.jar");
        assert_eq!(plan.summary.success_message(" (Minecraft 1.21.11)", &[]), "Installed Sodium Extra + Sodium, Fabric API (Minecraft 1.21.11). Optional extras: Reese's Sodium Options.");
    }

    #[test]
    fn installed_dependencies_are_left_alone_and_switched_on() {
        let m = world();
        let have = [installed("sodium", "sodium-v0", "sodium-0.6.jar.disabled", false, json!([dep("fabric-api", "required")]))];
        let plan = plan_modrinth(&m, "sodium-extra", "mod", "1.21.11", "fabric", &have).unwrap();
        assert!(plan.summary.deps.is_empty(), "Sodium is there, so Fabric API isn't followed through it: {:?}", plan.summary.deps);
        assert_eq!(plan.summary.present, vec!["Sodium"]);
        assert_eq!(plan.enable_files, vec!["sodium-0.6.jar.disabled"]);
    }

    #[test]
    fn incompatibilities_are_found_both_ways() {
        let m = world();
        // The new mod lists an installed one.
        let have = [installed("optifine-ish", "optifine-ish-v0", "optifabric.jar", true, json!([]))];
        let plan = plan_modrinth(&m, "sodium-extra", "mod", "1.21.11", "fabric", &have).unwrap();
        assert!(plan.summary.blocked());
        assert_eq!(plan.summary.problems[0].kind, "incompatible");
        assert!(plan.summary.problems[0].message.contains("Sodium Extra is marked incompatible with OptiFabric (installed: optifabric.jar)"), "{:?}", plan.summary.problems);
        // A switched-off copy doesn't count.
        let off = [installed("optifine-ish", "optifine-ish-v0", "optifabric.jar.disabled", false, json!([]))];
        assert!(!plan_modrinth(&m, "sodium-extra", "mod", "1.21.11", "fabric", &off).unwrap().summary.blocked());
        // An installed mod lists a dependency of the new one.
        let have = [installed("hater", "hater-v0", "hater.jar", true, json!([dep("sodium", "incompatible")]))];
        let plan = plan_modrinth(&m, "sodium-extra", "mod", "1.21.11", "fabric", &have).unwrap();
        assert!(plan.summary.problems.iter().any(|p| p.message.starts_with("Hater (installed: hater.jar) is marked incompatible with Sodium.")), "{:?}", plan.summary.problems);
        // Version-specific: only that version clashes.
        let have = [installed("hater", "hater-v0", "hater.jar", true, json!([{"project_id": "sodium", "version_id": "sodium-v9", "dependency_type": "incompatible"}]))];
        assert!(!plan_modrinth(&m, "sodium-extra", "mod", "1.21.11", "fabric", &have).unwrap().summary.blocked());
    }

    #[test]
    fn a_dependency_with_no_version_for_this_game_is_reported() {
        let m = world();
        let plan = plan_modrinth(&m, "needs-old", "mod", "1.21.11", "fabric", &[]).unwrap();
        assert!(plan.summary.blocked());
        let p = &plan.summary.problems[0];
        assert_eq!(p.kind, "missing");
        assert_eq!(p.message, "Needs Old needs Old Lib, which has no version for Minecraft 1.21.11 with Fabric, so it can't be installed for this version.");
        assert!(plan_modrinth(&m, "old-lib", "mod", "1.21.11", "fabric", &[]).unwrap_err().contains("No version of this works with Minecraft 1.21.11 with Fabric"));
    }

    #[test]
    fn duplicates_block_except_for_the_mod_being_replaced() {
        let m = world();
        let have = [
            installed("fabric-api", "fabric-api-v0", "fabric-api-a.jar", true, json!([])),
            installed("fabric-api", "fabric-api-v0", "fabric-api-b.jar", true, json!([])),
            installed("sodium-extra", "x", "sodium-extra-old.jar", true, json!([])),
            installed("sodium-extra", "y", "sodium-extra-older.jar", true, json!([])),
        ];
        let plan = plan_modrinth(&m, "sodium-extra", "mod", "1.21.11", "fabric", &have).unwrap();
        let dups: Vec<_> = plan.summary.problems.iter().filter(|p| p.kind == "duplicate").collect();
        assert_eq!(dups.len(), 1, "{:?}", plan.summary.problems);
        assert!(dups[0].message.starts_with("Fabric API is installed 2 times (fabric-api-a.jar, fabric-api-b.jar)"));
        assert_eq!(plan.main.replaces, vec!["sodium-extra-old.jar", "sodium-extra-older.jar"]);
    }

    #[test]
    fn loaders_pick_matching_versions() {
        let m = world();
        // Cloth has no 1.21.1 NeoForge build here, though Neo Mod has one.
        let plan = plan_modrinth(&m, "neo-mod", "mod", "1.21.1", "neoforge", &[]).unwrap();
        assert!(plan.summary.blocked() && plan.summary.problems[0].message.contains("Cloth Config, which has no version for Minecraft 1.21.1 with NeoForge"));
        let mut m = world();
        m.project("cloth", "Cloth Config", &[("1.21.1", &["neoforge"], json!([]))]);
        let plan = plan_modrinth(&m, "neo-mod", "mod", "1.21.1", "neoforge", &[]).unwrap();
        assert_eq!(plan.summary.deps, vec!["Cloth Config"]);
        // Quilt instances take Quilt and Fabric builds.
        let plan = plan_modrinth(&m, "quilt-mod", "mod", "1.21.11", "quilt", &[]).unwrap();
        assert_eq!(plan.summary.deps, vec!["Fabric API"]);
    }

    #[test]
    fn version_only_dependencies_resolve_to_projects() {
        let mut m = world();
        m.project("pinned", "Pinned", &[("1.21.11", &["fabric"], json!([{"project_id": null, "version_id": "fabric-api-v0", "dependency_type": "required"}]))]);
        let plan = plan_modrinth(&m, "pinned", "mod", "1.21.11", "fabric", &[]).unwrap();
        assert_eq!(plan.summary.deps, vec!["Fabric API"]);
    }

    #[test]
    fn dependency_cycles_end() {
        let mut m = Mock::default();
        m.project("a", "A", &[("1.21.11", &["fabric"], json!([dep("b", "required")]))]);
        m.project("b", "B", &[("1.21.11", &["fabric"], json!([dep("a", "required"), dep("b", "required")]))]);
        let plan = plan_modrinth(&m, "a", "mod", "1.21.11", "fabric", &[]).unwrap();
        assert_eq!(plan.summary.deps, vec!["B"]);
        assert_eq!(m.asked.borrow().len(), 2);
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lumos-plan-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn execute_saves_everything_or_nothing() {
        let m = world();
        let dir = temp("exec");
        fs::write(dir.join("sodium-extra-old.jar"), b"old").unwrap();
        fs::write(dir.join("dep.jar.disabled"), b"d").unwrap();
        let have = [installed("sodium-extra", "x", "sodium-extra-old.jar", true, json!([]))];
        let mut plan = plan_modrinth(&m, "sodium-extra", "mod", "1.21.11", "fabric", &have).unwrap();
        plan.enable_files.push("dep.jar.disabled".into());
        let dl = |f: &FileRef, p: &Path| fs::write(p, f.filename.as_bytes()).map_err(|e| e.to_string());
        let done = execute_modrinth(&plan, &dir, false, &dl, &|_| true, &|_| {}).unwrap();
        assert_eq!(done.files, vec!["sodium-extra-v0.jar", "sodium-v0.jar", "fabric-api-v0.jar"]);
        assert!(!dir.join("sodium-extra-old.jar").exists() && dir.join("dep.jar").exists());
        // A dependency that needs a newer loader stops the whole install (nothing saved) unless forced.
        let dir = temp("exec2");
        let picky = |p: &Path| !p.to_string_lossy().contains("fabric-api");
        match execute_modrinth(&plan, &dir, false, &dl, &picky, &|_| {}) {
            Err(ExecError::Blocked(p)) => assert!(p.message.starts_with("Fabric API needs a newer Fabric Loader"), "{p:?}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        assert_eq!(execute_modrinth(&plan, &dir, true, &dl, &picky, &|_| {}).unwrap().files.len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_finds_missing_conflicting_and_doubled_mods() {
        let m = world();
        let have = [
            installed("sodium-extra", "sodium-extra-v0", "se.jar", true, json!([dep("sodium", "required")])),
            installed("sodium", "sodium-v0", "sodium.jar.disabled", false, json!([])),
            installed("needs-old", "needs-old-v0", "no.jar", true, json!([dep("fabric-api", "required")])),
            installed("hater", "hater-v0", "hater.jar", true, json!([dep("sodium-extra", "incompatible")])),
            installed("cloth", "c", "cloth-a.jar", true, json!([])),
            installed("cloth", "c", "cloth-b.jar", true, json!([])),
            Installed { file: "mystery.jar".into(), enabled: true, ..Installed::default() },
        ];
        let r = check_modrinth(&m, &have, "1.21.11", "fabric").unwrap();
        assert_eq!(r.checked, 7);
        assert_eq!(r.unknown, vec!["mystery.jar"]);
        assert_eq!(r.missing.len(), 2);
        assert_eq!(r.missing[0], MissingDep { id: "sodium".into(), title: "Sodium".into(), needed_by: vec!["Sodium Extra".into()], installable: false, disabled_file: Some("sodium.jar.disabled".into()) });
        assert_eq!((r.missing[1].title.as_str(), r.missing[1].installable), ("Fabric API", true));
        assert_eq!(r.conflicts.len(), 1);
        assert!(r.conflicts[0].starts_with("Hater (installed: hater.jar) is marked incompatible with Sodium Extra (installed: se.jar)"));
        assert_eq!(r.duplicates, vec!["Cloth Config is installed 2 times (cloth-a.jar, cloth-b.jar). Remove all but one."]);
    }

    // ── Horizon ──

    fn hmod(manifest: &str) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            z.start_file("horizon-mod.toml", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(manifest.as_bytes()).unwrap();
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[derive(Default)]
    struct Reg {
        projects: HashMap<String, (String, String, String)>, // id → (kind, title, manifest)
    }

    impl Reg {
        fn add(&mut self, id: &str, kind: &str, title: &str, manifest: &str) {
            self.projects.insert(id.into(), (kind.into(), title.into(), manifest.into()));
        }
    }

    impl HorizonRegistry for Reg {
        fn kind(&self, id: &str) -> Option<(String, String)> {
            self.projects.get(id).map(|(k, t, _)| (k.clone(), t.clone()))
        }
        fn fetch(&self, id: &str) -> Result<Fetched, String> {
            let (kind, title, m) = self.projects.get(id).ok_or("not listed")?;
            Ok(Fetched { id: id.into(), title: title.clone(), kind: kind.clone(), name: format!("{id}.hmod"), bytes: hmod(m) })
        }
    }

    fn hinst(manifest: &str, enabled: bool) -> HInstalled {
        let m = parse_manifest(manifest).unwrap();
        HInstalled { file: format!("{}-x.hmod", m.id), manifest: m, enabled }
    }

    #[test]
    fn manifests_are_read() {
        let m = parse_manifest(
            "id = \"a\"\nversion = \"1.2.0\"\n[dependencies]\nbase = \"^1\"\nextra = { version = \">=2\", optional = true }\nsrv = { version = \"*\", side = \"server\" }\n[incompatible]\nold = \"<2\"\n[discouraged]\nshaders = { reason = \"Flickers\" }\n",
        )
        .unwrap();
        assert_eq!(m.id, "a");
        assert_eq!(m.version.as_deref(), Some("1.2.0"));
        assert_eq!(m.deps.len(), 2);
        assert_eq!(m.deps[1], HRule { id: "extra".into(), req: ">=2".into(), optional: true, reason: "".into() });
        assert_eq!(m.incompatible[0].req, "<2");
        assert_eq!(m.discouraged[0], HRule { id: "shaders".into(), req: "*".into(), optional: false, reason: "Flickers".into() });
        assert_eq!(req_matches("^1", Some("1")), Some(true));
        assert_eq!(req_matches("<2", Some("2.0")), Some(false));
        assert_eq!(req_matches("^1", None), None);
    }

    #[test]
    fn horizon_dependencies_install_from_the_registry() {
        let mut reg = Reg::default();
        reg.add("main", "mod", "Main", "id = \"main\"\nversion = \"1.0.0\"\n[dependencies]\nlib = \"^1\"\nnice = { version = \"*\", optional = true }\n");
        reg.add("lib", "mod", "Lib", "id = \"lib\"\nversion = \"1.4.0\"\n[dependencies]\ncore = \"*\"\n");
        reg.add("core", "mod", "Core", "id = \"core\"\nversion = \"0.1.0\"\n");
        reg.add("nice", "mod", "Nice", "id = \"nice\"\nversion = \"1.0.0\"\n");
        let plan = plan_horizon(&reg, "main", &[], &[]).unwrap();
        assert_eq!(plan.summary.deps, vec!["Lib", "Core"]);
        assert_eq!(plan.summary.optional, vec!["Nice"]);
        assert!(!plan.summary.blocked(), "{:?}", plan.summary.problems);
        // Installed (switched off) core is switched on instead of downloaded.
        let plan = plan_horizon(&reg, "main", &[hinst("id = \"core\"\nversion = \"0.1.0\"\n", false)], &[]).unwrap();
        assert_eq!((plan.summary.deps.clone(), plan.enable_ids.clone()), (vec!["Lib".to_owned()], vec!["core".to_owned()]));
        // Saved for real: dependencies and the mod, all switched on.
        let dir = temp("hz");
        execute_horizon(&plan, &dir).unwrap();
        assert!(dir.join("horizon-mods/lib.hmod").is_file() && dir.join("horizon-mods/main.hmod").is_file());
        assert!(horizon_mods::is_enabled(&dir, "main") && horizon_mods::is_enabled(&dir, "lib") && horizon_mods::is_enabled(&dir, "core"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn horizon_problems_are_reported() {
        let mut reg = Reg::default();
        reg.add("main", "mod", "Main", "id = \"main\"\nversion = \"2.0.0\"\n[dependencies]\nlib = \"^2\"\nghost = \"*\"\nzoom = \"*\"\n[incompatible]\nold = { version = \"<2\", reason = \"Breaks saves\" }\n[discouraged]\nshiny = \"*\"\n");
        reg.add("zoom", "native-mod", "Zoom", "");
        let have = [
            hinst("id = \"lib\"\nversion = \"1.5.0\"\n", true),
            hinst("id = \"old\"\nversion = \"1.9\"\n", true),
            hinst("id = \"shiny\"\nversion = \"1.0.0\"\n", true),
            hinst("id = \"fan\"\nversion = \"1.0.0\"\n[dependencies]\nmain = \"^1\"\n", true),
            hinst("id = \"twice\"\n", true),
            hinst("id = \"twice\"\n", true),
        ];
        let plan = plan_horizon(&reg, "main", &have, &[]).unwrap();
        let kinds: Vec<&str> = plan.summary.problems.iter().map(|p| p.kind).collect();
        let msgs: Vec<&str> = plan.summary.problems.iter().map(|p| p.message.as_str()).collect();
        assert!(plan.summary.blocked());
        assert!(kinds.contains(&"missing") && kinds.contains(&"incompatible") && kinds.contains(&"discouraged") && kinds.contains(&"duplicate"), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("\"ghost\", which isn't in Horizon's mod registry")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("Zoom, a native Rust mod")), "{msgs:?}");
        assert!(msgs.contains(&"Main needs lib ^2, but lib 1.5.0 would be loaded."), "{msgs:?}");
        assert!(msgs.contains(&"fan needs Main ^1, but Main 2.0.0 would be loaded."), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.starts_with("Main doesn't work with old 1.9 (Breaks saves)")), "{msgs:?}");
        // With Zoom built in, it counts as present.
        let plan = plan_horizon(&reg, "main", &have, &["zoom".to_owned()]).unwrap();
        assert!(plan.summary.present.contains(&"Zoom".to_owned()));
    }

    #[test]
    fn horizon_check_reports_installed_problems() {
        let mut reg = Reg::default();
        reg.add("lib", "mod", "Lib", "id = \"lib\"\n");
        let have = [
            hinst("id = \"a\"\n[dependencies]\nlib = \"*\"\nb = \"*\"\n", true),
            hinst("id = \"b\"\nversion = \"1.0.0\"\n", false),
            hinst("id = \"c\"\n[incompatible]\na = \"*\"\n", true),
        ];
        let r = check_horizon(&reg, &have, &[]);
        assert_eq!(r.missing.len(), 2);
        let find = |id: &str| r.missing.iter().find(|m| m.id == id).unwrap();
        assert_eq!((find("lib").installable, find("lib").disabled_file.as_deref()), (true, None));
        assert_eq!((find("b").installable, find("b").disabled_file.as_deref()), (false, Some("b-x.hmod")));
        assert_eq!(r.conflicts.len(), 1, "{:?}", r.conflicts);
    }

    /// Live: plans against the real Modrinth API (no files are saved). Run with --ignored.
    #[test]
    #[ignore]
    fn live_modrinth_plans() {
        let plan = plan_modrinth(&Live, "sodium-extra", "mod", "1.21.11", "fabric", &[]).expect("plan");
        eprintln!("{:#?}", plan.summary);
        assert!(plan.summary.deps.iter().any(|d| d == "Sodium"), "{:?}", plan.summary);
        assert!(!plan.summary.blocked());
        // LambDynamicLights lists projects it doesn't work with: pretend the first is installed.
        let ldl = Live.versions("lambdynamiclights", Some("1.21.11"), &["fabric"]).expect("lambdynamiclights");
        let enemy = deps_of(&ldl[0]).into_iter().find(|d| d.kind == "incompatible").and_then(|d| d.project_id).expect("an incompatible project");
        let have = [Installed { file: "enemy.jar".into(), enabled: true, project_id: Some(enemy.clone()), version_id: Some("x".into()), deps: Vec::new() }];
        let plan = plan_modrinth(&Live, "lambdynamiclights", "mod", "1.21.11", "fabric", &have).expect("plan");
        eprintln!("{:#?}", plan.summary);
        assert!(plan.summary.blocked() && plan.summary.problems.iter().any(|p| p.kind == "incompatible"), "{:?}", plan.summary);
        // And the other way round: LambDynamicLights installed, its enemy is planned (when it has a build).
        let have = [Installed { file: "ldl.jar".into(), enabled: true, project_id: ldl[0]["project_id"].as_str().map(str::to_owned), version_id: None, deps: deps_of(&ldl[0]) }];
        if let Ok(plan) = plan_modrinth(&Live, &enemy, "mod", "1.21.11", "fabric", &have) {
            eprintln!("{:#?}", plan.summary);
            assert!(plan.summary.problems.iter().any(|p| p.kind == "incompatible"), "{:?}", plan.summary);
        }
        // NeoForge and Forge pick their own builds.
        let neo = plan_modrinth(&Live, "jei", "mod", "1.21.1", "neoforge", &[]).expect("jei neoforge");
        assert!(neo.main.candidates[0].filename.to_lowercase().contains("neoforge"), "{:?}", neo.main.candidates[0]);
        let forge = plan_modrinth(&Live, "jei", "mod", "1.20.1", "forge", &[]).expect("jei forge");
        assert!(forge.main.candidates[0].filename.to_lowercase().contains("forge"), "{:?}", forge.main.candidates[0]);
        // Full install into a temp folder, then Check mods on it.
        let dir = temp("live");
        let plan = plan_modrinth(&Live, "sodium-extra", "mod", "1.21.11", "fabric", &[]).unwrap();
        let dl = |f: &FileRef, p: &Path| crate::net::download_verified(&f.url, p, f.sha1.as_deref(), f.size);
        let done = execute_modrinth(&plan, &dir, false, &dl, &|_| true, &|l| eprintln!("{l}")).expect("install");
        assert!(done.files.len() >= 2, "{done:?}");
        let installed = identify(&Live, scan_dir(&dir)).unwrap();
        let r = check_modrinth(&Live, &installed, "1.21.11", "fabric").unwrap();
        eprintln!("{r:#?}");
        assert!(r.missing.is_empty() && r.conflicts.is_empty() && r.duplicates.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Live: plans and installs a Horizon content mod from the real registry into a temp folder.
    #[test]
    #[ignore]
    fn live_horizon_plan() {
        let dir = temp("hlive");
        let plan = plan_horizon(&LiveHorizon, "flesh_to_leather", &[], &[]).expect("plan");
        eprintln!("{:#?}", plan.summary);
        assert!(!plan.summary.blocked());
        execute_horizon(&plan, &dir).unwrap();
        let installed = scan_horizon(&dir);
        assert_eq!(installed.len(), 1);
        assert!(installed[0].enabled);
        let r = check_horizon(&LiveHorizon, &installed, &[]);
        assert!(r.missing.is_empty() && r.conflicts.is_empty() && r.duplicates.is_empty(), "{r:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
