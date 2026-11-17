//! Ely.by accounts (Yggdrasil-compatible auth server), authlib-injector and skins.
//!
//! Ely.by has no public API to browse the skin catalog or change a skin, so the
//! launcher opens [`CATALOG_URL`] in a window for that; this module only reads
//! a player's current textures.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const AUTH_BASE: &str = "https://authserver.ely.by";
pub const CATALOG_URL: &str = "https://ely.by/skins";
pub const AUTHLIB_VERSION: &str = "1.2.8";

const SKINSYSTEM_HTTPS: &str = "https://skinsystem.ely.by";
const SKINSYSTEM_HTTP: &str = "http://skinsystem.ely.by";
const AUTHLIB_URL: &str =
    "https://github.com/yushijinhun/authlib-injector/releases/download/v1.2.8/authlib-injector-1.2.8.jar";
const AUTHLIB_SIZE: u64 = 349_681;
const AUTHLIB_SHA256: &str = "9c7f4343e6c82034958ffb48c14a2cb0c85928be7283103ce17da00c6d5a7b10";
const MAX_TEXTURE_BYTES: u64 = 512 * 1024;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ElySession {
    /// 32 lowercase hex characters, no dashes.
    pub uuid: String,
    pub name: String,
    pub access_token: String,
    pub client_token: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthError {
    NeedTotp,
    InvalidCredentials(String),
    RateLimited,
    Network(String),
    Server(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::NeedTotp => {
                write!(f, "This account uses two-factor authentication. Enter the code from your authenticator app.")
            }
            AuthError::InvalidCredentials(msg) => {
                if msg.trim().is_empty() {
                    write!(f, "Ely.by didn't accept those details. Check your login and password and try again.")
                } else {
                    write!(f, "Ely.by didn't accept those details: {}", msg.trim())
                }
            }
            AuthError::RateLimited => write!(f, "Too many sign-in attempts. Wait a minute and try again."),
            AuthError::Network(msg) => write!(f, "{msg}"),
            AuthError::Server(msg) => {
                if msg.trim().is_empty() {
                    write!(f, "Ely.by is having problems right now. Try again later.")
                } else {
                    write!(f, "Ely.by is having problems right now: {}", msg.trim())
                }
            }
        }
    }
}

impl std::error::Error for AuthError {}

/// A random-looking 32-hex client token (time + pid + counter + stack address, hashed).
pub fn new_client_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let marker = 0u8;
    let mut h = Sha256::new();
    h.update(nanos.to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    h.update(n.to_le_bytes());
    h.update((&marker as *const u8 as usize).to_le_bytes());
    h.update(format!("{:?}", std::thread::current().id()).as_bytes());
    hex(&h.finalize()[..16])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Strip dashes and lowercase a uuid.
fn normalize_uuid(u: &str) -> String {
    u.chars().filter(|c| *c != '-').collect::<String>().to_ascii_lowercase()
}

fn is_hex32(u: &str) -> bool {
    u.len() == 32 && u.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Undashed uuid -> dashed 8-4-4-4-12; returns input unchanged if not 32 hex.
pub fn dashed_uuid(u: &str) -> String {
    if !is_hex32(u) {
        return u.to_string();
    }
    format!("{}-{}-{}-{}-{}", &u[0..8], &u[8..12], &u[12..16], &u[16..20], &u[20..32])
}

// ---------------------------------------------------------------- auth

enum PostOutcome {
    Ok(Option<Value>),
    Status(u16, Option<Value>),
}

/// POST JSON to the auth server. Never includes the request body in errors.
fn post(endpoint: &str, body: &Value) -> Result<PostOutcome, AuthError> {
    let url = format!("{AUTH_BASE}{endpoint}");
    match crate::net::agent().post(&url).set("Content-Type", "application/json").send_json(body.clone()) {
        Ok(resp) => {
            let text = resp.into_string().unwrap_or_default();
            Ok(PostOutcome::Ok(serde_json::from_str(&text).ok()))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            Ok(PostOutcome::Status(code, serde_json::from_str(&text).ok()))
        }
        Err(e) => Err(AuthError::Network(crate::net::describe_error("Ely.by", &e))),
    }
}

/// Map a non-200 auth server response to an error.
fn map_error(status: u16, body: Option<&Value>) -> AuthError {
    let error = body.and_then(|b| b.get("error")).and_then(Value::as_str).unwrap_or("");
    let message = body.and_then(|b| b.get("errorMessage")).and_then(Value::as_str).unwrap_or("").to_string();
    let lower = message.to_lowercase();
    if status == 429 {
        return AuthError::RateLimited;
    }
    if lower.contains("two factor") || lower.contains("two-factor") || lower.contains("2fa") {
        return AuthError::NeedTotp;
    }
    if lower.contains("too many") || lower.contains("rate limit") {
        return AuthError::RateLimited;
    }
    match status {
        400 | 401 | 403 | 404 | 410 => AuthError::InvalidCredentials(message),
        _ if error == "ForbiddenOperationException" || error == "IllegalArgumentException" => {
            AuthError::InvalidCredentials(message)
        }
        500..=599 => AuthError::Server(if message.is_empty() { format!("error {status}") } else { message }),
        _ => AuthError::Server(if message.is_empty() { format!("unexpected answer ({status})") } else { message }),
    }
}

/// Parse an authenticate/refresh success body into a session.
fn parse_session(body: &Value, fallback_client_token: &str) -> Result<ElySession, AuthError> {
    let bad = |what: &str| AuthError::Server(format!("the sign-in answer was missing {what}"));
    let access_token =
        body.get("accessToken").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| bad("a token"))?;
    let client_token =
        body.get("clientToken").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(fallback_client_token);
    let profile = body
        .get("selectedProfile")
        .filter(|p| p.is_object())
        .or_else(|| body.get("availableProfiles").and_then(Value::as_array).and_then(|a| a.first()))
        .ok_or_else(|| AuthError::InvalidCredentials("This Ely.by account has no Minecraft profile.".into()))?;
    let uuid = normalize_uuid(profile.get("id").and_then(Value::as_str).unwrap_or(""));
    if !is_hex32(&uuid) {
        return Err(bad("a valid player id"));
    }
    let name =
        profile.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| bad("a player name"))?;
    Ok(ElySession {
        uuid,
        name: name.to_string(),
        access_token: access_token.to_string(),
        client_token: client_token.to_string(),
    })
}

fn session_from(outcome: PostOutcome, client_token: &str) -> Result<ElySession, AuthError> {
    match outcome {
        PostOutcome::Ok(Some(body)) => parse_session(&body, client_token),
        PostOutcome::Ok(None) => Err(AuthError::Server("the sign-in answer couldn't be read".into())),
        PostOutcome::Status(code, body) => Err(map_error(code, body.as_ref())),
    }
}

/// Ely.by takes the TOTP appended to the password as `password:totp`.
fn password_field(password: &str, totp: Option<&str>) -> String {
    match totp.map(|t| t.chars().filter(|c| !c.is_whitespace()).collect::<String>()).filter(|t| !t.is_empty()) {
        Some(t) => format!("{password}:{t}"),
        None => password.to_string(),
    }
}

pub fn authenticate(login: &str, password: &str, totp: Option<&str>, client_token: &str) -> Result<ElySession, AuthError> {
    let login = login.trim();
    if login.is_empty() || password.is_empty() {
        return Err(AuthError::InvalidCredentials("Enter your Ely.by login and password.".into()));
    }
    let body = json!({
        "username": login,
        "password": password_field(password, totp),
        "clientToken": client_token,
        "requestUser": true,
    });
    session_from(post("/auth/authenticate", &body)?, client_token)
}

pub fn refresh(session: &ElySession) -> Result<ElySession, AuthError> {
    let body = json!({
        "accessToken": session.access_token,
        "clientToken": session.client_token,
        "requestUser": true,
    });
    session_from(post("/auth/refresh", &body)?, &session.client_token)
}

/// Ok(false) for a rejected token, Err only for network trouble (or a rate-limited / failing server).
pub fn validate(access_token: &str) -> Result<bool, AuthError> {
    match post("/auth/validate", &json!({ "accessToken": access_token }))? {
        PostOutcome::Ok(_) => Ok(true),
        PostOutcome::Status(429, _) => Err(AuthError::RateLimited),
        PostOutcome::Status(code, _) if code >= 500 => Err(AuthError::Server(format!("error {code}"))),
        PostOutcome::Status(_, _) => Ok(false),
    }
}

/// Best effort; ignores errors.
pub fn invalidate(session: &ElySession) {
    let _ = post(
        "/auth/invalidate",
        &json!({ "accessToken": session.access_token, "clientToken": session.client_token }),
    );
}

/// Valid session to launch with: validate, else refresh; Err(InvalidCredentials) when the player must sign in again.
pub fn ensure_fresh(session: &ElySession) -> Result<ElySession, AuthError> {
    if validate(&session.access_token)? {
        return Ok(session.clone());
    }
    match refresh(session) {
        Ok(s) => Ok(s),
        Err(AuthError::InvalidCredentials(_)) | Err(AuthError::NeedTotp) => {
            Err(AuthError::InvalidCredentials("Your Ely.by sign-in has expired. Sign in again.".into()))
        }
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------- skins

#[derive(Clone, Debug, Default, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Textures {
    pub skin_url: Option<String>,
    pub slim: bool,
    pub cape_url: Option<String>,
}

fn valid_name(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn parse_textures(status: u16, body: Option<&Value>) -> Result<Textures, String> {
    match status {
        204 | 404 => Ok(Textures::default()),
        200 => {
            let Some(body) = body else { return Ok(Textures::default()) };
            let url_of = |k: &str| {
                body.get(k)
                    .and_then(|t| t.get("url"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            let slim = body
                .get("SKIN")
                .and_then(|s| s.get("metadata"))
                .and_then(|m| m.get("model"))
                .and_then(Value::as_str)
                .is_some_and(|m| m.eq_ignore_ascii_case("slim"));
            Ok(Textures { skin_url: url_of("SKIN"), slim, cape_url: url_of("CAPE") })
        }
        other => Err(format!("Ely.by's skin server answered with an error ({other}).")),
    }
}

/// Outer Err = transport failure (worth retrying over http); inner = final answer.
fn get_textures_from(base: &str, name: &str) -> Result<Result<Textures, String>, ureq::Error> {
    let url = format!("{base}/textures/{name}");
    match crate::net::agent().get(&url).call() {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.into_string().unwrap_or_default();
            let body: Option<Value> = if text.trim().is_empty() { None } else { serde_json::from_str(&text).ok() };
            if status == 200 && !text.trim().is_empty() && body.is_none() {
                return Ok(Err("Ely.by's skin server sent data the launcher couldn't read.".into()));
            }
            Ok(parse_textures(status, body.as_ref()))
        }
        Err(ureq::Error::Status(code, _)) => Ok(parse_textures(code, None)),
        Err(e) => Err(e),
    }
}

pub fn fetch_textures(name: &str) -> Result<Textures, String> {
    let name = name.trim();
    if !valid_name(name) {
        return Err("That isn't a valid player name.".into());
    }
    match get_textures_from(SKINSYSTEM_HTTPS, name) {
        Ok(r) => r,
        Err(https_err) => match get_textures_from(SKINSYSTEM_HTTP, name) {
            Ok(r) => r,
            Err(_) => Err(crate::net::describe_error("skinsystem.ely.by", &https_err)),
        },
    }
}

/// Validate a texture URL: host ely.by or *.ely.by, upgraded to https. Returns the https URL.
fn safe_texture_url(url: &str) -> Result<String, String> {
    let deny = || "Only Ely.by texture links can be shown.".to_string();
    let url = url.trim();
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).ok_or_else(deny)?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].to_ascii_lowercase();
    let ok = !host.is_empty()
        && !host.contains(['@', ':', '\\'])
        && !host.starts_with('.')
        && (host == "ely.by" || host.ends_with(".ely.by"));
    if !ok {
        return Err(deny());
    }
    Ok(format!("https://{host}{}", &rest[end..]))
}

/// PNG bytes as a data: URI ("data:image/png;base64,...") for the UI, max 512 KiB, must start with the PNG signature.
pub fn fetch_texture_data_uri(url: &str) -> Result<String, String> {
    let url = safe_texture_url(url)?;
    let bytes = crate::net::get_bytes(&url, MAX_TEXTURE_BYTES)?;
    png_data_uri(&bytes)
}

fn png_data_uri(bytes: &[u8]) -> Result<String, String> {
    use base64::Engine;
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err("The skin file from Ely.by isn't a PNG image.".into());
    }
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

// ---------------------------------------------------------------- authlib-injector

fn sha256_file(path: &Path) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(hex(&h.finalize()))
}

fn jar_is_valid(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() == AUTHLIB_SIZE)
        && sha256_file(path).is_some_and(|h| h == AUTHLIB_SHA256)
}

/// Ensure authlib-injector exists at `<dir>/authlib-injector-1.2.8.jar`: copy from `bundled` when given and it hashes right,
/// else download from GitHub; verify sha256 + size; atomic write. Returns the jar path.
pub fn ensure_authlib_injector(dir: &Path, bundled: Option<&Path>) -> Result<PathBuf, String> {
    let dest = dir.join(format!("authlib-injector-{AUTHLIB_VERSION}.jar"));
    if jar_is_valid(&dest) {
        return Ok(dest);
    }
    fs::create_dir_all(dir).map_err(|e| format!("Couldn't create folder {}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".authlib-injector-{AUTHLIB_VERSION}.jar.tmp"));
    let _ = fs::remove_file(&tmp);

    if let Some(src) = bundled {
        if jar_is_valid(src) && fs::copy(src, &tmp).is_ok() && jar_is_valid(&tmp) {
            fs::rename(&tmp, &dest).map_err(|e| format!("Couldn't save {}: {e}", dest.display()))?;
            return Ok(dest);
        }
        let _ = fs::remove_file(&tmp);
    }

    crate::net::download_verified(AUTHLIB_URL, &tmp, None, Some(AUTHLIB_SIZE))
        .map_err(|e| format!("Couldn't download authlib-injector: {e}"))?;
    if !jar_is_valid(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err("The downloaded authlib-injector failed its integrity check.".into());
    }
    fs::rename(&tmp, &dest).map_err(|e| format!("Couldn't save {}: {e}", dest.display()))?;
    Ok(dest)
}

/// "-javaagent:<path>=ely.by"
pub fn javaagent_arg(jar: &Path) -> String {
    format!("-javaagent:{}=ely.by", jar.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_factor_maps_to_need_totp() {
        // Exact wording from docs.ely.by/en/minecraft-auth.html
        let body = json!({"error":"ForbiddenOperationException","errorMessage":"Account protected with two factor auth."});
        assert_eq!(map_error(401, Some(&body)), AuthError::NeedTotp);
        let body = json!({"error":"ForbiddenOperationException","errorMessage":"ACCOUNT PROTECTED WITH TWO-FACTOR AUTH"});
        assert_eq!(map_error(401, Some(&body)), AuthError::NeedTotp);
    }

    #[test]
    fn error_mapping() {
        let bad = json!({"error":"ForbiddenOperationException","errorMessage":"Invalid credentials. Invalid email or password."});
        assert_eq!(
            map_error(401, Some(&bad)),
            AuthError::InvalidCredentials("Invalid credentials. Invalid email or password.".into())
        );
        assert_eq!(map_error(429, None), AuthError::RateLimited);
        assert_eq!(map_error(429, Some(&bad)), AuthError::RateLimited);
        assert!(matches!(map_error(503, None), AuthError::Server(_)));
        assert!(matches!(
            map_error(400, Some(&json!({"error":"IllegalArgumentException","errorMessage":"x"}))),
            AuthError::InvalidCredentials(_)
        ));
        for e in [
            AuthError::NeedTotp,
            AuthError::RateLimited,
            AuthError::InvalidCredentials(String::new()),
            AuthError::Server(String::new()),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }

    #[test]
    fn session_parsing() {
        let body = json!({
            "accessToken": "tok",
            "clientToken": "ct",
            "selectedProfile": {"id": "FFC8FDC9-5824-509E-8A57-C99B940FB996", "name": "ErickSkrauch"},
            "availableProfiles": [],
            "user": {"id": "x"}
        });
        let s = parse_session(&body, "fallback").unwrap();
        assert_eq!(s.uuid, "ffc8fdc95824509e8a57c99b940fb996");
        assert_eq!(s.name, "ErickSkrauch");
        assert_eq!(s.access_token, "tok");
        assert_eq!(s.client_token, "ct");

        let no_ct = json!({"accessToken":"t","selectedProfile":{"id":"ffc8fdc95824509e8a57c99b940fb996","name":"A"}});
        assert_eq!(parse_session(&no_ct, "fb").unwrap().client_token, "fb");

        let via_available =
            json!({"accessToken":"t","availableProfiles":[{"id":"ffc8fdc95824509e8a57c99b940fb996","name":"B"}]});
        assert_eq!(parse_session(&via_available, "fb").unwrap().name, "B");

        assert!(matches!(parse_session(&json!({"accessToken":"t"}), "fb"), Err(AuthError::InvalidCredentials(_))));
        assert!(parse_session(&json!({"selectedProfile":{"id":"ffc8fdc95824509e8a57c99b940fb996","name":"A"}}), "fb")
            .is_err());
        assert!(parse_session(&json!({"accessToken":"t","selectedProfile":{"id":"nothex","name":"A"}}), "fb").is_err());
    }

    #[test]
    fn session_serde_is_camel_case() {
        let s = ElySession { uuid: "a".into(), name: "b".into(), access_token: "c".into(), client_token: "d".into() };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["accessToken"], "c");
        assert_eq!(v["clientToken"], "d");
        assert_eq!(serde_json::from_value::<ElySession>(v).unwrap(), s);
    }

    #[test]
    fn password_with_totp() {
        assert_eq!(password_field("pw", None), "pw");
        assert_eq!(password_field("pw", Some("")), "pw");
        assert_eq!(password_field("pw", Some(" 123 456 ")), "pw:123456");
    }

    #[test]
    fn uuid_dashing() {
        assert_eq!(dashed_uuid("ffc8fdc95824509e8a57c99b940fb996"), "ffc8fdc9-5824-509e-8a57-c99b940fb996");
        assert_eq!(dashed_uuid("nope"), "nope");
        assert_eq!(dashed_uuid("ffc8fdc9-5824-509e-8a57-c99b940fb996"), "ffc8fdc9-5824-509e-8a57-c99b940fb996");
        assert_eq!(dashed_uuid("zzc8fdc95824509e8a57c99b940fb996"), "zzc8fdc95824509e8a57c99b940fb996");
    }

    #[test]
    fn name_validation() {
        assert!(valid_name("ErickSkrauch"));
        assert!(valid_name("a_b_1"));
        assert!(valid_name("abcdefghijklmnop"));
        assert!(!valid_name(""));
        assert!(!valid_name("abcdefghijklmnopq"));
        assert!(!valid_name("bad name"));
        assert!(!valid_name("../etc"));
        assert!(!valid_name("näme"));
        assert!(fetch_textures("bad/name").is_err());
    }

    #[test]
    fn texture_parsing() {
        assert_eq!(parse_textures(204, None).unwrap(), Textures::default());
        let body = json!({"SKIN":{"url":"http://ely.by/storage/skins/x.png","metadata":{"model":"slim"}},"CAPE":{"url":"http://ely.by/c.png"}});
        let t = parse_textures(200, Some(&body)).unwrap();
        assert_eq!(t.skin_url.as_deref(), Some("http://ely.by/storage/skins/x.png"));
        assert!(t.slim);
        assert_eq!(t.cape_url.as_deref(), Some("http://ely.by/c.png"));
        let classic = parse_textures(200, Some(&json!({"SKIN":{"url":"u"}}))).unwrap();
        assert!(!classic.slim);
        assert!(classic.cape_url.is_none());
        assert!(parse_textures(500, None).is_err());
        let v = serde_json::to_value(&t).unwrap();
        assert!(v.get("skinUrl").is_some() && v.get("capeUrl").is_some());
    }

    #[test]
    fn texture_url_allow_list() {
        assert_eq!(safe_texture_url("http://ely.by/storage/skins/a.png").unwrap(), "https://ely.by/storage/skins/a.png");
        assert_eq!(safe_texture_url("https://cdn.ely.by/a.png").unwrap(), "https://cdn.ely.by/a.png");
        assert_eq!(safe_texture_url("http://ELY.BY/a.png?x=1").unwrap(), "https://ely.by/a.png?x=1");
        for bad in [
            "https://evil.com/a.png",
            "https://notely.by/a.png",
            "https://ely.by.evil.com/a.png",
            "https://ely.by@evil.com/a.png",
            "https://ely.by:8080/a.png",
            "ftp://ely.by/a.png",
            "file:///etc/passwd",
            "https://.ely.by/a",
            "//ely.by/a.png",
        ] {
            assert!(safe_texture_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn png_check() {
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(b"rest");
        assert!(png_data_uri(&png).unwrap().starts_with("data:image/png;base64,iVBORw0KGg"));
        assert!(png_data_uri(b"<html>").is_err());
    }

    #[test]
    fn client_tokens() {
        let a = new_client_token();
        let b = new_client_token();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(a, b);
        let set: std::collections::HashSet<_> = (0..1000).map(|_| new_client_token()).collect();
        assert_eq!(set.len(), 1000);
    }

    #[test]
    fn javaagent() {
        assert_eq!(
            javaagent_arg(Path::new("/tmp/x/authlib-injector-1.2.8.jar")),
            "-javaagent:/tmp/x/authlib-injector-1.2.8.jar=ely.by"
        );
    }

    #[test]
    fn jar_with_wrong_hash_is_rejected() {
        let dir = std::env::temp_dir().join(format!("lumos-elyby-bundled-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("fake.jar");
        fs::write(&fake, b"not a jar").unwrap();
        assert!(!jar_is_valid(&fake));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore]
    fn live_fetch_textures() {
        let t = fetch_textures("ErickSkrauch").unwrap();
        let url = t.skin_url.expect("skin url");
        assert!(url.contains("ely.by"));
        let uri = fetch_texture_data_uri(&url).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
        assert_eq!(fetch_textures("Notch").unwrap().skin_url, None);
    }

    #[test]
    #[ignore]
    fn live_authlib_download() {
        let dir = std::env::temp_dir().join(format!("lumos-elyby-authlib-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let jar = ensure_authlib_injector(&dir, None).unwrap();
        assert!(jar_is_valid(&jar));
        assert_eq!(ensure_authlib_injector(&dir, None).unwrap(), jar);
        let dir2 = dir.join("copy");
        let jar2 = ensure_authlib_injector(&dir2, Some(&jar)).unwrap();
        assert!(jar_is_valid(&jar2));
        let _ = fs::remove_dir_all(&dir);
    }
}
