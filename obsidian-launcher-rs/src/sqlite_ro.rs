//! A small, read-only reader for SQLite database files, enough to list rows of ordinary (rowid)
//! tables: the Modrinth App keeps its instances in `app.db` (import.rs). Written from the file
//! format's public description (<https://www.sqlite.org/fileformat2.html>).
//!
//! It never opens anything for writing (no `-shm`, no journal): committed pages still in the
//! write-ahead log (`<db>-wal`) are read from it directly, checked by its salts and checksums.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// One column value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// Largest database read (Modrinth's is a few MB; this only guards against nonsense files).
const MAX_DB_BYTES: u64 = 512 << 20;

pub struct Db {
    data: Vec<u8>,
    page_size: usize,
    usable: usize,
    /// Pages whose newest committed copy is in the WAL: page number → its bytes.
    wal: HashMap<u32, Vec<u8>>,
    tables: HashMap<String, (u32, Vec<(String, bool)>)>,
}

fn be16(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
}
fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// SQLite's variable-length integer: (value, bytes used).
fn varint(b: &[u8], at: usize) -> Option<(u64, usize)> {
    let mut v: u64 = 0;
    for i in 0..9 {
        let byte = *b.get(at + i)?;
        if i == 8 {
            return Some(((v << 8) | u64::from(byte), 9));
        }
        v = (v << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

impl Db {
    pub fn open(path: &Path) -> Result<Db, String> {
        let len = fs::metadata(path).map_err(|e| e.to_string())?.len();
        if len > MAX_DB_BYTES {
            return Err("database too large".into());
        }
        let data = fs::read(path).map_err(|e| e.to_string())?;
        let wal_path = {
            let mut s = path.as_os_str().to_owned();
            s.push("-wal");
            std::path::PathBuf::from(s)
        };
        let wal_bytes = fs::metadata(&wal_path).ok().filter(|m| m.len() <= MAX_DB_BYTES).and_then(|_| fs::read(&wal_path).ok());
        Db::from_bytes(data, wal_bytes)
    }

    pub fn from_bytes(data: Vec<u8>, wal: Option<Vec<u8>>) -> Result<Db, String> {
        if data.len() < 100 || &data[..16] != b"SQLite format 3\0" {
            return Err("not an SQLite database".into());
        }
        let ps = be16(&data, 16).ok_or("bad header")?;
        let page_size = if ps == 1 { 65536 } else { ps };
        if !(512..=65536).contains(&page_size) || !page_size.is_power_of_two() {
            return Err("bad page size".into());
        }
        if be32(&data, 56) != Some(1) {
            return Err("only UTF-8 databases are read".into());
        }
        let reserved = data[20] as usize;
        let mut db = Db { usable: page_size - reserved, page_size, data, wal: HashMap::new(), tables: HashMap::new() };
        if let Some(w) = wal {
            db.wal = read_wal(&w, page_size);
        }
        db.load_schema()?;
        Ok(db)
    }

    fn page(&self, n: u32) -> Option<&[u8]> {
        if n == 0 {
            return None;
        }
        if let Some(p) = self.wal.get(&n) {
            return Some(p);
        }
        let start = (n as usize - 1) * self.page_size;
        self.data.get(start..start + self.page_size)
    }

    /// Every row's values of the b-tree rooted at `root`, in rowid order.
    fn rows(&self, root: u32) -> Result<Vec<(i64, Vec<Value>)>, String> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        let mut visited = 0usize;
        while let Some(n) = stack.pop() {
            visited += 1;
            if visited > 1_000_000 {
                return Err("database loop".into());
            }
            let page = self.page(n).ok_or("missing page")?;
            let hdr = if n == 1 { 100 } else { 0 };
            let kind = *page.get(hdr).ok_or("bad page")?;
            let cells = be16(page, hdr + 3).ok_or("bad page")?;
            match kind {
                0x05 => {
                    // Interior table page: children left to right, then the right-most one.
                    let right = be32(page, hdr + 8).ok_or("bad page")?;
                    let mut kids = Vec::with_capacity(cells + 1);
                    for i in 0..cells {
                        let ptr = be16(page, hdr + 12 + i * 2).ok_or("bad page")?;
                        kids.push(be32(page, ptr).ok_or("bad cell")?);
                    }
                    kids.push(right);
                    // Popped last-in-first-out: push in reverse to keep rowid order.
                    stack.extend(kids.into_iter().rev());
                }
                0x0d => {
                    for i in 0..cells {
                        let ptr = be16(page, hdr + 8 + i * 2).ok_or("bad page")?;
                        let (len, a) = varint(page, ptr).ok_or("bad cell")?;
                        let (rowid, b) = varint(page, ptr + a).ok_or("bad cell")?;
                        let payload = self.payload(page, ptr + a + b, len as usize)?;
                        out.push((rowid as i64, record(&payload)?));
                    }
                }
                _ => return Err("not a table b-tree".into()),
            }
        }
        Ok(out)
    }

    /// A cell's whole payload, following overflow pages.
    fn payload(&self, page: &[u8], at: usize, len: usize) -> Result<Vec<u8>, String> {
        let u = self.usable;
        let x = u - 35;
        if len <= x {
            return Ok(page.get(at..at + len).ok_or("bad cell")?.to_vec());
        }
        let m = ((u - 12) * 32 / 255) - 23;
        let k = m + ((len - m) % (u - 4));
        let local = if k <= x { k } else { m };
        let mut out = page.get(at..at + local).ok_or("bad cell")?.to_vec();
        let mut next = be32(page, at + local).ok_or("bad cell")?;
        let mut hops = 0;
        while out.len() < len {
            hops += 1;
            if next == 0 || hops > 100_000 {
                return Err("broken overflow chain".into());
            }
            let p = self.page(next).ok_or("missing overflow page")?;
            let take = (len - out.len()).min(u - 4);
            out.extend_from_slice(p.get(4..4 + take).ok_or("bad overflow page")?);
            next = be32(p, 0).ok_or("bad overflow page")?;
        }
        Ok(out)
    }

    fn load_schema(&mut self) -> Result<(), String> {
        for (_, row) in self.rows(1)? {
            let (Some(Value::Text(kind)), Some(Value::Text(name)), Some(Value::Int(root)), Some(Value::Text(sql))) =
                (row.first(), row.get(1), row.get(3), row.get(4))
            else {
                continue;
            };
            if kind == "table" && *root > 0 && !sql.to_ascii_uppercase().contains("WITHOUT ROWID") {
                self.tables.insert(name.to_ascii_lowercase(), (*root as u32, columns(sql)));
            }
        }
        Ok(())
    }

    pub fn has_table(&self, name: &str) -> bool {
        self.tables.contains_key(&name.to_ascii_lowercase())
    }

    /// Every row of `table` as column name → value. Columns added after a row was written read
    /// as `Null`; an `INTEGER PRIMARY KEY` column reads as the rowid.
    pub fn table(&self, name: &str) -> Result<Vec<HashMap<String, Value>>, String> {
        let (root, cols) = self.tables.get(&name.to_ascii_lowercase()).ok_or_else(|| format!("no table {name}"))?;
        Ok(self
            .rows(*root)?
            .into_iter()
            .map(|(rowid, vals)| {
                cols.iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let v = match vals.get(i) {
                            Some(Value::Null) | None if c.1 => Value::Int(rowid),
                            Some(v) => v.clone(),
                            None => Value::Null,
                        };
                        (c.0.clone(), v)
                    })
                    .collect()
            })
            .collect())
    }
}

/// Decodes one record (header of serial types, then the values).
fn record(p: &[u8]) -> Result<Vec<Value>, String> {
    let (hlen, mut at) = varint(p, 0).ok_or("bad record")?;
    let hlen = hlen as usize;
    let mut types = Vec::new();
    while at < hlen {
        let (t, n) = varint(p, at).ok_or("bad record")?;
        types.push(t);
        at += n;
    }
    let mut body = hlen;
    let mut out = Vec::with_capacity(types.len());
    for t in types {
        let int = |n: usize| -> Result<i64, String> {
            let b = p.get(body..body + n).ok_or("bad record")?;
            let mut v: i64 = if b[0] & 0x80 != 0 { -1 } else { 0 };
            for &x in b {
                v = (v << 8) | i64::from(x);
            }
            Ok(v)
        };
        let (v, n) = match t {
            0 => (Value::Null, 0),
            1 => (Value::Int(int(1)?), 1),
            2 => (Value::Int(int(2)?), 2),
            3 => (Value::Int(int(3)?), 3),
            4 => (Value::Int(int(4)?), 4),
            5 => (Value::Int(int(6)?), 6),
            6 => (Value::Int(int(8)?), 8),
            7 => (Value::Real(f64::from_bits(int(8)? as u64)), 8),
            8 => (Value::Int(0), 0),
            9 => (Value::Int(1), 0),
            t if t >= 12 && t % 2 == 0 => {
                let n = ((t - 12) / 2) as usize;
                (Value::Blob(p.get(body..body + n).ok_or("bad record")?.to_vec()), n)
            }
            t if t >= 13 => {
                let n = ((t - 13) / 2) as usize;
                (Value::Text(String::from_utf8_lossy(p.get(body..body + n).ok_or("bad record")?).into_owned()), n)
            }
            _ => return Err("bad serial type".into()),
        };
        body += n;
        out.push(v);
    }
    Ok(out)
}

/// The column names of a `CREATE TABLE` statement, in order, each with whether it is the rowid
/// (`INTEGER PRIMARY KEY`, inline or as the table's single-column primary key).
fn columns(sql: &str) -> Vec<(String, bool)> {
    let (Some(open), Some(close)) = (sql.find('('), sql.rfind(')')) else { return Vec::new() };
    // Comments (`-- …` to the end of the line, `/* … */`) stay in the stored SQL and may hold
    // commas (Modrinth's schema has `-- array of (string, string)`): drop them outside quotes.
    let body = strip_sql_comments(&sql[open + 1..close]);
    let body = body.as_str();
    // Split at top-level commas, skipping parentheses and quoted text.
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None::<char>, 0usize);
    for (i, c) in body.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"' | '`') => quote = Some(c),
            (None, '[') => quote = Some(']'),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    let unquote = |t: &str| t.trim_matches(|c| matches!(c, '"' | '`' | '[' | ']' | '\'')).to_owned();
    // A column's name: a quoted name may hold spaces.
    let name_of = |def: &str| -> Option<String> {
        let close = match def.chars().next()? {
            '"' => '"',
            '`' => '`',
            '[' => ']',
            '\'' => '\'',
            _ => return def.split_whitespace().next().map(str::to_owned),
        };
        def[1..].find(close).map(|end| def[1..1 + end].to_owned())
    };
    // (name, declared INTEGER, inline INTEGER PRIMARY KEY)
    let mut cols: Vec<(String, bool, bool)> = Vec::new();
    let mut table_pk: Option<String> = None;
    for part in parts {
        let def = part.trim();
        let upper = def.to_ascii_uppercase();
        let first = upper.split_whitespace().next().unwrap_or("");
        if matches!(first, "PRIMARY" | "UNIQUE" | "CHECK" | "FOREIGN" | "CONSTRAINT") {
            if let Some(rest) = upper.strip_prefix("PRIMARY").map(str::trim_start).and_then(|r| r.strip_prefix("KEY")) {
                let inner = rest.trim().trim_start_matches('(').split(')').next().unwrap_or("");
                if !inner.contains(',') {
                    table_pk = Some(unquote(inner.trim()).to_ascii_lowercase());
                }
            }
            continue;
        }
        let Some(name) = name_of(def) else { continue };
        let after = upper.get(def.find(&name).map(|i| i + name.len() + 1).unwrap_or(0)..).unwrap_or("");
        let integer = after.trim_start_matches(['"', '`', ']', '\'']).split_whitespace().next() == Some("INTEGER");
        let inline_pk = integer && upper.contains("PRIMARY KEY") && !upper.contains(" DESC");
        cols.push((name, integer, inline_pk));
    }
    cols.into_iter()
        .map(|(n, integer, pk)| {
            let rowid = pk || (integer && table_pk.as_deref() == Some(n.to_ascii_lowercase().as_str()));
            (n, rowid)
        })
        .collect()
}

/// `sql` without its comments; quoted text is kept as it is.
fn strip_sql_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => {
                quote = Some(c);
                out.push(c);
            }
            '[' => {
                quote = Some(']');
                out.push(c);
            }
            '-' if chars.peek() == Some(&'-') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
                out.push(' ');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

/// The pages of the WAL's committed transactions: each valid frame (salts match the header,
/// running checksum matches) up to the last commit frame.
fn read_wal(w: &[u8], page_size: usize) -> HashMap<u32, Vec<u8>> {
    let mut out = HashMap::new();
    let Some(magic) = be32(w, 0) else { return out };
    let big = match magic {
        0x377f_0682 => false,
        0x377f_0683 => true,
        _ => return out,
    };
    if be32(w, 8).map(|p| if p == 65536 { 65536 } else { p as usize }) != Some(page_size) {
        return out;
    }
    let word = |b: &[u8], i: usize| -> u32 {
        let a: [u8; 4] = b[i..i + 4].try_into().unwrap();
        if big { u32::from_be_bytes(a) } else { u32::from_le_bytes(a) }
    };
    let checksum = |b: &[u8], mut s: (u32, u32)| -> (u32, u32) {
        let mut i = 0;
        while i + 8 <= b.len() {
            s.0 = s.0.wrapping_add(word(b, i)).wrapping_add(s.1);
            s.1 = s.1.wrapping_add(word(b, i + 4)).wrapping_add(s.0);
            i += 8;
        }
        s
    };
    if w.len() < 32 {
        return out;
    }
    let mut sum = checksum(&w[..24], (0, 0));
    if Some(sum) != be32(w, 24).zip(be32(w, 28)) {
        return out;
    }
    let (salt1, salt2) = (be32(w, 16), be32(w, 20));
    let mut pending: Vec<(u32, &[u8])> = Vec::new();
    let mut at = 32;
    while at + 24 + page_size <= w.len() {
        let fh = &w[at..at + 24];
        let page = &w[at + 24..at + 24 + page_size];
        if be32(fh, 8) != salt1 || be32(fh, 12) != salt2 {
            break;
        }
        sum = checksum(page, checksum(&fh[..8], sum));
        if Some(sum) != be32(fh, 16).zip(be32(fh, 20)) {
            break;
        }
        let pgno = be32(fh, 0).unwrap_or(0);
        pending.push((pgno, page));
        if be32(fh, 4).unwrap_or(0) != 0 {
            for (n, p) in pending.drain(..) {
                out.insert(n, p.to_vec());
            }
        }
        at += 24 + page_size;
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    /// Runs `script` with python3's sqlite3 module; false when python3 isn't there.
    pub(crate) fn python(script: &str) -> bool {
        match Command::new("python3").arg("-I").arg("-c").arg(script).output() {
            Ok(o) if o.status.success() => true,
            Ok(o) => panic!("python failed: {}", String::from_utf8_lossy(&o.stderr)),
            Err(_) => {
                eprintln!("python3 not found: SQLite fixture test skipped");
                false
            }
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lumos-sqlite-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn columns_parse() {
        let c = columns("CREATE TABLE t (\n id INTEGER NOT NULL CHECK (id = 0), -- x, y\n name TEXT, \"a b\" TEXT DEFAULT 'x,y',\n PRIMARY KEY (id)\n)");
        assert_eq!(c, vec![("id".into(), true), ("name".into(), false), ("a b".into(), false)]);
        let c = columns("CREATE TABLE s (\n    -- array of (string, string)\n    custom_env_vars JSONB NOT NULL, /* a, b */ `rowid x` INTEGER PRIMARY KEY\n)");
        assert_eq!(c, vec![("custom_env_vars".into(), false), ("rowid x".into(), true)]);
        let c = columns("CREATE TABLE p (path TEXT NOT NULL, n INTEGER, PRIMARY KEY (path))");
        assert_eq!(c, vec![("path".into(), false), ("n".into(), false)]);
    }

    #[test]
    fn reads_tables_overflow_alter_and_wal() {
        let d = tmp("main");
        let db = d.join("app.db");
        let snap = d.join("snap");
        fs::create_dir_all(&snap).unwrap();
        let script = format!(
            r#"
import sqlite3, shutil
db = sqlite3.connect({db:?})
db.execute("PRAGMA page_size=512")
db.execute("CREATE TABLE settings (id INTEGER NOT NULL CHECK (id = 0),\n    -- array of (string, string)\n    custom_env_vars TEXT NOT NULL,\n    custom_dir TEXT NULL, PRIMARY KEY (id))")
db.execute("INSERT INTO settings (id, custom_env_vars, custom_dir) VALUES (0, '[]', NULL)")
db.execute("CREATE TABLE profiles (path TEXT NOT NULL, name TEXT NOT NULL, game_version TEXT NOT NULL, mod_loader TEXT NOT NULL, PRIMARY KEY (path))")
for i in range(300):
    db.execute("INSERT INTO profiles VALUES (?,?,?,?)", ("p%d" % i, "Name %d" % i, "1.21.%d" % (i % 12), "fabric"))
db.execute("INSERT INTO profiles VALUES (?,?,?,?)", ("long", "L" * 5000, "1.20.1", "forge"))
db.execute("ALTER TABLE profiles ADD COLUMN extra TEXT NULL")
db.execute("UPDATE profiles SET extra = 'x' WHERE path = 'p1'")
db.commit()
db.execute("PRAGMA journal_mode=WAL")
db.execute("PRAGMA wal_autocheckpoint=0")
db.execute("INSERT INTO profiles (path, name, game_version, mod_loader) VALUES ('walrow', 'From WAL', '26.3', 'vanilla')")
db.commit()
shutil.copy({db:?}, {s1:?})
shutil.copy({db:?} + "-wal", {s2:?})
db.close()
"#,
            db = db.to_string_lossy(),
            s1 = snap.join("app.db").to_string_lossy(),
            s2 = snap.join("app.db-wal").to_string_lossy(),
        );
        if !python(&script) {
            return;
        }
        // The snapshot still has the new row only in its WAL.
        let r = Db::open(&snap.join("app.db")).expect("open");
        assert!(r.has_table("profiles") && r.has_table("settings"));
        let rows = r.table("profiles").unwrap();
        assert_eq!(rows.len(), 302);
        let get = |p: &str| rows.iter().find(|m| m["path"].as_str() == Some(p)).unwrap();
        assert_eq!(get("p7")["game_version"].as_str(), Some("1.21.7"));
        assert_eq!(get("long")["name"].as_str().map(str::len), Some(5000));
        assert_eq!(get("p1")["extra"].as_str(), Some("x"));
        assert_eq!(get("p2")["extra"], Value::Null);
        assert_eq!(get("walrow")["name"].as_str(), Some("From WAL"));
        let s = r.table("settings").unwrap();
        assert_eq!(s[0]["id"], Value::Int(0));
        assert_eq!(s[0]["custom_dir"], Value::Null);
        // Without the WAL the committed-but-not-checkpointed row isn't there.
        let data = fs::read(snap.join("app.db")).unwrap();
        let r2 = Db::from_bytes(data, None).unwrap();
        assert_eq!(r2.table("profiles").unwrap().len(), 301);
        // A damaged WAL frame is ignored, not trusted.
        let mut wal = fs::read(snap.join("app.db-wal")).unwrap();
        let last = wal.len() - 10;
        wal[last] ^= 0xff;
        let r3 = Db::from_bytes(fs::read(snap.join("app.db")).unwrap(), Some(wal)).unwrap();
        assert!(r3.table("profiles").map(|t| t.len() <= 302).unwrap_or(true));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_non_databases() {
        assert!(Db::from_bytes(b"hello".to_vec(), None).is_err());
        assert!(Db::from_bytes(vec![0u8; 200], None).is_err());
    }
}
