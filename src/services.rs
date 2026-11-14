use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::i18n::{Lang, tr};
use crate::ports;
use crate::settings::{ServiceCfg, Settings};

pub struct ServiceDef {
    pub key: &'static str,
    pub name: &'static str,
    /// Folder under <root>\bin
    pub dir: &'static str,
    /// Program inside a version folder
    pub exe: &'static str,
    pub default_port: u16,
    pub default_on: bool,
    /// More than one can run side by side (each on its own port).
    pub multi: bool,
}

// Kinds of server Ninho runs, with their usual ports.
#[cfg(windows)]
pub const SERVICES: &[ServiceDef] = &[
    ServiceDef { key: "apache", name: "Apache", dir: "apache", exe: r"bin\httpd.exe", default_port: 80, default_on: true, multi: false },
    ServiceDef { key: "mysql", name: "MySQL", dir: "mysql", exe: r"bin\mysqld.exe", default_port: 3306, default_on: true, multi: true },
    ServiceDef { key: "nginx", name: "Nginx", dir: "nginx", exe: "nginx.exe", default_port: 8080, default_on: false, multi: false },
    // 2015 (Caddy's classic port): Apache keeps 80.
    ServiceDef { key: "caddy", name: "Caddy", dir: "caddy", exe: "caddy.exe", default_port: 2015, default_on: false, multi: false },
    ServiceDef { key: "redis", name: "Redis", dir: "redis", exe: "redis-server.exe", default_port: 6379, default_on: false, multi: true },
    ServiceDef { key: "memcached", name: "Memcached", dir: "memcached", exe: "memcached.exe", default_port: 11211, default_on: false, multi: true },
    ServiceDef { key: "php", name: "PHP Server", dir: "php", exe: "php.exe", default_port: 8000, default_on: false, multi: true },
    ServiceDef { key: "mailpit", name: "Mailpit", dir: "mailpit", exe: "mailpit.exe", default_port: 1025, default_on: true, multi: true },
    ServiceDef { key: "postgres", name: "PostgreSQL", dir: "postgresql", exe: r"bin\postgres.exe", default_port: 5432, default_on: false, multi: true },
    // 3307 so it runs beside MySQL (3306).
    ServiceDef { key: "mariadb", name: "MariaDB", dir: "mariadb", exe: r"bin\mariadbd.exe", default_port: 3307, default_on: false, multi: true },
    ServiceDef { key: "mongodb", name: "MongoDB", dir: "mongodb", exe: r"bin\mongod.exe", default_port: 27017, default_on: false, multi: true },
];

// Linux and macOS (preview): the same kinds, run from the programs the package manager installed, linked into
// <root>/bin/<kind>/<version>-system (see platform/systools.rs). Apache listens on 8080: ports below 1024 need root.
#[cfg(unix)]
pub const SERVICES: &[ServiceDef] = &[
    ServiceDef { key: "apache", name: "Apache", dir: "apache", exe: "bin/httpd", default_port: 8080, default_on: true, multi: false },
    ServiceDef { key: "mysql", name: "MySQL", dir: "mysql", exe: "bin/mysqld", default_port: 3306, default_on: true, multi: true },
    ServiceDef { key: "nginx", name: "Nginx", dir: "nginx", exe: "nginx", default_port: 8081, default_on: false, multi: false },
    ServiceDef { key: "redis", name: "Redis", dir: "redis", exe: "redis-server", default_port: 6379, default_on: false, multi: true },
    ServiceDef { key: "memcached", name: "Memcached", dir: "memcached", exe: "memcached", default_port: 11211, default_on: false, multi: true },
    ServiceDef { key: "php", name: "PHP Server", dir: "php", exe: "php", default_port: 8000, default_on: false, multi: true },
    ServiceDef { key: "mailpit", name: "Mailpit", dir: "mailpit", exe: "mailpit", default_port: 1025, default_on: true, multi: true },
    ServiceDef { key: "postgres", name: "PostgreSQL", dir: "postgresql", exe: "bin/postgres", default_port: 5432, default_on: false, multi: true },
    ServiceDef { key: "mariadb", name: "MariaDB", dir: "mariadb", exe: "bin/mariadbd", default_port: 3307, default_on: false, multi: true },
];

/// Mailpit's own web page port (estimated: Mailpit's default). An extra Mailpit uses its SMTP port + 7000.
pub const MAILPIT_WEB_PORT: u16 = 8025;

pub fn mailpit_web_port(c: &ServiceCfg) -> u16 {
    if c.is_extra() { c.port.saturating_add(7000) } else { MAILPIT_WEB_PORT }
}

pub fn def(key: &str) -> &'static ServiceDef {
    SERVICES.iter().find(|d| d.key == key).expect("known service")
}

/// Program file name of a kind (mysqld.exe...), to tell our servers from other programs on the same port.
pub fn image(key: &str) -> String {
    Path::new(def(key).exe).file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase()
}

/// No console window for the programs Ninho starts (Windows; nothing to do elsewhere).
pub use crate::platform::hide_window;

/// Folder a MySQL / MariaDB build calls home (--basedir): the version folder on Windows; on Linux and macOS the
/// folder the linked system program really lives in (/usr for /usr/sbin/mysqld), where its share files are.
pub fn basedir(vdir: &Path, exe_rel: &str) -> PathBuf {
    #[cfg(unix)]
    if let Some(b) = std::fs::canonicalize(vdir.join(exe_rel)).ok().and_then(|real| real.parent().and_then(Path::parent).map(Path::to_path_buf)) {
        return b;
    }
    let _ = exe_rel;
    vdir.to_path_buf()
}

/// Linux and macOS: the Unix socket of a MySQL / MariaDB / PostgreSQL server, in the temporary folder (the
/// system default under /run belongs to root).
#[cfg(unix)]
pub fn socket_dir() -> PathBuf {
    std::env::temp_dir()
}

/// Version folders installed for a kind, oldest name first.
/// The answer of `f` for `key`, reused for one second. The window redraws on every mouse move; reading the bin and
/// www folders each time made clicks feel slow, and a second is far shorter than any install or folder change.
pub fn memo<T: Clone + Send + 'static>(key: String, f: impl FnOnce() -> T) -> T {
    use std::any::Any;
    type Cache = Mutex<HashMap<String, (Instant, Box<dyn Any + Send>)>>;
    static M: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
    let m = M.get_or_init(Default::default);
    if let Some((t, v)) = m.lock().unwrap().get(&key) {
        if t.elapsed() < Duration::from_secs(1) {
            if let Some(v) = v.downcast_ref::<T>() {
                return v.clone();
            }
        }
    }
    let v = f();
    m.lock().unwrap().insert(key, (Instant::now(), Box::new(v.clone())));
    v
}

pub fn versions(root: &str, d: &ServiceDef) -> Vec<String> {
    memo(format!("versions|{root}|{}|{}", d.dir, d.exe), || versions_now(root, d))
}

fn versions_now(root: &str, d: &ServiceDef) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(Path::new(root).join("bin").join(d.dir))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter(|e| e.path().join(d.exe).is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The version folder a server uses: the chosen one, else the newest (estimated rule).
pub fn version_dir(s: &Settings, id: &str) -> Option<PathBuf> {
    let c = s.svc(id);
    let d = def(&c.key);
    let vs = versions(&s.root, d);
    let v = if !c.version.is_empty() && vs.contains(&c.version) { c.version.clone() } else { vs.last()?.clone() };
    Some(Path::new(&s.root).join("bin").join(d.dir).join(v))
}

pub fn version_label(s: &Settings, id: &str) -> Option<String> {
    version_dir(s, id).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
}

pub fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    Stopped,
    Running,
    Busy,
    /// The port is taken by a program that is not this server.
    Foreign,
}

pub struct Shared {
    pub log: Mutex<Vec<String>>,
    pub state: Mutex<HashMap<String, State>>,
    pub busy: Mutex<HashMap<String, bool>>,
    /// id → (port, program image of its kind)
    pub ports: Mutex<HashMap<String, (u16, String)>>,
    pub children: Mutex<HashMap<String, Child>>,
    /// id → program that holds the port when the state is Foreign.
    pub foreign: Mutex<HashMap<String, String>>,
    /// Last full list of listening ports (the Ports tab reads it).
    pub listeners: Mutex<Vec<ports::Listener>>,
    /// Download/install in progress: id → message.
    pub installing: Mutex<HashMap<String, String>>,
    /// Newest stable version of each tool on its official site (tool → version, or why it could not be read).
    pub latest: Mutex<HashMap<String, Result<String, String>>>,
    /// Tools a download just installed ("redis"...): the window turns their server on in Modules.
    pub just_installed: Mutex<Vec<String>>,
    /// Running Docker containers (empty without Docker), read by the watcher while the Ports tab is open.
    pub containers: Mutex<Vec<crate::docker::Container>>,
    /// When the Ports tab last drew (seconds since the epoch): Docker is only asked while someone looks.
    pub ports_seen: std::sync::atomic::AtomicU64,
    /// Seconds since the epoch of the last `docker ps`, and whether one is running now.
    pub docker_last: std::sync::atomic::AtomicU64,
    pub docker_busy: std::sync::atomic::AtomicBool,
}

fn epoch_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            log: Mutex::new(Vec::new()),
            state: Mutex::new(HashMap::new()),
            busy: Mutex::new(HashMap::new()),
            ports: Mutex::new(HashMap::new()),
            children: Mutex::new(HashMap::new()),
            foreign: Mutex::new(HashMap::new()),
            listeners: Mutex::new(Vec::new()),
            installing: Mutex::new(HashMap::new()),
            latest: Mutex::new(HashMap::new()),
            just_installed: Mutex::new(Vec::new()),
            containers: Mutex::new(Vec::new()),
            ports_seen: std::sync::atomic::AtomicU64::new(0),
            docker_last: std::sync::atomic::AtomicU64::new(0),
            docker_busy: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub fn say(&self, msg: impl Into<String>) {
        let line = format!("{}  {}", clock(), msg.into());
        let mut log = self.log.lock().unwrap();
        log.push(line);
        let n = log.len();
        if n > 300 {
            log.drain(0..n - 300);
        }
    }

    pub fn state(&self, id: &str) -> State {
        if *self.busy.lock().unwrap().get(id).unwrap_or(&false) {
            return State::Busy;
        }
        *self.state.lock().unwrap().get(id).unwrap_or(&State::Stopped)
    }

    fn set_busy(&self, id: &str, b: bool) {
        self.busy.lock().unwrap().insert(id.into(), b);
    }

    fn set_state(&self, id: &str, st: State) {
        self.state.lock().unwrap().insert(id.into(), st);
    }

    /// The Ports tab is on screen: Docker is asked for a while (see refresh_docker).
    pub fn ports_seen_now(self: &Arc<Self>) {
        use std::sync::atomic::Ordering::Relaxed;
        let first = epoch_secs().saturating_sub(self.ports_seen.load(Relaxed)) > 15;
        self.ports_seen.store(epoch_secs(), Relaxed);
        if first {
            self.refresh_docker(true);
        }
    }

    /// Ask Docker which containers run, on a thread of its own (a stopped daemon can take seconds to answer). Only
    /// while the Ports tab was drawn in the last 15 seconds, and at most every 4 seconds, unless `force`. Without
    /// Docker the answer is an empty list and nothing is said.
    pub fn refresh_docker(self: &Arc<Self>, force: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        let now = epoch_secs();
        if !force && (now.saturating_sub(self.ports_seen.load(Relaxed)) > 15 || now.saturating_sub(self.docker_last.load(Relaxed)) < 4) {
            return;
        }
        if self.docker_busy.swap(true, Relaxed) {
            return;
        }
        self.docker_last.store(now, Relaxed);
        let me = self.clone();
        std::thread::spawn(move || {
            let list = crate::docker::running();
            *me.containers.lock().unwrap() = list;
            me.docker_busy.store(false, Relaxed);
        });
    }

    /// Look at the ports again now (after a start or stop).
    pub fn refresh(&self) {
        let list = ports::listening();
        let watched: Vec<(String, (u16, String))> = self.ports.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (id, (port, img)) in watched {
            let (st, who) = match ports::owner(&list, port) {
                None => (State::Stopped, String::new()),
                Some(l) if l.program.to_ascii_lowercase() == img || l.program.is_empty() => (State::Running, String::new()),
                Some(l) => (State::Foreign, format!("{} (PID {})", l.program, l.pid)),
            };
            self.set_state(&id, st);
            self.foreign.lock().unwrap().insert(id, who);
        }
        *self.listeners.lock().unwrap() = list;
    }
}

/// Background watcher: who is listening on each server's port, and the full list for the Ports tab.
pub fn spawn_watcher(shared: Arc<Shared>, ctx: eframe::egui::Context) {
    std::thread::spawn(move || loop {
        // A program we started that has exited is no longer ours.
        shared.children.lock().unwrap().retain(|_, c| !matches!(c.try_wait(), Ok(Some(_))));
        shared.refresh();
        shared.refresh_docker(false);
        ctx.request_repaint();
        std::thread::sleep(Duration::from_millis(2000));
    });
}

fn wait_port(port: u16, up: bool, limit: Duration) -> bool {
    let t = Instant::now();
    while t.elapsed() < limit {
        if port_open(port) == up {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    port_open(port) == up
}

fn mysql_datadir(ini: &Path) -> Option<String> {
    let text = std::fs::read_to_string(ini).ok()?;
    text.lines()
        .map(str::trim)
        .find(|l| l.to_ascii_lowercase().starts_with("datadir"))
        .and_then(|l| l.split_once('='))
        .map(|(_, v)| crate::platform::native_slashes(v.trim().trim_matches('"')))
}

/// Data folder of a MySQL server: its own for an extra one, the one in my.ini for the first.
pub fn mysql_data(s: &Settings, c: &ServiceCfg, vdir: &Path) -> Option<String> {
    if c.is_extra() {
        Some(if c.data_dir.is_empty() { crate::platform::join(&s.root, &["data", &c.id]) } else { c.data_dir.clone() })
    } else {
        mysql_datadir(&vdir.join("my.ini"))
    }
}

/// Data folder of PostgreSQL / MariaDB / MongoDB: its own for an extra server, else <root>/data/postgresql-<major> or
/// mariadb-<major.minor> (one per series: a newer patch version opens the same data); <root>/data/mongodb for MongoDB.
pub fn engine_data(s: &Settings, c: &ServiceCfg, vdir: &Path) -> String {
    if c.is_extra() && !c.data_dir.is_empty() {
        return c.data_dir.clone();
    }
    if c.is_extra() {
        return Path::new(&s.root).join("data").join(&c.id).to_string_lossy().into_owned();
    }
    if c.key == "mongodb" {
        return Path::new(&s.root).join("data").join("mongodb").to_string_lossy().into_owned();
    }
    let folder = vdir.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let nums: Vec<&str> = folder.split(|ch: char| !ch.is_ascii_digit()).filter(|x| !x.is_empty()).collect();
    let series = match c.key.as_str() {
        "postgres" => format!("postgresql-{}", nums.first().copied().unwrap_or("0")),
        _ => format!("mariadb-{}.{}", nums.first().copied().unwrap_or("0"), nums.get(1).copied().unwrap_or("0")),
    };
    Path::new(&s.root).join("data").join(series).to_string_lossy().into_owned()
}

/// First start of PostgreSQL / MariaDB: create the data folder (local superuser without a password, like the
/// fresh MySQL), unless it already holds data.
pub fn engine_init(shared: &Shared, lang: Lang, key: &str, vdir: &Path, data: &str, port: u16) -> bool {
    let name = if key == "postgres" { "PostgreSQL" } else { "MariaDB" };
    let p = Path::new(data);
    let ready = match key {
        "postgres" => p.join("PG_VERSION").is_file(),
        _ => p.join("mysql").is_dir(),
    };
    if ready {
        return true;
    }
    if p.is_dir() && std::fs::read_dir(p).map(|mut r| r.next().is_some()).unwrap_or(false) {
        shared.say(format!("{name}: {data}: {}", tr(lang, "log_data_not_empty")));
        return false;
    }
    shared.say(format!("{name}: {} {data}...", tr(lang, "log_data_creating")));
    let bin = vdir.join("bin");
    let mut c = match key {
        "postgres" => {
            let mut c = Command::new(bin.join(crate::platform::exe("initdb")));
            c.args(["-D", data, "-U", "postgres", "-A", "trust", "-E", "UTF8"]);
            c
        }
        _ => {
            let mut c = Command::new(bin.join(crate::platform::exe("mariadb-install-db")));
            c.arg(format!("--datadir={data}")).arg(format!("--port={port}"));
            // Linux and macOS: root without a password over TCP, like on Windows (the default is the Unix socket
            // login of the system user root, which Ninho's tools never use).
            #[cfg(unix)]
            c.arg(format!("--basedir={}", basedir(vdir, "bin/mariadbd").display())).arg("--auth-root-authentication-method=normal").arg("--skip-test-db");
            c
        }
    };
    c.stdin(Stdio::null());
    hide_window(&mut c);
    match c.output() {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            shared.say(format!("{name}: {} {}", tr(lang, "log_data_create_failed"), String::from_utf8_lossy(&o.stderr).trim()));
            false
        }
        Err(e) => {
            shared.say(format!("{name}: {} {e}", tr(lang, "log_data_create_failed")));
            false
        }
    }
}

/// Create an empty MySQL data folder (root without password, like a fresh install) when it does not exist yet.
fn mysql_init(shared: &Shared, lang: Lang, vdir: &Path, data: &str) -> bool {
    let p = Path::new(data);
    if p.join("mysql").is_dir() {
        return true;
    }
    if p.is_dir() && std::fs::read_dir(p).map(|mut r| r.next().is_some()).unwrap_or(false) {
        shared.say(format!("MySQL: {data}: {}", tr(lang, "log_data_not_empty")));
        return false;
    }
    shared.say(format!("MySQL: {} {data}...", tr(lang, "log_data_creating")));
    let mut c = Command::new(vdir.join("bin").join(crate::platform::exe("mysqld")));
    c.args(["--no-defaults", "--initialize-insecure", &format!("--basedir={}", basedir(vdir, "bin/mysqld").display()), &format!("--datadir={data}")]);
    hide_window(&mut c);
    match c.output() {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            shared.say(format!("MySQL: {} {}", tr(lang, "log_data_create_failed"), String::from_utf8_lossy(&o.stderr).trim()));
            false
        }
        Err(e) => {
            shared.say(format!("MySQL: {} {e}", tr(lang, "log_data_create_failed")));
            false
        }
    }
}

/// Rewrite the first `Listen <port>` line of httpd.conf when the port changed.
fn apache_set_port(conf: &Path, port: u16) {
    let Ok(text) = std::fs::read_to_string(conf) else { return };
    let mut done = false;
    let mut changed = false;
    let lines: Vec<String> = text
        .lines()
        .map(|l| {
            if !done && l.starts_with("Listen ") && l[7..].trim().chars().all(|c| c.is_ascii_digit()) {
                done = true;
                let new = format!("Listen {port}");
                if l != new {
                    changed = true;
                }
                return new;
            }
            l.to_string()
        })
        .collect();
    if changed {
        let _ = std::fs::write(conf, lines.join("\r\n") + "\r\n");
    }
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Makes an Apache folder Ninho's: httpd.conf points at its own folder and includes conf/ninho.conf, which is written
/// again at every start with the PHP chosen in Ninho (as an Apache module), the sites folder and the .test sites.
/// Only ever adds or fixes those lines; everything else in httpd.conf is left as the person has it.
pub fn apache_setup(s: &Settings, vdir: &Path) -> Result<(), String> {
    let conf = vdir.join("conf").join("httpd.conf");
    let text = std::fs::read_to_string(&conf).map_err(|e| format!("{}: {e}", conf.display()))?;
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let www = slash(Path::new(&s.doc_root));
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        let fixed = if t.starts_with("Define SRVROOT ") {
            format!("Define SRVROOT \"{}\"", slash(vdir))
        } else if t.starts_with("#ServerName ") {
            "ServerName localhost".to_string()
        } else if t.starts_with("#LoadModule rewrite_module ") {
            t.trim_start_matches('#').to_string()
        } else if t.starts_with("DocumentRoot \"${SRVROOT}/htdocs\"") {
            format!("DocumentRoot \"{www}\"")
        } else if t.starts_with("<Directory \"${SRVROOT}/htdocs\">") {
            format!("<Directory \"{www}\">")
        } else {
            line.to_string()
        };
        out.push(fixed);
    }
    if !out.iter().any(|l| l.trim() == "Include conf/ninho.conf") {
        out.push(String::new());
        out.push("# Ninho: PHP, the sites folder and the .test sites (conf/ninho.conf is written again at every start).".into());
        out.push("Include conf/ninho.conf".into());
    }
    let new = out.join(nl) + nl;
    if new != text {
        std::fs::write(&conf, new).map_err(|e| e.to_string())?;
    }
    // conf/ninho.conf
    let mut n = vec!["# Written by Ninho at every Apache start. Put your own settings in httpd.conf.".to_string()];
    // Linux and macOS: the system Apache uses the system's own mod_php, loaded in httpd.conf (systools.rs).
    let php_dir = if cfg!(windows) { version_dir(s, "php") } else { None };
    if cfg!(unix) {
        n.push("# PHP: the system's mod_php, when there is one, is loaded in httpd.conf.".into());
    }
    match php_dir {
        _ if cfg!(unix) => {}
        Some(php) => {
            let module = std::fs::read_dir(&php).ok().and_then(|rd| {
                rd.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
                    let f = p.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
                    f.starts_with("php") && f.ends_with("apache2_4.dll")
                })
            });
            match module {
                Some(m) => {
                    n.push(format!("LoadModule php_module \"{}\"", slash(&m)));
                    n.push(format!("PHPIniDir \"{}\"", slash(&php)));
                    n.push("<FilesMatch \"\\.php$\">".into());
                    n.push("    SetHandler application/x-httpd-php".into());
                    n.push("</FilesMatch>".into());
                    php_absolute_ext_dir(&php);
                }
                None => n.push("# The chosen PHP has no Apache module (only thread-safe builds have one): .php files are not run.".into()),
            }
        }
        None => n.push("# PHP is not installed: .php files are not run.".into()),
    }
    n.push("DirectoryIndex index.php index.html".into());
    n.push(format!("<Directory \"{www}\">"));
    n.push("    Options Indexes FollowSymLinks".into());
    n.push("    AllowOverride All".into());
    n.push("    Require all granted".into());
    n.push("</Directory>".into());
    // HTTPS: the SSL modules and port 443, unless httpd.conf already has them on.
    if s.https && vdir.join("bin").join("openssl.exe").is_file() {
        let active: Vec<String> = text.lines().map(|l| l.trim().to_string()).filter(|l| !l.starts_with('#')).collect();
        for l in crate::certs::apache_ssl_lines(443) {
            let key = l.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
            if !active.iter().any(|a| a.starts_with(&key)) {
                n.push(l);
            }
        }
    }
    if s.home_page {
        crate::home::write(s);
        n.extend(crate::home::apache_lines(s));
    }
    n.push(format!("IncludeOptional \"{}/etc/apache2/sites-enabled/*.conf\"", slash(Path::new(&s.root))));
    std::fs::write(vdir.join("conf").join("ninho.conf"), n.join(nl) + nl).map_err(|e| e.to_string())
}

/// Inside Apache, PHP looks for its extensions from Apache's folder: give php.ini the full path to ext.
fn php_absolute_ext_dir(php: &Path) {
    let ini = php.join("php.ini");
    let Ok(text) = std::fs::read_to_string(&ini) else { return };
    let want = format!("extension_dir = \"{}/ext\"", slash(php));
    let fixed: Vec<String> = text
        .lines()
        .map(|l| if l.trim_start().starts_with("extension_dir") && !l.contains(&want) { want.clone() } else { l.to_string() })
        .collect();
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let new = fixed.join(nl) + nl;
    if new != text {
        let _ = std::fs::write(&ini, new);
    }
}

fn build_command(s: &Settings, c: &ServiceCfg, vdir: &Path) -> Command {
    let d = def(&c.key);
    let port = c.port;
    let mut cmd = Command::new(vdir.join(d.exe));
    cmd.current_dir(vdir);
    match c.key.as_str() {
        "mysql" => {
            let ini = vdir.join("my.ini");
            if ini.is_file() {
                cmd.arg(format!("--defaults-file={}", ini.display()));
            } else if cfg!(unix) {
                // Never the system's /etc/my.cnf: it points at the system's data folder and sockets.
                cmd.arg("--no-defaults");
            }
            cmd.arg(format!("--port={port}"));
            // Linux and macOS: the system MySQL's own home (its error messages and share files), and its socket.
            #[cfg(unix)]
            cmd.arg(format!("--basedir={}", basedir(vdir, "bin/mysqld").display()))
                .arg(format!("--socket={}", socket_dir().join(format!("ninho-mysql-{port}.sock")).display()));
            if let Some(data) = mysql_data(s, c, vdir) {
                if c.is_extra() {
                    // Its own data, and no second "X protocol" port (33060 belongs to the first MySQL).
                    cmd.arg(format!("--datadir={data}"));
                    cmd.arg("--mysqlx=OFF");
                }
                // Error log beside the data.
                cmd.arg(format!("--log-error={}", crate::platform::join(&data, &["mysqld.log"])));
            }
        }
        "postgres" => {
            // Started directly (pg_ctl start would keep our pipes); only on this PC.
            let data = engine_data(s, c, vdir);
            cmd.args(["-D", &data, "-p", &port.to_string(), "-c", "listen_addresses=127.0.0.1"]);
            #[cfg(unix)]
            cmd.arg("-k").arg(socket_dir());
            cmd.stdin(Stdio::null()).stdout(Stdio::null());
            match std::fs::OpenOptions::new().create(true).append(true).open(Path::new(&data).join("postgres.log")) {
                Ok(f) => cmd.stderr(Stdio::from(f)),
                Err(_) => cmd.stderr(Stdio::null()),
            };
            hide_window(&mut cmd);
            return cmd;
        }
        "mariadb" => {
            let data = engine_data(s, c, vdir);
            let ini = Path::new(&data).join("my.ini");
            if ini.is_file() {
                cmd.arg(format!("--defaults-file={}", ini.display()));
            } else if cfg!(unix) {
                cmd.arg("--no-defaults");
            }
            cmd.arg(format!("--datadir={data}")).arg(format!("--basedir={}", basedir(vdir, "bin/mariadbd").display()));
            cmd.arg(format!("--port={port}")).arg("--bind-address=127.0.0.1");
            #[cfg(unix)]
            cmd.arg(format!("--socket={}", socket_dir().join(format!("ninho-mariadb-{port}.sock")).display()));
            cmd.arg(format!("--log-error={}", Path::new(&data).join("mariadb.log").display()));
        }
        "mongodb" => {
            // Its own data folder, only this PC; the log is a file in the data folder (mongod.log).
            let data = engine_data(s, c, vdir);
            cmd.args(crate::mongo::server_args(&data, port));
        }
        "caddy" => {
            // The Caddyfile is written before the start; Caddy's messages go to etc\caddy\caddy.log.
            cmd.args(crate::caddy::run_args(&crate::caddy::caddyfile_path(&s.root)));
            cmd.stdin(Stdio::null()).stdout(Stdio::null());
            let log = crate::caddy::log_path(&s.root);
            match std::fs::OpenOptions::new().create(true).append(true).open(log) {
                Ok(f) => cmd.stderr(Stdio::from(f)),
                Err(_) => cmd.stderr(Stdio::null()),
            };
            hide_window(&mut cmd);
            return cmd;
        }
        "redis" => {
            if vdir.join("redis.windows.conf").is_file() {
                cmd.arg("redis.windows.conf");
            }
            cmd.args(["--port", &port.to_string()]);
        }
        "memcached" => {
            cmd.args(["-p", &port.to_string()]);
        }
        // Linux and macOS: the system Apache with Ninho's own config folder, in the foreground (so it is our child
        // and stops with us), as this user.
        #[cfg(unix)]
        "apache" => {
            cmd.arg("-d").arg(vdir).arg("-f").arg(vdir.join("conf").join("httpd.conf")).arg("-DFOREGROUND");
        }
        "php" => {
            let root = if c.data_dir.is_empty() { &s.doc_root } else { &c.data_dir };
            cmd.args(["-S", &format!("localhost:{port}"), "-t", root]);
        }
        "mailpit" => {
            cmd.args(["--smtp", &format!("127.0.0.1:{port}")]);
            cmd.args(["--listen", &format!("127.0.0.1:{}", mailpit_web_port(c))]);
            cmd.args(["--log-file", &vdir.join(format!("{}.log", c.id)).to_string_lossy()]);
            if c.is_extra() {
                cmd.args(["--database", &vdir.join(format!("{}.db", c.id)).to_string_lossy()]);
            }
        }
        _ => {}
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    hide_window(&mut cmd);
    cmd
}

pub fn start(shared: &Arc<Shared>, s: &Settings, id: &str) {
    let c = s.svc(id).clone();
    let d = def(&c.key);
    let name = c.title();
    let port = c.port;
    let lang = s.language;
    if shared.state(id) == State::Running {
        return;
    }
    let Some(vdir) = version_dir(s, id) else {
        shared.say(format!("{name}: {} {}", tr(lang, "log_not_installed_in"), crate::platform::join(&s.root, &["bin", d.dir])));
        return;
    };
    if port_open(port) {
        shared.refresh();
        let who = shared.foreign.lock().unwrap().get(id).cloned().unwrap_or_default();
        shared.say(format!("{name}: {} {port}. {} {who}", tr(lang, "log_port_busy"), tr(lang, "log_port_busy_hint")));
        return;
    }
    shared.set_busy(id, true);
    if c.key == "mysql" {
        if let Some(data) = mysql_data(s, &c, &vdir) {
            // Linux and macOS: the first MySQL too, so a data folder that could not be made at install time (Ubuntu's
            // AppArmor...) is made at the next start, once the cause is fixed.
            if (c.is_extra() || cfg!(unix)) && !mysql_init(shared, lang, &vdir, &data) {
                shared.set_busy(id, false);
                return;
            }
        }
    }
    if c.key == "postgres" || c.key == "mariadb" {
        let data = engine_data(s, &c, &vdir);
        if !engine_init(shared, lang, &c.key, &vdir, &data, c.port) {
            shared.set_busy(id, false);
            return;
        }
    }
    if c.key == "apache" {
        if let Err(e) = apache_setup(s, &vdir) {
            shared.say(format!("Apache: {e}"));
        }
        apache_set_port(&vdir.join("conf").join("httpd.conf"), port);
        crate::sites::refresh(shared, s, false);
    }
    if c.key == "nginx" {
        crate::sites::refresh(shared, s, false);
    }
    if c.key == "mongodb" {
        // mongod does not make its data folder.
        if let Err(e) = std::fs::create_dir_all(engine_data(s, &c, &vdir)) {
            shared.say(format!("MongoDB: {} {e}", tr(lang, "log_data_create_failed")));
            shared.set_busy(id, false);
            return;
        }
    }
    if c.key == "caddy" {
        crate::sites::refresh(shared, s, false);
        crate::caddy::prepare(shared, s);
    }
    let mut cmd = build_command(s, &c, &vdir);
    match cmd.spawn() {
        Ok(child) => {
            shared.children.lock().unwrap().insert(id.into(), child);
            let label = vdir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            if wait_port(port, true, Duration::from_secs(20)) {
                shared.say(format!("{name} ({label}) {} {port}", tr(lang, "log_started_on_port")));
            } else {
                shared.say(format!("{name} ({label}) {} {port}. {}", tr(lang, "log_no_answer_on_port"), tr(lang, "log_check_its_log")));
            }
        }
        Err(e) => shared.say(format!("{name}: {} {e}", tr(lang, "log_start_failed"))),
    }
    shared.refresh();
    shared.set_busy(id, false);
}

pub fn stop(shared: &Arc<Shared>, s: &Settings, id: &str) {
    let c = s.svc(id).clone();
    let name = c.title();
    let port = c.port;
    let lang = s.language;
    shared.set_busy(id, true);
    let vdir = version_dir(s, id);
    let child = shared.children.lock().unwrap().remove(id);

    // Polite stop first.
    match (c.key.as_str(), &vdir) {
        ("nginx", Some(v)) => {
            let mut cmd = Command::new(v.join(crate::platform::exe("nginx")));
            cmd.args(["-s", "stop"]).current_dir(v);
            hide_window(&mut cmd);
            let _ = cmd.output();
        }
        ("postgres", Some(v)) => {
            let data = engine_data(s, &c, v);
            let mut cmd = Command::new(v.join("bin").join(crate::platform::exe("pg_ctl")));
            cmd.args(["-D", &data, "stop", "-m", "fast", "-t", "20"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            hide_window(&mut cmd);
            let _ = cmd.output();
        }
        ("caddy", Some(v)) => crate::caddy::polite_stop(&v.join("caddy.exe")),
        ("mariadb", Some(v)) => {
            let mut cmd = Command::new(v.join("bin").join(crate::platform::exe("mariadb-admin")));
            cmd.args(["--no-defaults", "-h", "127.0.0.1", "-P", &port.to_string(), "-u", "root", "shutdown"]);
            crate::mysqlpw::set_env(&mut cmd, s, id, "");
            hide_window(&mut cmd);
            let _ = cmd.output();
        }
        ("mysql", Some(v)) => {
            let mut cmd = Command::new(v.join("bin").join(crate::platform::exe("mysqladmin")));
            cmd.args(["-u", "root", "-h", "127.0.0.1", &format!("--port={port}"), "shutdown"]);
            crate::mysqlpw::set_env(&mut cmd, s, id, "");
            hide_window(&mut cmd);
            let _ = cmd.output();
        }
        _ => {}
    }
    // MongoDB: through mongosh when it is installed; without it the program is simply ended (it journals its writes).
    let mongo_shell = c.key == "mongodb" && crate::mongo::shutdown(&s.root, port);
    if c.key == "mongodb" && !mongo_shell {
        shared.say(format!("MongoDB: {}", tr(lang, "log_mongo_no_shell")));
    }
    let polite = (matches!(c.key.as_str(), "nginx" | "mysql" | "postgres" | "mariadb" | "caddy") || mongo_shell) && wait_port(port, false, Duration::from_secs(8));
    if !polite {
        if c.key == "mysql" {
            shared.say(tr(lang, "log_mysql_force_stop"));
        }
        // Only the program on THIS port (and only if it is this kind of server): another MySQL on another port
        // keeps running.
        let img = image(&c.key);
        let list = ports::listening();
        let pid = match &child {
            Some(ch) => Some(ch.id()),
            None => ports::owner(&list, port).filter(|l| l.program.to_ascii_lowercase() == img).map(|l| l.pid),
        };
        if let Some(pid) = pid {
            let _ = ports::kill(pid);
        }
        if c.key == "apache" || c.key == "nginx" {
            // Their worker processes, which answer on the same port.
            for l in ports::listening().iter().filter(|l| l.port == port && l.program.to_ascii_lowercase() == img) {
                let _ = ports::kill(l.pid);
            }
        }
    }
    if let Some(mut ch) = child {
        let _ = ch.wait();
    }
    if c.key == "caddy" {
        crate::caddy::stop_helpers(shared);
    }
    wait_port(port, false, Duration::from_secs(4));
    shared.refresh();
    shared.say(if !port_open(port) { format!("{name} {}", tr(lang, "log_svc_stopped")) } else { format!("{name} {} {port}", tr(lang, "log_still_answering")) });
    shared.set_busy(id, false);
}

/// Run a short command line tool from MySQL's folder (create database...) against one MySQL server.
pub fn mysql_exec(s: &Settings, id: &str, sql: &str, password: &str) -> Result<(), String> {
    let v = version_dir(s, id).ok_or("MySQL is not installed")?;
    let mut c = Command::new(crate::mysqlpw::client_exe(&v).ok_or("mysql.exe not found")?);
    c.args(["-u", "root", "-h", "127.0.0.1", &format!("--port={}", s.svc(id).port)]);
    // Password through the environment: the command line is visible to every program on the PC. Empty = the one
    // Ninho remembers for this server (if any).
    crate::mysqlpw::set_env(&mut c, s, id, password);
    c.args(["-e", sql]);
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Local time as HH:MM:SS, for the activity list.
pub fn clock() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) + crate::platform::utc_offset_min() * 60;
    let d = secs.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", d / 3600, d / 60 % 60, d % 60)
}

/// The settings file of a server, when it has one (opened by Config).
pub fn config_file(s: &Settings, id: &str) -> Option<PathBuf> {
    let v = version_dir(s, id)?;
    let f = match s.svc(id).key.as_str() {
        "apache" => v.join("conf").join("httpd.conf"),
        "nginx" => v.join("conf").join("nginx.conf"),
        "mysql" => v.join("my.ini"),
        "php" => v.join("php.ini"),
        "redis" => v.join("redis.windows.conf"),
        "postgres" => PathBuf::from(engine_data(s, s.svc(id), &v)).join("postgresql.conf"),
        "mariadb" => PathBuf::from(engine_data(s, s.svc(id), &v)).join("my.ini"),
        "caddy" => crate::caddy::caddyfile_path(&s.root),
        _ => return None,
    };
    f.is_file().then_some(f)
}

/// The log file of a server, when it writes one (opened by Logs).
pub fn log_file(s: &Settings, id: &str) -> Option<PathBuf> {
    let c = s.svc(id);
    let v = version_dir(s, id)?;
    let f = match c.key.as_str() {
        "apache" | "nginx" => v.join("logs").join("error.log"),
        "mysql" => PathBuf::from(mysql_data(s, c, &v)?).join("mysqld.log"),
        "mailpit" => v.join(format!("{}.log", c.id)),
        "postgres" => PathBuf::from(engine_data(s, c, &v)).join("postgres.log"),
        "mariadb" => PathBuf::from(engine_data(s, c, &v)).join("mariadb.log"),
        "mongodb" => PathBuf::from(engine_data(s, c, &v)).join("mongod.log"),
        "caddy" => crate::caddy::log_path(&s.root),
        _ => return None,
    };
    f.is_file().then_some(f)
}

/// PIDs of this kind of server listening on its port.
pub fn pids(shared: &Shared, s: &Settings, id: &str) -> Vec<u32> {
    let c = s.svc(id);
    let img = image(&c.key);
    let mut v: Vec<u32> = shared.listeners.lock().unwrap().iter().filter(|l| l.port == c.port && l.program.to_ascii_lowercase() == img).map(|l| l.pid).collect();
    v.dedup();
    v
}

/// PATH with the installed tools first, for terminals and project commands.
pub fn tool_path(s: &Settings) -> String {
    let mut parts: Vec<String> = Vec::new();
    for key in ["php", "apache", "nginx", "caddy", "redis"] {
        if let Some(v) = version_dir(s, key) {
            parts.push(v.to_string_lossy().into_owned());
        }
    }
    for key in ["mysql", "mongodb"] {
        if let Some(v) = version_dir(s, key) {
            parts.push(v.join("bin").to_string_lossy().into_owned());
        }
    }
    if let Some(sh) = crate::mongo::mongosh_exe(&s.root).and_then(|p| p.parent().map(Path::to_path_buf)) {
        parts.push(sh.to_string_lossy().into_owned());
    }
    for extra in [&["bin", "composer"][..], &["bin", "git", "cmd"], &["bin", "git", "bin"]] {
        let p = extra.iter().fold(PathBuf::from(&s.root), |p, x| p.join(x));
        if p.is_dir() {
            parts.push(p.to_string_lossy().into_owned());
        }
    }
    if let Ok(node) = std::fs::read_dir(Path::new(&s.root).join("bin").join("nodejs")) {
        if let Some(n) = node.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).max() {
            parts.push(n.to_string_lossy().into_owned());
        }
    }
    parts.push(std::env::var("PATH").unwrap_or_default());
    parts.join(crate::platform::PATH_SEP)
}
