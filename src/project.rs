//! `ninho.project.json`: an optional file in a site's folder that says what the project needs (PHP and Node.js version,
//! servers, its own port, its database). The person commits it to the repo, so everyone who opens the project in Ninho
//! gets the same setup. A bad file never breaks anything: the error is shown on the site's row.
//!
//! ```json
//! { "php": "8.3", "node": "22", "servers": ["mysql", "redis"], "port": 8100, "database": "shop" }
//! ```
//! Every field is optional and unknown fields are ignored (and kept when Ninho writes the file again).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use serde_json::Value;

use crate::install::short_ver;
use crate::services::{SERVICES, Shared, State};
use crate::settings::{Settings, SiteCfg, name_ok, port_ok};

pub const FILE: &str = "ninho.project.json";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    /// "8.3": the PHP the site runs on (major.minor; a patch is ignored).
    pub php: Option<String>,
    /// "22": the Node.js version when the folder has no `.nvmrc`.
    pub node: Option<String>,
    /// Kinds of server the project uses ("mysql", "redis"...): started with the site.
    pub servers: Vec<String>,
    /// Port of the site's own PHP server.
    pub port: Option<u16>,
    /// Database the project needs on its MySQL / MariaDB.
    pub database: Option<String>,
}

#[derive(Deserialize)]
struct Raw {
    php: Option<Value>,
    node: Option<Value>,
    servers: Option<Vec<String>>,
    port: Option<u64>,
    database: Option<String>,
}

/// "8.3" or 8.3 (a JSON number works too, but "8.10" must be a string): digits and dots, no "v".
fn version_field(name: &str, v: Option<Value>) -> Result<Option<String>, String> {
    let text = match v {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) => s,
        Some(Value::Number(n)) => n.to_string(),
        Some(_) => return Err(format!("\"{name}\" must be a version like \"8.3\"")),
    };
    let t = text.trim().trim_start_matches(['v', 'V']);
    if t.is_empty() {
        return Ok(None);
    }
    if !t.chars().all(|c| c.is_ascii_digit() || c == '.') || t.starts_with('.') || t.ends_with('.') || t.contains("..") {
        return Err(format!("\"{name}\": \"{text}\" is not a version (use numbers like \"8.3\")"));
    }
    Ok(Some(t.to_string()))
}

/// Read the text of a project file.
pub fn parse(text: &str) -> Result<Project, String> {
    let raw: Raw = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("invalid {FILE}: {e}"))?;
    let port = match raw.port {
        None | Some(0) => None,
        Some(p) if p <= 65535 && port_ok(p as u32) => Some(p as u16),
        Some(p) => return Err(format!("\"port\": {p} is not a valid port")),
    };
    let database = match raw.database.map(|d| d.trim().to_string()) {
        None => None,
        Some(d) if d.is_empty() => None,
        Some(d) if name_ok(&d) && d.len() <= 64 && !d.contains('.') => Some(d),
        Some(d) => return Err(format!("\"database\": \"{d}\" is not a valid name (letters, digits, _ -)")),
    };
    let mut servers: Vec<String> = Vec::new();
    for s in raw.servers.unwrap_or_default() {
        let s = s.trim().to_ascii_lowercase();
        if !s.is_empty() && !servers.contains(&s) {
            servers.push(s);
        }
    }
    Ok(Project { php: version_field("php", raw.php)?, node: version_field("node", raw.node)?, servers, port, database })
}

/// The project file of a site folder: None when there is none, Some(Err) when it cannot be used.
pub fn load(dir: &Path) -> Option<Result<Project, String>> {
    let path = dir.join(FILE);
    if !path.is_file() {
        return None;
    }
    Some(std::fs::read_to_string(&path).map_err(|e| format!("cannot read {FILE}: {e}")).and_then(|t| parse(&t)))
}

/// The Node.js version a project file asks for (used when the folder has no `.nvmrc`).
pub fn node_of(dir: &Path) -> Option<String> {
    load(dir)?.ok()?.node
}

/// Kinds in `servers` that Ninho does not know ("mysq", "php"...). The rest is still used.
pub fn unknown_servers(p: &Project) -> Vec<String> {
    p.servers.iter().filter(|k| k.as_str() == "php" || !SERVICES.iter().any(|d| d.key == k.as_str())).cloned().collect()
}

// ---- versions ----

fn numbers(v: &str) -> Vec<u32> {
    v.split('.').map_while(|p| p.parse().ok()).collect()
}

/// The installed version folder (newest patch) for a wanted "8.3" / "8.3.12" / "8": the same major.minor (a lone major
/// matches any minor). `folders` are names like "php-8.3.36-Win32-vs16-x64".
pub fn match_version(wanted: &str, folders: &[String]) -> Option<String> {
    let want = numbers(wanted);
    if want.is_empty() {
        return None;
    }
    let depth = want.len().min(2);
    folders
        .iter()
        .filter_map(|f| {
            let have = numbers(&short_ver(f));
            (have.len() >= depth && have[..depth] == want[..depth]).then_some((have, f))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, f)| f.clone())
}

/// What the project's PHP asks resolved to on this PC.
#[derive(Clone, Debug, PartialEq)]
pub enum PhpStatus {
    /// The project says nothing about PHP.
    Free,
    /// Installed: the version folder.
    Pinned(String),
    /// Not installed: the version the project asks for ("8.3").
    Missing(String),
}

pub fn php_status(p: &Project, installed: &[String]) -> PhpStatus {
    match &p.php {
        None => PhpStatus::Free,
        Some(w) => match match_version(w, installed) {
            Some(f) => PhpStatus::Pinned(f),
            None => PhpStatus::Missing(w.clone()),
        },
    }
}

/// The site's settings with the project's PHP and port on top (kept only in memory, never saved to ninho.json).
pub fn apply(cfg: &SiteCfg, p: &Project, installed: &[String]) -> SiteCfg {
    let mut c = cfg.clone();
    if let PhpStatus::Pinned(f) = php_status(p, installed) {
        c.php = f;
    }
    if let Some(port) = p.port {
        c.port = port;
    }
    c
}

// ---- servers ----

/// Ids of the servers to start with the site: the first server of each kind listed, and what is wrong with the list.
pub fn server_ids(s: &Settings, p: &Project) -> Vec<String> {
    p.servers.iter().filter_map(|k| s.services.iter().find(|c| &c.key == k && c.id == c.key).or_else(|| s.services.iter().find(|c| &c.key == k))).map(|c| c.id.clone()).collect()
}

/// The MySQL / MariaDB server that holds the project's database: the first one listed, else MySQL.
pub fn db_server(s: &Settings, p: &Project) -> Option<String> {
    p.database.as_ref()?;
    let kind = p.servers.iter().find(|k| matches!(k.as_str(), "mysql" | "mariadb")).cloned().unwrap_or_else(|| "mysql".into());
    s.services.iter().find(|c| c.key == kind && c.id == c.key).or_else(|| s.services.iter().find(|c| c.key == kind)).map(|c| c.id.clone())
}

// ---- writing ----

/// The text of the file: known fields first in a fixed order, then the unknown ones the file already had.
pub fn render(p: &Project, extra: &serde_json::Map<String, Value>) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut add = |k: &str, v: Value| lines.push(format!("  \"{k}\": {}", serde_json::to_string(&v).unwrap_or_default()));
    if let Some(v) = &p.php {
        add("php", Value::String(v.clone()));
    }
    if let Some(v) = &p.node {
        add("node", Value::String(v.clone()));
    }
    if !p.servers.is_empty() {
        add("servers", Value::Array(p.servers.iter().cloned().map(Value::String).collect()));
    }
    if let Some(v) = p.port {
        add("port", Value::from(v));
    }
    if let Some(v) = &p.database {
        add("database", Value::String(v.clone()));
    }
    let mut keys: Vec<&String> = extra.keys().filter(|k| !matches!(k.as_str(), "php" | "node" | "servers" | "port" | "database")).collect();
    keys.sort();
    for k in keys {
        let v = serde_json::to_string(&extra[k]).unwrap_or_default();
        lines.push(format!("  {}: {v}", serde_json::to_string(k).unwrap_or_default()));
    }
    if lines.is_empty() { "{}\n".into() } else { format!("{{\n{}\n}}\n", lines.join(",\n")) }
}

/// Write the project file of a site folder. A file that is already there keeps the fields Ninho does not know; one that
/// is not valid JSON is not overwritten.
pub fn save(dir: &Path, p: &Project) -> Result<(), String> {
    let path = dir.join(FILE);
    let extra = match std::fs::read_to_string(&path) {
        Ok(t) => match serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')) {
            Ok(Value::Object(m)) => m,
            _ => return Err(format!("{FILE} is not valid JSON: fix or delete it first")),
        },
        Err(_) => serde_json::Map::new(),
    };
    std::fs::write(&path, render(p, &extra)).map_err(|e| format!("cannot write {FILE}: {e}"))
}

/// "8.3" for a version folder ("php-8.3.36-Win32-vs16-x64").
pub fn major_minor(folder: &str) -> String {
    short_ver(folder).split('.').take(2).collect::<Vec<_>>().join(".")
}

// ---- what the Sites tab keeps about each project ----

#[derive(Clone, Debug, PartialEq)]
pub enum DbState {
    /// Not asked yet (or the server is off).
    Unknown,
    Checking,
    Exists,
    Missing,
    Failed(String),
}

struct Entry {
    stamp: Option<(SystemTime, u64)>,
    read_at: Instant,
    parsed: Option<Result<Project, String>>,
    db: Arc<Mutex<DbState>>,
    db_asked: Option<Instant>,
}

/// The project files of the sites, read again only when they change, and the database checks.
#[derive(Default)]
pub struct Tracker {
    map: HashMap<String, Entry>,
}

impl Tracker {
    /// The project file of a site (None = no file). Looks at the disk at most every 2 seconds.
    pub fn get(&mut self, folder: &str, dir: &Path) -> Option<Result<Project, String>> {
        let e = self.map.entry(folder.to_string()).or_insert_with(|| Entry {
            stamp: None,
            read_at: Instant::now() - Duration::from_secs(60),
            parsed: None,
            db: Arc::new(Mutex::new(DbState::Unknown)),
            db_asked: None,
        });
        if e.read_at.elapsed() >= Duration::from_secs(2) {
            e.read_at = Instant::now();
            let stamp = std::fs::metadata(dir.join(FILE)).ok().map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
            if stamp != e.stamp || e.parsed.is_none() && stamp.is_some() {
                e.stamp = stamp;
                e.parsed = if stamp.is_some() { load(dir) } else { None };
                *e.db.lock().unwrap() = DbState::Unknown;
                e.db_asked = None;
            }
        }
        e.parsed.clone()
    }

    /// Is the project's database there? Asks the running server in the background (again every 15 s while it runs).
    pub fn db_state(&mut self, s: &Settings, shared: &Arc<Shared>, folder: &str, p: &Project) -> Option<(String, String, DbState)> {
        let name = p.database.clone()?;
        let id = db_server(s, p)?;
        let e = self.map.get_mut(folder)?;
        if shared.state(&id) != State::Running {
            *e.db.lock().unwrap() = DbState::Unknown;
            e.db_asked = None;
            return Some((id, name, DbState::Unknown));
        }
        let stale = e.db_asked.is_none_or(|t| t.elapsed() >= Duration::from_secs(15));
        let busy = *e.db.lock().unwrap() == DbState::Checking;
        if stale && !busy {
            e.db_asked = Some(Instant::now());
            let (slot, s2, id2, name2) = (e.db.clone(), s.clone(), id.clone(), name.clone());
            let prev = slot.lock().unwrap().clone();
            // Keep showing the last answer while asking again; only the first time is "checking".
            if prev == DbState::Unknown {
                *slot.lock().unwrap() = DbState::Checking;
            }
            std::thread::spawn(move || {
                let r = crate::mysqlpw::list_databases(&s2, &id2, "");
                let st = match r {
                    Ok(list) if list.iter().any(|d| d.eq_ignore_ascii_case(&name2)) => DbState::Exists,
                    Ok(_) => DbState::Missing,
                    Err(e) => DbState::Failed(e),
                };
                *slot.lock().unwrap() = st;
            });
        }
        let st = e.db.lock().unwrap().clone();
        Some((id, name, st))
    }

    /// The person made the database: show it as there at once.
    pub fn mark_created(&mut self, folder: &str) {
        if let Some(e) = self.map.get(folder) {
            *e.db.lock().unwrap() = DbState::Exists;
        }
    }
}

/// Create the project's database (the person agreed by pressing the button).
pub fn create_database(s: &Settings, id: &str, name: &str) -> Result<(), String> {
    if !name_ok(name) {
        return Err(format!("invalid database name '{name}'"));
    }
    crate::services::mysql_exec(s, id, &format!("CREATE DATABASE IF NOT EXISTS `{name}` CHARACTER SET utf8mb4"), "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_the_documented_file() {
        let p = parse(r#"{ "php": "8.3", "node": "22", "servers": ["mysql","Redis"], "port": 8100, "database": "shop" }"#).unwrap();
        assert_eq!(p, Project { php: Some("8.3".into()), node: Some("22".into()), servers: vec!["mysql".into(), "redis".into()], port: Some(8100), database: Some("shop".into()) });
    }

    #[test]
    fn parsing_is_lenient_and_strict_where_it_matters() {
        // Empty object, unknown fields, a BOM, numbers for versions, duplicates.
        assert_eq!(parse("{}").unwrap(), Project::default());
        assert_eq!(parse("\u{feff}{\"future\": [1,2], \"php\": 8.3}").unwrap().php.as_deref(), Some("8.3"));
        assert_eq!(parse(r#"{"servers":["mysql","mysql",""]}"#).unwrap().servers, vec!["mysql"]);
        assert_eq!(parse(r#"{"php":"v8.4","node":"v22"}"#).unwrap().node.as_deref(), Some("22"));
        assert_eq!(parse(r#"{"port":0}"#).unwrap().port, None);
        // Errors name the problem and never panic.
        for bad in ["", "[]", "{", "null", "{\"php\": true}", "{\"php\": \"eight\"}", "{\"php\": \"8..3\"}", "{\"port\": 70000}", "{\"port\": -1}", "{\"port\": \"x\"}", "{\"database\": \"a b\"}", "{\"database\": \"x`; drop\"}", "{\"servers\": \"mysql\"}", "{\"servers\": [1]}"] {
            let e = parse(bad).expect_err(bad);
            assert!(!e.is_empty());
        }
        assert!(parse("{\"port\": 70000}").unwrap_err().contains("port"));
    }

    #[test]
    fn unknown_servers_are_listed() {
        let p = parse(r#"{"servers":["mysql","mysq","php"]}"#).unwrap();
        assert_eq!(unknown_servers(&p), vec!["mysq", "php"]);
    }

    #[test]
    fn version_matching() {
        let inst = folders(&["php-8.3.36-Win32-vs16-x64", "php-8.3.9-Win32-vs16-x64", "php-8.5.12-1-Win32-vs17-x64"]);
        assert_eq!(match_version("8.3", &inst).as_deref(), Some("php-8.3.36-Win32-vs16-x64"));
        assert_eq!(match_version("8.3.12", &inst).as_deref(), Some("php-8.3.36-Win32-vs16-x64"));
        assert_eq!(match_version("8.5", &inst).as_deref(), Some("php-8.5.12-1-Win32-vs17-x64"));
        assert_eq!(match_version("8", &inst).as_deref(), Some("php-8.5.12-1-Win32-vs17-x64"));
        assert_eq!(match_version("8.4", &inst), None);
        assert_eq!(match_version("7.4", &inst), None);
        assert_eq!(match_version("", &inst), None);
        assert_eq!(match_version("8.3", &[]), None);
        // 8.10 is newer than 8.9, and 8.1 is not 8.10.
        let more = folders(&["php-8.9.1-x", "php-8.10.0-x", "php-8.1.30-x"]);
        assert_eq!(match_version("8.10", &more).as_deref(), Some("php-8.10.0-x"));
        assert_eq!(match_version("8.1", &more).as_deref(), Some("php-8.1.30-x"));
        assert_eq!(match_version("8", &more).as_deref(), Some("php-8.10.0-x"));
    }

    #[test]
    fn project_overrides_the_site() {
        let inst = folders(&["php-8.3.36-Win32-vs16-x64"]);
        let cfg = SiteCfg { folder: "shop".into(), php: "php-8.5.12".into(), port: 8101 };
        let p = Project { php: Some("8.3".into()), port: Some(8100), ..Default::default() };
        let c = apply(&cfg, &p, &inst);
        assert_eq!((c.php.as_str(), c.port), ("php-8.3.36-Win32-vs16-x64", 8100));
        // Missing PHP keeps the site's own; a project with no PHP or port changes nothing.
        let m = Project { php: Some("7.4".into()), ..Default::default() };
        assert_eq!(php_status(&m, &inst), PhpStatus::Missing("7.4".into()));
        assert_eq!(apply(&cfg, &m, &inst).php, "php-8.5.12");
        assert_eq!(apply(&cfg, &Project::default(), &inst).port, 8101);
        assert_eq!(php_status(&Project::default(), &inst), PhpStatus::Free);
    }

    #[test]
    fn servers_map_to_ids() {
        let mut s = Settings::default();
        let p = Project { servers: vec!["mysql".into(), "redis".into(), "nope".into()], database: Some("shop".into()), ..Default::default() };
        assert_eq!(server_ids(&s, &p), vec!["mysql", "redis"]);
        assert_eq!(db_server(&s, &p).as_deref(), Some("mysql"));
        let maria = Project { servers: vec!["mariadb".into()], database: Some("shop".into()), ..Default::default() };
        assert_eq!(db_server(&s, &maria).as_deref(), Some("mariadb"));
        assert_eq!(db_server(&s, &Project::default()), None);
        let id = s.next_id("mysql");
        let mut extra = crate::settings::ServiceCfg::base("mysql");
        extra.id = id;
        s.services.push(extra);
        assert_eq!(server_ids(&s, &p)[0], "mysql");
    }

    #[test]
    fn writing_and_reading_back() {
        let dir = std::env::temp_dir().join(format!("ninho-proj-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = Project { php: Some("8.3".into()), node: Some("22".into()), servers: vec!["mysql".into(), "redis".into()], port: Some(8100), database: Some("shop".into()) };
        save(&dir, &p).unwrap();
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(text.starts_with("{\n  \"php\": \"8.3\",\n  \"node\": \"22\",\n  \"servers\": [\"mysql\",\"redis\"],\n  \"port\": 8100,\n  \"database\": \"shop\"\n}"), "{text}");
        assert_eq!(load(&dir).unwrap().unwrap(), p);
        // Saving again keeps what Ninho does not know.
        std::fs::write(dir.join(FILE), r#"{"php":"8.1","team":{"owner":"ana"},"database":"old"}"#).unwrap();
        save(&dir, &Project { php: Some("8.4".into()), ..Default::default() }).unwrap();
        let again = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(again.contains("\"php\": \"8.4\"") && again.contains("\"team\": {\"owner\":\"ana\"}") && !again.contains("old"), "{again}");
        assert_eq!(load(&dir).unwrap().unwrap().php.as_deref(), Some("8.4"));
        // A file that is not JSON is never overwritten, and is reported, not a crash.
        std::fs::write(dir.join(FILE), "{ broken").unwrap();
        assert!(save(&dir, &p).is_err());
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap(), "{ broken");
        assert!(load(&dir).unwrap().is_err());
        assert_eq!(node_of(&dir), None);
        std::fs::remove_file(dir.join(FILE)).unwrap();
        assert!(load(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_project_renders_an_empty_object() {
        assert_eq!(render(&Project::default(), &serde_json::Map::new()), "{}\n");
        assert_eq!(major_minor("php-8.3.36-Win32-vs16-x64"), "8.3");
    }
}
