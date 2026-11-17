//! Lumos capes: free cosmetic capes for Ely.by players.
//!
//! The capes server (`capes-server/` in the repo, a Cloudflare Worker) keeps the catalog and
//! each player's choice. To change a cape the launcher proves the account is the player's the
//! way a Minecraft server would: it asks the capes server for a one-time `serverId`, "joins"
//! it on Ely.by with the player's own session (`/session/join`), then sends the choice; the
//! capes server confirms it with Ely.by's `/session/hasJoined`. The token never leaves for
//! anywhere but Ely.by.
//!
//! In game the LumosCapes Fabric mod (`lumoscapes/`) draws the chosen capes; the launcher
//! copies it into Fabric 1.21.11 instances and passes it the server address.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use crate::elyby::{ElySession, AUTH_BASE};

/// Address of the deployed capes server, without a trailing slash. Empty until it is deployed
/// (see capes-server/README.md); the launcher then says the capes server isn't set up yet.
/// `LUMOS_CAPES_URL` in the environment overrides it (for trying a local `wrangler dev`).
pub const LUMOS_CAPES_URL: &str = "";

/// The in-game mod's file name inside an instance's mods folder.
pub const MOD_JAR: &str = "lumoscapes-1.21.11.jar";
/// System property the mod reads the server address from.
pub const URL_PROPERTY: &str = "lumoscapes.url";

pub const NOT_SET_UP: &str = "Capes server not set up yet.";
const MAX_CAPE_BYTES: u64 = 64 * 1024;

/// The server address to use, if any: the environment override, else the built-in one.
pub fn server_url() -> Option<String> {
    pick_url(std::env::var("LUMOS_CAPES_URL").ok().as_deref(), LUMOS_CAPES_URL)
}

fn pick_url(env: Option<&str>, built_in: &str) -> Option<String> {
    [env.unwrap_or(""), built_in].into_iter().map(|u| u.trim().trim_end_matches('/')).find(|u| !u.is_empty()).filter(|u| valid_base(u)).map(str::to_owned)
}

/// https anywhere; plain http only for a server on this computer.
fn valid_base(u: &str) -> bool {
    let host_ok = |rest: &str| {
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        !host.is_empty() && !host.contains('@') && !rest.contains(['?', '#', ' '])
    };
    if let Some(rest) = u.strip_prefix("https://") {
        return host_ok(rest);
    }
    if let Some(rest) = u.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or("");
        return host_ok(rest) && (host == "localhost" || host == "127.0.0.1");
    }
    false
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn is_hex32(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

// ---------------------------------------------------------------- HTTP

/// The few HTTP calls this module makes, so tests can stand in for the network.
pub trait Http {
    fn get(&self, url: &str) -> Result<(u16, Option<Value>), String>;
    fn post(&self, url: &str, body: &Value) -> Result<(u16, Option<Value>), String>;
}

/// The real network, through the launcher's shared agent.
pub struct Net;

fn answer(res: Result<ureq::Response, ureq::Error>, host: &str) -> Result<(u16, Option<Value>), String> {
    match res {
        Ok(r) => {
            let code = r.status();
            Ok((code, r.into_string().ok().and_then(|t| serde_json::from_str(&t).ok())))
        }
        Err(ureq::Error::Status(code, r)) => Ok((code, r.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()))),
        Err(e) => Err(crate::net::describe_error(host, &e)),
    }
}

fn host_name(url: &str) -> &str {
    if url.starts_with(AUTH_BASE) {
        "Ely.by"
    } else {
        "the capes server"
    }
}

impl Http for Net {
    fn get(&self, url: &str) -> Result<(u16, Option<Value>), String> {
        answer(crate::net::agent().get(url).call(), host_name(url))
    }
    fn post(&self, url: &str, body: &Value) -> Result<(u16, Option<Value>), String> {
        answer(
            crate::net::agent().post(url).set("Content-Type", "application/json").send_json(body.clone()),
            host_name(url),
        )
    }
}

fn server_error(code: u16, body: Option<&Value>) -> String {
    let msg = body.and_then(|b| b.get("error")).and_then(Value::as_str).unwrap_or("").trim();
    match code {
        429 => "Too many cape changes. Wait a minute and try again.".into(),
        _ if !msg.is_empty() => format!("The capes server said: {msg}"),
        500..=599 => format!("The capes server is having problems ({code}). Try again later."),
        _ => format!("The capes server answered with an error ({code})."),
    }
}

// ---------------------------------------------------------------- catalog

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Cape {
    pub id: String,
    pub name: String,
    pub url: String,
}

/// Reads the catalog. Texture addresses always point at `base`, whatever the answer says.
pub fn parse_catalog(body: &Value, base: &str) -> Vec<Cape> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = c.get("id")?.as_str()?;
            if !valid_id(id) {
                return None;
            }
            let name = c.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty()).unwrap_or(id);
            Some(Cape { id: id.into(), name: name.chars().take(40).collect(), url: format!("{base}/capes/{id}.png") })
        })
        .collect()
}

pub fn catalog(http: &dyn Http, base: &str) -> Result<Vec<Cape>, String> {
    match http.get(&format!("{base}/v1/capes"))? {
        (200, Some(body)) => Ok(parse_catalog(&body, base)),
        (code, body) => Err(server_error(code, body.as_ref())),
    }
}

/// The cape a player wears (by Ely.by name), if any.
pub fn player_cape(http: &dyn Http, base: &str, name: &str) -> Result<Option<String>, String> {
    let url = format!("{base}/v1/player/{}", crate::url_encode(name));
    match http.get(&url)? {
        (200, Some(body)) => Ok(body.get("cape").and_then(Value::as_str).filter(|c| valid_id(c)).map(str::to_owned)),
        (code, body) => Err(server_error(code, body.as_ref())),
    }
}

/// A cape texture as a `data:` URI for the launcher's preview.
pub fn texture_data_uri(url: &str) -> Result<String, String> {
    let bytes = crate::net::get_bytes(url, MAX_CAPE_BYTES)?;
    png_data_uri(&bytes)
}

fn png_data_uri(bytes: &[u8]) -> Result<String, String> {
    const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if !bytes.starts_with(&PNG) {
        return Err("That cape isn't a PNG image.".into());
    }
    Ok(format!("data:image/png;base64,{}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)))
}

// ---------------------------------------------------------------- choosing a cape

#[derive(Debug, PartialEq)]
pub enum SelectError {
    /// Ely.by refused the session: the player has to sign in again.
    SignInAgain,
    Failed(String),
}

impl std::fmt::Display for SelectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectError::SignInAgain => write!(f, "Your Ely.by sign-in has expired. Sign in again to change your cape."),
            SelectError::Failed(m) => write!(f, "{m}"),
        }
    }
}

impl From<String> for SelectError {
    fn from(s: String) -> Self {
        SelectError::Failed(s)
    }
}

/// Wears `cape` (or no cape for `None`) on the session's account.
/// `session` should be fresh (`elyby::ensure_fresh`).
pub fn select(http: &dyn Http, base: &str, session: &ElySession, cape: Option<&str>) -> Result<(), SelectError> {
    if let Some(c) = cape {
        if !valid_id(c) {
            return Err(SelectError::Failed("That cape isn't in the catalog.".into()));
        }
    }
    // 1. A one-time id from the capes server.
    let server_id = match http.get(&format!("{base}/v1/challenge"))? {
        (200, Some(body)) => body.get("serverId").and_then(Value::as_str).filter(|s| is_hex32(s)).map(str::to_owned),
        (code, body) => return Err(SelectError::Failed(server_error(code, body.as_ref()))),
    }
    .ok_or_else(|| SelectError::Failed("The capes server sent an answer the launcher couldn't read.".into()))?;

    // 2. Join it on Ely.by, like joining a server.
    let join = json!({
        "accessToken": session.access_token,
        "selectedProfile": session.uuid,
        "serverId": server_id,
    });
    match http.post(&format!("{AUTH_BASE}/session/join"), &join)? {
        (200..=299, _) => {}
        (401 | 403, _) => return Err(SelectError::SignInAgain),
        (429, _) => return Err(SelectError::Failed("Ely.by is rate-limiting requests. Wait a minute and try again.".into())),
        (code, _) => return Err(SelectError::Failed(format!("Ely.by couldn't confirm your account right now ({code}). Try again later."))),
    }

    // 3. Send the choice; the capes server checks the join with Ely.by.
    let body = json!({ "username": session.name, "serverId": server_id, "cape": cape });
    match http.post(&format!("{base}/v1/select"), &body)? {
        (200, _) => Ok(()),
        (code, body) => Err(SelectError::Failed(server_error(code, body.as_ref()))),
    }
}

// ---------------------------------------------------------------- the in-game mod

/// LumosCapes is built against Minecraft 1.21.11 (Mojang names, remapped), so only that version.
pub fn mod_supported(version: &str) -> bool {
    matches!(crate::installer::parse_spec(version), crate::installer::Spec::Fabric(mc) if mc == "1.21.11")
}

/// Puts LumosCapes in (or takes it out of) an instance's mods folder before launch, and returns
/// the JVM argument that tells it where the capes server is (None when it isn't installed).
pub fn sync_mod(game_dir: &Path, version: &str, enabled: bool, bundled: Option<&Path>, base: Option<&str>) -> Option<String> {
    let mods_dir = game_dir.join("mods");
    let dest = mods_dir.join(MOD_JAR);
    match (enabled && mod_supported(version), bundled, base) {
        (true, Some(src), Some(base)) => {
            let _ = fs::create_dir_all(&mods_dir);
            fs::copy(src, &dest).ok()?;
            Some(format!("-D{URL_PROPERTY}={base}"))
        }
        _ => {
            if dest.is_file() {
                let _ = fs::remove_file(&dest);
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// Answers calls from a script and records what was asked.
    struct Script {
        answers: RefCell<VecDeque<Result<(u16, Option<Value>), String>>>,
        calls: RefCell<Vec<(String, String, Option<Value>)>>,
    }

    impl Script {
        fn new(answers: Vec<Result<(u16, Option<Value>), String>>) -> Self {
            Script { answers: RefCell::new(answers.into()), calls: RefCell::new(vec![]) }
        }
        fn next(&self) -> Result<(u16, Option<Value>), String> {
            self.answers.borrow_mut().pop_front().expect("unexpected extra request")
        }
    }

    impl Http for Script {
        fn get(&self, url: &str) -> Result<(u16, Option<Value>), String> {
            self.calls.borrow_mut().push(("GET".into(), url.into(), None));
            self.next()
        }
        fn post(&self, url: &str, body: &Value) -> Result<(u16, Option<Value>), String> {
            self.calls.borrow_mut().push(("POST".into(), url.into(), Some(body.clone())));
            self.next()
        }
    }

    const BASE: &str = "https://capes.example.dev";
    const SID: &str = "0123456789abcdef0123456789abcdef";

    fn session() -> ElySession {
        ElySession { uuid: "a".repeat(32), name: "Steve".into(), access_token: "tok".into(), client_token: "ct".into() }
    }

    #[test]
    fn server_url_is_optional_and_checked() {
        assert_eq!(pick_url(None, ""), None);
        assert_eq!(pick_url(Some("  "), ""), None);
        assert_eq!(pick_url(None, "https://lumos-capes.x.workers.dev/"), Some("https://lumos-capes.x.workers.dev".into()));
        assert_eq!(pick_url(Some("http://localhost:8787"), "https://a.dev"), Some("http://localhost:8787".into()));
        assert_eq!(pick_url(Some("http://evil.example"), ""), None);
        assert_eq!(pick_url(None, "https://user@evil.example"), None);
        assert_eq!(pick_url(None, "ftp://x"), None);
    }

    #[test]
    fn catalog_urls_stay_on_the_server() {
        let body = json!([
            {"id": "aurora", "name": "Aurora", "url": "https://elsewhere.example/a.png"},
            {"id": "../etc", "name": "Bad"},
            {"id": "galaxy"}
        ]);
        let list = parse_catalog(&body, BASE);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], Cape { id: "aurora".into(), name: "Aurora".into(), url: format!("{BASE}/capes/aurora.png") });
        assert_eq!(list[1].name, "galaxy");
    }

    #[test]
    fn player_lookup() {
        let http = Script::new(vec![Ok((200, Some(json!({"cape": "ocean", "url": "x"})))), Ok((200, Some(json!({"cape": null}))))]);
        assert_eq!(player_cape(&http, BASE, "Steve Two").unwrap(), Some("ocean".into()));
        assert_eq!(player_cape(&http, BASE, "Alex").unwrap(), None);
        assert_eq!(http.calls.borrow()[0].1, format!("{BASE}/v1/player/Steve%20Two"));
    }

    #[test]
    fn select_runs_challenge_join_select() {
        let http = Script::new(vec![
            Ok((200, Some(json!({"serverId": SID})))),
            Ok((204, None)),
            Ok((200, Some(json!({"ok": true})))),
        ]);
        select(&http, BASE, &session(), Some("aurora")).unwrap();
        let calls = http.calls.borrow();
        assert_eq!((calls[0].0.as_str(), calls[0].1.as_str()), ("GET", &*format!("{BASE}/v1/challenge")));
        assert_eq!(calls[1].1, "https://authserver.ely.by/session/join");
        assert_eq!(calls[1].2, Some(json!({"accessToken": "tok", "selectedProfile": "a".repeat(32), "serverId": SID})));
        assert_eq!(calls[2].1, format!("{BASE}/v1/select"));
        assert_eq!(calls[2].2, Some(json!({"username": "Steve", "serverId": SID, "cape": "aurora"})));
        // The token only goes to Ely.by.
        assert!(!calls[2].2.as_ref().unwrap().to_string().contains("tok"));
    }

    #[test]
    fn removing_sends_null() {
        let http = Script::new(vec![Ok((200, Some(json!({"serverId": SID})))), Ok((200, None)), Ok((200, None))]);
        select(&http, BASE, &session(), None).unwrap();
        assert_eq!(http.calls.borrow()[2].2.as_ref().unwrap()["cape"], Value::Null);
    }

    #[test]
    fn select_errors() {
        // Expired Ely.by token: sign in again, and the choice is never sent.
        let http = Script::new(vec![Ok((200, Some(json!({"serverId": SID})))), Ok((403, None))]);
        assert_eq!(select(&http, BASE, &session(), Some("aurora")), Err(SelectError::SignInAgain));
        assert_eq!(http.calls.borrow().len(), 2);
        // A bad challenge stops before anything reaches Ely.by.
        let http = Script::new(vec![Ok((200, Some(json!({"serverId": "nope"}))))]);
        assert!(matches!(select(&http, BASE, &session(), Some("aurora")), Err(SelectError::Failed(_))));
        assert_eq!(http.calls.borrow().len(), 1);
        // The server's own message is shown.
        let http = Script::new(vec![
            Ok((200, Some(json!({"serverId": SID})))),
            Ok((204, None)),
            Ok((403, Some(json!({"error": "challenge expired, try again"})))),
        ]);
        assert_eq!(
            select(&http, BASE, &session(), Some("aurora")),
            Err(SelectError::Failed("The capes server said: challenge expired, try again".into()))
        );
        // Network trouble passes through; odd cape ids never leave the launcher.
        let http = Script::new(vec![Err("Couldn't connect".into())]);
        assert_eq!(select(&http, BASE, &session(), Some("aurora")), Err(SelectError::Failed("Couldn't connect".into())));
        let http = Script::new(vec![]);
        assert!(select(&http, BASE, &session(), Some("../x")).is_err());
    }

    #[test]
    fn png_only() {
        assert!(png_data_uri(b"<html>").is_err());
        let uri = png_data_uri(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn mod_goes_into_fabric_1_21_11_only_when_ready() {
        assert!(mod_supported("Fabric 1.21.11"));
        assert!(!mod_supported("Fabric 1.21.4") && !mod_supported("1.21.11") && !mod_supported("Horizon 26.3 (Rust)"));

        let dir = std::env::temp_dir().join(format!("lumos-capes-{}", crate::now_millis()));
        fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("built.jar");
        fs::write(&jar, b"jar").unwrap();
        let dest = dir.join("game/mods").join(MOD_JAR);

        let arg = sync_mod(&dir.join("game"), "Fabric 1.21.11", true, Some(&jar), Some(BASE));
        assert_eq!(arg.as_deref(), Some("-Dlumoscapes.url=https://capes.example.dev"));
        assert!(dest.is_file());
        // Turned off, another version, or no server: the mod is taken out again.
        assert_eq!(sync_mod(&dir.join("game"), "Fabric 1.21.11", false, Some(&jar), Some(BASE)), None);
        assert!(!dest.exists());
        sync_mod(&dir.join("game"), "Fabric 1.21.11", true, Some(&jar), Some(BASE));
        assert_eq!(sync_mod(&dir.join("game"), "Fabric 1.21.11", true, Some(&jar), None), None);
        assert!(!dest.exists());
        assert_eq!(sync_mod(&dir.join("game"), "Fabric 1.21.4", true, Some(&jar), Some(BASE)), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
