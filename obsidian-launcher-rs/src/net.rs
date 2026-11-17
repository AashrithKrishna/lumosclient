//! Shared HTTP client: one agent for the whole launcher, trusting the operating
//! system's certificate store (so managed networks whose inspection certificate
//! is installed and trusted work like a browser), with sha1-verified downloads
//! and error messages a player can act on.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

pub const USER_AGENT: &str = concat!("LumosClient/", env!("CARGO_PKG_VERSION"), " (+https://github.com/AashrithKrishna/lumosclient)");

/// The launcher-wide HTTP agent (connection pooling, timeouts, native roots).
pub fn agent() -> ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            ureq::AgentBuilder::new()
                .user_agent(USER_AGENT)
                .timeout_connect(Duration::from_secs(15))
                .timeout_read(Duration::from_secs(60))
                .max_idle_connections_per_host(8)
                .build()
        })
        .clone()
}

/// Turns a transport/status error into a sentence for the UI.
pub fn describe_error(host_hint: &str, err: &ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, _) => match *code {
            404 => format!("{host_hint} doesn't have that file (404)."),
            429 => format!("{host_hint} is rate-limiting requests. Wait a minute and try again."),
            500..=599 => format!("{host_hint} is having problems right now ({code}). Try again later."),
            _ => format!("{host_hint} answered with an error ({code})."),
        },
        ureq::Error::Transport(t) => {
            let text = t.to_string();
            let lower = text.to_lowercase();
            if lower.contains("certificate") || lower.contains("unknownissuer") || lower.contains("invalid peer") || lower.contains("tls") {
                format!(
                    "Couldn't open a secure connection to {host_hint}. Your network may be blocking or inspecting it \
                     (common on school or work Wi-Fi). Try another network."
                )
            } else if lower.contains("dns") || lower.contains("resolve") || lower.contains("lookup") {
                format!("Couldn't find {host_hint}. Check your internet connection.")
            } else if lower.contains("timed out") || lower.contains("timeout") {
                format!("{host_hint} took too long to answer. Check your connection and try again.")
            } else if lower.contains("connection refused") || lower.contains("connect") {
                format!("Couldn't connect to {host_hint}. Check your internet connection.")
            } else {
                format!("Network problem reaching {host_hint}: {text}")
            }
        }
    }
}

fn host_of(url: &str) -> &str {
    url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url)
}

/// GET a URL and parse JSON.
pub fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let resp = agent().get(url).call().map_err(|e| describe_error(host_of(url), &e))?;
    resp.into_json::<serde_json::Value>()
        .map_err(|e| format!("{} sent data the launcher couldn't read: {e}", host_of(url)))
}

/// GET a URL into memory (capped, for small files such as skins).
pub fn get_bytes(url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    let resp = agent().get(url).call().map_err(|e| describe_error(host_of(url), &e))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(max_bytes + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("Download from {} was interrupted: {e}", host_of(url)))?;
    if buf.len() as u64 > max_bytes {
        return Err(format!("File from {} is larger than expected.", host_of(url)));
    }
    Ok(buf)
}

pub fn sha1_file(path: &Path) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let mut h = sha1_smol::Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.digest().to_string())
}

/// True when `path` exists and matches the expected sha1 / size (either may be absent).
pub fn file_is_valid(path: &Path, sha1: Option<&str>, size: Option<u64>) -> bool {
    let Ok(meta) = fs::metadata(path) else { return false };
    if !meta.is_file() {
        return false;
    }
    if let Some(sz) = size {
        if meta.len() != sz {
            return false;
        }
    }
    match sha1 {
        Some(expected) => sha1_file(path).is_some_and(|got| got.eq_ignore_ascii_case(expected)),
        None => meta.len() > 0,
    }
}

/// Download `url` to `dest` atomically (temp file + rename), verifying sha1/size
/// when given. Skips the download when `dest` already matches. Retries twice.
pub fn download_verified(url: &str, dest: &Path, sha1: Option<&str>, size: Option<u64>) -> Result<(), String> {
    if file_is_valid(dest, sha1, size) {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Couldn't create folder {}: {e}", parent.display()))?;
    }
    let file_name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "download".into());
    let tmp = dest.with_file_name(format!(".{file_name}.part"));
    let mut last_err = String::new();
    for attempt in 0u32..5 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(800 * (1 << (attempt - 1))));
        }
        match fetch_to(url, &tmp, sha1) {
            Ok(written) => {
                if let Some(sz) = size {
                    if written != sz {
                        last_err = format!("{file_name} downloaded with the wrong size ({written} of {sz} bytes).");
                        let _ = fs::remove_file(&tmp);
                        continue;
                    }
                }
                fs::rename(&tmp, dest).map_err(|e| format!("Couldn't save {}: {e}", dest.display()))?;
                return Ok(());
            }
            Err(e) => {
                if e.contains("integrity") {
                    let _ = fs::remove_file(&tmp);
                }
                last_err = e;
            }
        }
    }
    Err(last_err)
}

/// Downloads into `tmp`, resuming a partial file from an earlier attempt when the server
/// supports ranges. Keeps the partial file on a dropped connection so the next try resumes.
fn fetch_to(url: &str, tmp: &Path, sha1: Option<&str>) -> Result<u64, String> {
    let have = fs::metadata(tmp).map(|m| m.len()).unwrap_or(0);
    let mut req = agent().get(url);
    if have > 0 {
        req = req.set("Range", &format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| describe_error(host_of(url), &e))?;
    let resumed = have > 0 && resp.status() == 206;
    let mut hasher = sha1_smol::Sha1::new();
    let mut total = 0u64;
    let mut out = if resumed {
        // Hash what we already have, then append.
        let mut existing = fs::File::open(tmp).map_err(|e| format!("Couldn't read {}: {e}", tmp.display()))?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = existing.read(&mut buf).map_err(|e| format!("Couldn't read {}: {e}", tmp.display()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            total += n as u64;
        }
        fs::OpenOptions::new().append(true).open(tmp).map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?
    } else {
        fs::File::create(tmp).map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?
    };
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|_| {
            format!("The connection to {} dropped during a download. Check your internet and try again.", host_of(url))
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
        total += n as u64;
    }
    out.flush().map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
    if let Some(expected) = sha1 {
        let got = hasher.digest().to_string();
        if !got.eq_ignore_ascii_case(expected) {
            return Err(format!("A downloaded file from {} failed its integrity check.", host_of(url)));
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_parsing() {
        assert_eq!(host_of("https://piston-meta.mojang.com/mc/x.json"), "piston-meta.mojang.com");
        assert_eq!(host_of("nohost"), "nohost");
    }

    #[test]
    fn validity_checks_size_and_sha1() {
        let dir = std::env::temp_dir().join(format!("lumos-net-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.txt");
        fs::write(&p, b"hello").unwrap();
        // sha1("hello")
        assert!(file_is_valid(&p, Some("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"), Some(5)));
        assert!(!file_is_valid(&p, Some("0000000000000000000000000000000000000000"), Some(5)));
        assert!(!file_is_valid(&p, None, Some(4)));
        assert!(!file_is_valid(&dir.join("missing"), None, None));
        let _ = fs::remove_dir_all(&dir);
    }
}
