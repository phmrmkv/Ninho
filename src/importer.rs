//! Bring sites and databases over from XAMPP, WampServer or Laragon.
//! The old install holds the person's only copy: everything here only READS from it (folders are copied, never moved;
//! databases are dumped with the old install's own mysqldump). Everything blocks, so the caller runs it on a thread.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::services::{hide_window, port_open, version_dir};
use crate::settings::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Xampp,
    Wamp,
    Laragon,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Xampp => "XAMPP",
            Kind::Wamp => "WampServer",
            Kind::Laragon => "Laragon",
        }
    }
    /// Lowercase word for file names.
    fn slug(self) -> &'static str {
        match self {
            Kind::Xampp => "xampp",
            Kind::Wamp => "wamp",
            Kind::Laragon => "laragon",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Found {
    pub kind: Kind,
    pub root: PathBuf,
    /// htdocs (XAMPP) or www.
    pub www: PathBuf,
    /// Site folders in `www`, without the install's own pages, sorted A-Z.
    pub sites: Vec<String>,
    /// Folder with mysqld/mariadbd and mysqldump of that install.
    pub mysql_bin: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
}

/// Pages XAMPP puts in htdocs; they are not the person's sites. Lowercase.
const XAMPP_OWN: [&str; 8] =
    ["dashboard", "img", "webalizer", "xampp", "favicon.ico", "index.php", "applications.html", "bitnami.css"];
/// Same for WampServer's www.
const WAMP_OWN: [&str; 6] = ["wampthemes", "wamplangues", "wamp_tools", "add_vhost.php", "index.php", "favicon.ico"];
/// Schemas that belong to the server (phpmyadmin is XAMPP's config storage).
const SYSTEM_DBS: [&str; 5] = ["mysql", "sys", "information_schema", "performance_schema", "phpmyadmin"];

const SERVER_EXES: [&str; 2] = ["mysqld.exe", "mariadbd.exe"];
const CLIENT_EXES: [&str; 2] = ["mysql.exe", "mariadb.exe"];
const DUMP_EXES: [&str; 2] = ["mysqldump.exe", "mariadb-dump.exe"];
const ADMIN_EXES: [&str; 2] = ["mysqladmin.exe", "mariadb-admin.exe"];

/// Ports an old server usually listens on, tried when looking for one that is already running.
const OLD_PORTS: std::ops::RangeInclusive<u16> = 3306..=3310;
/// First port tried for the temporary server.
const TEMP_PORT_START: u16 = 3399;

// ---------------------------------------------------------------- find

/// Installs in the usual places (C: and D:), without Ninho's own root.
pub fn find(ninho_root: &Path) -> Vec<Found> {
    let mut places: Vec<(Kind, PathBuf)> = Vec::new();
    for drive in ["C", "D"] {
        for (kind, dir) in [
            (Kind::Xampp, "xampp"),
            (Kind::Wamp, "wamp64"),
            (Kind::Wamp, "wamp"),
            (Kind::Laragon, "laragon"),
        ] {
            places.push((kind, PathBuf::from(format!(r"{drive}:\{dir}"))));
        }
    }
    scan(ninho_root, &places)
}

/// One folder the person points at (an install that is not in the usual places): which of the three it is is read from
/// its own files, then the same detection as `find` runs. It is only read. None when it is Ninho itself or holds
/// neither a web root nor a database server.
pub fn find_in(ninho_root: &Path, dir: &Path) -> Option<Found> {
    if !dir.is_dir() || norm(dir) == norm(ninho_root) {
        return None;
    }
    let has = |f: &str| dir.join(f).exists();
    let order = if has("htdocs") || has(r"mysql\bin") {
        [Kind::Xampp, Kind::Laragon, Kind::Wamp]
    } else if has("wampmanager.exe") || has("wampmanager.conf") {
        [Kind::Wamp, Kind::Laragon, Kind::Xampp]
    } else {
        [Kind::Laragon, Kind::Wamp, Kind::Xampp]
    };
    order.into_iter().find_map(|k| inspect(k, dir))
}

/// `find` over given places (the tests use a fake tree).
fn scan(ninho_root: &Path, places: &[(Kind, PathBuf)]) -> Vec<Found> {
    let own = norm(ninho_root);
    places
        .iter()
        .filter(|(_, root)| root.is_dir() && norm(root) != own)
        .filter_map(|(kind, root)| inspect(*kind, root))
        .collect()
}

fn inspect(kind: Kind, root: &Path) -> Option<Found> {
    let www = root.join(if kind == Kind::Xampp { "htdocs" } else { "www" });
    let (mysql_bin, data_dir) = match kind {
        Kind::Xampp => {
            let bin = root.join(r"mysql\bin");
            let ok = has_server_tools(&bin);
            (ok.then_some(bin), Some(root.join(r"mysql\data")).filter(|d| d.is_dir()))
        }
        Kind::Wamp => wamp_mysql(root),
        Kind::Laragon => laragon_mysql(root),
    };
    // Nothing to bring over from a folder that is neither a web root nor a database server.
    if !www.is_dir() && mysql_bin.is_none() {
        return None;
    }
    let sites = list_sites(kind, &www);
    Some(Found { kind, root: root.to_path_buf(), www, sites, mysql_bin, data_dir })
}

/// WAMP: the newest of bin\mysql\mysql* and bin\mariadb\mariadb*; data from its my.ini, else its data folder.
fn wamp_mysql(root: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for family in [r"bin\mysql", r"bin\mariadb"] {
        for dir in subdirs(&root.join(family)) {
            if has_server_tools(&dir.join("bin")) {
                let key = ver_key(&file_name(&dir));
                if best.as_ref().is_none_or(|(k, _)| key > *k) {
                    best = Some((key, dir));
                }
            }
        }
    }
    let Some((_, dir)) = best else { return (None, None) };
    let from_ini = fs::read_to_string(dir.join("my.ini")).ok().and_then(|t| parse_datadir(&t, &dir));
    let data = from_ini.or_else(|| Some(dir.join("data"))).filter(|d| d.is_dir());
    (Some(dir.join("bin")), data)
}

/// Laragon: bin\mysql\*\bin (newest) and data\mysql-* (newest), falling back to the data folder beside the binaries.
fn laragon_mysql(root: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let best = subdirs(&root.join(r"bin\mysql"))
        .into_iter()
        .filter(|d| has_server_tools(&d.join("bin")))
        .max_by_key(|d| ver_key(&file_name(d)));
    let Some(dir) = best else { return (None, None) };
    let data = subdirs(&root.join("data"))
        .into_iter()
        .filter(|d| file_name(d).to_lowercase().starts_with("mysql"))
        .max_by_key(|d| ver_key(&file_name(d)))
        .or_else(|| Some(dir.join("data")).filter(|d| d.is_dir()));
    (Some(dir.join("bin")), data)
}

/// Site folders: directories only (loose files are the install's pages), no dot folders, no install pages.
fn list_sites(kind: Kind, www: &Path) -> Vec<String> {
    let own: &[&str] = match kind {
        Kind::Xampp => &XAMPP_OWN,
        Kind::Wamp => &WAMP_OWN,
        Kind::Laragon => &[],
    };
    let mut v: Vec<String> = subdirs(www).iter().map(|d| file_name(d)).filter(|n| keep_site(n, own)).collect();
    v.sort_by_key(|n| n.to_lowercase());
    v
}

fn keep_site(name: &str, own: &[&str]) -> bool {
    !name.starts_with('.') && !own.iter().any(|o| o.eq_ignore_ascii_case(name))
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(p) else { return Vec::new() };
    rd.filter_map(|e| e.ok()).filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect()
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Folder that holds a server and its dump tool.
fn has_server_tools(bin: &Path) -> bool {
    pick(bin, &SERVER_EXES).is_some() && pick(bin, &DUMP_EXES).is_some()
}

/// First of these programs that exists in `bin` (MySQL and MariaDB name them differently).
fn pick(bin: &Path, names: &[&str]) -> Option<PathBuf> {
    names.iter().map(|n| bin.join(n)).find(|p| p.is_file())
}

/// "mysql-8.0.30-winx64" to [8, 0, 30, 64]: compares versions by number, so 10 beats 9.
fn ver_key(name: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut cur: Option<u64> = None;
    for c in name.chars() {
        if let Some(d) = c.to_digit(10) {
            cur = Some(cur.unwrap_or(0).saturating_mul(10).saturating_add(d as u64));
        } else if let Some(n) = cur.take() {
            out.push(n);
        }
    }
    out.extend(cur);
    out
}

/// `datadir` of the [mysqld] section of a my.ini (last one wins; relative paths are from `base`).
fn parse_datadir(ini: &str, base: &Path) -> Option<PathBuf> {
    let mut in_mysqld = false;
    let mut found = None;
    for line in ini.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_mysqld = l.trim_end().eq_ignore_ascii_case("[mysqld]");
        } else if in_mysqld && !l.starts_with('#') && !l.starts_with(';') {
            if let Some((k, v)) = l.split_once('=') {
                if k.trim().eq_ignore_ascii_case("datadir") {
                    // Drop a trailing comment, then the quotes.
                    let v = v.split(" #").next().unwrap_or(v).trim().trim_matches('"').trim_matches('\'');
                    if !v.is_empty() {
                        let p = PathBuf::from(v.replace('/', "\\"));
                        found = Some(if p.is_absolute() { p } else { base.join(p) });
                    }
                }
            }
        }
    }
    found
}

/// Comparable form of a path: no `\\?\`, one kind of slash, lowercase, no trailing slash (Windows paths ignore case).
fn norm(p: &Path) -> String {
    let real = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = real.to_string_lossy().replace('/', "\\").to_lowercase();
    s.trim_start_matches(r"\\?\").trim_end_matches('\\').to_string()
}

// ---------------------------------------------------------------- sites

/// Copy site folders into `dest_www` (Ninho's sites folder). `only` names the sites to copy; empty means all of them.
/// A name that is not in `f.sites` is ignored (it also keeps `..` out). A folder that already exists in `dest_www` is
/// never touched: it goes to the skipped list, and so does a site whose copy failed (the partial copy is removed).
/// Returns (copied, skipped).
pub fn copy_sites(
    f: &Found,
    dest_www: &Path,
    only: &[String],
    progress: &dyn Fn(&str),
) -> Result<(Vec<String>, Vec<String>), String> {
    if norm(dest_www) == norm(&f.www) {
        return Err("the sites folder is the old install's own folder".into());
    }
    fs::create_dir_all(dest_www).map_err(|e| format!("cannot create {}: {e}", dest_www.display()))?;
    let (mut copied, mut skipped) = (Vec::new(), Vec::new());
    for name in f.sites.iter().filter(|n| only.is_empty() || only.contains(n)) {
        let dest = dest_www.join(name);
        if dest.exists() {
            progress(&format!("{name}: already in Ninho, skipped"));
            skipped.push(name.clone());
            continue;
        }
        progress(&format!("Copying {name}..."));
        match copy_tree(&f.www.join(name), &dest) {
            Ok(()) => copied.push(name.clone()),
            Err(e) => {
                // The folder is ours (it did not exist a moment ago), so cleaning the half copy is safe.
                let _ = fs::remove_dir_all(&dest);
                progress(&format!("{name}: failed ({e})"));
                skipped.push(name.clone());
            }
        }
    }
    Ok((copied, skipped))
}

fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let t = e.file_type()?;
        let to = dst.join(e.file_name());
        if t.is_dir() {
            copy_tree(&e.path(), &to)?;
        } else if t.is_file() {
            fs::copy(e.path(), &to)?;
        }
        // Links and junctions are left out: following them could loop or pull in folders outside the site.
    }
    Ok(())
}

// ---------------------------------------------------------------- databases

/// True when `dump_databases` failed only because the old server wants a root password; ask for it and call again.
pub fn needs_password(err: &str) -> bool {
    err.starts_with("PASSWORD:")
}

/// Dump every user database of the old install into `<ninho_root>\backups\import-<kind>-<YYYYMMDD-HHMMSS UTC>.sql`.
/// `password` is the old server's root password ("" = none). If its server is already answering (3306..3310, and its
/// data folder matches), it is used as is. Otherwise its own mysqld is started on a free local port with
/// `--skip-grant-tables` on its data folder, dumped, and shut down again. Returns the file and the database names.
/// Errors starting with "PASSWORD:" mean a running server rejected the password (see `needs_password`).
pub fn dump_databases(
    f: &Found,
    ninho_root: &Path,
    ninho_port: u16,
    password: &str,
    progress: &dyn Fn(&str),
) -> Result<(PathBuf, Vec<String>), String> {
    let bin = f.mysql_bin.as_ref().ok_or(format!("{} has no MySQL/MariaDB to read", f.kind.name()))?;
    let client = pick(bin, &CLIENT_EXES).ok_or("the old install has no mysql client")?;
    let dump = pick(bin, &DUMP_EXES).ok_or("the old install has no mysqldump")?;

    progress("Looking for the old server...");
    let mut temp: Option<TempServer> = None;
    let (port, pw) = match running_port(f, &client, password, ninho_port)? {
        Some(p) => {
            progress(&format!("Old server is running on port {p}"));
            (p, password)
        }
        None => {
            let data = f.data_dir.as_ref().ok_or("the old install's data folder was not found")?;
            let p = free_port(TEMP_PORT_START).ok_or("no free port for the temporary server")?;
            progress("Copying the old data folder and starting a temporary server on the copy...");
            temp = Some(TempServer::start(bin, data, p, &client)?);
            // skip-grant-tables: no password is checked.
            (p, "")
        }
    };
    let res = dump_from(f, ninho_root, &client, &dump, port, pw, progress);
    // Stop the temporary server (if any) before reporting, whatever the result.
    drop(temp);
    res
}

fn dump_from(
    f: &Found,
    ninho_root: &Path,
    client: &Path,
    dump: &Path,
    port: u16,
    pw: &str,
    progress: &dyn Fn(&str),
) -> Result<(PathBuf, Vec<String>), String> {
    let out = sql(client, port, pw, "SHOW DATABASES").map_err(wrap_denied)?;
    let skip_system = |n: &str| SYSTEM_DBS.iter().any(|s| s.eq_ignore_ascii_case(n));
    let mut dbs: Vec<String> = out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !skip_system(l)).collect();
    dbs.sort_by_key(|n| n.to_lowercase());
    if dbs.is_empty() {
        return Err("the old server has no databases of its own to bring".into());
    }
    progress(&format!("Dumping {} database(s): {}", dbs.len(), dbs.join(", ")));

    let dir = ninho_root.join("backups");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let file = dir.join(format!("import-{}-{}.sql", f.kind.slug(), stamp_utc()));
    let out_file = File::create(&file).map_err(|e| format!("cannot create {}: {e}", file.display()))?;

    let mut c = Command::new(dump);
    c.arg("--no-defaults").args(["-h", "127.0.0.1", "-u", "root", "--protocol=tcp"]);
    c.arg(format!("--port={port}"));
    c.args(["--single-transaction", "--routines", "--events", "--hex-blob", "--default-character-set=utf8mb4"]);
    c.arg("--databases").args(&dbs);
    if !pw.is_empty() {
        c.env("MYSQL_PWD", pw);
    }
    hide_window(&mut c);
    // Straight to disk; only stderr is kept.
    let res = c.stdin(Stdio::null()).stdout(Stdio::from(out_file)).stderr(Stdio::piped()).output();
    match res {
        Ok(o) if o.status.success() => Ok((file, dbs)),
        Ok(o) => {
            // A half-written dump looks like a good one later.
            let _ = fs::remove_file(&file);
            Err(wrap_denied(stderr_of(&o)))
        }
        Err(e) => {
            let _ = fs::remove_file(&file);
            Err(e.to_string())
        }
    }
}

/// Load a dump from `dump_databases` into Ninho's MySQL (id "mysql", must be running), with Ninho's own mysql.exe.
/// `password` is Ninho's root password ("" = none). The dump uses `--databases`, so it creates each database and
/// REPLACES same-named tables already in Ninho: tell the person before calling.
pub fn load_dump(s: &Settings, dump: &Path, password: &str) -> Result<(), String> {
    if !dump.extension().is_some_and(|e| e.eq_ignore_ascii_case("sql")) {
        return Err("only plain .sql files are supported".into());
    }
    let port = s.svc("mysql").port;
    if !port_open(port) {
        return Err("Ninho's MySQL is not running: start it first".into());
    }
    let v = version_dir(s, "mysql").ok_or("MySQL is not installed")?;
    let exe = pick(&v.join("bin"), &CLIENT_EXES).ok_or("mysql.exe not found in Ninho's MySQL")?;
    let input = File::open(dump).map_err(|e| format!("cannot open {}: {e}", dump.display()))?;

    let mut c = Command::new(exe);
    c.arg("--no-defaults").args(["-h", "127.0.0.1", "-u", "root", "--protocol=tcp", "--default-character-set=utf8mb4"]);
    c.arg(format!("--port={port}"));
    if !password.is_empty() {
        c.env("MYSQL_PWD", password);
    }
    hide_window(&mut c);
    // The file feeds stdin, so a big dump never sits in memory.
    let out = c.stdin(Stdio::from(input)).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(stderr_of(&out)) }
}

/// A server of the old install that is answering now: its data folder must be the old one (port 3306 may be Ninho's
/// own MySQL or anything else). `Ok(None)` = none found, start the temporary one.
fn running_port(f: &Found, client: &Path, password: &str, ninho_port: u16) -> Result<Option<u16>, String> {
    let mut denied = false;
    // Ninho's own MySQL is never the old one (and its password is not the old server's).
    for port in OLD_PORTS.filter(|p| *p != ninho_port && port_open(*p)) {
        match sql(client, port, password, "SELECT @@datadir") {
            Ok(dir) => {
                let same = match &f.data_dir {
                    Some(d) => norm(Path::new(dir.trim())) == norm(d),
                    // Without a known data folder there is nothing to compare; trust the first answering server.
                    None => true,
                };
                if same {
                    return Ok(Some(port));
                }
            }
            Err(e) if is_denied(&e) => denied = true,
            Err(_) => {}
        }
    }
    if denied {
        // Could not look at any server's data folder, so one of them may be the old one.
        return Err(wrap_denied("Access denied".into()));
    }
    Ok(None)
}

fn is_denied(e: &str) -> bool {
    e.contains("Access denied") || e.contains("ERROR 1045")
}

fn wrap_denied(e: String) -> String {
    if is_denied(&e) {
        "PASSWORD: the old server asked for the root password (empty or wrong one given)".into()
    } else {
        e
    }
}

/// One statement through a mysql client; tab-separated rows without header.
fn sql(client: &Path, port: u16, pw: &str, stmt: &str) -> Result<String, String> {
    let mut c = Command::new(client);
    c.arg("--no-defaults").args(["-h", "127.0.0.1", "-u", "root", "--protocol=tcp", "--connect-timeout=5", "-N", "-B"]);
    c.arg(format!("--port={port}"));
    if !pw.is_empty() {
        c.env("MYSQL_PWD", pw);
    }
    c.args(["-e", stmt]);
    hide_window(&mut c);
    let out = c.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(String::from_utf8_lossy(&out.stdout).into_owned()) } else { Err(stderr_of(&out)) }
}

fn stderr_of(out: &std::process::Output) -> String {
    let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if e.is_empty() { format!("exited with {}", out.status) } else { e }
}

/// First free local port from `start` (checked by binding it, so a port held by someone else is skipped).
fn free_port(start: u16) -> Option<u16> {
    (start..start.saturating_add(100)).find(|p| !port_open(*p) && TcpListener::bind(("127.0.0.1", *p)).is_ok())
}

/// The old install's mysqld running on a temporary port. It goes away when this is dropped, in every path.
struct TempServer {
    child: Child,
    port: u16,
    admin: Option<PathBuf>,
    dir: PathBuf,
}

impl TempServer {
    fn start(bin: &Path, data: &Path, port: u16, client: &Path) -> Result<TempServer, String> {
        if !data.is_dir() {
            return Err(format!("data folder not found: {}", data.display()));
        }
        let exe = pick(bin, &SERVER_EXES).ok_or("the old install has no mysqld")?;
        // A server still using the folder would make the copy inconsistent (and two servers on one folder corrupt it).
        if in_use(data) {
            return Err("the old MySQL/MariaDB is still running (its data files are locked): close XAMPP, WampServer or Laragon and try again".into());
        }
        // The server runs on a COPY in a scratch folder: InnoDB recovery, upgrades and logs never touch the original.
        let dir = std::env::temp_dir().join(format!("ninho-import-{port}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let copy = dir.join("data");
        if let Err(e) = copy_data(data, &copy) {
            let _ = fs::remove_dir_all(&dir);
            return Err(format!("could not copy the old data folder: {e}"));
        }
        let log = dir.join("mysqld.err");

        let mut c = Command::new(exe);
        c.arg("--no-defaults");
        c.arg(format!("--datadir={}", copy.display()));
        c.arg(format!("--port={port}"));
        // Read-only use: only this PC, no password checks, no binary log, no connections from outside.
        c.args(["--bind-address=127.0.0.1", "--skip-grant-tables", "--skip-networking=OFF", "--skip-log-bin"]);
        c.arg(format!("--pid-file={}", dir.join("mysqld.pid").display()));
        c.arg(format!("--log-error={}", log.display()));
        c.arg(format!("--tmpdir={}", dir.display()));
        hide_window(&mut c);
        let child = c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
        let mut me = TempServer { child, port, admin: pick(bin, &ADMIN_EXES), dir };

        // A big InnoDB recovery can take a while.
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if let Ok(Some(st)) = me.child.try_wait() {
                return Err(format!("the old server exited ({st}): {}", tail(&log)));
            }
            if port_open(port) && sql(client, port, "", "SELECT 1").is_ok() {
                return Ok(me);
            }
            if Instant::now() > deadline {
                return Err(format!("the old server did not answer in time: {}", tail(&log)));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}

impl Drop for TempServer {
    fn drop(&mut self) {
        // Clean shutdown first (keeps InnoDB tidy), the kill is the last resort.
        if let Some(a) = &self.admin {
            let mut c = Command::new(a);
            c.arg("--no-defaults").args(["-h", "127.0.0.1", "-u", "root", "--protocol=tcp"]);
            c.arg(format!("--port={}", self.port)).arg("shutdown");
            hide_window(&mut c);
            let _ = c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Copy of a MySQL/MariaDB data folder without the old server's pid, error log and lock files.
fn copy_data(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let t = e.file_type()?;
        let name = e.file_name().to_string_lossy().to_lowercase();
        let to = dst.join(e.file_name());
        if t.is_dir() {
            copy_data(&e.path(), &to)?;
        } else if t.is_file() && !(name.ends_with(".pid") || name.ends_with(".err") || name.ends_with(".lock")) {
            fs::copy(e.path(), &to)?;
        }
    }
    Ok(())
}

/// Is a server holding this data folder? InnoDB keeps ibdata1 (and Aria its control file) locked while running.
fn in_use(data: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        for f in ["ibdata1", "aria_log_control"] {
            let p = data.join(f);
            if p.is_file() && fs::OpenOptions::new().read(true).share_mode(0).open(&p).is_err() {
                return true;
            }
        }
    }
    #[cfg(not(windows))]
    let _ = data;
    false
}

/// Last bytes of a log, for an error message.
fn tail(p: &Path) -> String {
    let Ok(mut f) = File::open(p) else { return "(no log)".into() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(600)));
    let mut s = String::new();
    let _ = f.read_to_string(&mut s);
    s.trim().to_string()
}

// ---------------------------------------------------------------- time

/// "YYYYMMDD-HHMMSS" in UTC (std has no time zones; this is only a unique, sortable file name).
fn stamp_utc() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    stamp_from_secs(secs)
}

fn stamp_from_secs(secs: i64) -> String {
    // Hinnant's civil_from_days.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let t = secs.rem_euclid(86_400);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", t / 3600, t / 60 % 60, t % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh empty folder in temp.
    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ninho-importer-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    fn fake_bin(bin: &Path) {
        for n in ["mysqld.exe", "mysqldump.exe", "mysql.exe", "mysqladmin.exe"] {
            touch(&bin.join(n));
        }
    }

    #[test]
    fn finds_one_folder_the_person_points_at() {
        let base = tmp("find-in");
        let x = base.join("meu-xampp");
        fs::create_dir_all(x.join(r"htdocs\loja")).unwrap();
        fake_bin(&x.join(r"mysql\bin"));
        let f = find_in(&base.join("ninho"), &x).expect("a XAMPP-like folder");
        assert_eq!(f.kind, Kind::Xampp);
        assert_eq!(f.sites, vec!["loja".to_string()]);
        // Ninho itself and a folder with nothing to bring over are not installs.
        assert!(find_in(&x, &x).is_none());
        let empty = base.join("vazia");
        fs::create_dir_all(&empty).unwrap();
        assert!(find_in(&base.join("ninho"), &empty).is_none());
        assert!(find_in(&base.join("ninho"), &base.join("nao-existe")).is_none());
    }

    #[test]
    fn finds_the_three_installs_and_skips_ninho_itself() {
        let base = tmp("find");
        // XAMPP: htdocs with its own pages and two sites.
        let x = base.join("xampp");
        for d in ["dashboard", "img", "webalizer", "xampp", "loja", "Blog", ".git"] {
            fs::create_dir_all(x.join("htdocs").join(d)).unwrap();
        }
        for f in ["favicon.ico", "index.php", "applications.html", "bitnami.css", "solto.php"] {
            touch(&x.join("htdocs").join(f));
        }
        fake_bin(&x.join(r"mysql\bin"));
        fs::create_dir_all(x.join(r"mysql\data\loja")).unwrap();
        // WAMP: two engines, the newest one (mariadb 11) wins; its my.ini points the data folder.
        let w = base.join("wamp64");
        fs::create_dir_all(w.join(r"www\site1")).unwrap();
        fs::create_dir_all(w.join(r"www\wampthemes")).unwrap();
        fake_bin(&w.join(r"bin\mysql\mysql8.0.9\bin"));
        fake_bin(&w.join(r"bin\mariadb\mariadb11.4.2\bin"));
        fs::create_dir_all(w.join(r"bin\mariadb\mariadb11.4.2\data")).unwrap();
        fs::create_dir_all(w.join("dados")).unwrap();
        fs::write(
            w.join(r"bin\mariadb\mariadb11.4.2\my.ini"),
            format!("[client]\ndatadir=zzz\n[mysqld]\n# datadir=nope\ndatadir = \"{}\"\n", w.join("dados").display().to_string().replace('\\', "/")),
        )
        .unwrap();
        // Laragon (here also "Ninho's own root"): must be skipped. Another one on a second place stays.
        let l = base.join("laragon");
        fs::create_dir_all(l.join("www\\app")).unwrap();
        fake_bin(&l.join(r"bin\mysql\mysql-5.7.0-winx64\bin"));
        fake_bin(&l.join(r"bin\mysql\mysql-8.0.30-winx64\bin"));
        fs::create_dir_all(l.join(r"data\mysql-5")).unwrap();
        fs::create_dir_all(l.join(r"data\mysql-8")).unwrap();
        let l2 = base.join("laragon2");
        fs::create_dir_all(l2.join("www\\z")).unwrap();
        // A folder with neither web root nor server is nothing to import.
        fs::create_dir_all(base.join("empty")).unwrap();

        let places = vec![
            (Kind::Xampp, x.clone()),
            (Kind::Wamp, w.clone()),
            (Kind::Wamp, base.join("wamp-ausente")),
            (Kind::Laragon, l.clone()),
            (Kind::Laragon, l2.clone()),
            (Kind::Xampp, base.join("empty")),
        ];
        // Same folder written differently (case, slashes, trailing slash) is still Ninho's own root.
        let own = PathBuf::from(format!("{}\\", l.display().to_string().to_uppercase().replace('\\', "/")));
        let got = scan(&own, &places);
        assert_eq!(got.len(), 3, "{got:?}");

        let gx = &got[0];
        assert_eq!(gx.kind, Kind::Xampp);
        assert_eq!(gx.sites, vec!["Blog", "loja"]);
        assert_eq!(gx.mysql_bin.as_deref(), Some(x.join(r"mysql\bin").as_path()));
        assert_eq!(gx.data_dir.as_deref(), Some(x.join(r"mysql\data").as_path()));

        let gw = &got[1];
        assert_eq!(gw.sites, vec!["site1"]);
        assert_eq!(gw.mysql_bin.as_deref(), Some(w.join(r"bin\mariadb\mariadb11.4.2\bin").as_path()));
        assert_eq!(norm(gw.data_dir.as_ref().unwrap()), norm(&w.join("dados")));

        let gl = &got[2];
        assert_eq!(gl.root, l2);
        assert!(gl.mysql_bin.is_none() && gl.data_dir.is_none());

        // Laragon alone, not skipped: newest binaries, newest data folder.
        let only_l = scan(Path::new(r"Z:\outro"), &[(Kind::Laragon, l.clone())]);
        assert_eq!(only_l[0].mysql_bin.as_deref(), Some(l.join(r"bin\mysql\mysql-8.0.30-winx64\bin").as_path()));
        assert_eq!(only_l[0].data_dir.as_deref(), Some(l.join(r"data\mysql-8").as_path()));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn htdocs_skip_list() {
        for n in XAMPP_OWN {
            assert!(!keep_site(n, &XAMPP_OWN), "{n}");
            assert!(!keep_site(&n.to_uppercase(), &XAMPP_OWN), "{n}");
        }
        assert!(keep_site("meu-site", &XAMPP_OWN));
        assert!(keep_site("dashboard2", &XAMPP_OWN));
        assert!(!keep_site(".git", &[]));
    }

    #[test]
    fn free_port_skips_busy_ones() {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let busy = l.local_addr().unwrap().port();
        let p = free_port(busy).expect("a free port");
        assert_ne!(p, busy);
        assert!(p > busy && p < busy + 100);
        // Free means we can really bind it.
        assert!(TcpListener::bind(("127.0.0.1", p)).is_ok());
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(ver_key("mysql10.0") > ver_key("mysql9.9"));
        assert_eq!(ver_key("mysql-8.0.30-winx64"), vec![8, 0, 30, 64]);
        assert!(ver_key("mariadb11.4.2") > ver_key("mysql8.0.9"));
        assert!(ver_key("mysql-8") > ver_key("mysql-5"));
    }

    #[test]
    fn my_ini_datadir() {
        let base = Path::new(r"C:\wamp64\bin\mysql\mysql8");
        assert_eq!(parse_datadir("[mysqld]\ndatadir=\"c:/wamp64/data\"\n", base), Some(PathBuf::from(r"c:\wamp64\data")));
        assert_eq!(parse_datadir("[mysqld]\ndatadir=data # here\n", base), Some(base.join("data")));
        assert_eq!(parse_datadir("[mysqld]\n;datadir=x\n[client]\ndatadir=y\n", base), None);
        assert_eq!(parse_datadir("", base), None);
    }

    #[test]
    fn copies_without_overwriting() {
        let base = tmp("copy");
        let www = base.join("htdocs");
        touch(&www.join("loja/index.php"));
        touch(&www.join("loja/sub/a.txt"));
        touch(&www.join("blog/index.php"));
        touch(&www.join("novo/index.php"));
        let dest = base.join("sites");
        touch(&dest.join("blog/MEU.txt"));
        let f = Found {
            kind: Kind::Xampp,
            root: base.clone(),
            www: www.clone(),
            sites: vec!["blog".into(), "loja".into(), "novo".into()],
            mysql_bin: None,
            data_dir: None,
        };
        let seen = std::cell::RefCell::new(Vec::new());
        let say = |m: &str| seen.borrow_mut().push(m.to_string());
        let only = vec!["blog".to_string(), "loja".to_string(), "..".to_string()];
        let (copied, skipped) = copy_sites(&f, &dest, &only, &say).unwrap();
        assert_eq!(copied, vec!["loja"]);
        assert_eq!(skipped, vec!["blog"]);
        assert!(dest.join("loja/sub/a.txt").is_file());
        assert!(!dest.join("novo").exists(), "not asked for");
        // The existing folder is untouched and the old one is still whole.
        assert!(dest.join("blog/MEU.txt").is_file() && !dest.join("blog/index.php").exists());
        assert!(www.join("loja/index.php").is_file());
        assert!(!seen.borrow().is_empty());
        // Empty `only` means everything; copying into the old folder itself is refused.
        let (c2, s2) = copy_sites(&f, &dest, &[], &say).unwrap();
        assert_eq!((c2, s2), (vec!["novo".to_string()], vec!["blog".to_string(), "loja".to_string()]));
        assert!(copy_sites(&f, &www, &[], &say).is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn password_errors() {
        assert!(needs_password(&wrap_denied("ERROR 1045 (28000): Access denied for user 'root'@'localhost'".into())));
        assert!(!needs_password(&wrap_denied("Can't connect".into())));
    }

    #[test]
    fn stamps() {
        assert_eq!(stamp_from_secs(0), "19700101-000000");
        assert_eq!(stamp_from_secs(1_700_000_000), "20231114-221320");
    }
}
