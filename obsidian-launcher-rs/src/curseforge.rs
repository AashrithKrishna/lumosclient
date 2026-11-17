//! CurseForge as a Discover source (needs the player's own API key, kept only in the local
//! launcher config and never logged).
//!
//! Search (`/v1/mods/search`, game 432, one class per content type), categories
//! (`/v1/categories`), project pages (description HTML, screenshots, links), files, and
//! installs planned like Modrinth ones (mod_plan.rs `Summary`): required dependencies
//! (relationType 3) are installed with it, incompatible ones (relationType 5) are reported,
//! optional ones (2) suggested. Some authors don't allow downloads outside CurseForge's own
//! app: those files have no `downloadUrl`, and the page offers "Open on CurseForge" instead.
//!
//! What Lumos installed from CurseForge is remembered per content folder in
//! [`SIDECAR`] (file name → project + file id), so cards show Installed and the older copy of a
//! project is replaced on update. CurseForge modpacks (`manifest.json` zips) become a new
//! instance like Modrinth ones; worlds unpack into `saves/`; data packs go into a world.

use crate::discover::Query;
use crate::mod_plan::{Problem, Summary};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const API: &str = "https://api.curseforge.com/v1";
pub const GAME: u64 = 432;
/// Per content folder: what Lumos installed from CurseForge.
pub const SIDECAR: &str = ".lumos-curseforge.json";
const MAX_DEPTH: usize = 6;
const CATS_FRESH: Duration = Duration::from_secs(6 * 3600);
const SITE: &str = "https://www.curseforge.com/minecraft";

// ───────────────────────── Ids ─────────────────────────

/// CurseForge class for a Discover content type.
pub fn class_id(project_type: &str) -> Option<u64> {
    match project_type {
        "mod" => Some(6),
        "resourcepack" => Some(12),
        "shader" => Some(6552),
        "modpack" => Some(4471),
        "world" => Some(17),
        "datapack" => Some(6945),
        _ => None,
    }
}

pub fn type_of_class(class: u64) -> &'static str {
    match class {
        12 => "resourcepack",
        6552 => "shader",
        4471 => "modpack",
        17 => "world",
        6945 => "datapack",
        _ => "mod",
    }
}

/// The site path of a class ("mc-mods", "texture-packs", ...).
fn site_section(project_type: &str) -> &'static str {
    match project_type {
        "resourcepack" => "texture-packs",
        "shader" => "shaders",
        "modpack" => "modpacks",
        "world" => "worlds",
        "datapack" => "data-packs",
        _ => "mc-mods",
    }
}

/// `modLoaderType` values for a loader (Quilt instances also run Fabric mods).
pub fn loader_types(loader: &str) -> Vec<u64> {
    match loader {
        "forge" => vec![1],
        "fabric" => vec![4],
        "quilt" => vec![5, 4],
        "neoforge" => vec![6],
        _ => vec![],
    }
}

/// CurseForge `sortField` for a sort choice (CurseForge has no follower count: popularity).
pub fn sort_field(sort: &str) -> u64 {
    match sort {
        "relevance" => 1,
        "follows" => 2,
        "updated" => 3,
        "newest" => 11,
        _ => 6,
    }
}

/// Where CurseForge files may be downloaded from (https only).
pub fn allowed_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
    !host.contains('@') && !host.contains(':') && matches!(host.as_str(), "edge.forgecdn.net" | "mediafilez.forgecdn.net" | "media.forgecdn.net")
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn n(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

// ───────────────────────── Access ─────────────────────────

pub trait Api {
    /// GET `API` + `path` (path starts with `/`).
    fn get(&self, path: &str) -> Result<Value, String>;
    fn post(&self, path: &str, body: Value) -> Result<Value, String>;
}

/// The real API with the player's key.
pub struct Live {
    pub key: String,
}

fn cf_error(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Status(401 | 403, _) => "CurseForge didn't accept the API key. Check it in Settings → Discover.".into(),
        ureq::Error::Status(404, _) => "CurseForge doesn't have that.".into(),
        other => crate::net::describe_error("api.curseforge.com", other),
    }
}

impl Api for Live {
    fn get(&self, path: &str) -> Result<Value, String> {
        let resp = crate::net::agent().get(&format!("{API}{path}")).set("x-api-key", &self.key).set("Accept", "application/json").call().map_err(|e| cf_error(&e))?;
        resp.into_json().map_err(|e| format!("CurseForge sent data the launcher couldn't read: {e}"))
    }
    fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        let resp = crate::net::agent().post(&format!("{API}{path}")).set("x-api-key", &self.key).set("Accept", "application/json").send_json(body).map_err(|e| cf_error(&e))?;
        resp.into_json().map_err(|e| format!("CurseForge sent data the launcher couldn't read: {e}"))
    }
}

/// The page's message when no key is saved.
pub const NO_KEY: &str = "CurseForge needs an API key. Get one at console.curseforge.com, then paste it in Settings → Discover → CurseForge API key.";

// ───────────────────────── Search ─────────────────────────

pub fn search_path(q: &Query) -> String {
    let mut p = format!(
        "/mods/search?gameId={GAME}&classId={}&sortField={}&sortOrder=desc&index={}&pageSize={}",
        class_id(&q.project_type).unwrap_or(6),
        sort_field(&q.sort),
        q.offset.min(9_950),
        q.limit.min(50)
    );
    if !q.text.is_empty() {
        p.push_str(&format!("&searchFilter={}", crate::url_encode(&q.text)));
    }
    if let Some(gv) = &q.game_version {
        p.push_str(&format!("&gameVersion={}", crate::url_encode(gv)));
    }
    if q.project_type == "mod" || q.project_type == "modpack" {
        let types: Vec<u64> = q.loaders.iter().flat_map(|l| loader_types(l)).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        if !types.is_empty() {
            p.push_str(&format!("&modLoaderTypes={}", crate::url_encode(&json!(types).to_string())));
        }
    }
    let ids: Vec<u64> = q.categories.iter().filter_map(|c| c.parse().ok()).take(10).collect();
    if !ids.is_empty() {
        p.push_str(&format!("&categoryIds={}", crate::url_encode(&json!(ids).to_string())));
    }
    p
}

fn website(m: &Value) -> String {
    m.pointer("/links/websiteUrl").and_then(Value::as_str).filter(|u| u.starts_with("https://")).map(str::to_owned).unwrap_or_else(|| {
        let t = type_of_class(n(m, "classId"));
        format!("{SITE}/{}/{}", site_section(t), s(m, "slug"))
    })
}

/// A CurseForge mod in the page's card shape.
pub fn normalize_mod(m: &Value) -> Value {
    let shots: Vec<&Value> = m.get("screenshots").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let banner = shots.first().map(|g| Some(s(g, "thumbnailUrl")).filter(|x| !x.is_empty()).unwrap_or(s(g, "url")).to_owned());
    let cats: Vec<&str> = m.get("categories").and_then(Value::as_array).map(|a| a.iter().map(|c| s(c, "name")).filter(|c| !c.is_empty()).collect()).unwrap_or_default();
    let loaders: Vec<&str> = m
        .get("latestFilesIndexes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|i| i.get("modLoader").and_then(Value::as_u64)).map(loader_key).filter(|l| !l.is_empty()).collect::<std::collections::BTreeSet<_>>().into_iter().collect())
        .unwrap_or_default();
    json!({
        "id": n(m, "id").to_string(),
        "slug": s(m, "slug"),
        "title": s(m, "name"),
        "description": s(m, "summary"),
        "author": m.get("authors").and_then(Value::as_array).and_then(|a| a.first()).map(|a| s(a, "name")).unwrap_or(""),
        "downloads": n(m, "downloadCount"),
        "follows": n(m, "thumbsUpCount"),
        "icon_url": m.pointer("/logo/thumbnailUrl").and_then(Value::as_str).unwrap_or(""),
        "banner": banner,
        "categories": cats,
        "loaders": loaders,
        "date_modified": s(m, "dateModified"),
        "project_type": type_of_class(n(m, "classId")),
        "source": "curseforge",
        "allowDownload": m.get("allowModDistribution").and_then(Value::as_bool).unwrap_or(true),
        "view_url": website(m),
    })
}

fn loader_key(t: u64) -> &'static str {
    match t {
        1 => "forge",
        4 => "fabric",
        5 => "quilt",
        6 => "neoforge",
        _ => "",
    }
}

pub fn shape_search(raw: &Value, q: &Query) -> Value {
    let hits: Vec<Value> = raw.get("data").and_then(Value::as_array).map(|a| a.iter().map(normalize_mod).collect()).unwrap_or_default();
    // CurseForge pages stop at 10,000 results.
    let total = raw.pointer("/pagination/totalCount").and_then(Value::as_u64).unwrap_or(hits.len() as u64).min(10_000);
    json!({"hits": hits, "total_hits": total, "offset": q.offset, "source": "curseforge"})
}

pub fn search(api: &dyn Api, q: &Query) -> Value {
    match api.get(&search_path(q)) {
        Ok(v) => shape_search(&v, q),
        Err(e) => json!({"hits": [], "total_hits": 0, "source": "curseforge", "error": e}),
    }
}

// ───────────────────────── Categories ─────────────────────────

/// Categories per content type: [{header, items: [{value: id, label}]}], sub-categories under
/// their parent's name.
pub fn group_categories(all: &Value) -> Value {
    let list: Vec<&Value> = all.get("data").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let names: HashMap<u64, &str> = list.iter().map(|c| (n(c, "id"), s(c, "name"))).collect();
    let mut out = serde_json::Map::new();
    for t in ["mod", "resourcepack", "shader", "modpack", "world", "datapack"] {
        let class = class_id(t).unwrap_or(0);
        let mut groups: BTreeMap<(bool, String), Vec<Value>> = BTreeMap::new();
        for c in list.iter().filter(|c| n(c, "classId") == class && !c.get("isClass").and_then(Value::as_bool).unwrap_or(false)) {
            let parent = n(c, "parentCategoryId");
            let header = if parent == class || parent == 0 { "Categories".to_owned() } else { names.get(&parent).copied().unwrap_or("Categories").to_owned() };
            groups.entry((header != "Categories", header)).or_default().push(json!({"value": n(c, "id").to_string(), "label": s(c, "name")}));
        }
        let arr: Vec<Value> = groups
            .into_iter()
            .map(|((_, h), mut items)| {
                items.sort_by(|a, b| s(a, "label").cmp(s(b, "label")));
                json!({"header": h, "items": items})
            })
            .collect();
        out.insert(t.into(), Value::Array(arr));
    }
    Value::Object(out)
}

static CATS: Mutex<Option<(Instant, Value)>> = Mutex::new(None);

pub fn categories(api: &dyn Api) -> Result<Value, String> {
    if let Some((at, v)) = CATS.lock().unwrap().as_ref() {
        if at.elapsed() < CATS_FRESH {
            return Ok(v.clone());
        }
    }
    let raw = api.get(&format!("/categories?gameId={GAME}"))?;
    let out = json!({"source": "curseforge", "categories": group_categories(&raw)});
    *CATS.lock().unwrap() = Some((Instant::now(), out.clone()));
    Ok(out)
}

// ───────────────────────── Project page ─────────────────────────

pub fn normalize_project(m: &Value, description_html: &str) -> Value {
    let card = normalize_mod(m);
    let gallery: Vec<Value> = m
        .get("screenshots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|g| json!({"url": Some(s(g, "thumbnailUrl")).filter(|x| !x.is_empty()).unwrap_or(s(g, "url")), "raw_url": s(g, "url"), "title": s(g, "title"), "description": s(g, "description")}))
        .collect();
    let mut links = Vec::new();
    for (k, label) in [("sourceUrl", "Source"), ("issuesUrl", "Issues"), ("wikiUrl", "Wiki")] {
        if let Some(u) = m.pointer(&format!("/links/{k}")).and_then(Value::as_str).filter(|u| u.starts_with("https://")) {
            links.push(json!({"label": label, "url": u}));
        }
    }
    let mut versions: Vec<String> = m
        .get("latestFilesIndexes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|i| s(i, "gameVersion").to_owned())
        .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    versions.sort_by(|a, b| cmp_versions(a, b));
    json!({
        "source": "curseforge",
        "id": card["id"],
        "slug": card["slug"],
        "title": card["title"],
        "description": card["description"],
        "body": description_html,
        "bodyFormat": "html",
        "icon_url": card["icon_url"],
        "banner": card["banner"],
        "project_type": card["project_type"],
        "categories": card["categories"],
        "loaders": card["loaders"],
        "gameVersions": versions,
        "downloads": card["downloads"],
        "followers": card["follows"],
        "published": s(m, "dateReleased"),
        "updated": s(m, "dateModified"),
        "license": {"id": "", "name": "", "url": ""},
        "team": m.get("authors").and_then(Value::as_array).map(|a| a.iter().map(|x| json!({"name": s(x, "name"), "role": "Author"})).collect::<Vec<_>>()).unwrap_or_default(),
        "organization": Value::Null,
        "gallery": gallery,
        "links": links,
        "allowDownload": card["allowDownload"],
        "view_url": card["view_url"],
    })
}

/// "1.20.10" after "1.20.9".
fn cmp_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |v: &str| v.split(['.', '-']).map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    key(a).cmp(&key(b))
}

fn valid_num(id: &str) -> Option<u64> {
    id.parse().ok().filter(|n| *n > 0)
}

pub fn project(api: &dyn Api, id: &str) -> Result<Value, String> {
    let id = valid_num(id).ok_or("That CurseForge project id isn't valid.")?;
    let m = api.get(&format!("/mods/{id}"))?;
    let m = m.get("data").cloned().unwrap_or(Value::Null);
    let desc = api.get(&format!("/mods/{id}/description")).ok().and_then(|d| d.get("data").and_then(Value::as_str).map(str::to_owned)).unwrap_or_default();
    Ok(normalize_project(&m, &desc))
}

/// A file's game versions and loaders (CurseForge lists both in `gameVersions`).
fn split_versions(f: &Value) -> (Vec<String>, Vec<String>) {
    let mut gv = Vec::new();
    let mut ld = Vec::new();
    for g in f.get("gameVersions").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        if g.starts_with(|c: char| c.is_ascii_digit()) {
            gv.push(g.to_owned());
        } else if matches!(g.to_ascii_lowercase().as_str(), "forge" | "fabric" | "quilt" | "neoforge") {
            ld.push(g.to_ascii_lowercase());
        }
    }
    (gv, ld)
}

fn channel(f: &Value) -> &'static str {
    match n(f, "releaseType") {
        2 => "beta",
        3 => "alpha",
        _ => "release",
    }
}

/// One file for the Versions tab.
pub fn normalize_file(f: &Value, mc: Option<&str>, loaders: &[&str]) -> Value {
    let (gv, ld) = split_versions(f);
    json!({
        "id": n(f, "id").to_string(),
        "name": s(f, "displayName"),
        "number": s(f, "fileName"),
        "channel": channel(f),
        "loaders": ld,
        "gameVersions": gv,
        "date": s(f, "fileDate"),
        "downloads": n(f, "downloadCount"),
        "file": {"name": s(f, "fileName"), "size": n(f, "fileLength")},
        "deps": f.get("dependencies").and_then(Value::as_array).map(|d| d.iter().filter(|x| n(x, "relationType") == 3).count()).unwrap_or(0),
        "compatible": crate::discover::version_fits(&gv, &ld, mc, loaders) || (ld.is_empty() && crate::discover::version_fits(&gv, &[], mc, &[])),
        "downloadable": f.get("downloadUrl").and_then(Value::as_str).is_some_and(allowed_url),
    })
}

pub fn files(api: &dyn Api, id: &str, mc: Option<&str>, loaders: &[&str]) -> Result<Value, String> {
    let id = valid_num(id).ok_or("That CurseForge project id isn't valid.")?;
    let raw = api.get(&format!("/mods/{id}/files?pageSize=50"))?;
    let list: Vec<Value> = raw.get("data").and_then(Value::as_array).map(|a| a.iter().map(|f| normalize_file(f, mc, loaders)).collect()).unwrap_or_default();
    Ok(Value::Array(list))
}

// ───────────────────────── What's installed ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Entry {
    #[serde(rename = "projectId")]
    pub project_id: u64,
    #[serde(rename = "fileId")]
    pub file_id: u64,
}

/// File name → what it is on CurseForge, for one content folder (files gone since are dropped).
pub fn read_sidecar(dir: &Path) -> BTreeMap<String, Entry> {
    let mut m: BTreeMap<String, Entry> = fs::read_to_string(dir.join(SIDECAR)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    m.retain(|f, _| dir.join(f).is_file() || dir.join(format!("{f}.disabled")).is_file());
    m
}

pub fn write_sidecar(dir: &Path, m: &BTreeMap<String, Entry>) -> Result<(), String> {
    let tmp = dir.join(format!("{SIDECAR}.part"));
    fs::write(&tmp, serde_json::to_string_pretty(m).unwrap_or_default()).map_err(|e| format!("Couldn't save {SIDECAR}: {e}"))?;
    fs::rename(&tmp, dir.join(SIDECAR)).map_err(|e| format!("Couldn't save {SIDECAR}: {e}"))
}

/// CurseForge project ids installed in a folder.
pub fn installed_ids(dir: &Path) -> HashSet<u64> {
    read_sidecar(dir).values().map(|e| e.project_id).collect()
}

// ───────────────────────── Install plans ─────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub mod_id: u64,
    pub file_id: u64,
    pub file_name: String,
    /// None: the author only allows downloads on CurseForge's site.
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub deps: Vec<(u64, u64)>,
    /// Installed files of the same project, removed once the new one is saved.
    pub replaces: Vec<String>,
}

pub struct Plan {
    pub summary: Summary,
    pub main: Pick,
    pub deps: Vec<Pick>,
}

/// Why a CurseForge install can't go ahead.
#[derive(Debug, PartialEq)]
pub enum PlanError {
    /// The main file can only be downloaded on the site: (title, page url).
    Manual(String, String),
    Failed(String),
}

impl From<String> for PlanError {
    fn from(e: String) -> Self {
        PlanError::Failed(e)
    }
}

fn pick_of(f: &Value) -> Option<Pick> {
    let file_name = Path::new(s(f, "fileName")).file_name()?.to_string_lossy().into_owned();
    if !crate::safe_file_name(&file_name) {
        return None;
    }
    Some(Pick {
        mod_id: n(f, "modId"),
        file_id: n(f, "id"),
        file_name,
        url: f.get("downloadUrl").and_then(Value::as_str).filter(|u| allowed_url(u)).map(str::to_owned),
        sha1: f.get("hashes").and_then(Value::as_array).and_then(|h| h.iter().find(|x| n(x, "algo") == 1)).map(|x| s(x, "value").to_ascii_lowercase()).filter(|h| h.len() == 40),
        size: f.get("fileLength").and_then(Value::as_u64),
        deps: f.get("dependencies").and_then(Value::as_array).map(|d| d.iter().map(|x| (n(x, "modId"), n(x, "relationType"))).filter(|(m, _)| *m > 0).collect()).unwrap_or_default(),
        replaces: Vec::new(),
    })
}

/// The best file for this game version + loader: releases first, newest first.
pub fn best_file(list: &[Value], mc: Option<&str>, loaders: &[&str]) -> Option<Value> {
    let mut fits: Vec<&Value> = list
        .iter()
        .filter(|f| f.get("isAvailable").and_then(Value::as_bool).unwrap_or(true))
        .filter(|f| {
            let (gv, ld) = split_versions(f);
            let mc_ok = mc.is_none_or(|m| gv.iter().any(|g| g == m));
            let ld_ok = loaders.is_empty() || ld.is_empty() || ld.iter().any(|l| loaders.contains(&l.as_str()));
            mc_ok && ld_ok
        })
        .collect();
    fits.sort_by(|a, b| {
        let r = |f: &Value| match n(f, "releaseType") {
            1 => 0,
            2 => 1,
            _ => 2,
        };
        r(a).cmp(&r(b)).then_with(|| s(b, "fileDate").cmp(s(a, "fileDate")))
    });
    fits.first().map(|f| (*f).clone())
}

fn files_for(api: &dyn Api, mod_id: u64, mc: Option<&str>, loader_types: &[u64]) -> Result<Vec<Value>, String> {
    let gv = mc.map(|m| format!("&gameVersion={}", crate::url_encode(m))).unwrap_or_default();
    if loader_types.is_empty() {
        let v = api.get(&format!("/mods/{mod_id}/files?pageSize=50{gv}"))?;
        return Ok(v.get("data").and_then(Value::as_array).cloned().unwrap_or_default());
    }
    for lt in loader_types {
        let v = api.get(&format!("/mods/{mod_id}/files?pageSize=50{gv}&modLoaderType={lt}"))?;
        let list = v.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
        if !list.is_empty() {
            return Ok(list);
        }
    }
    Ok(Vec::new())
}

fn titles(api: &dyn Api, ids: &[u64]) -> HashMap<u64, (String, String)> {
    if ids.is_empty() {
        return HashMap::new();
    }
    api.post("/mods", json!({"modIds": ids}))
        .ok()
        .and_then(|v| v.get("data").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .map(|m| (n(m, "id"), (s(m, "name").to_owned(), website(m))))
        .collect()
}

/// Works out what installing CurseForge project `mod_id` (a specific `file_id`, or the best
/// file for the instance) means: dependencies, conflicts with what's installed, older copies.
pub fn plan(api: &dyn Api, mod_id: u64, file_id: Option<u64>, project_type: &str, mc: &str, loader: Option<&str>, installed: &BTreeMap<String, Entry>) -> Result<Plan, PlanError> {
    let is_mod = project_type == "mod";
    let lts = if is_mod { loader.map(loader_types).unwrap_or_default() } else { Vec::new() };
    let loader_keys: Vec<&str> = if is_mod { lts.iter().map(|t| loader_key(*t)).collect() } else { Vec::new() };
    let gv = if matches!(project_type, "shader" | "world" | "modpack") { None } else { Some(mc) };
    let mut problems = Vec::new();
    let main_file = match file_id {
        Some(fid) => {
            let f = api.get(&format!("/mods/{mod_id}/files/{fid}"))?.get("data").cloned().unwrap_or(Value::Null);
            if best_file(std::slice::from_ref(&f), gv, &loader_keys).is_none() {
                let (fv, fl) = split_versions(&f);
                problems.push(Problem {
                    kind: "version",
                    message: format!("{} is made for {}, not this instance (Minecraft {mc}). It may not load.", s(&f, "displayName"), fv.iter().chain(fl.iter()).cloned().collect::<Vec<_>>().join(", ")),
                    blocking: true,
                });
            }
            f
        }
        None => {
            let list = files_for(api, mod_id, gv, &lts)?;
            best_file(&list, gv, &loader_keys).ok_or_else(|| PlanError::Failed(format!("No file of this works with Minecraft {mc}{}.", loader.filter(|_| is_mod).map(|l| format!(" with {}", crate::mod_plan::loader_name(l))).unwrap_or_default())))?
        }
    };
    let mut main = pick_of(&main_file).ok_or_else(|| PlanError::Failed("CurseForge listed a file Lumos can't use.".into()))?;
    let by_project: HashMap<u64, Vec<&String>> = installed.iter().fold(HashMap::new(), |mut m, (f, e)| {
        m.entry(e.project_id).or_default().push(f);
        m
    });
    main.replaces = by_project.get(&mod_id).map(|v| v.iter().map(|f| (*f).clone()).collect()).unwrap_or_default();

    let mut picks: Vec<Pick> = Vec::new();
    let mut seen: HashSet<u64> = HashSet::from([mod_id]);
    let mut present: Vec<u64> = Vec::new();
    let mut optional: Vec<u64> = Vec::new();
    let mut missing: Vec<(u64, u64)> = Vec::new();
    let mut incompatible: Vec<(u64, u64)> = Vec::new(); // (owner, target)
    if is_mod {
        let mut queue: VecDeque<(u64, Vec<(u64, u64)>, usize)> = VecDeque::from([(mod_id, main.deps.clone(), 0)]);
        while let Some((parent, deps, depth)) = queue.pop_front() {
            for (dep, rel) in deps {
                match rel {
                    3 => {
                        if !seen.insert(dep) {
                            continue;
                        }
                        if by_project.contains_key(&dep) {
                            present.push(dep);
                            continue;
                        }
                        match best_file(&files_for(api, dep, gv, &lts)?, gv, &loader_keys).and_then(|f| pick_of(&f)) {
                            None => missing.push((dep, parent)),
                            Some(p) => {
                                if depth + 1 < MAX_DEPTH {
                                    queue.push_back((dep, p.deps.clone(), depth + 1));
                                }
                                picks.push(p);
                            }
                        }
                    }
                    2 if depth == 0 && !by_project.contains_key(&dep) && !optional.contains(&dep) => optional.push(dep),
                    5 => incompatible.push((parent, dep)),
                    _ => {}
                }
            }
        }
    }
    optional.retain(|o| !seen.contains(o));
    let new_ids: HashSet<u64> = std::iter::once(mod_id).chain(picks.iter().map(|p| p.mod_id)).collect();
    incompatible.retain(|(_, t)| by_project.contains_key(t) || new_ids.contains(t));

    let mut ids: Vec<u64> = vec![mod_id];
    ids.extend(picks.iter().map(|p| p.mod_id));
    ids.extend(&present);
    ids.extend(&optional);
    ids.extend(missing.iter().flat_map(|(a, b)| [*a, *b]));
    ids.extend(incompatible.iter().flat_map(|(a, b)| [*a, *b]));
    ids.sort();
    ids.dedup();
    let names = titles(api, &ids);
    let t = |id: u64| names.get(&id).map(|x| x.0.clone()).unwrap_or_else(|| format!("CurseForge project {id}"));

    if main.url.is_none() {
        let page = names.get(&mod_id).map(|x| format!("{}/files/{}", x.1, main.file_id)).unwrap_or_else(|| format!("{SITE}/mc-mods/{mod_id}"));
        return Err(PlanError::Manual(t(mod_id), page));
    }
    for (owner, target) in &incompatible {
        problems.push(Problem { kind: "incompatible", message: format!("{} is marked incompatible with {}. Using both can stop the game from starting.", t(*owner), t(*target)), blocking: true });
    }
    for (dep, by) in &missing {
        problems.push(Problem { kind: "missing", message: format!("{} needs {}, which has no file for Minecraft {mc}, so it can't be installed for this version.", t(*by), t(*dep)), blocking: true });
    }
    for p in picks.iter().filter(|p| p.url.is_none()) {
        problems.push(Problem {
            kind: "missing",
            message: format!("{} (needed) can only be downloaded on CurseForge's website. Download it there and add it in the instance's Mods tab.", t(p.mod_id)),
            blocking: true,
        });
    }
    let summary = Summary {
        title: t(mod_id),
        deps: picks.iter().filter(|p| p.url.is_some()).map(|p| t(p.mod_id)).collect(),
        present: present.iter().map(|p| t(*p)).collect(),
        enable: Vec::new(),
        optional: optional.iter().map(|p| t(*p)).collect(),
        problems,
    };
    Ok(Plan { summary, main, deps: picks })
}

/// Downloads every file of a plan (checked against its sha1), then saves them together, removes
/// the older copies and remembers what came from CurseForge. Files only on the site are skipped.
pub fn execute(plan: &Plan, dest: &Path, download: &dyn Fn(&str, &Path, Option<&str>, Option<u64>) -> Result<(), String>, log: &dyn Fn(&str)) -> Result<Vec<String>, String> {
    fs::create_dir_all(dest).map_err(|e| format!("Couldn't create {}: {e}", dest.display()))?;
    let mut staged: Vec<(&Pick, PathBuf)> = Vec::new();
    let mut warnings = Vec::new();
    for (i, p) in std::iter::once(&plan.main).chain(&plan.deps).enumerate() {
        let Some(url) = &p.url else {
            warnings.push(format!("{} wasn't downloaded (CurseForge website only)", p.file_name));
            continue;
        };
        let tmp = dest.join(format!(".tmp_cf_{}_{}_{i}_{}", std::process::id(), crate::now_millis(), p.file_name));
        if let Err(e) = download(url, &tmp, p.sha1.as_deref(), p.size) {
            let _ = fs::remove_file(&tmp);
            if i == 0 {
                for (_, t) in &staged {
                    let _ = fs::remove_file(t);
                }
                return Err(e);
            }
            log(&e);
            warnings.push(format!("{} couldn't be downloaded", p.file_name));
            continue;
        }
        staged.push((p, tmp));
    }
    let mut side = read_sidecar(dest);
    for (n, (p, tmp)) in staged.iter().enumerate() {
        if let Err(e) = fs::rename(tmp, dest.join(&p.file_name)) {
            for (_, t) in &staged[n..] {
                let _ = fs::remove_file(t);
            }
            return Err(format!("Couldn't save {}: {e}", p.file_name));
        }
        for old in p.replaces.iter().filter(|o| **o != p.file_name && crate::safe_file_name(o)) {
            if fs::remove_file(dest.join(old)).is_ok() || fs::remove_file(dest.join(format!("{old}.disabled"))).is_ok() {
                log(&format!("Replaced {old}"));
            }
            side.remove(old);
        }
        side.insert(p.file_name.clone(), Entry { project_id: p.mod_id, file_id: p.file_id });
        log(&format!("Installed {}", p.file_name));
    }
    write_sidecar(dest, &side)?;
    Ok(warnings)
}

// ───────────────────────── Worlds and data packs ─────────────────────────

/// Unpacks a world zip into `saves/<name>` (the zip may hold the world at its root or in one
/// folder). Returns the folder name used.
pub fn install_world(zip_path: &Path, saves: &Path, name: &str) -> Result<String, String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("Couldn't open the world: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "That download isn't a world (.zip).".to_owned())?;
    let names: Vec<String> = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|e| e.name().replace('\\', "/"))).collect();
    let level = names.iter().filter(|n| n.ends_with("level.dat")).min_by_key(|n| n.len()).ok_or("That download has no level.dat, so it isn't a world.")?;
    let prefix = level.strip_suffix("level.dat").unwrap_or("").to_owned();
    let base: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_')).take(40).collect::<String>().trim().to_owned();
    let base = if base.is_empty() { "World".to_owned() } else { base };
    let mut folder = base.clone();
    let mut k = 2;
    while saves.join(&folder).exists() {
        folder = format!("{base} ({k})");
        k += 1;
    }
    let target = saves.join(&folder);
    fs::create_dir_all(&target).map_err(|e| format!("Couldn't create {}: {e}", target.display()))?;
    let result = (|| {
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| format!("The world is damaged: {e}"))?;
            let name = entry.name().replace('\\', "/");
            let Some(rel) = name.strip_prefix(&prefix) else { continue };
            if rel.is_empty() || entry.is_dir() {
                continue;
            }
            if entry.enclosed_name().is_none() {
                return Err(format!("The world has a file outside its folder ({name})."));
            }
            let dest = crate::modpack::target_in(&target, rel).ok_or_else(|| format!("The world has a file outside its folder ({name})."))?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
            }
            let mut out = fs::File::create(&dest).map_err(|e| format!("Couldn't write {rel}: {e}"))?;
            std::io::copy(&mut (&mut entry).take(1 << 31), &mut out).map_err(|e| format!("Couldn't unpack {rel}: {e}"))?;
            out.flush().map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&target);
        return Err(e);
    }
    Ok(folder)
}

/// World folders in an instance (for data packs).
pub fn worlds(game_dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(game_dir.join("saves"))
        .map(|rd| rd.flatten().filter(|e| e.path().join("level.dat").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

// ───────────────────────── Modpacks ─────────────────────────

/// A CurseForge modpack's `manifest.json`.
#[derive(Debug, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub mc: String,
    /// ("fabric-loader" | "forge" | "neoforge" | "quilt-loader", version): the mrpack names,
    /// so modpack.rs picks the Lumos version.
    pub loader: Option<(String, String)>,
    /// (project id, file id) of every required file.
    pub files: Vec<(u64, u64)>,
    pub overrides: String,
}

pub fn parse_manifest(v: &Value) -> Result<Manifest, String> {
    if s(v, "manifestType") != "minecraftModpack" {
        return Err("That file isn't a CurseForge modpack.".into());
    }
    let mc = v.pointer("/minecraft/version").and_then(Value::as_str).ok_or("The modpack doesn't say which Minecraft it needs.")?.to_owned();
    if mc.is_empty() || mc.len() > 32 || !mc.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return Err("The modpack names an invalid Minecraft version.".into());
    }
    let loaders: Vec<&Value> = v.pointer("/minecraft/modLoaders").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let primary = loaders.iter().find(|l| l.get("primary").and_then(Value::as_bool) == Some(true)).or_else(|| loaders.first());
    let loader = primary.and_then(|l| {
        let id = s(l, "id");
        let (k, ver) = id.split_once('-')?;
        let key = match k {
            "fabric" => "fabric-loader",
            "quilt" => "quilt-loader",
            "forge" => "forge",
            "neoforge" => "neoforge",
            _ => return None,
        };
        Some((key.to_owned(), ver.to_owned()))
    });
    let files = v
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| f.get("required").and_then(Value::as_bool).unwrap_or(true))
        .map(|f| (n(f, "projectID"), n(f, "fileID")))
        .filter(|(p, f)| *p > 0 && *f > 0)
        .collect();
    Ok(Manifest {
        name: Some(s(v, "name").trim()).filter(|x| !x.is_empty()).unwrap_or("Modpack").to_owned(),
        mc,
        loader,
        files,
        overrides: Some(s(v, "overrides")).filter(|x| !x.is_empty()).unwrap_or("overrides").to_owned(),
    })
}

pub fn read_manifest(zip_path: &Path) -> Result<Manifest, String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("Couldn't open the modpack: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "That file isn't a modpack (.zip).".to_owned())?;
    let mut raw = String::new();
    zip.by_name("manifest.json")
        .map_err(|_| "That file isn't a CurseForge modpack (no manifest.json).".to_owned())?
        .take(8 << 20)
        .read_to_string(&mut raw)
        .map_err(|e| format!("Couldn't read the modpack: {e}"))?;
    parse_manifest(&serde_json::from_str(&raw).map_err(|e| format!("The modpack's manifest is damaged: {e}"))?)
}

/// The Lumos version a CurseForge pack runs on (same rules as Modrinth packs).
pub fn version_spec(m: &Manifest) -> Result<String, String> {
    crate::modpack::version_spec(&crate::modpack::PackIndex { name: m.name.clone(), version_id: String::new(), mc: m.mc.clone(), loader: m.loader.clone(), files: Vec::new() })
}

/// Downloads every file a pack lists into the instance (the right folder per project class),
/// in parallel. Returns the files that can only be downloaded on CurseForge's website.
pub fn download_pack_files(api: &dyn Api, m: &Manifest, dir: &Path, report: &(dyn Fn(&str, f64) + Sync)) -> Result<Vec<String>, String> {
    let file_ids: Vec<u64> = m.files.iter().map(|(_, f)| *f).collect();
    let mut files: Vec<Value> = Vec::new();
    for chunk in file_ids.chunks(500) {
        let v = api.post("/mods/files", json!({"fileIds": chunk}))?;
        files.extend(v.get("data").and_then(Value::as_array).cloned().unwrap_or_default());
    }
    let mod_ids: Vec<u64> = m.files.iter().map(|(p, _)| *p).collect();
    let mut classes: HashMap<u64, u64> = HashMap::new();
    for chunk in mod_ids.chunks(500) {
        if let Ok(v) = api.post("/mods", json!({"modIds": chunk})) {
            for x in v.get("data").and_then(Value::as_array).into_iter().flatten() {
                classes.insert(n(x, "id"), n(x, "classId"));
            }
        }
    }
    let mut manual = Vec::new();
    let mut jobs: Vec<(Pick, PathBuf)> = Vec::new();
    for f in &files {
        let Some(p) = pick_of(f) else { continue };
        let folder = match classes.get(&p.mod_id).copied().unwrap_or(6) {
            12 => "resourcepacks",
            6552 => "shaderpacks",
            _ => "mods",
        };
        if p.url.is_none() {
            manual.push(p.file_name.clone());
            continue;
        }
        jobs.push((p, dir.join(folder)));
    }
    let total = jobs.len().max(1);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let error: Mutex<Option<String>> = Mutex::new(None);
    let side: Mutex<HashMap<PathBuf, BTreeMap<String, Entry>>> = Mutex::new(HashMap::new());
    let label = format!("Downloading {} files for {}", jobs.len(), m.name);
    report(&label, 0.0);
    std::thread::scope(|sc| {
        for _ in 0..6.min(jobs.len()) {
            sc.spawn(|| loop {
                if error.lock().unwrap().is_some() {
                    return;
                }
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some((p, folder)) = jobs.get(i) else { return };
                let res = fs::create_dir_all(folder).map_err(|e| e.to_string()).and_then(|_| crate::net::download_verified(p.url.as_deref().unwrap_or(""), &folder.join(&p.file_name), p.sha1.as_deref(), p.size));
                if let Err(e) = res {
                    error.lock().unwrap().get_or_insert(e);
                    return;
                }
                side.lock().unwrap().entry(folder.clone()).or_default().insert(p.file_name.clone(), Entry { project_id: p.mod_id, file_id: p.file_id });
                let d = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                report(&label, (d as f64 / total as f64).min(0.999));
            });
        }
    });
    if let Some(e) = error.into_inner().unwrap() {
        return Err(e);
    }
    for (folder, entries) in side.into_inner().unwrap() {
        let _ = write_sidecar(&folder, &entries);
    }
    Ok(manual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_sorts() {
        assert_eq!(class_id("shader"), Some(6552));
        assert_eq!(type_of_class(4471), "modpack");
        assert_eq!(loader_types("quilt"), vec![5, 4]);
        assert_eq!(sort_field("downloads"), 6);
        assert!(allowed_url("https://edge.forgecdn.net/files/1/2/a.jar"));
        assert!(!allowed_url("http://edge.forgecdn.net/a.jar"));
        assert!(!allowed_url("https://evil.example/a.jar"));
        assert!(!allowed_url("https://edge.forgecdn.net@evil.example/a.jar"));
    }

    #[test]
    fn search_paths_carry_the_filters() {
        let q = Query::from_json(&json!({"query": "jei", "projectType": "mod", "sortBy": "updated", "gameVersion": "1.20.1", "loaders": ["forge", "neoforge"], "categories": ["423", "x"], "offset": 48, "limit": 24}));
        let p = search_path(&q);
        assert!(p.starts_with("/mods/search?gameId=432&classId=6&sortField=3&sortOrder=desc&index=48&pageSize=24"), "{p}");
        assert!(p.contains("&searchFilter=jei") && p.contains("&gameVersion=1.20.1"));
        assert!(p.contains(&format!("&modLoaderTypes={}", crate::url_encode("[1,6]"))));
        assert!(p.contains(&format!("&categoryIds={}", crate::url_encode("[423]"))));
        // Resource packs don't filter by loader.
        let rp = Query::from_json(&json!({"projectType": "resourcepack", "loaders": ["forge"]}));
        assert!(!search_path(&rp).contains("modLoaderTypes"));
    }

    fn sample_mod() -> Value {
        json!({"id": 238222, "name": "Just Enough Items (JEI)", "slug": "jei", "summary": "View items", "downloadCount": 300000000u64, "thumbsUpCount": 10,
            "classId": 6, "logo": {"thumbnailUrl": "https://media.forgecdn.net/l.png"}, "authors": [{"name": "mezz"}],
            "links": {"websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/jei", "sourceUrl": "https://github.com/mezz/JustEnoughItems", "issuesUrl": "javascript:x"},
            "categories": [{"name": "API and Library"}], "screenshots": [{"url": "https://media.forgecdn.net/s.png", "thumbnailUrl": "https://media.forgecdn.net/st.png"}],
            "latestFilesIndexes": [{"gameVersion": "1.20.1", "modLoader": 1}, {"gameVersion": "1.21.1", "modLoader": 6}], "allowModDistribution": true, "dateModified": "2026-01-01"})
    }

    #[test]
    fn mods_become_cards_and_pages() {
        let out = shape_search(&json!({"data": [sample_mod()], "pagination": {"totalCount": 50000}}), &Query::default());
        assert_eq!(out["total_hits"], 10_000);
        let h = &out["hits"][0];
        assert_eq!((h["id"].as_str(), h["author"].as_str(), h["banner"].as_str()), (Some("238222"), Some("mezz"), Some("https://media.forgecdn.net/st.png")));
        assert_eq!(h["loaders"], json!(["forge", "neoforge"]));
        let p = normalize_project(&sample_mod(), "<p>Hi</p>");
        assert_eq!(p["bodyFormat"], "html");
        assert_eq!(p["links"], json!([{"label": "Source", "url": "https://github.com/mezz/JustEnoughItems"}]));
        assert_eq!(p["gameVersions"], json!(["1.20.1", "1.21.1"]));
    }

    #[test]
    fn categories_group_by_class_and_parent() {
        let raw = json!({"data": [
            {"id": 6, "name": "Mods", "classId": 6, "isClass": true},
            {"id": 420, "name": "Storage", "classId": 6, "parentCategoryId": 6},
            {"id": 421, "name": "API and Library", "classId": 6, "parentCategoryId": 6},
            {"id": 406, "name": "World Gen", "classId": 6, "parentCategoryId": 6},
            {"id": 407, "name": "Biomes", "classId": 6, "parentCategoryId": 406},
            {"id": 393, "name": "16x", "classId": 12, "parentCategoryId": 12}
        ]});
        let g = group_categories(&raw);
        assert_eq!(g["mod"][0]["header"], "Categories");
        assert_eq!(g["mod"][0]["items"][0], json!({"value": "421", "label": "API and Library"}));
        assert_eq!(g["mod"][1], json!({"header": "World Gen", "items": [{"value": "407", "label": "Biomes"}]}));
        assert_eq!(g["resourcepack"][0]["items"][0]["label"], "16x");
    }

    fn file(id: u64, mod_id: u64, gv: &[&str], rel: u64, date: &str, url: Option<&str>, deps: Value) -> Value {
        json!({"id": id, "modId": mod_id, "displayName": format!("f{id}"), "fileName": format!("m{mod_id}-{id}.jar"), "releaseType": rel, "fileDate": date,
            "gameVersions": gv, "downloadUrl": url, "hashes": [{"value": "A".repeat(40), "algo": 1}], "fileLength": 3, "dependencies": deps})
    }

    #[test]
    fn files_are_picked_for_version_and_loader() {
        let list = vec![
            file(1, 9, &["1.20.1", "Forge"], 1, "2024-01-01", None, json!([])),
            file(2, 9, &["1.20.1", "Forge"], 2, "2024-06-01", None, json!([])),
            file(3, 9, &["1.20.1", "Fabric"], 1, "2024-07-01", None, json!([])),
            file(4, 9, &["1.20.1", "Forge"], 1, "2024-03-01", None, json!([])),
        ];
        assert_eq!(best_file(&list, Some("1.20.1"), &["forge"]).unwrap()["id"], 4);
        assert_eq!(best_file(&list, Some("1.20.1"), &["fabric"]).unwrap()["id"], 3);
        assert!(best_file(&list, Some("1.21"), &["forge"]).is_none());
        let nf = normalize_file(&list[1], Some("1.20.1"), &["forge"]);
        assert_eq!((nf["channel"].as_str(), nf["compatible"].as_bool(), nf["downloadable"].as_bool()), (Some("beta"), Some(true), Some(false)));
        assert_eq!(nf["loaders"], json!(["forge"]));
    }

    struct Mock {
        files: HashMap<u64, Vec<Value>>,
    }
    impl Api for Mock {
        fn get(&self, path: &str) -> Result<Value, String> {
            let id: u64 = path.trim_start_matches("/mods/").split(['/', '?']).next().unwrap().parse().unwrap();
            if let Some(fid) = path.split("/files/").nth(1) {
                let fid: u64 = fid.parse().unwrap();
                return Ok(json!({"data": self.files[&id].iter().find(|f| f["id"] == fid).cloned()}));
            }
            Ok(json!({"data": self.files.get(&id).cloned().unwrap_or_default()}))
        }
        fn post(&self, _: &str, body: Value) -> Result<Value, String> {
            let ids: Vec<u64> = body["modIds"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
            Ok(json!({"data": ids.iter().map(|i| json!({"id": i, "name": format!("Mod{i}"), "classId": 6, "links": {"websiteUrl": format!("https://www.curseforge.com/minecraft/mc-mods/m{i}")}})).collect::<Vec<_>>()}))
        }
    }

    const URL: &str = "https://edge.forgecdn.net/files/x.jar";

    #[test]
    fn plans_follow_required_and_report_incompatible() {
        let mut files = HashMap::new();
        files.insert(1, vec![file(10, 1, &["1.20.1", "Forge"], 1, "2024", Some(URL), json!([{"modId": 2, "relationType": 3}, {"modId": 3, "relationType": 2}, {"modId": 5, "relationType": 5}]))]);
        files.insert(2, vec![file(20, 2, &["1.20.1", "Forge"], 1, "2024", Some(URL), json!([{"modId": 4, "relationType": 3}]))]);
        files.insert(4, vec![file(40, 4, &["1.19.2", "Forge"], 1, "2024", Some(URL), json!([]))]);
        let mut installed = BTreeMap::new();
        installed.insert("old1.jar".to_owned(), Entry { project_id: 1, file_id: 9 });
        installed.insert("five.jar".to_owned(), Entry { project_id: 5, file_id: 50 });
        let p = plan(&Mock { files }, 1, None, "mod", "1.20.1", Some("forge"), &installed).unwrap();
        assert_eq!(p.main.file_id, 10);
        assert_eq!(p.main.replaces, vec!["old1.jar"]);
        assert_eq!(p.summary.deps, vec!["Mod2"]);
        assert_eq!(p.summary.optional, vec!["Mod3"]);
        let kinds: Vec<&str> = p.summary.problems.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, vec!["incompatible", "missing"], "{:?}", p.summary.problems);
        assert!(p.summary.problems[0].message.contains("Mod1 is marked incompatible with Mod5"));
        assert!(p.summary.problems[1].message.contains("Mod2 needs Mod4"));
    }

    #[test]
    fn site_only_files_open_curseforge() {
        let mut files = HashMap::new();
        files.insert(1, vec![file(10, 1, &["1.20.1"], 1, "2024", None, json!([]))]);
        match plan(&Mock { files }, 1, None, "resourcepack", "1.20.1", None, &BTreeMap::new()) {
            Err(PlanError::Manual(t, url)) => assert_eq!((t.as_str(), url.as_str()), ("Mod1", "https://www.curseforge.com/minecraft/mc-mods/m1/files/10")),
            other => panic!("{:?}", other.err()),
        }
    }

    #[test]
    fn a_picked_file_for_another_version_asks_first() {
        let mut files = HashMap::new();
        files.insert(1, vec![file(10, 1, &["1.19.2", "Forge"], 1, "2024", Some(URL), json!([]))]);
        let p = plan(&Mock { files }, 1, Some(10), "mod", "1.20.1", Some("forge"), &BTreeMap::new()).unwrap();
        assert!(p.summary.blocked() && p.summary.problems[0].kind == "version");
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lumos-cf-{name}-{}-{}", std::process::id(), crate::now_millis()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn execute_replaces_and_remembers() {
        let dir = temp("exec");
        fs::write(dir.join("old1.jar"), b"old").unwrap();
        let mut side = BTreeMap::new();
        side.insert("old1.jar".to_owned(), Entry { project_id: 1, file_id: 9 });
        write_sidecar(&dir, &side).unwrap();
        let pick = |m: u64, f: u64, url: Option<&str>, replaces: Vec<String>| Pick { mod_id: m, file_id: f, file_name: format!("m{m}.jar"), url: url.map(str::to_owned), sha1: None, size: None, deps: vec![], replaces };
        let plan = Plan { summary: Summary::default(), main: pick(1, 10, Some(URL), vec!["old1.jar".into()]), deps: vec![pick(2, 20, Some(URL), vec![]), pick(3, 30, None, vec![])] };
        let dl = |_: &str, p: &Path, _: Option<&str>, _: Option<u64>| fs::write(p, b"new").map_err(|e| e.to_string());
        let warnings = execute(&plan, &dir, &dl, &|_| {}).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(!dir.join("old1.jar").exists() && dir.join("m1.jar").exists() && dir.join("m2.jar").exists());
        let side = read_sidecar(&dir);
        assert_eq!(side.get("m1.jar"), Some(&Entry { project_id: 1, file_id: 10 }));
        assert!(!side.contains_key("old1.jar"));
        assert_eq!(installed_ids(&dir), HashSet::from([1, 2]));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifests_are_read() {
        let m = parse_manifest(&json!({"manifestType": "minecraftModpack", "name": "Pack", "overrides": "overrides",
            "minecraft": {"version": "1.20.1", "modLoaders": [{"id": "forge-47.2.0", "primary": true}]},
            "files": [{"projectID": 1, "fileID": 2, "required": true}, {"projectID": 3, "fileID": 4, "required": false}]}))
        .unwrap();
        assert_eq!(m.loader, Some(("forge".into(), "47.2.0".into())));
        assert_eq!(m.files, vec![(1, 2)]);
        assert_eq!(version_spec(&m).unwrap(), "Forge 1.20.1");
        assert!(parse_manifest(&json!({"manifestType": "other"})).is_err());
        let fabric = parse_manifest(&json!({"manifestType": "minecraftModpack", "minecraft": {"version": "1.21.1", "modLoaders": [{"id": "fabric-0.16.0"}]}})).unwrap();
        assert_eq!(version_spec(&fabric).unwrap(), "Fabric 1.21.1");
    }

    #[test]
    fn worlds_unpack_into_saves() {
        let dir = temp("world");
        let zip_path = dir.join("w.zip");
        {
            let f = fs::File::create(&zip_path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("My World/level.dat", o).unwrap();
            z.write_all(b"lvl").unwrap();
            z.start_file("My World/region/r.0.0.mca", o).unwrap();
            z.write_all(b"r").unwrap();
            z.finish().unwrap();
        }
        let saves = dir.join("saves");
        assert_eq!(install_world(&zip_path, &saves, "Sky/Block!").unwrap(), "SkyBlock");
        assert!(saves.join("SkyBlock/level.dat").is_file() && saves.join("SkyBlock/region/r.0.0.mca").is_file());
        assert_eq!(install_world(&zip_path, &saves, "Sky/Block!").unwrap(), "SkyBlock (2)");
        assert_eq!(worlds(&dir), vec!["SkyBlock", "SkyBlock (2)"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
