//! "Download and update": finds the newest stable version of each tool on its official site, downloads the ZIP and
//! unpacks it in <root>\bin\<tool>\<version>, beside the versions already there (nothing is deleted or overwritten:
//! an existing my.ini or php.ini is kept, and a MySQL data folder is only created when it does not exist yet).
//!   PHP      windows.php.net releases.json, newest branch, thread-safe x64
//!   MySQL    newest 8.4.x (the LTS line, same as the data folder); 9.x is not offered, it would convert the data
//!   Node.js  nodejs.org/dist/index.json, newest LTS
//!   Mailpit  GitHub, latest release
//!   MongoDB  downloads.mongodb.org/current.json (mongo.rs); mongosh from GitHub and downloads.mongodb.com
//!   Caddy    GitHub caddyserver/caddy, the windows_amd64 ZIP (caddy.rs)
//!   Redis    the last Windows build (tporadowski 5.0.14.1)
//!   Apache   the newest Win64 build on apachelounge.com (the Windows builds the Apache project points to)
//! Linux and macOS (preview): Mailpit and Node.js come from the same official sites (their Linux / macOS builds);
//! Apache, PHP, MySQL, MariaDB, PostgreSQL, Redis and Memcached come from the package manager and are linked in
//! (platform/systools.rs).
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use crate::i18n::{Lang, tr};
use crate::services::{Shared, hide_window};
use crate::settings::Settings;

/// Built as 32-bit (x86)? Then the tools are fetched in their 32-bit builds, where one exists.
pub const X86: bool = cfg!(target_arch = "x86");

fn no_32bit(lang: Lang, tool: &str) -> String {
    format!("{tool} {}", tr(lang, "log_no_32bit"))
}

/// Tools the "Download and update" window offers: (id, name, folder under bin).
pub const TOOLS: &[(&str, &str, &str)] = &[
    ("apache", "Apache", "apache"),
    ("postgres", "PostgreSQL", "postgresql"),
    ("mariadb", "MariaDB", "mariadb"),
    ("php", "PHP", "php"),
    ("mysql", "MySQL", "mysql"),
    ("nodejs", "Node.js", "nodejs"),
    ("mailpit", "Mailpit", "mailpit"),
    ("redis", "Redis", "redis"),
    ("mongodb", "MongoDB", "mongodb"),
    ("mongosh", "MongoDB Shell (mongosh)", "mongosh"),
    ("caddy", "Caddy", "caddy"),
];

/// A version that can be downloaded.
#[derive(Clone, Debug)]
pub struct Latest {
    pub version: String,
    pub url: String,
    /// Folder inside the ZIP (empty when the files are at the top).
    pub top: String,
    /// Version folder it ends up in.
    pub folder: String,
}

/// Kinds with no fixed official Windows ZIP: the page to download them by hand.
pub fn page(key: &str) -> Option<&'static str> {
    match key {
        "nginx" => Some("https://nginx.org/en/download.html"),
        "memcached" if cfg!(windows) => Some("https://memcached.org/downloads"),
        _ => None,
    }
}

/// curl arguments that every site accepts (Apache Lounge wants a browser-like agent and its own page as referrer).
fn polite(c: &mut Command, url: &str) {
    c.args(["-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Ninho"]);
    if url.contains("apachelounge.com") {
        c.args(["-e", "https://www.apachelounge.com/download/"]);
    }
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    let mut c = Command::new(crate::platform::curl());
    c.args(["-L", "-f", "-s", "-S"]);
    polite(&mut c, url);
    c.arg(url);
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(out.stdout) } else { Err(format!("{url}: {}", String::from_utf8_lossy(&out.stderr).trim())) }
}

pub(crate) fn json(url: &str) -> Result<serde_json::Value, String> {
    serde_json::from_slice(&get(url)?).map_err(|e| format!("{url}: {e}"))
}

pub(crate) fn exists(url: &str) -> bool {
    let mut c = Command::new(crate::platform::curl());
    c.args(["-s", "-o", crate::platform::NULL_DEVICE, "-I", "-L", "-w", "%{http_code}", url]);
    hide_window(&mut c);
    c.output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "200").unwrap_or(false)
}

/// Sort key for "8.5.12-1", "v26.13.1"...: the numbers in order.
pub fn ver_key(v: &str) -> Vec<u32> {
    v.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
}

/// The version inside a folder name: "mysql-8.4.3-winx64" → "8.4.3", "node-v26.13.1-win-x64" → "26.13.1",
/// "php-8.5.12-1-Win32-vs17-x64" → "8.5.12", "redis-x64-5.0.14.1" → "5.0.14.1" (the first dotted number between dashes,
/// so the "64" of "x64" is not taken for the version).
pub fn short_ver(folder: &str) -> String {
    let dotted = folder
        .split(['-', '_', ' '])
        .map(|p| p.strip_prefix(['v', 'V']).unwrap_or(p))
        .find(|p| p.starts_with(|c: char| c.is_ascii_digit()) && p.contains('.') && p.chars().all(|c| c.is_ascii_digit() || c == '.'));
    if let Some(v) = dotted {
        return v.trim_end_matches('.').to_string();
    }
    let start = folder.find(|c: char| c.is_ascii_digit()).unwrap_or(0);
    let v: String = folder[start..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    if v.is_empty() { folder.to_string() } else { v.trim_end_matches('.').to_string() }
}

/// Is one of the installed version folders as new as `latest` (the version the official site announces)?
/// The folder names carry more than the version ("redis-x64-5.0.14.1", "php-8.5.12-1-Win32-vs17-x64"), so the
/// numeric version is taken out of each one first.
pub fn is_up_to_date(have: &[String], latest: &str) -> bool {
    // The site may announce a build too ("8.5.12-1" for PHP): only the version counts.
    let want = ver_key(&short_ver(latest));
    !want.is_empty() && have.iter().any(|h| ver_key(&short_ver(h)) >= want)
}

fn php_latest() -> Result<Latest, String> {
    let v = json("https://windows.php.net/downloads/releases/releases.json")?;
    let obj = v.as_object().ok_or("releases.json")?;
    let (_, e) = obj.iter().max_by_key(|(k, _)| ver_key(k)).ok_or("releases.json is empty")?;
    let entry = e.as_object().ok_or("releases.json")?;
    // Thread-safe x64 build (works with Apache and with the built-in server).
    let path = entry
        .iter()
        .find(|(k, _)| k.starts_with("ts-") && k.ends_with(if X86 { "-x86" } else { "-x64" }))
        .and_then(|(_, b)| b["zip"]["path"].as_str())
        .ok_or("no Windows x64 build in releases.json")?;
    Ok(Latest {
        version: e["version"].as_str().unwrap_or("?").to_string(),
        url: format!("https://windows.php.net/downloads/releases/{path}"),
        top: String::new(),
        folder: path.trim_end_matches(".zip").to_string(),
    })
}

fn mysql_latest(lang: Lang) -> Result<Latest, String> {
    if X86 {
        return Err(no_32bit(lang, "MySQL 8.4"));
    }
    // The current 8.4 builds sit in /Downloads (older ones move to /archives and some numbers are skipped), so try a
    // range at once and keep the highest that answers.
    let found: Vec<u32> = std::thread::scope(|sc| {
        let hs: Vec<_> = (3u32..=40)
            .map(|n| sc.spawn(move || exists(&format!("https://cdn.mysql.com/Downloads/MySQL-8.4/mysql-8.4.{n}-winx64.zip")).then_some(n)))
            .collect();
        hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });
    let n = found.into_iter().max().ok_or("no MySQL 8.4 build found on cdn.mysql.com")?;
    let name = format!("mysql-8.4.{n}-winx64");
    Ok(Latest { version: format!("8.4.{n}"), url: format!("https://cdn.mysql.com/Downloads/MySQL-8.4/{name}.zip"), top: name.clone(), folder: name })
}

fn node_latest() -> Result<Latest, String> {
    let v = json("https://nodejs.org/dist/index.json")?;
    // Newest LTS that has a Windows ZIP for this architecture (32-bit builds stopped after Node.js 22).
    let file = crate::nodever::index_key();
    let lts = v
        .as_array()
        .and_then(|a| a.iter().find(|r| r["lts"].as_str().is_some() && r["files"].as_array().is_some_and(|f| f.iter().any(|x| x.as_str() == Some(file.as_str())))))
        .ok_or("no LTS in index.json")?;
    let ver = lts["version"].as_str().ok_or("index.json")?;
    let name = format!("node-{ver}-{}", crate::nodever::ARCH);
    Ok(Latest { version: ver.trim_start_matches('v').to_string(), url: format!("https://nodejs.org/dist/{ver}/{name}.{}", crate::nodever::ARCHIVE), top: name.clone(), folder: name })
}

fn apache_latest() -> Result<Latest, String> {
    let page = String::from_utf8_lossy(&get("https://www.apachelounge.com/download/")?).into_owned();
    // href="/download/VS18/binaries/httpd-2.4.71-261112-Win64-VS18.zip" (and a win32 one for 32-bit Windows)
    let want = if X86 { "win32" } else { "win64" };
    let path = page
        .match_indices("/binaries/httpd-2.4.")
        .filter_map(|(at, _)| {
            let start = page[..at].rfind('"')? + 1;
            let end = at + page[at..].find('"')?;
            Some(&page[start..end])
        })
        .find(|p| p.to_ascii_lowercase().contains(want) && p.ends_with(".zip"))
        .ok_or("no Apache build for this Windows found on apachelounge.com")?;
    let name = path.rsplit('/').next().unwrap_or("").trim_end_matches(".zip").to_string();
    let version = name.trim_start_matches("httpd-").split('-').next().unwrap_or("?").to_string();
    // The ZIP holds an "Apache24" folder; it is renamed to the version folder.
    Ok(Latest { version, url: format!("https://www.apachelounge.com{path}"), top: "Apache24".into(), folder: name })
}

/// PostgreSQL: the current major from postgresql.org/versions.json, the Windows binaries ZIP from EnterpriseDB
/// (EDB's build number is probed: 1, 2, 3). The ZIP holds a "pgsql" folder, renamed to the version folder.
fn postgres_latest(lang: Lang) -> Result<Latest, String> {
    if X86 {
        return Err(no_32bit(lang, "PostgreSQL"));
    }
    let v = json("https://www.postgresql.org/versions.json")?;
    let cur = v.as_array().and_then(|a| a.iter().find(|r| r["current"].as_bool() == Some(true))).ok_or("no current PostgreSQL version")?;
    let major = cur["major"].as_str().map(String::from).or_else(|| cur["major"].as_u64().map(|n| n.to_string())).ok_or("versions.json")?;
    // versions.json gives the numbers as text ("18", "7"); accept numbers too.
    let minor = cur["latestMinor"].as_str().map(String::from).or_else(|| cur["latestMinor"].as_u64().map(|n| n.to_string())).ok_or("versions.json")?;
    let version = format!("{major}.{minor}");
    for build in 1..=3 {
        let url = format!("https://get.enterprisedb.com/postgresql/postgresql-{version}-{build}-windows-x64-binaries.zip");
        if exists(&url) {
            return Ok(Latest { version: version.clone(), url, top: "pgsql".into(), folder: format!("postgresql-{version}") });
        }
    }
    Err(format!("{} {version}", tr(lang, "log_no_pg_build")))
}

/// MariaDB: the newest stable long-term-support series from the official REST API, its newest patch release,
/// the winx64 ZIP from archive.mariadb.org.
fn mariadb_latest(lang: Lang) -> Result<Latest, String> {
    if X86 {
        return Err(no_32bit(lang, "MariaDB"));
    }
    let v = json("https://downloads.mariadb.org/rest-api/mariadb/")?;
    let series = v["major_releases"]
        .as_array()
        .ok_or("rest-api")?
        .iter()
        .filter(|r| r["release_status"].as_str() == Some("Stable") && r["release_support_type"].as_str().is_some_and(|t| t.contains("Long Term")))
        .filter_map(|r| r["release_id"].as_str())
        .max_by_key(|id| ver_key(id))
        .ok_or("no stable LTS MariaDB")?
        .to_string();
    let rel = json(&format!("https://downloads.mariadb.org/rest-api/mariadb/{series}/"))?;
    let version = rel["releases"].as_object().and_then(|o| o.keys().max_by_key(|k| ver_key(k)).cloned()).ok_or("no MariaDB release")?;
    let name = format!("mariadb-{version}-winx64");
    Ok(Latest { version: version.clone(), url: format!("https://archive.mariadb.org/mariadb-{version}/winx64-packages/{name}.zip"), top: name.clone(), folder: name })
}

/// The official Mailpit build for this system.
fn mailpit_asset() -> String {
    let arch = if X86 { "386" } else if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
    if cfg!(windows) {
        format!("mailpit-windows-{arch}.zip")
    } else if cfg!(target_os = "macos") {
        format!("mailpit-darwin-{arch}.tar.gz")
    } else {
        format!("mailpit-linux-{arch}.tar.gz")
    }
}

fn mailpit_latest() -> Result<Latest, String> {
    let v = json("https://api.github.com/repos/axllent/mailpit/releases/latest")?;
    let tag = v["tag_name"].as_str().ok_or("no release found")?;
    Ok(Latest {
        version: tag.trim_start_matches('v').to_string(),
        url: format!("https://github.com/axllent/mailpit/releases/download/{tag}/{}", mailpit_asset()),
        top: String::new(),
        folder: tag.trim_start_matches('v').to_string(),
    })
}

/// The newest stable version of a tool, read from its official site.
pub fn latest(lang: Lang, tool: &str) -> Result<Latest, String> {
    // Linux and macOS: the version the package manager installed (or what to install), see platform/systools.rs.
    #[cfg(unix)]
    if crate::platform::systools::from_system(tool) {
        let version = crate::platform::systools::latest(lang, tool)?;
        let folder = crate::platform::systools::folder_name(tool, &version);
        return Ok(Latest { version, url: String::new(), top: String::new(), folder });
    }
    match tool {
        "php" => php_latest(),
        "apache" => apache_latest(),
        "postgres" => postgres_latest(lang),
        "mariadb" => mariadb_latest(lang),
        "mysql" => mysql_latest(lang),
        "nodejs" => node_latest(),
        "mailpit" => mailpit_latest(),
        "mongodb" => crate::mongo::latest_server(lang),
        "mongosh" => crate::mongo::latest_shell(lang),
        "caddy" => crate::caddy::latest(lang),
        "redis" if X86 => Err(no_32bit(lang, "Redis")),
        "redis" => Ok(Latest {
            version: "5.0.14.1".into(),
            url: "https://github.com/tporadowski/redis/releases/download/v5.0.14.1/Redis-x64-5.0.14.1.zip".into(),
            top: String::new(),
            folder: "redis-x64-5.0.14.1".into(),
        }),
        other => Err(format!("{other}: {}", tr(lang, "log_no_auto_download"))),
    }
}

/// Version folders of a tool already in <root>\bin (any folder with files, newest last).
pub fn installed(root: &str, dir: &str) -> Vec<String> {
    crate::services::memo(format!("installed|{root}|{dir}"), || installed_now(root, dir))
}

fn installed_now(root: &str, dir: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(Path::new(root).join("bin").join(dir))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter(|e| {
                    let p = e.path();
                    let x = crate::platform::exe;
                    [p.join(x("php")), p.join("bin").join(x("mysqld")), p.join(x("node")), p.join(x("mailpit")), p.join(x("redis-server")), p.join("bin").join(x("httpd")), p.join("bin").join(x("postgres")), p.join("bin").join(x("mariadbd")), p.join("bin").join(x("mongod")), p.join("bin").join(x("mongosh")), p.join(x("caddy"))]
                        .iter()
                        .any(|f| f.is_file())
                        || (cfg!(unix) && p.join("memcached").is_file())
                })
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|s| ver_key(s));
    v
}

fn download(shared: &Shared, lang: Lang, id: &str, url: &str, to: &Path) -> Result<(), String> {
    let mut c = Command::new(crate::platform::curl());
    c.args(["-L", "-f", "-s", "-S", "--retry", "2"]);
    polite(&mut c, url);
    c.arg("-o").arg(to).arg(url);
    hide_window(&mut c);
    let mut child = c.spawn().map_err(|e| e.to_string())?;
    loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            if st.success() {
                return Ok(());
            }
            return Err(format!("{} {url}", tr(lang, "log_download_failed")));
        }
        let mb = std::fs::metadata(to).map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
        shared.installing.lock().unwrap().insert(id.into(), format!("{mb:.0} MB"));
        std::thread::sleep(Duration::from_millis(700));
    }
}

fn unzip(zip: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut c = Command::new(crate::platform::tar());
    c.arg("-xf").arg(zip).arg("-C").arg(dest);
    // The PostgreSQL ZIP carries pgAdmin (811 MB) and StackBuilder: Ninho does not need them.
    if zip.to_string_lossy().contains("postgresql-") {
        c.args(["--exclude", "pgsql/pgAdmin 4", "--exclude", "pgsql/StackBuilder"]);
    }
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

/// The one folder a ZIP unpacked into `dir` (its name).
fn sole_dir(dir: &Path) -> Result<String, String> {
    let names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if names.len() == 1 { Ok(names.into_iter().next().unwrap()) } else { Err(format!("{}: {} folders at the top of the ZIP", dir.display(), names.len())) }
}

/// The my.ini (or php.ini) of the newest other version folder, to carry the person's settings over.
fn previous_file(kind_dir: &Path, except: &str, file: &str) -> Option<PathBuf> {
    let mut vs: Vec<String> = std::fs::read_dir(kind_dir).ok()?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != except).collect();
    vs.sort_by_key(|s| ver_key(s));
    vs.iter().rev().map(|v| kind_dir.join(v).join(file)).find(|p| p.is_file())
}

/// What to do after unpacking, so the new version can start right away.
fn finish(shared: &Shared, s: &Settings, tool: &str, kind_dir: &Path, vdir: &Path) {
    let folder = vdir.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let lang = s.language;
    match tool {
        "mysql" => {
            let ini = vdir.join("my.ini");
            if !ini.is_file() {
                // Same settings and SAME data folder as the version used until now (8.4.x reads 8.4 data).
                if let Some(old) = previous_file(kind_dir, &folder, "my.ini") {
                    let _ = std::fs::copy(&old, &ini);
                    shared.say(format!("MySQL: {} {}", tr(lang, "log_myini_copied"), old.parent().unwrap().display()));
                } else {
                    let data = format!(r"{}/data/mysql-8.4", s.root.replace('\\', "/"));
                    let _ = std::fs::write(&ini, format!("[client]\r\nport=3306\r\n\r\n[mysqld]\r\ndatadir=\"{data}\"\r\nport=3306\r\nmax_allowed_packet=512M\r\nexplicit_defaults_for_timestamp=1\r\n"));
                    shared.say(format!("MySQL: {} {data}", tr(lang, "log_myini_created")));
                }
            } else {
                shared.say(format!("MySQL: {}", tr(lang, "log_myini_kept")));
            }
            let data = std::fs::read_to_string(&ini).ok().and_then(|t| {
                t.lines().find(|l| l.trim().to_ascii_lowercase().starts_with("datadir")).and_then(|l| l.split_once('=')).map(|(_, v)| crate::platform::native_slashes(v.trim().trim_matches('"')))
            });
            if let Some(data) = data {
                if Path::new(&data).join("mysql").is_dir() {
                    shared.say(format!("MySQL: {} {data}", tr(lang, "log_data_existing")));
                } else if !Path::new(&data).exists() {
                    let mut c = Command::new(vdir.join("bin").join(crate::platform::exe("mysqld")));
                    c.args(["--no-defaults", "--initialize-insecure", &format!("--basedir={}", crate::services::basedir(vdir, "bin/mysqld").display()), &format!("--datadir={data}")]);
                    hide_window(&mut c);
                    let out = c.output();
                    let ok = out.as_ref().map(|o| o.status.success()).unwrap_or(false);
                    shared.say(if ok { format!("MySQL: {} {data}", tr(lang, "log_data_new")) } else { format!("MySQL: {} {data}", tr(lang, "log_data_create_failed")) });
                    // Linux and macOS: say why (Ubuntu's AppArmor profile for mysqld only allows /var/lib/mysql...).
                    #[cfg(unix)]
                    if let (false, Ok(o)) = (ok, &out) {
                        let why: Vec<String> = String::from_utf8_lossy(&o.stderr).lines().filter(|l| l.contains("ERROR") && !l.contains("Aborting")).map(|l| l.trim().to_string()).collect();
                        shared.say(format!("MySQL: {}", why.join(" | ")));
                        if Path::new("/etc/apparmor.d/usr.sbin.mysqld").is_file() {
                            shared.say(format!("MySQL: {}", tr(lang, "unix_mysql_apparmor")));
                        }
                    }
                }
            }
        }
        #[cfg(unix)]
        "nodejs" => crate::nodever::link_bins(vdir),
        "apache" => match crate::services::apache_setup(s, vdir) {
            Ok(()) => shared.say(format!("Apache: {}", tr(lang, "log_httpd_ok"))),
            Err(e) => shared.say(format!("Apache: {} {e}", tr(lang, "log_httpd_failed"))),
        },
        "postgres" | "mariadb" => {
            let c = s.svc(tool).clone();
            let data = crate::services::engine_data(s, &c, vdir);
            if crate::services::engine_init(shared, lang, tool, vdir, &data, c.port) {
                shared.say(format!("{}: {} {data}", if tool == "postgres" { "PostgreSQL" } else { "MariaDB" }, tr(lang, "log_data_folder")));
            }
        }
        "mongodb" => {
            // The data folder is made now so the first start finds it.
            let c = s.svc(tool).clone();
            let data = crate::services::engine_data(s, &c, vdir);
            if std::fs::create_dir_all(&data).is_ok() {
                shared.say(format!("MongoDB: {} {data}", tr(lang, "log_data_folder")));
            }
        }
        "php" => {
            let ini = vdir.join("php.ini");
            if !ini.is_file() {
                if vdir.join("php.ini-development").is_file() {
                    let _ = std::fs::copy(vdir.join("php.ini-development"), &ini);
                    // The extensions most projects need (WordPress, Laravel...).
                    if let Ok(t) = std::fs::read_to_string(&ini) {
                        let mut t = t.replace(";extension_dir = \"ext\"", "extension_dir = \"ext\"");
                        for ext in ["curl", "fileinfo", "gd", "intl", "mbstring", "mysqli", "openssl", "pdo_mysql", "pdo_sqlite", "sqlite3", "zip"] {
                            t = t.replace(&format!(";extension={ext}\r\n"), &format!("extension={ext}\r\n")).replace(&format!(";extension={ext}\n"), &format!("extension={ext}\n"));
                        }
                        let _ = std::fs::write(&ini, t);
                    }
                    shared.say(format!("PHP: {}", tr(lang, "log_phpini_created")));
                }
            }
        }
        _ => {}
    }
}

/// Download and unpack the newest stable version of a tool. `id` is the screen row that shows the progress.
pub fn install(shared: Arc<Shared>, s: Settings, tool: String, id: String) {
    let lang = s.language;
    let (name, dir) = TOOLS.iter().find(|t| t.0 == tool).map(|t| (t.1, t.2)).unwrap_or((tool.as_str(), tool.as_str()));
    let kind_dir = Path::new(&s.root).join("bin").join(dir);
    let tmp = Path::new(&s.root).join("tmp").join("ninho-downloads");
    let _ = std::fs::create_dir_all(&tmp);
    shared.installing.lock().unwrap().insert(id.clone(), "…".into());
    let run = || -> Result<(String, PathBuf), String> {
        // Linux and macOS: the servers come from the package manager; Ninho links them (platform/systools.rs).
        #[cfg(unix)]
        if crate::platform::systools::from_system(&tool) {
            return crate::platform::systools::link(&shared, &s, &tool, &kind_dir);
        }
        shared.say(format!("{name}: {}", tr(lang, "log_looking_newest")));
        let l = latest(lang, &tool)?;
        if kind_dir.join(&l.folder).is_dir() && !installed(&s.root, dir).is_empty() && installed(&s.root, dir).contains(&l.folder) {
            return Err(format!("{} {}", tr(lang, "log_version_present"), l.version));
        }
        shared.say(format!("{name} {}: {} {}", l.version, tr(lang, "log_downloading"), l.url));
        let zip = tmp.join(l.url.rsplit('/').next().unwrap_or("download.zip"));
        download(&shared, lang, &id, &l.url, &zip)?;
        shared.installing.lock().unwrap().insert(id.clone(), "unpacking".into());
        // Files at the top of the ZIP → into the version folder. A top folder with the version's name → into the tool's
        // folder. A top folder with another name (Apache24) → unpacked aside, then moved to the version folder.
        let vdir = kind_dir.join(&l.folder);
        if l.top.is_empty() {
            unzip(&zip, &vdir)?;
        } else if l.top == l.folder {
            unzip(&zip, &kind_dir)?;
        } else {
            let aside = tmp.join(format!("unpack-{}", l.folder));
            let _ = std::fs::remove_dir_all(&aside);
            unzip(&zip, &aside)?;
            std::fs::create_dir_all(&kind_dir).map_err(|e| e.to_string())?;
            // "*": the ZIP has one folder at the top and its name does not matter (MongoDB names it by build).
            let top = if l.top == "*" { sole_dir(&aside)? } else { l.top.clone() };
            std::fs::rename(aside.join(&top), &vdir).map_err(|e| format!("{} {}: {e}", tr(lang, "log_move_failed"), l.top))?;
            let _ = std::fs::remove_dir_all(&aside);
        }
        let _ = std::fs::remove_file(&zip);
        Ok((l.version, vdir))
    };
    match run() {
        Ok((version, vdir)) => {
            finish(&shared, &s, &tool, &kind_dir, &vdir);
            shared.just_installed.lock().unwrap().push(tool.clone());
            shared.say(format!("{name} {version}: {} {}", tr(lang, "log_installed_in"), vdir.display()));
        }
        Err(e) => shared.say(format!("{name}: {e}")),
    }
    shared.installing.lock().unwrap().remove(&id);
    shared.refresh();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn version_out_of_a_folder_name() {
        assert_eq!(short_ver("redis-x64-5.0.14.1"), "5.0.14.1");
        assert_eq!(short_ver("php-8.5.12-1-Win32-vs17-x64"), "8.5.12");
        assert_eq!(short_ver("mysql-8.4.3-winx64"), "8.4.3");
        assert_eq!(short_ver("node-v26.13.1-win-x64"), "26.13.1");
        assert_eq!(short_ver("httpd-2.4.71-261112-Win64-VS18"), "2.4.71");
        assert_eq!(short_ver("postgresql-18.1"), "18.1");
        assert_eq!(short_ver("1.27.0"), "1.27.0");
    }

    #[test]
    fn installed_folder_counts_as_the_announced_version() {
        assert!(is_up_to_date(&have(&["redis-x64-5.0.14.1"]), "5.0.14.1"));
        assert!(is_up_to_date(&have(&["php-8.5.12-1-Win32-vs17-x64"]), "8.5.12"));
        assert!(is_up_to_date(&have(&["php-8.5.12-1-Win32-vs17-x64"]), "8.5.12-1"));
        assert!(is_up_to_date(&have(&["mysql-8.4.3-winx64"]), "8.4.3"));
        assert!(is_up_to_date(&have(&["httpd-2.4.71-261112-Win64-VS18"]), "2.4.71"));
        // An older folder is not enough, a newer one is.
        assert!(!is_up_to_date(&have(&["php-8.5.11-1-Win32-vs17-x64"]), "8.5.12"));
        assert!(!is_up_to_date(&have(&["mysql-8.4.2-winx64"]), "8.4.3"));
        assert!(is_up_to_date(&have(&["php-8.4.1-Win32-vs17-x64", "php-8.5.12-1-Win32-vs17-x64"]), "8.5.12"));
        assert!(!is_up_to_date(&[], "8.5.12"));
    }
}
