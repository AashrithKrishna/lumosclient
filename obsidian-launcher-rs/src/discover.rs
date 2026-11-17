//! Discover: the Content Hub (global) and the Content Browser (bound to one instance).
//!
//! Modrinth side of it: search with filters (game version, loaders, categories, environment,
//! open source), live tag lists (categories grouped by project type and header, game versions),
//! project pages (body, gallery, team, links), every version of a project, update checks for an
//! instance's files and "Recommended for this instance" suggestions.
//!
//! Results are normalised into one card shape the page renders for every source
//! (Modrinth, CurseForge, Horizon): `id, slug, title, description, author, downloads, follows,
//! icon_url, banner, categories, date_modified, project_type, source, view_url`.
//!
//! Installs of a specific version go through mod_plan.rs like every other install: [`Pinned`]
//! makes the planner see only that version of the main project, so dependencies, conflicts,
//! duplicates and the replaced older copy are worked out exactly as for "Install".

use crate::mod_plan::{self, LocalFile, Modrinth, Problem};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const API: &str = "https://api.modrinth.com/v2";
/// Results per page (infinite scroll asks for the next page at the bottom).
pub const PAGE: u64 = 24;
/// How long tag lists are kept before asking again.
const TAGS_FRESH: Duration = Duration::from_secs(6 * 3600);
/// How long suggestions for an unchanged instance are kept.
const SUGGEST_FRESH: Duration = Duration::from_secs(1800);

/// Content types the page knows. `world` exists only on CurseForge.
pub const TYPES: &[&str] = &["modpack", "mod", "resourcepack", "shader", "datapack", "world"];

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
}

/// A tag value from the page: short, no quotes or brackets (it goes into a facet string).
fn clean_tag(t: &str) -> Option<String> {
    let t = t.trim();
    (!t.is_empty() && t.len() <= 64 && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ' | '+'))).then(|| t.to_owned())
}

// ───────────────────────── Search ─────────────────────────

/// A Discover search as the page sends it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub text: String,
    pub project_type: String,
    pub sort: String,
    pub offset: u64,
    pub limit: u64,
    pub game_version: Option<String>,
    pub loaders: Vec<String>,
    pub categories: Vec<String>,
    /// "client" or "server".
    pub environment: Option<String>,
    pub open_source: bool,
}

impl Query {
    /// Reads (and checks) the page's request.
    pub fn from_json(v: &Value) -> Query {
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_owned();
        let list = |k: &str| v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).filter_map(clean_tag).take(16).collect::<Vec<String>>()).unwrap_or_default();
        let project_type = text("projectType");
        Query {
            text: text("query").chars().take(200).collect(),
            project_type: if TYPES.contains(&project_type.as_str()) { project_type } else { "mod".into() },
            sort: text("sortBy"),
            offset: v.get("offset").and_then(Value::as_u64).unwrap_or(0).min(9_900),
            limit: v.get("limit").and_then(Value::as_u64).unwrap_or(PAGE).clamp(1, 50),
            game_version: clean_tag(&text("gameVersion")).filter(|g| g != "any"),
            loaders: list("loaders").into_iter().map(|l| l.to_ascii_lowercase()).collect(),
            categories: list("categories"),
            environment: Some(text("environment")).filter(|e| e == "client" || e == "server"),
            open_source: v.get("openSource").and_then(Value::as_bool).unwrap_or(false),
        }
    }
}

/// The Modrinth `index` for a sort choice.
pub fn modrinth_index(sort: &str) -> &'static str {
    match sort {
        "relevance" => "relevance",
        "follows" => "follows",
        "newest" => "newest",
        "updated" => "updated",
        _ => "downloads",
    }
}

/// Modrinth search facets: each inner list is OR, the lists are AND.
pub fn modrinth_facets(q: &Query) -> String {
    let mut groups: Vec<Vec<String>> = vec![vec![format!("project_type:{}", q.project_type)]];
    // Loaders only mean something for mods and modpacks (packs and shaders have their own).
    if (q.project_type == "mod" || q.project_type == "modpack") && !q.loaders.is_empty() {
        groups.push(q.loaders.iter().map(|l| format!("categories:{l}")).collect());
    }
    if let Some(gv) = &q.game_version {
        groups.push(vec![format!("versions:{gv}")]);
    }
    for c in &q.categories {
        groups.push(vec![format!("categories:{c}")]);
    }
    match q.environment.as_deref() {
        Some("client") => groups.push(vec!["client_side:required".into(), "client_side:optional".into()]),
        Some("server") => groups.push(vec!["server_side:required".into(), "server_side:optional".into()]),
        _ => {}
    }
    if q.open_source {
        groups.push(vec!["open_source:true".into()]);
    }
    serde_json::to_string(&groups).unwrap_or_else(|_| "[]".into())
}

pub fn modrinth_search_url(q: &Query) -> String {
    format!(
        "{API}/search?query={}&facets={}&index={}&offset={}&limit={}",
        crate::url_encode(&q.text),
        crate::url_encode(&modrinth_facets(q)),
        modrinth_index(&q.sort),
        q.offset,
        q.limit
    )
}

/// Loader names Modrinth mixes into `categories`.
fn is_loader_tag(c: &str) -> bool {
    matches!(c, "fabric" | "forge" | "neoforge" | "quilt" | "minecraft" | "iris" | "optifine" | "canvas" | "vanilla" | "datapack" | "bukkit" | "paper" | "spigot" | "purpur" | "folia" | "sponge" | "velocity" | "bungeecord" | "waterfall" | "liteloader" | "modloader" | "rift" | "babric" | "legacy-fabric" | "ornithe" | "nilloader" | "java-agent" | "bta-babric" | "geyser")
}

/// One search hit in the page's card shape.
pub fn normalize_modrinth_hit(h: &Value, project_type: &str) -> Value {
    let cats = strs(h.get("display_categories").or_else(|| h.get("categories")));
    let all = strs(h.get("categories"));
    let banner = h.get("featured_gallery").and_then(Value::as_str).filter(|u| !u.is_empty()).map(str::to_owned).or_else(|| strs(h.get("gallery")).into_iter().next());
    let slug = Some(s(h, "slug")).filter(|x| !x.is_empty()).unwrap_or(s(h, "project_id"));
    // A datapack search finds projects whose main type is "mod".
    let ptype = if project_type == "datapack" { "datapack" } else { Some(s(h, "project_type")).filter(|x| !x.is_empty()).unwrap_or(project_type) };
    json!({
        "id": s(h, "project_id"),
        "slug": slug,
        "title": s(h, "title"),
        "description": s(h, "description"),
        "author": s(h, "author"),
        "downloads": h.get("downloads").and_then(Value::as_u64).unwrap_or(0),
        "follows": h.get("follows").and_then(Value::as_u64).unwrap_or(0),
        "icon_url": s(h, "icon_url"),
        "banner": banner,
        "color": h.get("color").cloned().unwrap_or(Value::Null),
        "categories": cats.iter().filter(|c| !is_loader_tag(c)).collect::<Vec<_>>(),
        "loaders": all.iter().filter(|c| is_loader_tag(c)).collect::<Vec<_>>(),
        "date_modified": s(h, "date_modified"),
        "project_type": ptype,
        "client_side": s(h, "client_side"),
        "server_side": s(h, "server_side"),
        "license": s(h, "license"),
        "source": "modrinth",
        "view_url": format!("https://modrinth.com/{}/{slug}", if ptype == "datapack" { "datapack" } else { ptype }),
    })
}

/// A Modrinth search answer in the page's shape.
pub fn shape_modrinth_search(raw: &Value, q: &Query) -> Value {
    let hits: Vec<Value> = raw.get("hits").and_then(Value::as_array).map(|a| a.iter().map(|h| normalize_modrinth_hit(h, &q.project_type)).collect()).unwrap_or_default();
    json!({"hits": hits, "total_hits": raw.get("total_hits").and_then(Value::as_u64).unwrap_or(0), "offset": q.offset, "source": "modrinth"})
}

pub fn modrinth_search(q: &Query) -> Value {
    if q.project_type == "world" {
        return json!({"hits": [], "total_hits": 0, "source": "modrinth", "notice": "Modrinth doesn't host worlds. Switch the source to CurseForge to find worlds."});
    }
    match crate::net::get_json(&modrinth_search_url(q)) {
        Ok(v) if v.get("hits").is_some() => shape_modrinth_search(&v, q),
        Ok(_) => json!({"hits": [], "total_hits": 0, "source": "modrinth", "error": "Modrinth sent an answer Lumos couldn't read."}),
        Err(e) => json!({"hits": [], "total_hits": 0, "source": "modrinth", "error": e}),
    }
}

// ───────────────────────── Tags ─────────────────────────

/// Modrinth's category list for one content type, grouped under its headers
/// (categories, features, resolutions, performance impact).
pub fn group_categories(all: &Value, project_type: &str) -> Value {
    // Data packs use the mod categories.
    let want = if project_type == "datapack" { "mod" } else { project_type };
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<Value>> = HashMap::new();
    for c in all.as_array().into_iter().flatten().filter(|c| s(c, "project_type") == want) {
        let header = s(c, "header").to_owned();
        if !groups.contains_key(&header) {
            order.push(header.clone());
        }
        groups.entry(header).or_default().push(json!({"value": s(c, "name"), "label": pretty_tag(s(c, "name"))}));
    }
    // "categories" first, then the rest as Modrinth lists them.
    order.sort_by_key(|h| h != "categories");
    Value::Array(
        order
            .into_iter()
            .map(|h| {
                let mut items = groups.remove(&h).unwrap_or_default();
                items.sort_by(|a, b| s(a, "label").cmp(s(b, "label")));
                json!({"header": pretty_tag(&h), "items": items})
            })
            .collect(),
    )
}

/// "game-mechanics" → "Game mechanics", "16x" stays.
pub fn pretty_tag(t: &str) -> String {
    let t = t.replace(['-', '_'], " ");
    let mut c = t.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Release game versions, newest first.
pub fn release_versions(all: &Value) -> Vec<String> {
    all.as_array().into_iter().flatten().filter(|v| s(v, "version_type") == "release").map(|v| s(v, "version").to_owned()).collect()
}

static TAGS: Mutex<Option<(Instant, Value)>> = Mutex::new(None);

/// Categories (per content type) and game versions, fetched live and kept for a few hours.
pub fn modrinth_tags() -> Result<Value, String> {
    if let Some((at, v)) = TAGS.lock().unwrap().as_ref() {
        if at.elapsed() < TAGS_FRESH {
            return Ok(v.clone());
        }
    }
    let cats = crate::net::get_json(&format!("{API}/tag/category"))?;
    let versions = crate::net::get_json(&format!("{API}/tag/game_version"))?;
    let mut by_type = serde_json::Map::new();
    for t in ["mod", "modpack", "resourcepack", "shader", "datapack"] {
        by_type.insert(t.into(), group_categories(&cats, t));
    }
    let out = json!({"source": "modrinth", "categories": by_type, "gameVersions": release_versions(&versions)});
    *TAGS.lock().unwrap() = Some((Instant::now(), out.clone()));
    Ok(out)
}

// ───────────────────────── Project page ─────────────────────────

/// A Modrinth project for its page: header, body (Markdown), gallery, information panel.
pub fn normalize_modrinth_project(p: &Value, team: &[(String, String)], org: Option<&str>) -> Value {
    let slug = Some(s(p, "slug")).filter(|x| !x.is_empty()).unwrap_or(s(p, "id"));
    let ptype = s(p, "project_type");
    let mut cats = strs(p.get("categories"));
    cats.extend(strs(p.get("additional_categories")));
    let gallery: Vec<Value> = p
        .get("gallery")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|g| json!({"url": s(g, "url"), "raw_url": Some(s(g, "raw_url")).filter(|x| !x.is_empty()).unwrap_or(s(g, "url")), "title": s(g, "title"), "description": s(g, "description"), "featured": g.get("featured").and_then(Value::as_bool).unwrap_or(false)}))
        .collect();
    let banner = gallery.iter().find(|g| g["featured"] == json!(true)).or_else(|| gallery.first()).map(|g| g["url"].clone()).unwrap_or(Value::Null);
    let versions = strs(p.get("game_versions"));
    let mut links = Vec::new();
    for (k, label) in [("source_url", "Source"), ("issues_url", "Issues"), ("wiki_url", "Wiki"), ("discord_url", "Discord")] {
        if let Some(u) = p.get(k).and_then(Value::as_str).filter(|u| u.starts_with("https://")) {
            links.push(json!({"label": label, "url": u}));
        }
    }
    for d in p.get("donation_urls").and_then(Value::as_array).into_iter().flatten() {
        if let Some(u) = d.get("url").and_then(Value::as_str).filter(|u| u.starts_with("https://")) {
            links.push(json!({"label": Some(s(d, "platform")).filter(|x| !x.is_empty()).unwrap_or("Donate"), "url": u}));
        }
    }
    json!({
        "source": "modrinth",
        "id": s(p, "id"),
        "slug": slug,
        "title": s(p, "title"),
        "description": s(p, "description"),
        "body": s(p, "body"),
        "bodyFormat": "markdown",
        "icon_url": s(p, "icon_url"),
        "banner": banner,
        "project_type": ptype,
        "categories": cats.iter().filter(|c| !is_loader_tag(c)).collect::<Vec<_>>(),
        "loaders": strs(p.get("loaders")),
        "gameVersions": versions,
        "downloads": p.get("downloads").and_then(Value::as_u64).unwrap_or(0),
        "followers": p.get("followers").and_then(Value::as_u64).unwrap_or(0),
        "published": s(p, "published"),
        "updated": s(p, "updated"),
        "client_side": s(p, "client_side"),
        "server_side": s(p, "server_side"),
        "license": {"id": p.pointer("/license/id").and_then(Value::as_str).unwrap_or(""), "name": p.pointer("/license/name").and_then(Value::as_str).unwrap_or(""), "url": p.pointer("/license/url").and_then(Value::as_str).unwrap_or("")},
        "team": team.iter().map(|(n, r)| json!({"name": n, "role": r})).collect::<Vec<_>>(),
        "organization": org,
        "gallery": gallery,
        "links": links,
        "view_url": format!("https://modrinth.com/{}/{slug}", if ptype.is_empty() { "project" } else { ptype }),
    })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn modrinth_project(id: &str) -> Result<Value, String> {
    if !valid_id(id) {
        return Err("That project name isn't valid.".into());
    }
    let p = crate::net::get_json(&format!("{API}/project/{id}"))?;
    let team: Vec<(String, String)> = crate::net::get_json(&format!("{API}/project/{id}/members"))
        .ok()
        .and_then(|m| m.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|m| (m.pointer("/user/username").and_then(Value::as_str).unwrap_or("").to_owned(), s(m, "role").to_owned()))
        .filter(|(n, _)| !n.is_empty())
        .collect();
    let org = p
        .get("organization")
        .and_then(Value::as_str)
        .filter(|o| valid_id(o))
        .and_then(|o| crate::net::get_json(&format!("https://api.modrinth.com/v3/organization/{o}")).ok())
        .and_then(|o| o.get("name").and_then(Value::as_str).map(str::to_owned));
    Ok(normalize_modrinth_project(&p, &team, org.as_deref()))
}

/// One version for the Versions tab.
pub fn normalize_modrinth_version(v: &Value, mc: Option<&str>, loaders: &[&str]) -> Value {
    let gv = strs(v.get("game_versions"));
    let ld = strs(v.get("loaders"));
    let file = v.get("files").and_then(Value::as_array).and_then(|a| a.iter().find(|f| f.get("primary").and_then(Value::as_bool) == Some(true)).or_else(|| a.first()));
    json!({
        "id": s(v, "id"),
        "name": s(v, "name"),
        "number": s(v, "version_number"),
        "channel": Some(s(v, "version_type")).filter(|x| !x.is_empty()).unwrap_or("release"),
        "loaders": ld,
        "gameVersions": gv,
        "date": s(v, "date_published"),
        "downloads": v.get("downloads").and_then(Value::as_u64).unwrap_or(0),
        "file": file.map(|f| json!({"name": s(f, "filename"), "size": f.get("size").and_then(Value::as_u64).unwrap_or(0)})),
        "deps": v.get("dependencies").and_then(Value::as_array).map(|d| d.iter().filter(|x| s(x, "dependency_type") == "required").count()).unwrap_or(0),
        "compatible": version_fits(&gv, &ld, mc, loaders),
    })
}

/// True when a version lists the game version (when one is given) and one of the loaders (when given).
pub fn version_fits(game_versions: &[String], version_loaders: &[String], mc: Option<&str>, loaders: &[&str]) -> bool {
    let mc_ok = mc.is_none_or(|m| game_versions.iter().any(|g| g == m));
    let ld_ok = loaders.is_empty() || version_loaders.iter().any(|l| loaders.contains(&l.as_str()));
    mc_ok && ld_ok
}

pub fn modrinth_versions(id: &str, mc: Option<&str>, loaders: &[&str]) -> Result<Value, String> {
    let all = mod_plan::Live.versions(id, None, &[])?;
    Ok(json!(all.iter().map(|v| normalize_modrinth_version(v, mc, loaders)).collect::<Vec<_>>()))
}

// ───────────────────────── Specific versions ─────────────────────────

/// The Modrinth API, except that the main project only has the version the player picked.
pub struct Pinned<'a> {
    pub inner: &'a dyn Modrinth,
    pub version: Value,
}

impl Pinned<'_> {
    pub fn project_id(&self) -> &str {
        s(&self.version, "project_id")
    }
}

impl Modrinth for Pinned<'_> {
    fn versions(&self, project: &str, mc: Option<&str>, loaders: &[&str]) -> Result<Vec<Value>, String> {
        if project == self.project_id() {
            return Ok(vec![self.version.clone()]);
        }
        self.inner.versions(project, mc, loaders)
    }
    fn version(&self, id: &str) -> Result<Value, String> {
        if id == s(&self.version, "id") {
            return Ok(self.version.clone());
        }
        self.inner.version(id)
    }
    fn projects(&self, ids: &[String]) -> Result<Vec<Value>, String> {
        self.inner.projects(ids)
    }
    fn version_files(&self, sha1s: &[String]) -> Result<HashMap<String, Value>, String> {
        self.inner.version_files(sha1s)
    }
}

/// A picked version made for another game version or loader: the player confirms first.
pub fn pinned_problem(version: &Value, project_type: &str, mc: &str, loaders: &[&str]) -> Option<Problem> {
    let gv = strs(version.get("game_versions"));
    let ld = strs(version.get("loaders"));
    let is_mod = project_type == "mod";
    let mc_ok = project_type == "shader" || gv.is_empty() || gv.iter().any(|g| g == mc);
    let ld_ok = !is_mod || version_fits(&[], &ld, None, loaders);
    if mc_ok && ld_ok {
        return None;
    }
    let what = if !mc_ok { format!("Minecraft {}", gv.iter().rev().take(3).cloned().collect::<Vec<_>>().join(", ")) } else { ld.iter().map(|l| mod_plan::loader_name(l)).collect::<Vec<_>>().join(", ") };
    Some(Problem {
        kind: "version",
        message: format!("{} {} is made for {what}, not this instance (Minecraft {mc}). It may not load.", s(version, "name"), s(version, "version_number")).replace("  ", " "),
        blocking: true,
    })
}

// ───────────────────────── Updates ─────────────────────────

/// A newer compatible version of an installed file.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub project_id: String,
    pub file: String,
    /// mods, resourcepacks or shaderpacks.
    pub folder: String,
    /// mod, resourcepack or shader.
    pub project_type: String,
    pub current: String,
    pub version_id: String,
    pub version: String,
    pub channel: String,
}

/// Compares what's installed (`current`: sha1 → its version) with what Modrinth would pick now
/// (`latest`: sha1 → newest compatible version). Only strictly newer versions count, and a
/// stable install isn't offered an alpha.
pub fn updates_from(files: &[LocalFile], current: &HashMap<String, Value>, latest: &HashMap<String, Value>, folder: &str, project_type: &str) -> Vec<Update> {
    let rank = |c: &str| match c {
        "release" => 0,
        "beta" => 1,
        _ => 2,
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for f in files {
        let (Some(cur), Some(new)) = (current.get(&f.sha1), latest.get(&f.sha1)) else { continue };
        if s(cur, "id") == s(new, "id") || s(new, "id").is_empty() || s(cur, "project_id") != s(new, "project_id") {
            continue;
        }
        if s(new, "date_published") <= s(cur, "date_published") {
            continue;
        }
        if rank(s(new, "version_type")) > rank(s(cur, "version_type")).max(1) {
            continue;
        }
        if !seen.insert(s(cur, "project_id").to_owned()) {
            continue;
        }
        out.push(Update {
            project_id: s(cur, "project_id").into(),
            file: f.file.clone(),
            folder: folder.into(),
            project_type: project_type.into(),
            current: s(cur, "version_number").into(),
            version_id: s(new, "id").into(),
            version: s(new, "version_number").into(),
            channel: Some(s(new, "version_type")).filter(|x| !x.is_empty()).unwrap_or("release").into(),
        });
    }
    out
}

/// The update endpoint: sha1 → the newest version for these loaders + game versions.
pub fn latest_versions(sha1s: &[String], loaders: &[&str], mc: Option<&str>) -> Result<HashMap<String, Value>, String> {
    if sha1s.is_empty() {
        return Ok(HashMap::new());
    }
    let mut body = json!({"hashes": sha1s, "algorithm": "sha1", "loaders": loaders});
    if let Some(mc) = mc {
        body["game_versions"] = json!([mc]);
    }
    let resp = crate::net::agent().post(&format!("{API}/version_files/update")).send_json(body).map_err(|e| crate::net::describe_error("api.modrinth.com", &e))?;
    let v: Value = resp.into_json().map_err(|e| format!("api.modrinth.com sent data the launcher couldn't read: {e}"))?;
    Ok(v.as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default())
}

/// Every update for an instance's mods (when it has a loader), resource packs and shaders.
pub fn check_updates(game_dir: &Path, mc: &str, loader: Option<&str>) -> Result<Vec<Update>, String> {
    let mut out = Vec::new();
    let folders: [(&str, &str, Vec<&str>, Option<&str>); 3] = [
        ("mods", "mod", loader.map(crate::forge::compatible_modrinth_loaders).unwrap_or_default(), Some(mc)),
        ("resourcepacks", "resourcepack", vec!["minecraft"], Some(mc)),
        // Shader packs rarely tag game versions.
        ("shaderpacks", "shader", vec!["iris", "optifine", "canvas", "vanilla"], None),
    ];
    for (folder, ptype, loaders, gv) in folders {
        if loaders.is_empty() {
            continue;
        }
        let files: Vec<LocalFile> = mod_plan::scan_dir(&game_dir.join(folder)).into_iter().filter(|f| f.enabled || folder != "mods").collect();
        if files.is_empty() {
            continue;
        }
        let hashes: Vec<String> = files.iter().map(|f| f.sha1.clone()).collect();
        let current = mod_plan::Live.version_files(&hashes)?;
        let known: Vec<String> = hashes.into_iter().filter(|h| current.contains_key(h)).collect();
        let latest = latest_versions(&known, &loaders, gv)?;
        out.extend(updates_from(&files, &current, &latest, folder, ptype));
    }
    Ok(out)
}

// ───────────────────────── Suggestions ─────────────────────────

/// Companions of popular mods (Modrinth slugs): what players usually add next to them.
pub const COMPANIONS: &[(&str, &[&str])] = &[
    ("sodium", &["sodium-extra", "reeses-sodium-options", "iris", "lithium", "entityculling", "immediatelyfast"]),
    ("iris", &["sodium", "sodium-extra"]),
    ("lithium", &["ferrite-core", "sodium", "modernfix"]),
    ("fabric-api", &["modmenu", "sodium", "lithium"]),
    ("create", &["jei", "jade", "create-steam-n-rails"]),
    ("jei", &["jade", "appleskin"]),
    ("emi", &["jade", "appleskin"]),
    ("xaeros-minimap", &["xaeros-world-map"]),
    ("journeymap", &["jade"]),
    ("modmenu", &["cloth-config"]),
    ("voxy", &["sodium"]),
    ("distanthorizons", &["sodium", "iris"]),
    ("waystones", &["balm"]),
    ("farmers-delight", &["appleskin"]),
];

/// A ranked suggestion: project id and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Reason {
    pub key: String,
    pub reason: String,
}

/// Works out what to suggest, best first, from what an instance has:
/// missing optional dependencies, companions of installed mods, then popular mods sharing the
/// instance's top categories. `projects` are the installed projects (id, slug, title, categories);
/// `category_hits` are search hits for the top categories. Nothing installed is suggested.
pub fn rank_suggestions(projects: &[Value], optional_missing: &[(String, String)], category_hits: &[Value], hits_reason: &str) -> Vec<Reason> {
    let installed_ids: HashSet<&str> = projects.iter().map(|p| s(p, "id")).collect();
    let installed_slugs: HashSet<&str> = projects.iter().map(|p| s(p, "slug")).collect();
    let mut out: Vec<Reason> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut push = |key: &str, reason: String, out: &mut Vec<Reason>| {
        if key.is_empty() || installed_ids.contains(key) || installed_slugs.contains(key) || !seen.insert(key.to_owned()) {
            return;
        }
        out.push(Reason { key: key.to_owned(), reason });
    };
    for (id, needed_by) in optional_missing {
        push(id, format!("Optional for {needed_by}"), &mut out);
    }
    for p in projects {
        if let Some((_, list)) = COMPANIONS.iter().find(|(k, _)| *k == s(p, "slug")) {
            for c in *list {
                push(c, format!("Pairs with {}", s(p, "title")), &mut out);
            }
        }
    }
    for h in category_hits {
        let key = Some(s(h, "slug")).filter(|x| !x.is_empty()).unwrap_or(s(h, "project_id"));
        if installed_ids.contains(s(h, "project_id")) {
            continue;
        }
        push(key, hits_reason.to_owned(), &mut out);
    }
    out
}

/// The categories most of an instance's mods are in (loaders and "library" left out).
pub fn top_categories(projects: &[Value]) -> Vec<String> {
    let mut count: HashMap<String, usize> = HashMap::new();
    for p in projects {
        for c in strs(p.get("categories")) {
            if !is_loader_tag(&c) && c != "library" && c != "utility" {
                *count.entry(c).or_default() += 1;
            }
        }
    }
    let mut v: Vec<(String, usize)> = count.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.into_iter().map(|(c, _)| c).collect()
}

type SuggestKey = (PathBuf, String, String, Vec<String>);
static SUGGEST: Mutex<Option<HashMap<SuggestKey, (Instant, Value)>>> = Mutex::new(None);

/// "Recommended for this instance": up to 12 cards, each with a `reason`, compatible with the
/// instance's game version + loader and not installed. Kept per instance until its mods change.
pub fn suggest(game_dir: &Path, mc: &str, loader: &str) -> Result<Value, String> {
    let files = mod_plan::scan_dir(&game_dir.join("mods"));
    let mut sig: Vec<String> = files.iter().map(|f| f.sha1.clone()).collect();
    sig.sort();
    let key: SuggestKey = (game_dir.to_path_buf(), mc.to_owned(), loader.to_owned(), sig);
    if let Some((at, v)) = SUGGEST.lock().unwrap().as_ref().and_then(|m| m.get(&key)) {
        if at.elapsed() < SUGGEST_FRESH {
            return Ok(v.clone());
        }
    }
    let loaders = crate::forge::compatible_modrinth_loaders(loader);
    let installed = mod_plan::identify(&mod_plan::Live, files)?;
    let ids: Vec<String> = installed.iter().filter_map(|i| i.project_id.clone()).collect::<HashSet<_>>().into_iter().collect();
    let projects = if ids.is_empty() { Vec::new() } else { mod_plan::Live.projects(&ids)? };
    let titles: HashMap<&str, &str> = projects.iter().map(|p| (s(p, "id"), s(p, "title"))).collect();
    let mut optional: Vec<(String, String)> = Vec::new();
    for i in &installed {
        for d in i.deps.iter().filter(|d| d.kind == "optional") {
            if let Some(pid) = &d.project_id {
                if !ids.contains(pid) {
                    let by = i.project_id.as_deref().and_then(|p| titles.get(p).copied()).unwrap_or("an installed mod");
                    optional.push((pid.clone(), by.to_owned()));
                }
            }
        }
    }
    let tops = top_categories(&projects);
    let top = tops.first().cloned();
    let category_hits = match &top {
        Some(cat) => {
            let q = Query { project_type: "mod".into(), sort: "downloads".into(), limit: 20, game_version: Some(mc.to_owned()), loaders: loaders.iter().map(|l| l.to_string()).collect(), categories: vec![cat.clone()], ..Default::default() };
            crate::net::get_json(&modrinth_search_url(&q)).ok().and_then(|v| v.get("hits").and_then(Value::as_array).cloned()).unwrap_or_default()
        }
        None => {
            // An empty instance: the most downloaded mods for it.
            let q = Query { project_type: "mod".into(), sort: "downloads".into(), limit: 16, game_version: Some(mc.to_owned()), loaders: loaders.iter().map(|l| l.to_string()).collect(), ..Default::default() };
            crate::net::get_json(&modrinth_search_url(&q)).ok().and_then(|v| v.get("hits").and_then(Value::as_array).cloned()).unwrap_or_default()
        }
    };
    let hits_reason = match &top {
        Some(cat) => format!("Popular in {}", pretty_tag(cat)),
        None => format!("Popular for {mc}"),
    };
    let ranked = rank_suggestions(&projects, &optional, &category_hits, &hits_reason);
    // One lookup for every candidate, to check it fits this instance and get its card.
    let keys: Vec<String> = ranked.iter().map(|r| r.key.clone()).take(40).collect();
    let found = if keys.is_empty() { Vec::new() } else { mod_plan::Live.projects(&keys)? };
    let mut cards = Vec::new();
    for r in &ranked {
        let Some(p) = found.iter().find(|p| s(p, "id") == r.key || s(p, "slug") == r.key) else { continue };
        if ids.iter().any(|i| i == s(p, "id")) || s(p, "project_type") != "mod" {
            continue;
        }
        if !version_fits(&strs(p.get("game_versions")), &strs(p.get("loaders")), Some(mc), &loaders) {
            continue;
        }
        let mut card = normalize_modrinth_project(p, &[], None);
        card["reason"] = json!(r.reason);
        if let Some(obj) = card.as_object_mut() {
            obj.remove("body");
        }
        cards.push(card);
        if cards.len() >= 12 {
            break;
        }
    }
    let out = json!({"cards": cards});
    SUGGEST.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, (Instant::now(), out.clone()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(v: Value) -> Query {
        Query::from_json(&v)
    }

    #[test]
    fn queries_are_read_and_checked() {
        let x = q(json!({"query": " sodium ", "projectType": "nope", "offset": 999999, "limit": 500, "gameVersion": "any",
            "loaders": ["Fabric", "bad\"tag"], "categories": ["optimization"], "environment": "both", "openSource": true}));
        assert_eq!(x.text, "sodium");
        assert_eq!(x.project_type, "mod");
        assert_eq!((x.offset, x.limit), (9_900, 50));
        assert_eq!(x.game_version, None);
        assert_eq!(x.loaders, vec!["fabric"]);
        assert_eq!(x.environment, None);
        assert!(x.open_source);
    }

    #[test]
    fn facets_combine_every_filter() {
        let x = q(json!({"projectType": "mod", "gameVersion": "1.21.1", "loaders": ["fabric", "quilt"], "categories": ["optimization", "utility"], "environment": "client", "openSource": true}));
        let f: Value = serde_json::from_str(&modrinth_facets(&x)).unwrap();
        assert_eq!(
            f,
            json!([["project_type:mod"], ["categories:fabric", "categories:quilt"], ["versions:1.21.1"], ["categories:optimization"], ["categories:utility"],
                ["client_side:required", "client_side:optional"], ["open_source:true"]])
        );
        // Resource packs ignore loaders; no version means any.
        let y = q(json!({"projectType": "resourcepack", "loaders": ["fabric"]}));
        assert_eq!(modrinth_facets(&y), r#"[["project_type:resourcepack"]]"#);
        assert!(modrinth_search_url(&x).contains("index=downloads"));
        assert_eq!(modrinth_index("newest"), "newest");
    }

    #[test]
    fn hits_become_cards() {
        let raw = json!({"total_hits": 18708, "hits": [{"project_id": "AANobbMI", "slug": "sodium", "project_type": "mod", "title": "Sodium", "author": "jellysquid3",
            "description": "Fast", "categories": ["fabric", "optimization"], "display_categories": ["fabric", "optimization"], "downloads": 10, "follows": 3,
            "icon_url": "https://cdn.modrinth.com/i.png", "gallery": ["https://cdn.modrinth.com/g1.png"], "featured_gallery": null, "date_modified": "2026-01-01"}]});
        let out = shape_modrinth_search(&raw, &q(json!({"projectType": "mod"})));
        assert_eq!(out["total_hits"], 18708);
        let h = &out["hits"][0];
        assert_eq!(h["banner"], "https://cdn.modrinth.com/g1.png");
        assert_eq!(h["categories"], json!(["optimization"]));
        assert_eq!(h["loaders"], json!(["fabric"]));
        assert_eq!(h["view_url"], "https://modrinth.com/mod/sodium");
        // Data pack searches find "mod" projects: they stay data packs.
        let dp = normalize_modrinth_hit(&json!({"project_id": "x", "slug": "vein", "project_type": "mod"}), "datapack");
        assert_eq!(dp["project_type"], "datapack");
    }

    #[test]
    fn categories_group_by_type_and_header() {
        let all = json!([
            {"name": "optimization", "project_type": "mod", "header": "categories"},
            {"name": "adventure", "project_type": "mod", "header": "categories"},
            {"name": "16x", "project_type": "resourcepack", "header": "resolutions"},
            {"name": "realistic", "project_type": "resourcepack", "header": "categories"},
            {"name": "low", "project_type": "shader", "header": "performance impact"},
            {"name": "pbr", "project_type": "shader", "header": "features"}
        ]);
        let rp = group_categories(&all, "resourcepack");
        assert_eq!(rp[0]["header"], "Categories");
        assert_eq!(rp[1]["header"], "Resolutions");
        assert_eq!(rp[1]["items"][0], json!({"value": "16x", "label": "16x"}));
        let mods = group_categories(&all, "datapack");
        assert_eq!(mods[0]["items"].as_array().unwrap().len(), 2);
        assert_eq!(mods[0]["items"][0]["value"], "adventure");
        assert_eq!(group_categories(&all, "shader").as_array().unwrap().len(), 2);
        let gv = json!([{"version": "1.21.2", "version_type": "release"}, {"version": "25w01a", "version_type": "snapshot"}, {"version": "1.21.1", "version_type": "release"}]);
        assert_eq!(release_versions(&gv), vec!["1.21.2", "1.21.1"]);
        assert_eq!(pretty_tag("game-mechanics"), "Game mechanics");
    }

    #[test]
    fn projects_and_versions_are_shaped_for_the_page() {
        let p = json!({"id": "AANobbMI", "slug": "sodium", "title": "Sodium", "project_type": "mod", "body": "# Hi", "categories": ["optimization"],
            "loaders": ["fabric"], "game_versions": ["1.21.1"], "license": {"id": "MIT", "name": "MIT License", "url": null},
            "source_url": "https://github.com/x", "discord_url": "javascript:alert(1)", "donation_urls": [{"platform": "Ko-fi", "url": "https://ko-fi.com/x"}],
            "gallery": [{"url": "https://a/1", "featured": false}, {"url": "https://a/2", "featured": true, "title": "T"}]});
        let n = normalize_modrinth_project(&p, &[("jelly".into(), "Owner".into())], Some("CaffeineMC"));
        assert_eq!(n["banner"], "https://a/2");
        assert_eq!(n["links"], json!([{"label": "Source", "url": "https://github.com/x"}, {"label": "Ko-fi", "url": "https://ko-fi.com/x"}]));
        assert_eq!(n["team"][0]["name"], "jelly");
        assert_eq!(n["license"]["id"], "MIT");
        let v = json!({"id": "v1", "name": "Sodium 1", "version_number": "1.0", "version_type": "beta", "loaders": ["fabric"], "game_versions": ["1.21.1"],
            "files": [{"filename": "a.jar", "size": 5, "primary": true}], "dependencies": [{"dependency_type": "required"}, {"dependency_type": "optional"}]});
        let nv = normalize_modrinth_version(&v, Some("1.21.1"), &["fabric"]);
        assert_eq!((nv["channel"].as_str(), nv["compatible"].as_bool(), nv["deps"].as_u64()), (Some("beta"), Some(true), Some(1)));
        assert_eq!(normalize_modrinth_version(&v, Some("1.20.1"), &["fabric"])["compatible"], false);
        assert_eq!(normalize_modrinth_version(&v, None, &["neoforge"])["compatible"], false);
    }

    struct Api;
    impl Modrinth for Api {
        fn versions(&self, project: &str, _: Option<&str>, _: &[&str]) -> Result<Vec<Value>, String> {
            Ok(vec![json!({"id": format!("{project}-latest"), "project_id": project})])
        }
        fn version(&self, id: &str) -> Result<Value, String> {
            Ok(json!({"id": id}))
        }
        fn projects(&self, _: &[String]) -> Result<Vec<Value>, String> {
            Ok(vec![])
        }
        fn version_files(&self, _: &[String]) -> Result<HashMap<String, Value>, String> {
            Ok(HashMap::new())
        }
    }

    #[test]
    fn pinned_versions_replace_only_the_main_project() {
        let pinned = Pinned { inner: &Api, version: json!({"id": "old1", "project_id": "P", "name": "P 1.0", "version_number": "1.0", "game_versions": ["1.20.1"], "loaders": ["forge"]}) };
        assert_eq!(pinned.versions("P", Some("1.21.1"), &["fabric"]).unwrap()[0]["id"], "old1");
        assert_eq!(pinned.versions("dep", None, &[]).unwrap()[0]["id"], "dep-latest");
        let p = pinned_problem(&pinned.version, "mod", "1.21.1", &["fabric"]).unwrap();
        assert!(p.blocking && p.message.contains("Minecraft 1.20.1"));
        let ok = json!({"name": "P", "game_versions": ["1.21.1"], "loaders": ["fabric"]});
        assert!(pinned_problem(&ok, "mod", "1.21.1", &["fabric"]).is_none());
        let wrong_loader = json!({"name": "P", "game_versions": ["1.21.1"], "loaders": ["forge"]});
        assert!(pinned_problem(&wrong_loader, "mod", "1.21.1", &["fabric"]).unwrap().message.contains("Forge"));
        // Resource packs don't care about loaders.
        assert!(pinned_problem(&wrong_loader, "resourcepack", "1.21.1", &["fabric"]).is_none());
    }

    #[test]
    fn updates_are_newer_compatible_versions_only() {
        let files = vec![
            LocalFile { file: "a.jar".into(), sha1: "h1".into(), enabled: true },
            LocalFile { file: "b.jar".into(), sha1: "h2".into(), enabled: true },
            LocalFile { file: "c.jar".into(), sha1: "h3".into(), enabled: true },
            LocalFile { file: "d.jar".into(), sha1: "h4".into(), enabled: true },
        ];
        let v = |id: &str, pid: &str, num: &str, ty: &str, date: &str| json!({"id": id, "project_id": pid, "version_number": num, "version_type": ty, "date_published": date});
        let current: HashMap<String, Value> = [
            ("h1".into(), v("a1", "A", "1.0", "release", "2025-01-01")),
            ("h2".into(), v("b1", "B", "2.0", "release", "2025-01-01")),
            ("h3".into(), v("c2", "C", "3.0", "release", "2025-06-01")),
            ("h4".into(), v("d1", "D", "1.0", "release", "2025-01-01")),
        ]
        .into();
        let latest: HashMap<String, Value> = [
            ("h1".into(), v("a2", "A", "1.1", "release", "2025-02-01")),
            ("h2".into(), v("b1", "B", "2.0", "release", "2025-01-01")),
            ("h3".into(), v("c1", "C", "2.9", "release", "2025-03-01")),
            ("h4".into(), v("d2", "D", "1.1-alpha", "alpha", "2025-02-01")),
        ]
        .into();
        let u = updates_from(&files, &current, &latest, "mods", "mod");
        assert_eq!(u.len(), 1);
        assert_eq!((u[0].project_id.as_str(), u[0].file.as_str(), u[0].version_id.as_str(), u[0].current.as_str()), ("A", "a.jar", "a2", "1.0"));
    }

    #[test]
    fn suggestions_rank_optional_companions_then_categories() {
        let projects = vec![
            json!({"id": "AANobbMI", "slug": "sodium", "title": "Sodium", "categories": ["optimization"]}),
            json!({"id": "gvQqBUqZ", "slug": "lithium", "title": "Lithium", "categories": ["optimization"]}),
        ];
        let hits = vec![json!({"project_id": "AANobbMI", "slug": "sodium"}), json!({"project_id": "X", "slug": "moreculling"})];
        let r = rank_suggestions(&projects, &[("OPT".into(), "Sodium".into())], &hits, "Popular in Optimization");
        let keys: Vec<&str> = r.iter().map(|x| x.key.as_str()).collect();
        assert_eq!(keys[0], "OPT");
        assert_eq!(r[0].reason, "Optional for Sodium");
        assert!(keys.contains(&"sodium-extra") && keys.contains(&"reeses-sodium-options") && keys.contains(&"iris"));
        assert!(!keys.contains(&"sodium") && !keys.contains(&"lithium"), "installed mods aren't suggested");
        assert_eq!(*keys.last().unwrap(), "moreculling");
        assert_eq!(r.last().unwrap().reason, "Popular in Optimization");
        assert_eq!(top_categories(&projects), vec!["optimization"]);
    }

    /// Live Modrinth: tags, a filtered search, a project's versions and an update check.
    #[test]
    #[ignore]
    fn live_modrinth_discover() {
        let tags = modrinth_tags().unwrap();
        let mods = tags["categories"]["mod"].as_array().unwrap();
        assert!(mods.iter().any(|g| g["header"] == "Categories" && g["items"].as_array().unwrap().iter().any(|i| i["value"] == "optimization")));
        assert!(tags["categories"]["resourcepack"].as_array().unwrap().iter().any(|g| g["header"] == "Resolutions"));
        assert!(tags["gameVersions"].as_array().unwrap().iter().any(|v| v == "1.21.1"));

        let q = Query::from_json(&json!({"query": "sodium", "projectType": "mod", "sortBy": "relevance", "gameVersion": "1.21.1", "loaders": ["fabric"], "categories": ["optimization"], "environment": "client", "openSource": false}));
        let res = modrinth_search(&q);
        assert!(res["total_hits"].as_u64().unwrap() > 0, "{res}");
        assert!(res["hits"].as_array().unwrap().iter().any(|h| h["slug"] == "sodium"));

        let page = modrinth_project("sodium").unwrap();
        assert_eq!(page["title"], "Sodium");
        assert!(!page["body"].as_str().unwrap().is_empty());
        let versions = modrinth_versions("sodium", Some("1.21.1"), &["fabric"]).unwrap();
        let list = versions.as_array().unwrap();
        assert!(list.len() > 10);
        assert!(list.iter().any(|v| v["compatible"] == true) && list.iter().any(|v| v["compatible"] == false));

        // An old Sodium for 1.21.1 has a newer compatible version.
        let old = mod_plan::Live.versions("sodium", Some("1.21.1"), &["fabric"]).unwrap();
        let oldest = old.last().unwrap();
        let sha1 = oldest.pointer("/files/0/hashes/sha1").and_then(Value::as_str).unwrap().to_owned();
        let latest = latest_versions(std::slice::from_ref(&sha1), &["fabric"], Some("1.21.1")).unwrap();
        let current = mod_plan::Live.version_files(std::slice::from_ref(&sha1)).unwrap();
        let files = vec![LocalFile { file: "sodium-old.jar".into(), sha1, enabled: true }];
        let ups = updates_from(&files, &current, &latest, "mods", "mod");
        assert_eq!(ups.len(), 1, "{latest:?}");
        assert_eq!(ups[0].project_id, "AANobbMI");
        assert_ne!(ups[0].version_id, oldest["id"].as_str().unwrap());
    }

    /// Live: install an old Sodium Extra version through the planner (pulls in Sodium), find its
    /// update, apply it (the old file is replaced), and get suggestions for the instance.
    #[test]
    #[ignore]
    fn live_pinned_install_update_and_suggest() {
        let dir = std::env::temp_dir().join(format!("lumos-disc-live-{}", std::process::id()));
        let mods = dir.join("mods");
        let _ = std::fs::remove_dir_all(&dir);
        let download = |f: &mod_plan::FileRef, p: &Path| crate::net::download_verified(&f.url, p, f.sha1.as_deref(), f.size);
        let install = |v: Value| {
            let installed = mod_plan::identify(&mod_plan::Live, mod_plan::scan_dir(&mods)).unwrap();
            let pinned = Pinned { inner: &mod_plan::Live, version: v };
            let pid = pinned.project_id().to_owned();
            let plan = mod_plan::plan_modrinth(&pinned, &pid, "mod", "1.21.1", "fabric", &installed).unwrap();
            assert!(!plan.summary.blocked(), "{:?}", plan.summary.problems);
            mod_plan::execute_modrinth(&plan, &mods, false, &download, &|_| true, &|_| {}).unwrap();
            plan
        };
        let old = mod_plan::Live.versions("sodium-extra", Some("1.21.1"), &["fabric"]).unwrap();
        let oldest = old.last().unwrap().clone();
        let plan = install(oldest.clone());
        assert!(plan.summary.deps.iter().any(|d| d == "Sodium"), "{:?}", plan.summary.deps);
        let old_file = plan.main.candidates[0].filename.clone();
        assert!(mods.join(&old_file).is_file());

        let ups = check_updates(&dir, "1.21.1", Some("fabric")).unwrap();
        let up = ups.iter().find(|u| u.file == old_file).expect("an update for the old Sodium Extra");
        let plan = install(mod_plan::Live.version(&up.version_id).unwrap());
        assert_eq!(plan.main.replaces, vec![old_file.clone()]);
        assert!(!mods.join(&old_file).exists(), "the old file was replaced");
        assert!(check_updates(&dir, "1.21.1", Some("fabric")).unwrap().iter().all(|u| u.project_id != up.project_id));

        let s = suggest(&dir, "1.21.1", "fabric").unwrap();
        let cards = s["cards"].as_array().unwrap();
        assert!(!cards.is_empty());
        assert!(cards.iter().all(|c| c["slug"] != "sodium" && c["slug"] != "sodium-extra"));
        assert!(cards.iter().any(|c| c["reason"].as_str().unwrap().starts_with("Pairs with")), "{s}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
