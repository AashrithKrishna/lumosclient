//! Self-update from the GitHub releases of LumosClient.
//!
//! `check` asks GitHub for the latest release and compares its tag with this build's version.
//! `install` downloads this platform's asset, verifies it against the SHA-256 the release
//! publishes (a `SHA256SUMS` asset, or `<asset>.sha256`), puts the new app in place and
//! relaunches. A release without checksums is never installed automatically: the player gets
//! the release page instead. Player data lives in ~/LumosClient (%USERPROFILE%\LumosClient),
//! never inside the app, so replacing the app keeps it.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::net;

pub const LATEST_API: &str = "https://api.github.com/repos/AashrithKrishna/lumosclient/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/AashrithKrishna/lumosclient/releases/latest";
pub const SUMS_NAME: &str = "SHA256SUMS";
const MAX_NOTES: usize = 4000;

// ───────────────────────── Versions ─────────────────────────

/// A semantic version (`1.2.3`, `v1.2.3-beta.1`); build metadata is ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.strip_prefix('v').or_else(|| s.strip_prefix('V')).unwrap_or(s);
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, p.split('.').map(str::to_owned).collect()),
            None => (s, Vec::new()),
        };
        let mut nums = core.split('.');
        let num = |p: Option<&str>| -> Option<u64> {
            let p = p?;
            if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            p.parse().ok()
        };
        let major = num(nums.next())?;
        // "1.2" counts as "1.2.0".
        let minor = match nums.next() {
            Some(p) => num(Some(p))?,
            None => 0,
        };
        let patch = match nums.next() {
            Some(p) => num(Some(p))?,
            None => 0,
        };
        if nums.next().is_some() {
            return None;
        }
        Some(Version { major, minor, patch, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| {
            // A pre-release comes before its release; identifiers compare numerically when both are numbers.
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&other.pre) {
                        let ord = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            _ => a.cmp(b),
                        };
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// True when `candidate` (a tag such as `v0.4.0`) is newer than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (Version::parse(candidate), Version::parse(current)) {
        (Some(c), Some(cur)) => c > cur,
        _ => false,
    }
}

// ───────────────────────── Releases ─────────────────────────

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub tag: String,
    pub version: String,
    pub name: String,
    pub notes: String,
    pub page_url: String,
    pub assets: Vec<Asset>,
}

/// Reads GitHub's release JSON; drafts, pre-releases and unparseable tags give `None`.
pub fn parse_release(v: &Value) -> Option<Release> {
    if v.get("draft").and_then(Value::as_bool) == Some(true) || v.get("prerelease").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let tag = v.get("tag_name")?.as_str()?.to_owned();
    let version = Version::parse(&tag)?;
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let mut notes = text("body").replace("\r\n", "\n");
    if notes.len() > MAX_NOTES {
        let mut cut = MAX_NOTES;
        while !notes.is_char_boundary(cut) {
            cut -= 1;
        }
        notes.truncate(cut);
        notes.push('…');
    }
    let page_url = Some(text("html_url"))
        .filter(|u| u.starts_with("https://github.com/"))
        .unwrap_or_else(|| RELEASES_PAGE.to_owned());
    let assets = v
        .get("assets")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let url = x.get("browser_download_url")?.as_str()?;
                    url.starts_with("https://").then(|| Asset {
                        name: x.get("name").and_then(Value::as_str).unwrap_or("").to_owned(),
                        url: url.to_owned(),
                        size: x.get("size").and_then(Value::as_u64).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Release {
        version: format!("{}.{}.{}{}", version.major, version.minor, version.patch, if version.pre.is_empty() { String::new() } else { format!("-{}", version.pre.join(".")) }),
        name: Some(text("name")).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| tag.clone()),
        tag,
        notes,
        page_url,
        assets,
    })
}

/// The release asset this computer installs from.
pub fn platform_asset_name() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("Lumos.dmg")
    } else if cfg!(windows) {
        Some("Lumos-Windows-x64.zip")
    } else {
        None
    }
}

pub fn select_asset<'a>(assets: &'a [Asset], wanted: &str) -> Option<&'a Asset> {
    assets.iter().find(|a| a.name == wanted).or_else(|| assets.iter().find(|a| a.name.eq_ignore_ascii_case(wanted)))
}

// ───────────────────────── Checksums ─────────────────────────

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Parses `sha256sum`/`shasum -a 256` output (`<hex>  <name>` or `<hex> *<name>`) into
/// name → lowercase hex. Paths keep only their file name. A lone hash gets the name "".
pub fn parse_sums(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (hash, rest) = match line.split_once(char::is_whitespace) {
            Some((h, r)) => (h, r.trim()),
            None => (line, ""),
        };
        if !is_sha256_hex(hash) {
            continue;
        }
        let name = rest.trim_start_matches('*');
        let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        out.insert(name.to_owned(), hash.to_ascii_lowercase());
    }
    out
}

/// The expected SHA-256 of `asset_name` in a sums file's text.
pub fn sum_for(text: &str, asset_name: &str) -> Option<String> {
    let sums = parse_sums(text);
    sums.get(asset_name).cloned().or_else(|| (sums.len() == 1).then(|| sums.get("").cloned()).flatten())
}

/// Finds the published checksum for `asset` in the release (SHA256SUMS first, then `<asset>.sha256`).
pub fn published_sha256(release: &Release, asset: &Asset) -> Result<Option<String>, String> {
    let per_file = format!("{}.sha256", asset.name);
    for name in [SUMS_NAME, per_file.as_str()] {
        if let Some(a) = select_asset(&release.assets, name) {
            let bytes = net::get_bytes(&a.url, 256 * 1024)?;
            if let Some(h) = sum_for(&String::from_utf8_lossy(&bytes), &asset.name) {
                return Ok(Some(h));
            }
        }
    }
    Ok(None)
}

pub fn sha256_file(path: &Path) -> Option<String> {
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ───────────────────────── Check ─────────────────────────

static LATEST: Mutex<Option<Release>> = Mutex::new(None);

/// Asks GitHub for the latest release; `Some` when it is newer than this build.
pub fn check(current: &str) -> Result<Option<Release>, String> {
    let resp = net::agent()
        .get(LATEST_API)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| net::describe_error("GitHub", &e))?;
    let v: Value = resp.into_json().map_err(|e| format!("GitHub sent data the launcher couldn't read: {e}"))?;
    let release = parse_release(&v).filter(|r| is_newer(&r.tag, current));
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = release.clone();
    Ok(release)
}

/// The newer release the last `check` found.
pub fn last_found() -> Option<Release> {
    LATEST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

// ───────────────────────── Install ─────────────────────────

/// Why an update didn't install.
#[derive(Debug)]
pub enum UpdateError {
    /// The player should download it from the release page (no checksum, unsupported setup…).
    Manual(String),
    /// Something failed along the way (network, disk); trying again may help.
    Failed(String),
}

impl UpdateError {
    pub fn message(&self) -> &str {
        match self {
            UpdateError::Manual(m) | UpdateError::Failed(m) => m,
        }
    }
}

/// What to start once this process exits.
#[derive(Debug)]
pub struct Relaunch {
    #[allow(dead_code)]
    program: PathBuf,
}

fn work_dir(version: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lumos-update-{}", version.replace(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-', "_")))
}

/// Downloads `url` to `dest`, checking size and SHA-256. Reports 0..=1 progress.
fn download_sha256(url: &str, dest: &Path, sha256: &str, size: u64, progress: &dyn Fn(f64)) -> Result<(), String> {
    if sha256_file(dest).is_some_and(|h| h.eq_ignore_ascii_case(sha256)) {
        return Ok(());
    }
    let resp = net::agent().get(url).call().map_err(|e| net::describe_error("GitHub", &e))?;
    let total = if size > 0 { size } else { resp.header("Content-Length").and_then(|l| l.parse().ok()).unwrap_or(0) };
    let tmp = dest.with_extension("part");
    let mut out = fs::File::create(&tmp).map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
    let mut reader = resp.into_reader();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|_| "The connection dropped while downloading the update. Try again.".to_owned())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
        done += n as u64;
        if total > 0 {
            progress((done as f64 / total as f64).min(1.0));
        }
    }
    out.flush().map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
    drop(out);
    let got = hex(&hasher.finalize());
    if size > 0 && done != size {
        let _ = fs::remove_file(&tmp);
        return Err(format!("The update downloaded with the wrong size ({done} of {size} bytes). Try again."));
    }
    if !got.eq_ignore_ascii_case(sha256) {
        let _ = fs::remove_file(&tmp);
        return Err("The downloaded update didn't match its published checksum, so it wasn't installed.".into());
    }
    fs::rename(&tmp, dest).map_err(|e| format!("Couldn't save {}: {e}", dest.display()))
}

/// Downloads, verifies and puts the release in place. On success, call `relaunch_and_exit`.
pub fn install(release: &Release, report: &dyn Fn(&str, f64)) -> Result<Relaunch, UpdateError> {
    let wanted = platform_asset_name().ok_or_else(|| UpdateError::Manual("Updates install automatically on macOS and Windows only.".into()))?;
    let asset = select_asset(&release.assets, wanted)
        .ok_or_else(|| UpdateError::Manual(format!("The {} release has no {wanted} to install.", release.tag)))?;
    // Checked first, so an unsupported setup never downloads anything.
    let target = install_target().map_err(UpdateError::Manual)?;
    report("Checking the update", -1.0);
    let sha = published_sha256(release, asset)
        .map_err(UpdateError::Failed)?
        .ok_or_else(|| UpdateError::Manual(format!("The {} release has no published checksum, so it can't be installed automatically.", release.tag)))?;
    let work = work_dir(&release.version);
    fs::create_dir_all(&work).map_err(|e| UpdateError::Failed(format!("Couldn't create {}: {e}", work.display())))?;
    let pkg = work.join(&asset.name);
    let label = format!("Downloading LumosClient {}", release.version);
    download_sha256(&asset.url, &pkg, &sha, asset.size, &|f| report(&label, f)).map_err(UpdateError::Failed)?;
    report("Installing the update", -1.0);
    let program = put_in_place(&target, &pkg, &work, &release.version)?;
    // Windows' helper script removes the folder itself once it has copied the files.
    if cfg!(target_os = "macos") {
        let _ = fs::remove_dir_all(&work);
    }
    Ok(Relaunch { program })
}

/// Where the running app lives: the `.app` bundle (macOS) or the folder with Lumos.exe (Windows).
#[derive(Debug)]
struct Target {
    #[allow(dead_code)]
    path: PathBuf,
}

/// The `.app` bundle that contains `exe`, if any.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app") && p.join("Contents").is_dir()).map(Path::to_path_buf)
}

fn install_target() -> Result<Target, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't find LumosClient's own program: {e}"))?;
    let exe = exe.canonicalize().unwrap_or(exe);
    if cfg!(target_os = "macos") {
        let bundle = bundle_of(&exe).ok_or("This copy of LumosClient isn't an installed app (a development build?), so it can't update itself.")?;
        let s = bundle.to_string_lossy();
        if s.contains("/AppTranslocation/") || s.starts_with("/Volumes/") {
            return Err("Move LumosClient into your Applications folder first, then it can update itself.".into());
        }
        Ok(Target { path: bundle })
    } else if cfg!(windows) {
        let dir = exe.parent().ok_or("Couldn't find LumosClient's folder.")?.to_path_buf();
        if !dir.join("resources").is_dir() {
            return Err("This copy of LumosClient isn't a full install (no resources folder beside it), so it can't update itself.".into());
        }
        if script_unsafe(&dir.to_string_lossy()) {
            return Err("LumosClient's folder name has characters the updater can't handle.".into());
        }
        Ok(Target { path: dir })
    } else {
        Err("Updates install automatically on macOS and Windows only.".into())
    }
}

// ───────────────────────── macOS ─────────────────────────

/// Replaces `current` with `staged` by renaming, keeping the old copy until the new one is
/// in place (and putting it back if that fails).
pub fn swap_into_place(current: &Path, staged: &Path) -> Result<(), String> {
    let name = current.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Lumos.app".into());
    let old = current.with_file_name(format!(".{name}.old-{}", std::process::id()));
    let _ = fs::remove_dir_all(&old);
    fs::rename(current, &old).map_err(|e| format!("Couldn't move the current LumosClient aside ({e}). Download the update from the release page instead."))?;
    if let Err(e) = fs::rename(staged, current) {
        let _ = fs::rename(&old, current);
        return Err(format!("Couldn't put the new LumosClient in place ({e})."));
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

/// The app inside a mounted disk image.
pub fn find_app_in(dir: &Path) -> Option<PathBuf> {
    let mut apps: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "app") && p.join("Contents").is_dir())
        .collect();
    apps.sort();
    apps.iter().find(|p| p.file_name().is_some_and(|n| n == "Lumos.app")).cloned().or_else(|| apps.into_iter().next())
}

#[cfg(target_os = "macos")]
fn put_in_place(target: &Target, pkg: &Path, work: &Path, version: &str) -> Result<PathBuf, UpdateError> {
    use std::process::{Command, Stdio};
    let bundle = &target.path;
    let parent = bundle.parent().ok_or_else(|| UpdateError::Manual("Couldn't find the folder LumosClient is in.".into()))?;
    let mnt = work.join("mnt");
    let _ = Command::new("/usr/bin/hdiutil").arg("detach").arg(&mnt).arg("-force").stdout(Stdio::null()).stderr(Stdio::null()).status();
    fs::create_dir_all(&mnt).map_err(|e| UpdateError::Failed(format!("Couldn't create {}: {e}", mnt.display())))?;
    let attached = Command::new("/usr/bin/hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint"])
        .arg(&mnt)
        .arg(pkg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !attached {
        return Err(UpdateError::Failed("Couldn't open the downloaded update (disk image).".into()));
    }
    let result = (|| {
        let src = find_app_in(&mnt).ok_or_else(|| UpdateError::Failed("The update's disk image has no app inside.".into()))?;
        let name = bundle.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Lumos.app".into());
        let staged = parent.join(format!(".{name}.new-{}", std::process::id()));
        let _ = fs::remove_dir_all(&staged);
        let copied = Command::new("/usr/bin/ditto").arg(&src).arg(&staged).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if !copied {
            let _ = fs::remove_dir_all(&staged);
            return Err(UpdateError::Manual(format!(
                "LumosClient can't write to {}. Download the update from the release page instead.",
                parent.display()
            )));
        }
        let plist = fs::read_to_string(staged.join("Contents/Info.plist")).unwrap_or_default();
        let exe_ok = fs::read_dir(staged.join("Contents/MacOS")).is_ok_and(|mut d| d.next().is_some());
        if !exe_ok || !plist.contains(&format!("<string>{version}</string>")) {
            let _ = fs::remove_dir_all(&staged);
            return Err(UpdateError::Failed(format!("The downloaded app isn't LumosClient {version}; nothing was changed.")));
        }
        swap_into_place(bundle, &staged).map_err(|e| {
            let _ = fs::remove_dir_all(&staged);
            UpdateError::Manual(e)
        })?;
        Ok(bundle.clone())
    })();
    let detached = Command::new("/usr/bin/hdiutil").arg("detach").arg(&mnt).arg("-quiet").status().is_ok_and(|s| s.success());
    if !detached {
        let _ = Command::new("/usr/bin/hdiutil").arg("detach").arg(&mnt).arg("-force").stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    result
}

// ───────────────────────── Windows ─────────────────────────

/// Characters a path can't contain inside the helper script's `set "X=…"` lines.
fn script_unsafe(path: &str) -> bool {
    path.contains(['%', '"', '\n', '\r'])
}

/// Unpacks a zip into `dest`, refusing entries that would land outside it.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("Couldn't open {}: {e}", zip_path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("The update is damaged: {e}"))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| format!("The update is damaged: {e}"))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err("The update contains an unsafe file path.".into());
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("Couldn't create {}: {e}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
        }
        let mut f = fs::File::create(&out).map_err(|e| format!("Couldn't write {}: {e}", out.display()))?;
        std::io::copy(&mut entry, &mut f).map_err(|e| format!("Couldn't unpack {}: {e}", out.display()))?;
    }
    Ok(())
}

/// The folder in an unpacked Windows release that holds `exe_name` (`Lumos/` in the zip).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn windows_app_root(extracted: &Path, exe_name: &str) -> Option<PathBuf> {
    [extracted.join("Lumos"), extracted.to_path_buf()]
        .into_iter()
        .find(|d| d.join(exe_name).is_file() && d.join("resources").is_dir())
}

/// The batch script that waits for LumosClient (`pid`) to exit, backs the install folder up,
/// mirrors the new `resources` folder and copies the top-level files over, restores the backup
/// if copying fails, starts LumosClient again and cleans up after itself.
/// (`timeout` needs a console, so waiting uses `ping`.)
#[cfg_attr(not(windows), allow(dead_code))]
pub fn windows_helper_script(pid: u32, src: &Path, dst: &Path, backup: &Path, work: &Path, exe_name: &str) -> String {
    let q = "/R:10 /W:1 /NFL /NDL /NJH /NJS /NP >nul";
    format!(
        "@echo off\r\n\
         setlocal EnableExtensions\r\n\
         set \"PID={pid}\"\r\n\
         set \"SRC={src}\"\r\n\
         set \"DST={dst}\"\r\n\
         set \"BAK={bak}\"\r\n\
         set \"WORK={work}\"\r\n\
         set \"EXE={exe_name}\"\r\n\
         set /a TRIES=0\r\n\
         :wait\r\n\
         tasklist /FI \"PID eq %PID%\" /NH 2>nul | find \" %PID% \" >nul\r\n\
         if errorlevel 1 goto gone\r\n\
         set /a TRIES+=1\r\n\
         if %TRIES% GEQ 120 goto launch\r\n\
         ping -n 2 127.0.0.1 >nul\r\n\
         goto wait\r\n\
         :gone\r\n\
         robocopy \"%DST%\" \"%BAK%\" /E {q}\r\n\
         if errorlevel 8 goto launch\r\n\
         robocopy \"%SRC%\\resources\" \"%DST%\\resources\" /MIR {q}\r\n\
         if errorlevel 8 goto restore\r\n\
         robocopy \"%SRC%\" \"%DST%\" {q}\r\n\
         if errorlevel 8 goto restore\r\n\
         goto launch\r\n\
         :restore\r\n\
         robocopy \"%BAK%\\resources\" \"%DST%\\resources\" /MIR {q}\r\n\
         robocopy \"%BAK%\" \"%DST%\" {q}\r\n\
         :launch\r\n\
         start \"\" \"%DST%\\%EXE%\"\r\n\
         rd /s /q \"%WORK%\" >nul 2>&1\r\n\
         (goto) 2>nul & del \"%~f0\"\r\n",
        src = src.display(),
        dst = dst.display(),
        bak = backup.display(),
        work = work.display(),
    )
}

#[cfg(windows)]
fn put_in_place(target: &Target, pkg: &Path, work: &Path, _version: &str) -> Result<PathBuf, UpdateError> {
    let exe = std::env::current_exe().map_err(|e| UpdateError::Failed(e.to_string()))?;
    let exe_name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Lumos.exe".into());
    let extracted = work.join("new");
    let _ = fs::remove_dir_all(&extracted);
    extract_zip(pkg, &extracted).map_err(UpdateError::Failed)?;
    let src = windows_app_root(&extracted, "Lumos.exe").ok_or_else(|| UpdateError::Failed("The update has no Lumos.exe inside.".into()))?;
    // The new build keeps the running program's file name, so shortcuts keep working.
    if exe_name != "Lumos.exe" {
        let _ = fs::rename(src.join("Lumos.exe"), src.join(&exe_name));
    }
    let backup = work.join("backup");
    let _ = fs::remove_dir_all(&backup);
    for p in [&src, &backup, &work.to_path_buf()] {
        if script_unsafe(&p.to_string_lossy()) {
            return Err(UpdateError::Manual("Your temporary folder's name has characters the updater can't handle.".into()));
        }
    }
    let script = work.with_file_name(format!("lumos-update-{}.cmd", std::process::id()));
    let text = windows_helper_script(std::process::id(), &src, &target.path, &backup, work, &exe_name);
    fs::write(&script, text).map_err(|e| UpdateError::Failed(format!("Couldn't write {}: {e}", script.display())))?;
    Ok(script)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn put_in_place(_target: &Target, _pkg: &Path, _work: &Path, _version: &str) -> Result<PathBuf, UpdateError> {
    Err(UpdateError::Manual("Updates install automatically on macOS and Windows only.".into()))
}

/// Starts the new LumosClient once this one has exited, then exits.
pub fn relaunch_and_exit(r: Relaunch) -> ! {
    use std::process::{Command, Stdio};
    let pid = std::process::id().to_string();
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("/bin/sh")
            .arg("-c")
            .arg("while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; /usr/bin/open \"$2\"")
            .arg("sh")
            .arg(&pid)
            .arg(&r.program)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        crate::installer::no_console(&mut cmd);
        let _ = cmd.arg("/C").arg(&r.program).arg(&pid).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (&r, &pid, Command::new("true"), Stdio::null());
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn version_compare() {
        let cases = [
            ("v0.3.1", "0.3.0", true),
            ("v0.3.0", "0.3.0", false),
            ("0.2.9", "0.3.0", false),
            ("v1.0.0", "0.99.99", true),
            ("v0.10.0", "0.9.0", true),
            ("v0.4", "0.3.9", true),
            ("v0.4.0-beta.1", "0.3.0", true),
            ("v0.4.0-beta.1", "0.4.0", false),
            ("v0.4.0", "0.4.0-beta.2", true),
            ("v0.4.0-beta.10", "0.4.0-beta.9", true),
            ("v0.4.0-rc.1", "0.4.0-beta.9", true),
            ("v0.4.0+build.5", "0.4.0", false),
            ("nightly", "0.3.0", false),
            ("v1.2.3.4", "0.3.0", false),
            ("", "0.3.0", false),
        ];
        for (cand, cur, want) in cases {
            assert_eq!(is_newer(cand, cur), want, "{cand} vs {cur}");
        }
        assert_eq!(Version::parse("V2.0.1").map(|v| (v.major, v.minor, v.patch)), Some((2, 0, 1)));
    }

    fn sample_release() -> Value {
        json!({
            "tag_name": "v0.4.0", "name": "Lumos 0.4.0", "body": "Faster\r\nBetter", "draft": false, "prerelease": false,
            "html_url": "https://github.com/AashrithKrishna/lumosclient/releases/tag/v0.4.0",
            "assets": [
                {"name": "Lumos-Windows-x64.zip", "browser_download_url": "https://github.com/x/Lumos-Windows-x64.zip", "size": 10},
                {"name": "Lumos.dmg", "browser_download_url": "https://github.com/x/Lumos.dmg", "size": 20},
                {"name": "SHA256SUMS", "browser_download_url": "https://github.com/x/SHA256SUMS", "size": 1},
                {"name": "evil", "browser_download_url": "http://insecure/x", "size": 1}
            ]
        })
    }

    #[test]
    fn release_parsing_and_asset_selection() {
        let r = parse_release(&sample_release()).unwrap();
        assert_eq!((r.tag.as_str(), r.version.as_str(), r.name.as_str(), r.notes.as_str()), ("v0.4.0", "0.4.0", "Lumos 0.4.0", "Faster\nBetter"));
        assert_eq!(r.assets.len(), 3, "non-https assets are dropped");
        assert_eq!(select_asset(&r.assets, "Lumos.dmg").map(|a| a.size), Some(20));
        assert_eq!(select_asset(&r.assets, "lumos.DMG").map(|a| a.size), Some(20));
        assert_eq!(select_asset(&r.assets, "Lumos-Windows-x64.zip").map(|a| a.size), Some(10));
        assert!(select_asset(&r.assets, "Lumos-Linux.tar.gz").is_none());
        let mut draft = sample_release();
        draft["draft"] = json!(true);
        assert!(parse_release(&draft).is_none());
        let mut pre = sample_release();
        pre["prerelease"] = json!(true);
        assert!(parse_release(&pre).is_none());
        let mut odd = sample_release();
        odd["html_url"] = json!("https://evil.example/");
        assert_eq!(parse_release(&odd).unwrap().page_url, RELEASES_PAGE);
        if cfg!(target_os = "macos") {
            assert_eq!(platform_asset_name(), Some("Lumos.dmg"));
        }
    }

    #[test]
    fn checksum_parsing() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let text = format!("# sums\n{a}  Lumos.dmg\n{b} *dist/Lumos-Windows-x64.zip\nnot-a-hash  other\n{}  short\n", "c".repeat(10));
        let sums = parse_sums(&text);
        assert_eq!(sums.len(), 2);
        assert_eq!(sums.get("Lumos.dmg"), Some(&a));
        assert_eq!(sums.get("Lumos-Windows-x64.zip"), Some(&"b".repeat(64)));
        assert_eq!(sum_for(&text, "Lumos.dmg"), Some(a.clone()));
        assert_eq!(sum_for(&text, "Missing.zip"), None);
        // A per-file .sha256 with only the hash.
        assert_eq!(sum_for(&format!("{a}\n"), "Lumos.dmg"), Some(a.clone()));
        // shasum on Windows paths.
        assert_eq!(sum_for(&format!("{a}  C:\\dist\\Lumos.dmg\r\n"), "Lumos.dmg"), Some(a));
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lumos-updater-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sha256_of_file() {
        let d = temp("sha");
        fs::write(d.join("x"), b"hello").unwrap();
        assert_eq!(sha256_file(&d.join("x")).unwrap(), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn swap_keeps_old_until_new_is_in_place() {
        let d = temp("swap");
        let cur = d.join("Lumos.app");
        let new = d.join(".Lumos.app.new");
        fs::create_dir_all(cur.join("Contents")).unwrap();
        fs::write(cur.join("Contents/v"), "old").unwrap();
        fs::create_dir_all(new.join("Contents")).unwrap();
        fs::write(new.join("Contents/v"), "new").unwrap();
        swap_into_place(&cur, &new).unwrap();
        assert_eq!(fs::read_to_string(cur.join("Contents/v")).unwrap(), "new");
        assert!(!new.exists());
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1, "the old copy is removed");
        // A missing staged copy leaves the current app untouched.
        assert!(swap_into_place(&cur, &d.join("nope")).is_err());
        assert_eq!(fs::read_to_string(cur.join("Contents/v")).unwrap(), "new");
        assert_eq!(find_app_in(&d), Some(cur.clone()));
        assert_eq!(bundle_of(&cur.join("Contents/MacOS/lumos-launcher")), Some(cur));
        let _ = fs::remove_dir_all(&d);
    }

    /// Builds a disk image with a fake Lumos.app and installs it over a fake installed app.
    /// Uses hdiutil, so it's run by hand: `cargo test --release -- --ignored dmg_install`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn dmg_install_replaces_the_bundle() {
        use std::process::Command;
        let d = temp("dmg");
        let fake_app = |dir: &Path, version: &str, marker: &str| {
            fs::create_dir_all(dir.join("Contents/MacOS")).unwrap();
            fs::write(dir.join("Contents/MacOS/lumos-launcher"), marker).unwrap();
            fs::write(dir.join("Contents/Info.plist"), format!("<plist><dict><key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>")).unwrap();
        };
        fake_app(&d.join("src/Lumos.app"), "9.9.9", "new");
        let installed = d.join("Applications/Lumos.app");
        fake_app(&installed, "0.3.0", "old");
        let dmg = d.join("Lumos.dmg");
        assert!(Command::new("/usr/bin/hdiutil")
            .args(["create", "-quiet", "-volname", "LumosClient", "-srcfolder"])
            .arg(d.join("src/Lumos.app"))
            .args(["-ov", "-format", "UDZO"])
            .arg(&dmg)
            .status()
            .unwrap()
            .success());
        let target = Target { path: installed.clone() };
        // A disk image with another version is refused and changes nothing.
        let err = put_in_place(&target, &dmg, &d.join("work1"), "1.0.0").unwrap_err();
        assert!(matches!(err, UpdateError::Failed(_)), "{err:?}");
        assert_eq!(fs::read_to_string(installed.join("Contents/MacOS/lumos-launcher")).unwrap(), "old");
        let out = put_in_place(&target, &dmg, &d.join("work2"), "9.9.9").unwrap();
        assert_eq!(out, installed);
        assert_eq!(fs::read_to_string(installed.join("Contents/MacOS/lumos-launcher")).unwrap(), "new");
        let leftovers: Vec<_> = fs::read_dir(d.join("Applications")).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(leftovers.len(), 1, "{leftovers:?}");
        assert!(!d.join("work2/mnt").read_dir().is_ok_and(|mut r| r.next().is_some()), "the image is detached");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn zip_extraction_and_windows_script() {
        let d = temp("zip");
        let zp = d.join("u.zip");
        {
            let mut w = zip::ZipWriter::new(fs::File::create(&zp).unwrap());
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            w.start_file("Lumos/Lumos.exe", o).unwrap();
            w.write_all(b"exe").unwrap();
            w.start_file("Lumos/resources/bin/horizon-client.exe", o).unwrap();
            w.write_all(b"hc").unwrap();
            w.finish().unwrap();
        }
        let out = d.join("new");
        extract_zip(&zp, &out).unwrap();
        assert_eq!(fs::read_to_string(out.join("Lumos/resources/bin/horizon-client.exe")).unwrap(), "hc");
        assert_eq!(windows_app_root(&out, "Lumos.exe"), Some(out.join("Lumos")));

        let bad = d.join("bad.zip");
        {
            let mut w = zip::ZipWriter::new(fs::File::create(&bad).unwrap());
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            w.start_file("../escape.txt", o).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        assert!(extract_zip(&bad, &d.join("bad")).is_err());
        assert!(!d.join("escape.txt").exists());

        let s = windows_helper_script(42, Path::new(r"C:\T\new\Lumos"), Path::new(r"C:\Games\Lumos"), Path::new(r"C:\T\backup"), Path::new(r"C:\T"), "Lumos.exe");
        assert!(s.contains("set \"PID=42\""));
        assert!(s.contains(r#"set "DST=C:\Games\Lumos""#));
        assert!(s.contains("/MIR"));
        assert!(!s.contains("timeout"), "timeout needs a console");
        assert!(s.contains("start \"\" \"%DST%\\%EXE%\""));
        assert!(!s.contains("LumosClient"), "the user data folder is never touched");
        assert!(script_unsafe("C:\\100%\\x") && !script_unsafe(r"C:\Users\A B\Lumos"));
        let _ = fs::remove_dir_all(&d);
    }
}
