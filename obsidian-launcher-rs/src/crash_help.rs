//! Explains why the game closed with an error, in words a player can act on.
//!
//! `explain` matches known causes in a crash report or log (table-driven, no file access):
//! Fabric dependency problems, mixin failures, a crash inside a mod, running out of memory,
//! the wrong Java, graphics driver errors, Horizon engine panics and missing game files.
//! `diagnose` reads the instance's newest crash report, the game's last output and
//! logs/latest.log, and finds the mod jar behind a culprit so the UI can switch it off.

use serde::Serialize;
use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

// ───────────────────────── Game output tail ─────────────────────────

const TAIL_LINES: usize = 600;
static TAIL: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static USER_STOPPED: AtomicBool = AtomicBool::new(false);

/// A new game session started: forget the previous one's output.
pub fn begin_session() {
    TAIL.lock().unwrap_or_else(|e| e.into_inner()).clear();
    USER_STOPPED.store(false, Ordering::Relaxed);
}

/// Keeps the game's last lines of output (some errors never reach a log file).
pub fn remember_line(line: &str) {
    let mut t = TAIL.lock().unwrap_or_else(|e| e.into_inner());
    if t.len() >= TAIL_LINES {
        t.pop_front();
    }
    t.push_back(line.chars().take(2000).collect());
}

/// The player pressed Stop: that exit isn't a crash.
pub fn note_user_stop() {
    USER_STOPPED.store(true, Ordering::Relaxed);
}

/// Whether an exit deserves an explanation (not a normal quit, not the player's Stop).
pub fn should_explain(code: Option<i32>) -> bool {
    !USER_STOPPED.load(Ordering::Relaxed) && !matches!(code, Some(0) | Some(130) | Some(143))
}

fn tail_text() -> String {
    TAIL.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect::<Vec<_>>().join("\n")
}

// ───────────────────────── Diagnosis ─────────────────────────

/// What points at the mod behind a crash.
#[derive(Clone, Debug, PartialEq)]
pub enum Hint {
    ModId(String),
    MixinConfig(String),
    Class(String),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
    /// The mod (or part) most likely responsible, as the player knows it.
    pub culprit: Option<String>,
    #[serde(skip)]
    pub hint: Option<Hint>,
    /// The culprit's file in the instance's mods folder (what "Switch off" renames).
    pub mod_file: Option<String>,
    /// A missing mod to look for in Discover.
    pub needs: Option<String>,
    /// Buttons: disable_mod, find_mod, open_logs, repair, raise_memory, java_settings, inst_settings.
    pub actions: Vec<&'static str>,
    /// The lines that gave it away.
    pub excerpt: String,
    /// Where they came from (file name or "game output").
    pub source: String,
    pub profile_id: String,
}

impl Diagnosis {
    fn new(kind: &'static str, title: impl Into<String>, detail: impl Into<String>, actions: &[&'static str]) -> Self {
        Diagnosis {
            kind,
            title: title.into(),
            detail: detail.into(),
            culprit: None,
            hint: None,
            mod_file: None,
            needs: None,
            actions: actions.to_vec(),
            excerpt: String::new(),
            source: String::new(),
            profile_id: String::new(),
        }
    }
    fn culprit(mut self, name: Option<String>, hint: Option<Hint>) -> Self {
        self.culprit = name;
        self.hint = hint;
        self
    }
    fn excerpt(mut self, lines: &[&str]) -> Self {
        self.excerpt = lines.iter().map(|l| l.trim_end()).filter(|l| !l.is_empty()).take(6).collect::<Vec<_>>().join("\n");
        self
    }
}

type Rule = fn(&str) -> Option<Diagnosis>;

/// Most specific first: the first rule that recognises the text wins.
const RULES: &[Rule] = &[
    fabric_dependencies,
    duplicate_mods,
    out_of_memory,
    java_version,
    jvm_options,
    mixin_failure,
    entrypoint_failure,
    graphics,
    horizon_panic,
    missing_files,
    suspected_mod,
    mod_stack_frame,
];

/// The known cause in a crash report or log, if any.
pub fn explain(text: &str) -> Option<Diagnosis> {
    RULES.iter().find_map(|r| r(text))
}

// ── text helpers ──

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let a = s.find(start)? + start.len();
    let b = s[a..].find(end)? + a;
    Some(&s[a..b])
}

fn line_with<'a>(text: &'a str, needle: &str) -> Option<&'a str> {
    text.lines().find(|l| l.contains(needle))
}

fn mod_id_token(s: &str) -> Option<String> {
    let id: String = s.trim().trim_matches(|c| c == '\'' || c == '"').chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')).collect();
    let id = id.trim_end_matches('.').to_owned();
    (id.len() >= 2 && id.chars().next().is_some_and(|c| c.is_ascii_alphabetic())).then_some(id)
}

fn word_after<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let i = s.find(key)? + key.len();
    s[i..].split_whitespace().next()
}

// ── rules ──

/// One "Mod 'X' (x) 1.0 requires …" / "is incompatible with …" line from Fabric Loader.
#[derive(Debug, PartialEq)]
struct FabricProblem {
    name: String,
    id: String,
    need: String,
    dep: String,
    missing: bool,
    conflict: bool,
    present: Option<String>,
}

fn parse_fabric_line(line: &str) -> Option<FabricProblem> {
    let l = line.trim().trim_start_matches(['-', ' ', '\t']).trim();
    let rest = l.strip_prefix("Mod '")?;
    let (name, rest) = rest.split_once('\'')?;
    let id = between(rest, "(", ")").unwrap_or(name).to_owned();
    let (conflict, tail) = if let Some(i) = rest.find(" requires ") {
        (false, &rest[i + " requires ".len()..])
    } else if let Some(i) = rest.find(" is incompatible with ") {
        (true, &rest[i + " is incompatible with ".len()..])
    } else if let Some(i) = rest.find(" breaks ") {
        (true, &rest[i + " breaks ".len()..])
    } else {
        return None;
    };
    let missing = tail.contains("which is missing");
    let need = tail
        .split(", which is missing")
        .next()
        .and_then(|t| t.split(", but ").next())
        .unwrap_or(tail)
        .trim_end_matches(['!', '.'])
        .trim()
        .to_owned();
    let present = tail.split("present: ").nth(1).map(|p| p.trim_end_matches(['!', '.']).trim().to_owned());
    // "… of mod 'Minecraft' (minecraft)" / "… of fabric-api" / "… of mod fabric-api"
    let of = need.rsplit(" of ").next().unwrap_or(&need).trim();
    let of = of.strip_prefix("mod ").unwrap_or(of);
    let dep = match between(of, "'", "'") {
        Some(n) => n.to_owned(),
        None => of.split_whitespace().next().unwrap_or(of).to_owned(),
    };
    Some(FabricProblem { name: name.to_owned(), id, need, dep, missing, conflict, present })
}

fn fabric_dependencies(text: &str) -> Option<Diagnosis> {
    let header = ["Incompatible mods found", "Incompatible mod set", "Some of your mods are incompatible", "Mod resolution failed", "mods are incompatible with the game or each other"];
    let problems: Vec<(&str, FabricProblem)> = text.lines().filter_map(|l| parse_fabric_line(l).map(|p| (l, p))).collect();
    if problems.is_empty() {
        let line = text.lines().find(|l| header.iter().any(|h| l.contains(h)))?;
        return Some(
            Diagnosis::new("incompatible_mods", "Your mods don't fit together", "Fabric found mods that can't run together. Logs lists which ones.", &["open_logs"])
                .excerpt(&[line]),
        );
    }
    let (line, p) = &problems[0];
    let more = match problems.len() {
        1 => String::new(),
        n => format!(" ({} more problem{} in Logs.)", n - 1, if n == 2 { "" } else { "s" }),
    };
    let dep_lower = p.dep.to_lowercase();
    let platform = dep_lower == "minecraft" || dep_lower == "java" || dep_lower == "fabricloader" || dep_lower == "fabric loader";
    let d = if p.conflict {
        Diagnosis::new(
            "incompatible_mods",
            format!("{} can't run alongside {}", p.name, p.dep),
            format!("{} doesn't work with {}. Switch one of them off.{more}", p.name, p.dep),
            &["disable_mod", "open_logs"],
        )
    } else if p.missing {
        let mut d = Diagnosis::new(
            "missing_dependency",
            format!("{} needs {}", p.name, p.dep),
            format!("{} needs {}{}, which isn't installed. Add it from Discover, or switch {} off.{more}", p.name, p.dep, version_note(&p.need, &p.dep), p.name),
            &["find_mod", "disable_mod", "open_logs"],
        );
        d.needs = Some(p.dep.clone());
        d
    } else if platform {
        let found = p.present.as_deref().map(|v| format!(" This instance has {v}.")).unwrap_or_default();
        Diagnosis::new(
            "incompatible_mods",
            format!("{} is made for a different {}", p.name, p.dep),
            format!("{} needs {}.{found} Switch it off, or get the version made for this instance from Discover.{more}", p.name, p.need),
            &["disable_mod", "open_logs"],
        )
    } else {
        let found = p.present.as_deref().map(|v| format!(", but {v} is installed")).unwrap_or_default();
        let mut d = Diagnosis::new(
            "incompatible_mods",
            format!("{} needs a different {}", p.name, p.dep),
            format!("{} needs {}{found}. Update one of them from Discover, or switch {} off.{more}", p.name, p.need, p.name),
            &["find_mod", "disable_mod", "open_logs"],
        );
        d.needs = Some(p.dep.clone());
        d
    };
    Some(d.culprit(Some(p.name.clone()), Some(Hint::ModId(p.id.clone()))).excerpt(&[line]))
}

/// " (version 0.5.x)" unless any version will do.
fn version_note(need: &str, dep: &str) -> String {
    let v = need.split(" of ").next().unwrap_or("").trim();
    if v.is_empty() || v == "any version" || need == dep {
        String::new()
    } else {
        format!(" ({v})")
    }
}

fn duplicate_mods(text: &str) -> Option<Diagnosis> {
    let line = text.lines().find(|l| l.contains("Duplicate mods found") || l.contains("Found duplicate mod") || l.contains("duplicate mod ids"))?;
    let id = between(line, "'", "'").or_else(|| word_after(line, "mod id ")).and_then(mod_id_token);
    Some(
        Diagnosis::new("duplicate_mods", "The same mod is installed twice", "Two files in the mods folder are the same mod. Remove the older one.", &["open_logs"])
            .culprit(id.clone(), id.map(Hint::ModId))
            .excerpt(&[line]),
    )
}

fn out_of_memory(text: &str) -> Option<Diagnosis> {
    if let Some(line) = line_with(text, "Could not reserve enough space for").or_else(|| line_with(text, "Invalid maximum heap size")) {
        return Some(
            Diagnosis::new(
                "memory_too_high",
                "Java couldn't get the memory it was asked for",
                "The memory set for this instance is more than this computer can give Java. Choose less memory in the instance settings.",
                &["inst_settings", "open_logs"],
            )
            .excerpt(&[line]),
        );
    }
    if let Some(line) = line_with(text, "There is insufficient memory for the Java Runtime").or_else(|| line_with(text, "memory allocation of ")) {
        return Some(
            Diagnosis::new(
                "system_memory",
                "Your computer ran out of memory",
                "The game asked for more memory than was free. Close other apps, or choose less memory for this instance.",
                &["inst_settings", "open_logs"],
            )
            .excerpt(&[line]),
        );
    }
    let line = line_with(text, "java.lang.OutOfMemoryError").or_else(|| line_with(text, "OutOfMemoryError"))?;
    Some(
        Diagnosis::new(
            "out_of_memory",
            "The game ran out of memory",
            "Minecraft used all the memory it was given. Give this instance more memory (big modpacks and shaders need 6–8 GB).",
            &["raise_memory", "inst_settings", "open_logs"],
        )
        .excerpt(&[line]),
    )
}

/// Java release for a class file version (52 → 8, 65 → 21).
fn java_for_class_version(v: &str) -> Option<u32> {
    let n: f32 = v.trim().parse().ok()?;
    (n >= 45.0).then(|| n as u32 - 44)
}

fn java_version(text: &str) -> Option<Diagnosis> {
    let line = line_with(text, "UnsupportedClassVersionError")?;
    let need = between(line, "class file version ", ")").and_then(java_for_class_version);
    let have = line.split("versions up to ").nth(1).and_then(|v| java_for_class_version(v.trim_end_matches(['.', ' '])));
    let class = between(line, "UnsupportedClassVersionError: ", " has been compiled")
        .or_else(|| between(line, "UnsupportedClassVersionError: ", " (class file"))
        .map(|c| c.trim().replace('/', "."));
    let what = match (need, have) {
        (Some(n), Some(h)) => format!("needs Java {n}, but the game started with Java {h}"),
        (Some(n), None) => format!("needs Java {n} or newer"),
        _ => "needs a newer Java".to_owned(),
    };
    let detail = format!(
        "Part of the game {what}. If you set your own Java in Settings › Java, clear it so LumosClient picks the right one; a mod made for a newer Minecraft can cause this too."
    );
    let mut actions = vec!["java_settings", "repair", "open_logs"];
    if class.as_deref().is_some_and(|c| !is_platform_class(c)) {
        actions.insert(0, "disable_mod");
    }
    Some(Diagnosis::new("java_version", "Wrong Java version", detail, &actions).culprit(None, class.map(Hint::Class)).excerpt(&[line]))
}

fn jvm_options(text: &str) -> Option<Diagnosis> {
    let line = line_with(text, "Unrecognized VM option").or_else(|| line_with(text, "Unrecognized option:")).or_else(|| line_with(text, "Error: Could not create the Java Virtual Machine"))?;
    let opt = between(line, "option '", "'").or_else(|| line.split("Unrecognized option:").nth(1)).map(|o| o.trim().to_owned());
    let detail = match opt {
        Some(o) if !o.is_empty() => format!("Java doesn't understand the option {o}. Remove it from Java arguments in this instance's settings or in Settings › Java."),
        _ => "Java couldn't start with the Java arguments set for this instance. Remove extra flags in the instance settings or Settings › Java.".to_owned(),
    };
    Some(Diagnosis::new("jvm_options", "Java didn't start", detail, &["inst_settings", "java_settings", "open_logs"]).excerpt(&[line]))
}

fn mixin_failure(text: &str) -> Option<Diagnosis> {
    let markers = ["MixinApplyError", "MixinTransformerError", "InvalidInjectionException", "InvalidMixinException", "Mixin apply for mod", "Mixin prepare for mod", "Critical injection failure", "Mixin apply failed"];
    let line = text.lines().find(|l| markers.iter().any(|m| l.contains(m)))?;
    // The most specific line naming a mod or mixin config, anywhere in the report.
    let mod_line = text.lines().find(|l| l.contains("Mixin apply for mod ") || l.contains("Mixin prepare for mod "));
    let hint = mod_line
        .and_then(|l| word_after(l, " for mod "))
        .and_then(mod_id_token)
        .map(Hint::ModId)
        .or_else(|| text.lines().filter(|l| markers.iter().any(|m| l.contains(m))).find_map(|l| word_after(l, "from mod ").and_then(|w| mod_id_token(w.trim_end_matches([']', ',', ')']))).map(Hint::ModId)))
        .or_else(|| text.lines().filter(|l| l.contains("Mixin") || l.contains("mixin")).find_map(mixin_config).map(Hint::MixinConfig));
    let name = match &hint {
        Some(Hint::ModId(id)) => Some(id.clone()),
        Some(Hint::MixinConfig(c)) => Some(config_mod_guess(c)),
        _ => None,
    };
    let detail = match &name {
        Some(n) => format!("The mod {n} couldn't change the game's code. It's usually made for another Minecraft version or clashes with another mod. Switch it off or update it."),
        None => "A mod couldn't change the game's code. It's usually made for another Minecraft version or clashes with another mod.".to_owned(),
    };
    let lines: Vec<&str> = [Some(line), mod_line].into_iter().flatten().collect();
    Some(Diagnosis::new("mixin", "A mod didn't load", detail, &["disable_mod", "open_logs"]).culprit(name, hint).excerpt(&lines))
}

/// `sodium.mixins.json` (or `mixins.sodium.json`) in a line.
fn mixin_config(line: &str) -> Option<String> {
    line.split(|c: char| c.is_whitespace() || matches!(c, '[' | ']' | '(' | ')' | ',' | '\'' | '"'))
        .map(|t| t.split(':').next().unwrap_or(t))
        .find(|t| t.ends_with(".json") && t.contains("mixins") && !t.contains('/') && t.len() > 11)
        .map(str::to_owned)
}

fn config_mod_guess(config: &str) -> String {
    let base = config.trim_end_matches(".json");
    let parts: Vec<&str> = base.split('.').filter(|p| *p != "mixins" && *p != "mixin" && *p != "client" && *p != "common").collect();
    parts.first().map(|p| (*p).to_owned()).unwrap_or_else(|| base.to_owned())
}

fn entrypoint_failure(text: &str) -> Option<Diagnosis> {
    let line = text.lines().find(|l| l.contains("Could not execute entrypoint stage") && l.contains("provided by '"))?;
    let id = between(line, "provided by '", "'").and_then(mod_id_token)?;
    Some(
        Diagnosis::new(
            "mod_crash",
            format!("{id} crashed while starting"),
            format!("The mod {id} hit an error while the game was starting. Update it from Discover or switch it off."),
            &["disable_mod", "open_logs"],
        )
        .culprit(Some(id.clone()), Some(Hint::ModId(id)))
        .excerpt(&[line]),
    )
}

fn graphics(text: &str) -> Option<Diagnosis> {
    const SIGNS: &[&str] = &[
        "GLFW error 65542",
        "GLFW error 65543",
        "GLFW error 65545",
        "The driver does not appear to support OpenGL",
        "Pixel format not accelerated",
        "No OpenGL context",
        "Failed to create the GLFW window",
        "Couldn't set pixel format",
        "Could not create context",
        "OpenGL 3.2",
        "nvoglv64.dll",
        "atio6axx.dll",
        "atioglxx.dll",
        "ig9icd64.dll",
        "ig7icd64.dll",
        "ig75icd64.dll",
        "ig8icd64.dll",
        "igxelpicd64.dll",
        "amdvlk64.dll",
        "No suitable graphics adapter",
        "RequestAdapterError",
        "RequestDeviceError",
        "Failed to create surface",
        "Device lost",
        "DeviceLost",
        "ERROR_DEVICE_LOST",
        "VK_ERROR_",
        "Parent device is lost",
    ];
    let line = text.lines().find(|l| SIGNS.iter().any(|s| l.contains(s)))?;
    Some(
        Diagnosis::new(
            "graphics",
            "Your graphics driver stopped the game",
            "The game couldn't use your graphics card. Update your graphics driver, and if you use shaders, switch them off and try again.",
            &["open_logs"],
        )
        .excerpt(&[line]),
    )
}

fn horizon_panic(text: &str) -> Option<Diagnosis> {
    let lines: Vec<&str> = text.lines().collect();
    let i = lines.iter().position(|l| l.contains("panicked at "))?;
    let line = lines[i];
    // Old format: panicked at 'message', src/x.rs:1:2   New: panicked at src/x.rs:1:2:\nmessage
    let msg = match between(line, "panicked at '", "', ") {
        Some(m) => m.to_owned(),
        None => lines[i + 1..].iter().map(|l| l.trim()).find(|l| !l.is_empty() && !l.starts_with("note:")).unwrap_or("").to_owned(),
    };
    let lower = msg.to_lowercase();
    let files = ["no such file", "cannot find the file", "not found", "missing", "resources"].iter().any(|k| lower.contains(k));
    let short: String = msg.chars().take(220).collect();
    let detail = if files {
        format!("Horizon stopped because something it needs is missing ({short}). Repair instance gets it again.")
    } else if short.is_empty() {
        "Horizon stopped with an internal error. Repair instance re-checks its files; if it keeps happening, the details are in Logs.".to_owned()
    } else {
        format!("Horizon stopped with an internal error: {short}. Repair instance re-checks its files; if it keeps happening, the details are in Logs.")
    };
    let culprit = between(&msg, "mod '", "'").and_then(mod_id_token);
    let mut excerpt = vec![line];
    if let Some(next) = lines.get(i + 1) {
        excerpt.push(next);
    }
    Some(Diagnosis::new("horizon_panic", "The Horizon engine crashed", detail, &["repair", "open_logs"]).culprit(culprit, None).excerpt(&excerpt))
}

fn missing_files(text: &str) -> Option<Diagnosis> {
    const SIGNS: &[&str] = &[
        "Could not find or load main class",
        "ClassNotFoundException: net.minecraft",
        "ClassNotFoundException: com.mojang",
        "NoClassDefFoundError: net/minecraft",
        "NoClassDefFoundError: com/mojang",
        "NoClassDefFoundError: org/lwjgl",
        "zip END header not found",
        "invalid LOC header",
        "error in opening zip file",
        "zip file is empty",
        "java.util.zip.ZipException",
        "java.nio.file.NoSuchFileException",
        "java.io.FileNotFoundException",
        "No such file or directory (os error 2)",
        "The system cannot find the file specified. (os error 2)",
        "The system cannot find the path specified. (os error 3)",
        "Failed to load native library",
        "UnsatisfiedLinkError",
    ];
    let line = text.lines().find(|l| SIGNS.iter().any(|s| l.contains(s)))?;
    Some(
        Diagnosis::new(
            "missing_files",
            "Some game files are missing or damaged",
            "The game couldn't read a file it needs. Repair instance checks every game file and downloads the damaged ones again.",
            &["repair", "open_logs"],
        )
        .excerpt(&[line]),
    )
}

/// Minecraft's crash report section "Suspected Mod(s):".
fn suspected_mod(text: &str) -> Option<Diagnosis> {
    let lines: Vec<&str> = text.lines().collect();
    let i = lines.iter().position(|l| l.trim_start().starts_with("Suspected Mod"))?;
    let after = lines[i].split_once(':').map(|(_, v)| v.trim()).unwrap_or("");
    let value = if after.is_empty() { lines.get(i + 1).map(|l| l.trim()).unwrap_or("") } else { after };
    let lower = value.to_lowercase();
    if value.is_empty() || lower.starts_with("none") || lower.starts_with("unknown") || value.starts_with("~~") {
        return None;
    }
    let first = value.split(", Version").next().unwrap_or(value);
    let name = first.split(" (").next().unwrap_or(first).trim().to_owned();
    let id = between(first, "(", ")").and_then(mod_id_token);
    if id.as_deref().is_some_and(|i| matches!(i, "minecraft" | "java" | "fabricloader")) {
        return None;
    }
    let hint = id.map(Hint::ModId);
    Some(
        Diagnosis::new(
            "mod_crash",
            format!("{name} crashed the game"),
            format!("Minecraft's crash report points at {name}. Update it from Discover or switch it off and play again."),
            &["disable_mod", "open_logs"],
        )
        .culprit(Some(name), hint)
        .excerpt(&lines[i..(i + 2).min(lines.len())]),
    )
}

/// Packages that are the game, its loader or common libraries, not a mod.
fn is_platform_class(class: &str) -> bool {
    const PLATFORM: &[&str] = &[
        "java.", "javax.", "jdk.", "sun.", "com.sun.", "net.minecraft.", "com.mojang.", "net.fabricmc.", "org.lwjgl.", "com.google.", "io.netty.",
        "org.apache.", "it.unimi.", "org.spongepowered.", "org.slf4j.", "kotlin.", "kotlinx.", "org.objectweb.", "oshi.", "cpw.", "org.quiltmc.",
        "com.llamalad7.", "org.joml.", "com.electronwill.", "net.java.", "ca.weblite.", "moe.yushi.", "jdk.internal.", "org.jetbrains.", "com.github.benmanes.",
    ];
    PLATFORM.iter().any(|p| class.starts_with(p))
}

/// (class, mod id from a mixin handler name) of a stack frame line `at a.b.C.m(File.java:1)`.
fn parse_frame(line: &str) -> Option<(String, Option<String>)> {
    let t = line.trim_start();
    let t = t.strip_prefix("at ")?;
    let call = t.split('(').next()?.trim();
    // Module prefixes: "app//net.foo.Bar.baz" or "java.base/java.lang.Thread.run".
    let call = call.rsplit('/').next().unwrap_or(call);
    let (class, method) = call.rsplit_once('.')?;
    // Mixin handlers carry the mod id: handler$zza000$sodium$onInit
    let parts: Vec<&str> = method.split('$').collect();
    let from_mixin = (parts.len() >= 4
        && parts[1].len() == 6
        && parts[1][..3].chars().all(|c| c.is_ascii_lowercase())
        && parts[1][3..].chars().all(|c| c.is_ascii_digit()))
    .then(|| mod_id_token(parts[2]))
    .flatten();
    Some((class.to_owned(), from_mixin))
}

fn mod_stack_frame(text: &str) -> Option<Diagnosis> {
    // Only frames of an actual error: after the first exception line.
    let start = text
        .lines()
        .position(|l| (l.contains("Exception") || l.contains("Error:") || l.trim_end().ends_with("Error")) && !l.trim_start().starts_with("at "))?;
    let lines: Vec<&str> = text.lines().skip(start).take(400).collect();
    for l in &lines {
        let Some((class, mixin_mod)) = parse_frame(l) else { continue };
        if let Some(id) = mixin_mod {
            return Some(mod_crash_for(Some(id.clone()), Hint::ModId(id), &[lines[0], l]));
        }
        if !is_platform_class(&class) {
            return Some(mod_crash_for(None, Hint::Class(class), &[lines[0], l]));
        }
    }
    None
}

fn mod_crash_for(name: Option<String>, hint: Hint, excerpt: &[&str]) -> Diagnosis {
    let detail = match &name {
        Some(n) => format!("The crash happened inside {n}. Update it from Discover or switch it off and play again."),
        None => "The crash happened inside a mod. Update it from Discover or switch it off and play again.".to_owned(),
    };
    Diagnosis::new("mod_crash", "A mod crashed the game", detail, &["disable_mod", "open_logs"]).culprit(name, Some(hint)).excerpt(excerpt)
}

// ───────────────────────── Files ─────────────────────────

fn read_head(path: &Path, max: u64) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.by_ref().take(max).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// The last `lines` lines of a (possibly big) log.
fn read_tail(path: &Path, max_bytes: u64, lines: usize) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(max_bytes))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let all: Vec<&str> = text.lines().collect();
    Some(all[all.len().saturating_sub(lines)..].join("\n"))
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// The newest crash report written since `since`.
fn newest_crash_report(game_dir: &Path, since: SystemTime) -> Option<PathBuf> {
    fs::read_dir(game_dir.join("crash-reports"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .filter_map(|p| modified(&p).map(|m| (m, p)))
        .filter(|(m, _)| *m >= since)
        .max_by_key(|(m, _)| *m)
        .map(|(_, p)| p)
}

/// (id, name, provided ids) from a Fabric mod's fabric.mod.json.
fn fabric_meta<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> Option<(String, String, Vec<String>)> {
    let mut raw = String::new();
    zip.by_name("fabric.mod.json").ok()?.take(256 * 1024).read_to_string(&mut raw).ok()?;
    // Some mods put raw newlines inside strings, which strict JSON refuses.
    let v: serde_json::Value = serde_json::from_str(&raw).or_else(|_| serde_json::from_str(&raw.replace(['\n', '\r', '\t'], " "))).ok()?;
    let id = v.get("id")?.as_str()?.to_owned();
    let name = v.get("name").and_then(|n| n.as_str()).filter(|n| !n.trim().is_empty()).unwrap_or(&id).to_owned();
    let provides = v.get("provides").and_then(|p| p.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    Some((id, name, provides))
}

/// Finds the enabled mod jar a hint points at: (file name, mod name).
pub fn find_mod_jar(mods_dir: &Path, hint: &Hint) -> Option<(String, String)> {
    let mut jars: Vec<PathBuf> = fs::read_dir(mods_dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "jar")).collect();
    jars.sort();
    let class_entry = match hint {
        Hint::Class(c) => Some(format!("{}.class", c.split('$').next().unwrap_or(c).replace('.', "/"))),
        _ => None,
    };
    for jar in jars {
        let Ok(file) = fs::File::open(&jar) else { continue };
        let Ok(mut zip) = zip::ZipArchive::new(file) else { continue };
        let meta = fabric_meta(&mut zip);
        let hit = match hint {
            Hint::ModId(id) => meta.as_ref().is_some_and(|(mid, _, provides)| mid.eq_ignore_ascii_case(id) || provides.iter().any(|p| p.eq_ignore_ascii_case(id))),
            Hint::MixinConfig(c) => zip.by_name(c).is_ok(),
            Hint::Class(_) => class_entry.as_deref().is_some_and(|e| zip.by_name(e).is_ok()),
        };
        if hit {
            let file = jar.file_name()?.to_string_lossy().into_owned();
            let name = meta.map(|m| m.1).unwrap_or_else(|| file.trim_end_matches(".jar").to_owned());
            return Some((file, name));
        }
    }
    None
}

/// Reads what the game left behind and explains the exit. Always returns something to show.
pub fn diagnose(game_dir: &Path, horizon: bool, ran_for: Duration, code: Option<i32>, profile_id: &str) -> Diagnosis {
    let since = SystemTime::now().checked_sub(ran_for + Duration::from_secs(5)).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut sources: Vec<(String, String)> = Vec::new();
    if let Some(report) = newest_crash_report(game_dir, since) {
        if let Some(text) = read_head(&report, 1024 * 1024) {
            sources.push((format!("crash-reports/{}", report.file_name().unwrap_or_default().to_string_lossy()), text));
        }
    }
    let tail = tail_text();
    if !tail.trim().is_empty() {
        sources.push(("game output".into(), tail));
    }
    let latest = game_dir.join("logs").join("latest.log");
    if modified(&latest).is_some_and(|m| m >= since) {
        if let Some(text) = read_tail(&latest, 512 * 1024, 600) {
            sources.push(("logs/latest.log".into(), text));
        }
    }
    let found = sources.iter().find_map(|(name, text)| {
        explain(text).map(|mut d| {
            d.source = name.clone();
            d
        })
    });
    let mut d = found.unwrap_or_else(|| {
        let mut d = Diagnosis::new(
            "unknown",
            "The game closed with an error",
            format!(
                "It stopped{}. Logs shows its last messages; Repair instance re-checks the game files.",
                code.map(|c| format!(" with exit code {c}")).unwrap_or_default()
            ),
            &["open_logs", "repair"],
        );
        d.source = sources.first().map(|s| s.0.clone()).unwrap_or_default();
        d
    });
    d.profile_id = profile_id.to_owned();
    // Point "Switch off" at a real file (Java instances; Horizon mods are switched in the Mods tab).
    if !horizon {
        if let Some(hint) = d.hint.clone() {
            if let Some((file, name)) = find_mod_jar(&game_dir.join("mods"), &hint) {
                d.mod_file = Some(file);
                d.culprit = Some(name.clone());
                if d.kind == "mod_crash" || d.kind == "java_version" {
                    d.detail = d.detail.replace("inside a mod", &format!("inside {name}"));
                    if d.kind == "java_version" {
                        d.detail = format!("{name} {}", d.detail.trim_start_matches("Part of the game "));
                    }
                }
            }
        }
    }
    if d.mod_file.is_none() {
        d.actions.retain(|a| *a != "disable_mod");
    }
    if horizon {
        d.actions.retain(|a| !matches!(*a, "raise_memory" | "java_settings" | "find_mod"));
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// The output tail and stop flag are process-wide: tests touching them take turns.
    static SESSION: Mutex<()> = Mutex::new(());

    const FABRIC_MISSING: &str = "[main/ERROR]: Incompatible mods found!
net.fabricmc.loader.impl.FormattedException: Some of your mods are incompatible with the game or each other!
A potential solution has been determined, this may resolve your problem:
\t - Install fabric-api, any version.
More details:
\t - Mod 'Iris' (iris) 1.7.0+mc1.21 requires any version of fabric-api, which is missing!";

    const FABRIC_WRONG_MC: &str = "Incompatible mods found!
\t - Mod 'Mod Menu' (modmenu) 7.2.2 requires version 1.20.x of mod 'Minecraft' (minecraft), but only the wrong version is present: 1.21.11!
\t - Mod 'Sodium Extra' (sodium-extra) 0.5.1 requires version 0.5.x of sodium, but only the wrong version is present: 0.6.0!";

    const FABRIC_WRONG_DEP: &str = "\t - Mod 'Sodium Extra' (sodium-extra) 0.5.1 requires version 0.5.x of sodium, but only the wrong version is present: 0.6.0!";

    const FABRIC_CONFLICT: &str = "\t - Mod 'OptiFabric' (optifabric) 1.13 is incompatible with any version of mod 'Sodium' (sodium), but it is present: 0.6.0!";

    const MIXIN: &str = "---- Minecraft Crash Report ----
Description: Initializing game
org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: An unexpected critical error was encountered
\tat org.spongepowered.asm.mixin.transformer.MixinProcessor.applyMixins(MixinProcessor.java:392)
Caused by: org.spongepowered.asm.mixin.throwables.MixinApplyError: Mixin [coolmod.mixins.json:MixinTitleScreen] from phase [DEFAULT] in config [coolmod.mixins.json] FAILED during APPLY
\tat org.spongepowered.asm.mixin.transformer.MixinProcessor.handleMixinError(MixinProcessor.java:638)";

    const MIXIN_FABRIC: &str = "Caused by: org.spongepowered.asm.mixin.throwables.MixinApplyError: Mixin [iris.mixins.json:MixinLevelRenderer from mod iris] from phase [DEFAULT] in config [iris.mixins.json] FAILED during APPLY
[main/ERROR]: Mixin apply for mod iris failed iris.mixins.json:MixinLevelRenderer from mod iris -> net.minecraft.class_761";

    const OOM: &str = "---- Minecraft Crash Report ----
Description: Unexpected error
java.lang.OutOfMemoryError: Java heap space
\tat java.base/java.util.Arrays.copyOf(Arrays.java:3537)";

    const JAVA: &str = "Error: LinkageError occurred while loading main class net.fabricmc.loader.impl.launch.knot.KnotClient
java.lang.UnsupportedClassVersionError: net/fabricmc/loader/impl/launch/knot/KnotClient has been compiled by a more recent version of the Java Runtime (class file version 65.0), this version of the Java Runtime only recognizes class file versions up to 61.0";

    const JAVA_MOD: &str = "java.lang.UnsupportedClassVersionError: dev/cool/CoolMod has been compiled by a more recent version of the Java Runtime (class file version 65.0), this version of the Java Runtime only recognizes class file versions up to 61.0";

    const GL: &str = "[Render thread/ERROR]: GLFW error during init: [0x10007]65543: WGL: OpenGL profile requested but WGL_ARB_create_context_profile is unavailable
[Render thread/ERROR]: GLFW error 65542: WGL: The driver does not appear to support OpenGL";

    const DRIVER: &str = "#  EXCEPTION_ACCESS_VIOLATION (0xc0000005) at pc=0x00007ffd, pid=1234
# Problematic frame:
# C  [atio6axx.dll+0x1c2a3b]";

    const PANIC_NEW: &str = "thread 'main' panicked at crates/horizon-client/src/game/runtime/mod.rs:812:9:
index out of bounds: the len is 3 but the index is 7
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace";

    const PANIC_OLD: &str = "thread 'render' panicked at 'resources missing: textures/atlas.json', src/render.rs:10:5";

    const MISSING: &str = "Error: Could not find or load main class net.fabricmc.loader.impl.launch.knot.KnotClient
Caused by: java.lang.ClassNotFoundException: net.fabricmc.loader.impl.launch.knot.KnotClient";

    const ZIP: &str = "java.util.zip.ZipException: zip END header not found
\tat java.base/java.util.zip.ZipFile$Source.zerror(ZipFile.java:1607)";

    const SUSPECTED: &str = "-- Head --
Thread: Render thread
Suspected Mods:
\tSodium (sodium), Version: 0.6.0+mc1.21.1
\t\tIssue tracker URL: https://github.com/CaffeineMC/sodium/issues
Stacktrace:";

    const SUSPECTED_NONE: &str = "Suspected Mods: NONE
java.lang.IllegalStateException: boom
\tat net.minecraft.class_310.method_1514(class_310.java:42)";

    const ENTRYPOINT: &str = "java.lang.RuntimeException: Could not execute entrypoint stage 'client' due to errors, provided by 'coolmod' at 'dev.cool.CoolClient'!
\tat net.fabricmc.loader.impl.FabricLoaderImpl.invokeEntrypoints(FabricLoaderImpl.java:388)";

    const MOD_FRAME: &str = "java.lang.NullPointerException: Cannot invoke \"Object.toString()\" because \"x\" is null
\tat java.base/java.util.Objects.requireNonNull(Objects.java:233)
\tat net.minecraft.class_310.method_1514(class_310.java:42)
\tat dev.cool.render.Overlay.draw(Overlay.java:88)
\tat net.minecraft.class_310.method_1523(class_310.java:12)";

    const MIXIN_HANDLER_FRAME: &str = "java.lang.IllegalStateException: Rendersystem called from wrong thread
\tat com.mojang.blaze3d.systems.RenderSystem.assertOnRenderThread(RenderSystem.java:91)
\tat net.minecraft.class_761.handler$zza000$fancysky$onRender(class_761.java:2210)";

    const JVM_OPT: &str = "Unrecognized VM option 'UseZGCX'
Error: Could not create the Java Virtual Machine.
Error: A fatal exception has occurred. Program will exit.";

    const HEAP_TOO_BIG: &str = "Error occurred during initialization of VM
Could not reserve enough space for 16777216KB object heap";

    #[test]
    fn known_causes_table() {
        // (log, kind, culprit, hint)
        let cases: &[(&str, &str, Option<&str>, Option<Hint>)] = &[
            (FABRIC_MISSING, "missing_dependency", Some("Iris"), Some(Hint::ModId("iris".into()))),
            (FABRIC_WRONG_MC, "incompatible_mods", Some("Mod Menu"), Some(Hint::ModId("modmenu".into()))),
            (FABRIC_WRONG_DEP, "incompatible_mods", Some("Sodium Extra"), Some(Hint::ModId("sodium-extra".into()))),
            (FABRIC_CONFLICT, "incompatible_mods", Some("OptiFabric"), Some(Hint::ModId("optifabric".into()))),
            (MIXIN, "mixin", Some("coolmod"), Some(Hint::MixinConfig("coolmod.mixins.json".into()))),
            (MIXIN_FABRIC, "mixin", Some("iris"), Some(Hint::ModId("iris".into()))),
            (OOM, "out_of_memory", None, None),
            (HEAP_TOO_BIG, "memory_too_high", None, None),
            (JAVA, "java_version", None, Some(Hint::Class("net.fabricmc.loader.impl.launch.knot.KnotClient".into()))),
            (JAVA_MOD, "java_version", None, Some(Hint::Class("dev.cool.CoolMod".into()))),
            (JVM_OPT, "jvm_options", None, None),
            (GL, "graphics", None, None),
            (DRIVER, "graphics", None, None),
            (PANIC_NEW, "horizon_panic", None, None),
            (PANIC_OLD, "horizon_panic", None, None),
            (MISSING, "missing_files", None, None),
            (ZIP, "missing_files", None, None),
            (SUSPECTED, "mod_crash", Some("Sodium"), Some(Hint::ModId("sodium".into()))),
            (ENTRYPOINT, "mod_crash", Some("coolmod"), Some(Hint::ModId("coolmod".into()))),
            (MOD_FRAME, "mod_crash", None, Some(Hint::Class("dev.cool.render.Overlay".into()))),
            (MIXIN_HANDLER_FRAME, "mod_crash", Some("fancysky"), Some(Hint::ModId("fancysky".into()))),
        ];
        for (log, kind, culprit, hint) in cases {
            let d = explain(log).unwrap_or_else(|| panic!("no diagnosis for {kind}:\n{log}"));
            assert_eq!(d.kind, *kind, "{log}");
            assert_eq!(d.culprit.as_deref(), *culprit, "{log}");
            assert_eq!(&d.hint, hint, "{log}");
            assert!(!d.excerpt.is_empty() && !d.title.is_empty() && !d.detail.is_empty(), "{log}");
            assert!(d.actions.contains(&"open_logs"), "{log}");
        }
    }

    #[test]
    fn details_name_the_mod_and_what_it_needs() {
        let d = explain(FABRIC_MISSING).unwrap();
        assert_eq!(d.needs.as_deref(), Some("fabric-api"));
        assert!(d.detail.contains("Iris needs fabric-api"), "{}", d.detail);
        assert!(d.actions.contains(&"find_mod"));
        let d = explain(FABRIC_WRONG_MC).unwrap();
        assert!(d.title.contains("different Minecraft"), "{}", d.title);
        assert!(d.detail.contains("1.21.11") && d.detail.contains("1 more problem"), "{}", d.detail);
        let d = explain(JAVA).unwrap();
        assert!(d.detail.contains("Java 21") && d.detail.contains("Java 17"), "{}", d.detail);
        assert!(!d.actions.contains(&"disable_mod"), "the loader itself isn't a mod");
        assert!(explain(JAVA_MOD).unwrap().actions.contains(&"disable_mod"));
        assert!(explain(OOM).unwrap().actions.contains(&"raise_memory"));
        assert!(explain(PANIC_NEW).unwrap().detail.contains("index out of bounds"));
        assert!(explain(PANIC_OLD).unwrap().detail.contains("missing"));
        assert!(explain(JVM_OPT).unwrap().detail.contains("UseZGCX"));
    }

    #[test]
    fn ordinary_logs_explain_nothing() {
        let normal = "[Render thread/INFO]: Backend library: LWJGL version 3.3.3\n[Render thread/INFO]: OpenGL renderer: Apple M2\n[Render thread/INFO]: Stopping!";
        assert!(explain(normal).is_none());
        // A crash report that names no mod and has only game frames.
        assert!(explain(SUSPECTED_NONE).is_none());
    }

    #[test]
    fn frames_and_configs() {
        assert_eq!(parse_frame("\tat app//dev.cool.X.y(X.java:1)"), Some(("dev.cool.X".into(), None)));
        assert_eq!(parse_frame("\tat net.minecraft.class_1.handler$abc123$mymod$tick(class_1.java:1)").unwrap().1.as_deref(), Some("mymod"));
        assert_eq!(parse_frame("not a frame"), None);
        assert_eq!(mixin_config("in config [mixins.sodium.json] FAILED").as_deref(), Some("mixins.sodium.json"));
        assert_eq!(config_mod_guess("mixins.sodium.json"), "sodium");
        assert_eq!(config_mod_guess("coolmod.client.mixins.json"), "coolmod");
        assert_eq!(java_for_class_version("65.0"), Some(21));
        assert_eq!(java_for_class_version("52"), Some(8));
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lumos-crash-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn jar(path: &Path, entries: &[(&str, &str)]) {
        let mut w = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in entries {
            w.start_file(*name, o).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn finds_the_jar_and_reads_crash_reports() {
        let _turn = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        let game = temp("game");
        let mods = game.join("mods");
        fs::create_dir_all(&mods).unwrap();
        jar(&mods.join("cool-1.0.jar"), &[("fabric.mod.json", r#"{"id":"coolmod","name":"Cool Mod","provides":["cool"]}"#), ("coolmod.mixins.json", "{}"), ("dev/cool/render/Overlay.class", "")]);
        jar(&mods.join("iris-1.7.jar"), &[("fabric.mod.json", "{\"id\":\"iris\",\"name\":\"Iris\n Shaders\"}")]);
        jar(&mods.join("off.jar.disabled"), &[("fabric.mod.json", r#"{"id":"off"}"#)]);
        assert_eq!(find_mod_jar(&mods, &Hint::ModId("coolmod".into())), Some(("cool-1.0.jar".into(), "Cool Mod".into())));
        assert_eq!(find_mod_jar(&mods, &Hint::ModId("cool".into())).map(|j| j.0), Some("cool-1.0.jar".into()));
        assert_eq!(find_mod_jar(&mods, &Hint::MixinConfig("coolmod.mixins.json".into())).map(|j| j.0), Some("cool-1.0.jar".into()));
        assert_eq!(find_mod_jar(&mods, &Hint::Class("dev.cool.render.Overlay$1".into())).map(|j| j.0), Some("cool-1.0.jar".into()));
        assert_eq!(find_mod_jar(&mods, &Hint::ModId("iris".into())).map(|j| j.0), Some("iris-1.7.jar".into()));
        assert_eq!(find_mod_jar(&mods, &Hint::ModId("off".into())), None, "switched-off mods are skipped");

        fs::create_dir_all(game.join("crash-reports")).unwrap();
        fs::write(game.join("crash-reports/crash-2026-11-17_10.00.00-client.txt"), MOD_FRAME).unwrap();
        begin_session();
        let d = diagnose(&game, false, Duration::from_secs(60), Some(-1), "p1");
        assert_eq!((d.kind, d.mod_file.as_deref(), d.culprit.as_deref()), ("mod_crash", Some("cool-1.0.jar"), Some("Cool Mod")));
        assert!(d.detail.contains("inside Cool Mod"), "{}", d.detail);
        assert_eq!(d.source, "crash-reports/crash-2026-11-17_10.00.00-client.txt");
        assert_eq!(d.profile_id, "p1");

        // Nothing recognisable: still a card, without a "switch off" button.
        fs::remove_dir_all(game.join("crash-reports")).unwrap();
        let d = diagnose(&game, false, Duration::from_secs(60), Some(1), "p1");
        assert_eq!(d.kind, "unknown");
        assert!(d.detail.contains("exit code 1"));
        assert!(!d.actions.contains(&"disable_mod"));

        // The game's own output counts too.
        remember_line(OOM);
        let d = diagnose(&game, true, Duration::from_secs(60), Some(1), "p1");
        assert_eq!((d.kind, d.source.as_str()), ("out_of_memory", "game output"));
        assert!(!d.actions.contains(&"raise_memory"), "Horizon has no Java memory setting");
        begin_session();
        let _ = fs::remove_dir_all(&game);
    }

    #[test]
    fn stop_and_normal_exits_are_not_crashes() {
        let _turn = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        begin_session();
        assert!(!should_explain(Some(0)));
        assert!(should_explain(Some(1)));
        assert!(should_explain(None));
        note_user_stop();
        assert!(!should_explain(Some(1)));
        begin_session();
    }
}
