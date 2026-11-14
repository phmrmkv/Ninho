//! "Check my environment": looks at the whole setup and says, in plain language, what is wrong and how to fix it.
//! Read-only on purpose (no file is written, no program is started), so it is safe to run at any time.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::i18n::{Lang, tr};
use crate::settings::{ServiceCfg, Settings};
use crate::{certs, phpini, ports, services, sites};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Ok,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub level: Level,
    pub title: String,
    pub detail: String,
    /// What to do about it (None when there is nothing to do).
    pub fix: Option<String>,
}

/// How many findings are Ok / Warn / Error, for a one-line summary.
pub fn counts(list: &[Finding]) -> (usize, usize, usize) {
    let n = |l: Level| list.iter().filter(|f| f.level == l).count();
    (n(Level::Ok), n(Level::Warn), n(Level::Error))
}

/// A translated text with its {placeholders} filled in ("{port}", "{name}"...).
fn f(l: Lang, key: &str, pairs: &[(&str, &str)]) -> String {
    let mut s = tr(l, key).to_string();
    for (k, v) in pairs {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

fn mk(level: Level, title: impl Into<String>, detail: impl Into<String>, fix: Option<&str>) -> Finding {
    Finding { level, title: title.into(), detail: detail.into(), fix: fix.map(str::to_string) }
}
fn ok(title: impl Into<String>, detail: impl Into<String>) -> Finding {
    mk(Level::Ok, title, detail, None)
}
fn warn(title: impl Into<String>, detail: impl Into<String>, fix: &str) -> Finding {
    mk(Level::Warn, title, detail, Some(fix))
}
fn err(title: impl Into<String>, detail: impl Into<String>, fix: &str) -> Finding {
    mk(Level::Error, title, detail, Some(fix))
}

/// Runs every check (no changes made). Takes a few seconds (netstat, httpd -t, mysql...); call it on a thread.
pub fn check(s: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    // One look at the ports for all checks, so the servers are compared against the same picture.
    let listeners = ports::listening();
    let l = s.language;
    guarded(l, "Visual C++", &mut out, |o| vc_runtime(s, o));
    guarded(l, tr(l, "h_grp_servers"), &mut out, |o| servers(s, &listeners, o));
    guarded(l, "Apache", &mut out, |o| apache(s, o));
    guarded(l, "PHP", &mut out, |o| php(s, o));
    guarded(l, "MySQL", &mut out, |o| mysql(s, &listeners, o));
    guarded(l, tr(l, "h_grp_sites"), &mut out, |o| site_folder(s, o));
    guarded(l, tr(l, "h_grp_hosts"), &mut out, |o| hosts(s, o));
    guarded(l, "HTTPS", &mut out, |o| https(s, o));
    guarded(l, tr(l, "h_grp_disk"), &mut out, |o| disk(s, o));
    out
}

/// One check never takes the others down: a panic inside it becomes a finding, and what it found before stays.
fn guarded(l: Lang, name: &str, out: &mut Vec<Finding>, run_check: impl FnOnce(&mut Vec<Finding>)) {
    let mut local = Vec::new();
    let r = catch_unwind(AssertUnwindSafe(|| run_check(&mut local)));
    out.append(&mut local);
    if r.is_err() {
        out.push(warn(f(l, "h_guard_t", &[("name", name)]), tr(l, "h_guard_d"), tr(l, "h_guard_fix")));
    }
}

/// Runs a program and returns (success, stderr + stdout). None when it cannot even start.
fn run(exe: &Path, args: &[&str], cwd: Option<&Path>) -> Option<(bool, String)> {
    let mut c = Command::new(exe);
    c.args(args).stdin(Stdio::null());
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    services::hide_window(&mut c);
    let o = c.output().ok()?;
    // Apache prints "Syntax OK" on stderr, PHP prints its version on stdout: keep both.
    let mut text = String::from_utf8_lossy(&o.stderr).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&o.stdout));
    Some((o.status.success(), text))
}

// ---------------------------------------------------------------- Visual C++

/// Apache, PHP and PostgreSQL builds for Windows are linked against the 2015-2022 runtime; without it they die
/// at start with a missing-DLL popup, which is the most common "nothing starts" cause.
fn vc_runtime(s: &Settings, out: &mut Vec<Finding>) {
    let l = s.language;
    let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let sys = Path::new(&win).join("System32");
    let missing: Vec<&str> = ["vcruntime140.dll", "msvcp140.dll"].into_iter().filter(|f| !sys.join(f).is_file()).collect();
    let needed = ["apache", "php", "postgres"].iter().any(|k| services::version_dir(s, k).is_some());
    if missing.is_empty() {
        out.push(ok(tr(l, "h_vc_ok_t"), tr(l, "h_vc_ok_d")));
        return;
    }
    let detail = f(l, "h_vc_miss_d", &[("files", &missing.join(", "))]);
    let (title, fix) = (tr(l, "h_vc_miss_t"), tr(l, "h_vc_fix"));
    out.push(if needed { err(title, detail, fix) } else { warn(title, detail, fix) });
}

// ---------------------------------------------------------------- servers and ports

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PortStatus {
    Free,
    /// Held by the program of its own kind (the server is running).
    Own,
    Other,
}

/// Same rule as `Shared::refresh`: the right program name, or a name Windows did not tell, counts as ours.
pub(crate) fn port_status(holder: Option<&str>, image: &str) -> PortStatus {
    match holder {
        None => PortStatus::Free,
        Some(p) if p.is_empty() || p.eq_ignore_ascii_case(image) => PortStatus::Own,
        Some(_) => PortStatus::Other,
    }
}

/// Ports Windows keeps for itself (Hyper-V, WSL, Docker...), from `netsh int ipv4 show excludedportrange`.
/// A line is "  start   end" (a "*" after a number marks an administrator exclusion).
pub(crate) fn parse_excluded(text: &str) -> Vec<(u16, u16)> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace().filter(|w| *w != "*").map(|w| w.trim_end_matches('*'));
            let (a, b) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?);
            it.next().is_none().then_some((a, b))
        })
        .collect()
}

fn excluded_ranges() -> Vec<(u16, u16)> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut c = Command::new("netsh");
    c.args(["int", "ipv4", "show", "excludedportrange", "protocol=tcp"]);
    services::hide_window(&mut c);
    c.output().map(|o| parse_excluded(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

/// Servers (enabled ones) that were given the same port: only one of them can start.
pub(crate) fn duplicate_ports(list: &[(String, u16)]) -> Vec<(u16, Vec<String>)> {
    let mut out: Vec<(u16, Vec<String>)> = Vec::new();
    for (name, port) in list {
        match out.iter_mut().find(|(p, _)| p == port) {
            Some(e) => e.1.push(name.clone()),
            None => out.push((*port, vec![name.clone()])),
        }
    }
    out.retain(|(_, v)| v.len() > 1);
    out
}

fn port_finding(l: Lang, name: &str, port: u16, image: &str, listeners: &[ports::Listener], reserved: &[(u16, u16)]) -> Finding {
    let holder = ports::owner(listeners, port);
    match port_status(holder.map(|l| l.program.as_str()), image) {
        PortStatus::Own => ok(f(l, "h_port_own_t", &[("name", name), ("port", &port.to_string())]), tr(l, "h_port_own_d")),
        PortStatus::Free if reserved.iter().any(|&(a, b)| (a..=b).contains(&port)) => err(
            f(l, "h_port_res_t", &[("name", name), ("port", &port.to_string())]),
            tr(l, "h_port_res_d"),
            tr(l, "h_port_res_fix"),
        ),
        PortStatus::Free => ok(f(l, "h_port_free_t", &[("name", name), ("port", &port.to_string())]), tr(l, "h_port_free_d")),
        PortStatus::Other => {
            let h = holder.expect("Other has a holder");
            let pid = h.pid.to_string();
            let who = if h.program.is_empty() { f(l, "h_pid", &[("pid", &pid)]) } else { f(l, "h_who", &[("program", &h.program), ("pid", &pid)]) };
            let sys = ports::is_system(&h.program, h.pid);
            let port_s = port.to_string();
            let detail = if sys {
                f(l, "h_port_sys_d", &[("port", &port_s), ("who", &who)])
            } else {
                f(l, "h_port_busy_d", &[("port", &port_s), ("who", &who), ("name", name)])
            };
            let fix = tr(l, if sys { "h_port_sys_fix" } else { "h_port_busy_fix" });
            err(f(l, "h_port_busy_t", &[("name", name), ("port", &port_s)]), detail, fix)
        }
    }
}

fn servers(s: &Settings, listeners: &[ports::Listener], out: &mut Vec<Finding>) {
    let l = s.language;
    let on: Vec<&ServiceCfg> = s.services.iter().filter(|c| c.enabled).collect();
    if on.is_empty() {
        out.push(warn(tr(l, "h_none_t"), tr(l, "h_none_d"), tr(l, "h_none_fix")));
        return;
    }
    let reserved = excluded_ranges();
    for c in &on {
        let name = c.title();
        let d = services::def(&c.key);
        if services::version_dir(s, &c.id).is_none() {
            out.push(warn(
                f(l, "h_notinst_t", &[("name", &name)]),
                f(l, "h_notinst_d", &[("exe", &d.exe.to_string()), ("dir", &crate::platform::join(&s.root, &["bin", d.dir]))]),
                tr(l, "h_notinst_fix"),
            ));
            continue;
        }
        out.push(port_finding(l, &name, c.port, &services::image(&c.key), listeners, &reserved));
    }
    // HTTPS needs Apache on 443 too.
    if s.https && on.iter().any(|c| c.key == "apache") {
        out.push(port_finding(l, "Apache (HTTPS)", 443, &services::image("apache"), listeners, &reserved));
    }
    let pairs: Vec<(String, u16)> = on.iter().map(|c| (c.title(), c.port)).collect();
    for (port, names) in duplicate_ports(&pairs) {
        let port_s = port.to_string();
        out.push(err(f(l, "h_dup_t", &[("port", &port_s)]), f(l, "h_dup_d", &[("names", &names.join(", "))]), tr(l, "h_dup_fix")));
    }
}

// ---------------------------------------------------------------- Apache

/// Reads the output of `httpd -t`: Ok, or the line that says what is wrong. "Syntax OK" wins over warnings
/// (the usual AH00558 about the server name does not stop Apache).
pub(crate) fn httpd_verdict(text: &str) -> Result<(), String> {
    if text.to_ascii_lowercase().contains("syntax ok") {
        return Ok(());
    }
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let pick = lines
        .iter()
        .find(|l| l.contains("AH0") || l.to_ascii_lowercase().contains("error") || l.to_ascii_lowercase().contains("cannot"))
        .or(lines.first())
        .map(|l| l.chars().take(300).collect::<String>());
    // Empty when Apache said nothing: the caller words that in the player's language.
    Err(pick.unwrap_or_default())
}

fn apache(s: &Settings, out: &mut Vec<Finding>) {
    let l = s.language;
    if !s.svc("apache").enabled {
        return;
    }
    let Some(v) = services::version_dir(s, "apache") else { return };
    let exe = v.join("bin").join(crate::platform::exe("httpd"));
    // conf/ninho.conf is written at the first start; before that httpd.conf still points at the zip's own folder
    // and the test would complain about folders that are fine.
    if !v.join("conf").join("ninho.conf").is_file() {
        out.push(ok(tr(l, "h_apc_pending_t"), tr(l, "h_apc_pending_d")));
    } else {
        // Linux and macOS: the system Apache needs to be told where Ninho's config is.
        let conf = v.join("conf").join("httpd.conf").to_string_lossy().into_owned();
        let vs = v.to_string_lossy().into_owned();
        let args: Vec<&str> = if cfg!(windows) { vec!["-t"] } else { vec!["-d", &vs, "-f", &conf, "-t"] };
        match run(&exe, &args, Some(&v)) {
            None => out.push(err(tr(l, "h_apc_nostart_t"), f(l, "h_apc_nostart_d", &[("exe", &exe.display().to_string())]), tr(l, "h_apc_nostart_fix"))),
            Some((_, text)) => match httpd_verdict(&text) {
                Ok(()) => out.push(ok(tr(l, "h_apc_ok_t"), tr(l, "h_apc_ok_d"))),
                Err(e) => {
                    let e = if e.is_empty() { tr(l, "h_httpd_noresp").to_string() } else { e };
                    out.push(err(tr(l, "h_apc_bad_t"), e, tr(l, "h_apc_bad_fix")));
                }
            },
        }
    }
    // PHP as an Apache module (Windows; on Linux and macOS it is the system's mod_php, in httpd.conf).
    if let Some(php) = services::version_dir(s, "php").filter(|_| cfg!(windows)) {
        if php_module(&php).is_some() {
            out.push(ok(tr(l, "h_mod_ok_t"), tr(l, "h_mod_ok_d")));
        } else {
            out.push(warn(tr(l, "h_mod_no_t"), tr(l, "h_mod_no_d"), tr(l, "h_mod_no_fix")));
        }
    }
}

fn php_module(php: &Path) -> Option<PathBuf> {
    std::fs::read_dir(php).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        let f = p.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        f.starts_with("php") && f.ends_with("apache2_4.dll")
    })
}

// ---------------------------------------------------------------- PHP

const KEY_EXTENSIONS: &[&str] = &["mysqli", "pdo_mysql", "mbstring", "openssl", "curl"];

/// The key extensions that are not switched on in php.ini (absent or commented out).
pub(crate) fn missing_exts(list: &[(String, bool)], wanted: &[&str]) -> Vec<String> {
    wanted.iter().filter(|w| !list.iter().any(|(n, on)| *on && n == *w)).map(|w| w.to_string()).collect()
}

/// The value of the active `extension_dir` line, without quotes.
pub(crate) fn extension_dir_value(ini: &str) -> Option<String> {
    ini.lines().map(str::trim).filter(|l| !l.starts_with(';')).find_map(|l| {
        let rest = l.strip_prefix("extension_dir")?.trim_start().strip_prefix('=')?;
        let v = rest.split(';').next().unwrap_or("").trim().trim_matches('"').to_string();
        (!v.is_empty()).then_some(v)
    })
}

fn php(s: &Settings, out: &mut Vec<Finding>) {
    let l = s.language;
    let Some(v) = services::version_dir(s, "php") else { return };
    let label = v.file_name().unwrap_or_default().to_string_lossy().into_owned();
    // -n: no php.ini, so a broken ini cannot hide a broken PHP (and the other way round).
    match run(&v.join(crate::platform::exe("php")), &["-n", "-v"], Some(&v)) {
        Some((true, text)) if text.contains("PHP") => {
            let line = text.lines().map(str::trim).find(|x| x.starts_with("PHP")).unwrap_or(tr(l, "h_php_run_d")).to_string();
            out.push(ok(f(l, "h_php_run_t", &[("label", &label)]), line));
        }
        _ => {
            out.push(err(f(l, "h_php_norun_t", &[("label", &label)]), tr(l, "h_php_norun_d"), tr(l, "h_php_norun_fix")));
            return;
        }
    }
    // Linux and macOS: the system PHP keeps its php.ini (and extensions) where the package manager put them.
    if !cfg!(windows) {
        return;
    }
    let ini = v.join("php.ini");
    let Ok(text) = std::fs::read_to_string(&ini) else {
        out.push(warn(tr(l, "h_ini_none_t"), f(l, "h_ini_none_d", &[("path", &ini.display().to_string())]), tr(l, "h_ini_none_fix")));
        return;
    };
    out.push(ok(tr(l, "h_ini_ok_t"), ini.display().to_string()));
    match extension_dir_value(&text) {
        None => out.push(warn(tr(l, "h_extdir_none_t"), tr(l, "h_extdir_none_d"), tr(l, "h_extdir_none_fix"))),
        Some(d) => {
            let p = PathBuf::from(&d);
            let full = if p.is_absolute() { p } else { v.join(p) };
            if !full.is_dir() {
                out.push(warn(tr(l, "h_extdir_bad_t"), f(l, "h_extdir_bad_d", &[("path", &full.display().to_string())]), tr(l, "h_extdir_bad_fix")));
            }
        }
    }
    let bad = missing_exts(&phpini::list(&ini), KEY_EXTENSIONS);
    if bad.is_empty() {
        out.push(ok(tr(l, "h_exts_ok_t"), KEY_EXTENSIONS.join(", ")));
    } else {
        out.push(warn(tr(l, "h_exts_bad_t"), f(l, "h_exts_bad_d", &[("list", &bad.join(", "))]), tr(l, "h_exts_bad_fix")));
    }
}

// ---------------------------------------------------------------- MySQL

/// A short plain-language reason for a failed `SELECT 1`.
pub(crate) fn mysql_reason(l: Lang, e: &str) -> String {
    let low = e.to_ascii_lowercase();
    if low.contains("access denied") {
        tr(l, "h_my_reason_pw").into()
    } else if low.contains("can't connect") || low.contains("cant connect") || low.contains("2003") {
        tr(l, "h_my_reason_conn").into()
    } else {
        e.lines().find(|l| !l.trim().is_empty()).unwrap_or(tr(l, "h_my_reason_unk")).trim().chars().take(200).collect()
    }
}

fn mysql(s: &Settings, listeners: &[ports::Listener], out: &mut Vec<Finding>) {
    let l = s.language;
    for c in s.services.iter().filter(|c| c.enabled && c.key == "mysql") {
        let Some(v) = services::version_dir(s, &c.id) else { continue };
        let name = c.title();
        // Data folder: the first MySQL cannot start without it; an extra one is created at its first start.
        match services::mysql_data(s, c, &v) {
            Some(d) if Path::new(&d).is_dir() => out.push(ok(f(l, "h_my_data_ok_t", &[("name", &name)]), d)),
            Some(d) if c.is_extra() => out.push(ok(f(l, "h_my_data_new_t", &[("name", &name)]), d)),
            Some(d) => out.push(err(f(l, "h_my_data_no_t", &[("name", &name)]), f(l, "h_my_data_no_d", &[("path", &d)]), tr(l, "h_my_data_no_fix"))),
            None => out.push(warn(f(l, "h_my_nodd_t", &[("name", &name)]), tr(l, "h_my_nodd_d"), tr(l, "h_my_nodd_fix"))),
        }
        let running = port_status(ports::owner(listeners, c.port).map(|l| l.program.as_str()), &services::image("mysql")) == PortStatus::Own;
        if !running {
            continue;
        }
        match services::mysql_exec(s, &c.id, "SELECT 1", "") {
            Ok(()) => out.push(ok(f(l, "h_my_q_t", &[("name", &name)]), tr(l, "h_my_q_d"))),
            Err(e) => out.push(warn(
                f(l, "h_my_login_t", &[("name", &name)]),
                f(l, "h_my_login_d", &[("port", &c.port.to_string()), ("reason", &mysql_reason(l, &e))]),
                tr(l, "h_my_login_fix"),
            )),
        }
    }
}

// ---------------------------------------------------------------- sites and hosts

fn site_folder(s: &Settings, out: &mut Vec<Finding>) {
    let l = s.language;
    if !Path::new(&s.doc_root).is_dir() {
        out.push(warn(tr(l, "h_site_no_t"), f(l, "h_site_no_d", &[("path", &s.doc_root)]), tr(l, "h_site_no_fix")));
        return;
    }
    let n = sites::list(s).len();
    out.push(ok(f(l, "h_site_ok_t", &[("n", &n.to_string())]), s.doc_root.clone()));
}

fn hosts_path() -> PathBuf {
    PathBuf::from(crate::platform::hosts_path())
}

/// How many lines of the hosts file Ninho owns.
pub(crate) fn count_marked(hosts: &str) -> usize {
    hosts.lines().filter(|l| l.contains(sites::MARKER)).count()
}

fn hosts(s: &Settings, out: &mut Vec<Finding>) {
    let l = s.language;
    let path = hosts_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        out.push(warn(tr(l, "h_hosts_read_t"), f(l, "h_hosts_read_d", &[("path", &path.display().to_string())]), tr(l, "h_hosts_read_fix")));
        return;
    };
    let marked = count_marked(&text);
    // Opening for append changes nothing; it only tells whether Windows would let us write without asking.
    let writable = std::fs::OpenOptions::new().append(true).open(&path).is_ok();
    let access = tr(l, if writable { "h_hosts_w" } else { "h_hosts_a" });
    if !s.auto_vhosts {
        out.push(ok(tr(l, "h_hosts_off_t"), f(l, "h_hosts_off_d", &[("n", &marked.to_string()), ("access", access)])));
        return;
    }
    let want = sites::list(s).len();
    if marked == want {
        out.push(ok(f(l, "h_hosts_ok_t", &[("n", &marked.to_string())]), access));
    } else {
        out.push(warn(
            tr(l, "h_hosts_sync_t"),
            f(l, "h_hosts_sync_d", &[("want", &want.to_string()), ("marked", &marked.to_string()), ("access", access)]),
            tr(l, "h_hosts_sync_fix"),
        ));
    }
}

fn https(s: &Settings, out: &mut Vec<Finding>) {
    let l = s.language;
    if !s.https {
        return;
    }
    // Same file name certs.rs gives the local authority's certificate.
    let crt = certs::dir(s).join("ninho-ca.crt");
    if !crt.is_file() {
        out.push(warn(tr(l, "h_tls_nocert_t"), tr(l, "h_tls_nocert_d"), tr(l, "h_tls_nocert_fix")));
    } else if certs::ca_trusted(s) {
        out.push(ok(tr(l, "h_tls_ok_t"), tr(l, "h_tls_ok_d")));
    } else {
        out.push(warn(tr(l, "h_tls_untr_t"), tr(l, "h_tls_untr_d"), tr(l, "h_tls_untr_fix")));
    }
}

// ---------------------------------------------------------------- disk

const GB: u64 = 1 << 30;

pub(crate) fn disk_level(free: u64) -> Level {
    if free < GB / 2 {
        Level::Error
    } else if free < 2 * GB {
        Level::Warn
    } else {
        Level::Ok
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetDiskFreeSpaceExW(dir: *const u16, free_to_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> i32;
}

/// Free bytes on the drive that holds `path` (its nearest folder that exists, since the root may not be created yet).
#[cfg(windows)]
fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let dir = path.ancestors().find(|p| p.is_dir())?;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and the three outputs are valid u64 locals.
    let r = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
    (r != 0).then_some(avail)
}

/// Linux and macOS: `df -Pk` (POSIX output, 1 KB blocks), column "Available" of its second line.
#[cfg(not(windows))]
fn free_bytes(path: &Path) -> Option<u64> {
    let dir = path.ancestors().find(|p| p.is_dir())?;
    let out = Command::new("df").arg("-Pk").arg(dir).stdin(Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let cols: Vec<&str> = text.lines().nth(1)?.split_whitespace().collect();
    cols.get(3)?.parse::<u64>().ok().map(|k| k * 1024)
}

/// Gigabytes with one decimal, written the way the language writes numbers (1,5 or 1.5).
fn gb(l: Lang, b: u64) -> String {
    let n = format!("{:.1}", b as f64 / GB as f64);
    if tr(l, "h_decimal") == "," { n.replace('.', ",") } else { n }
}

fn disk(s: &Settings, out: &mut Vec<Finding>) {
    let l = s.language;
    let Some(free) = free_bytes(Path::new(&s.root)) else { return };
    let title = f(l, "h_disk_t", &[("gb", &gb(l, free))]);
    match disk_level(free) {
        Level::Ok => out.push(ok(title, f(l, "h_disk_ok_d", &[("path", &s.root)]))),
        Level::Warn => out.push(warn(title, tr(l, "h_disk_warn_d"), tr(l, "h_disk_warn_fix"))),
        Level::Error => out.push(err(title, tr(l, "h_disk_err_d"), tr(l, "h_disk_err_fix"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn httpd_syntax_ok_even_with_warning() {
        let t = "AH00558: httpd.exe: Could not reliably determine the server's fully qualified domain name\r\nSyntax OK\r\n";
        assert!(httpd_verdict(t).is_ok());
    }

    #[test]
    fn httpd_error_picks_the_useful_line() {
        let t = "\nAH00526: Syntax error on line 12 of C:/x/conf/httpd.conf:\nInvalid command 'Foo'\n";
        let e = httpd_verdict(t).unwrap_err();
        assert!(e.starts_with("AH00526"), "{e}");
        assert!(httpd_verdict("").is_err());
    }

    #[test]
    fn port_owner_rules() {
        assert_eq!(port_status(None, "httpd.exe"), PortStatus::Free);
        assert_eq!(port_status(Some("HTTPD.exe"), "httpd.exe"), PortStatus::Own);
        assert_eq!(port_status(Some(""), "httpd.exe"), PortStatus::Own);
        assert_eq!(port_status(Some("skype.exe"), "httpd.exe"), PortStatus::Other);
    }

    #[test]
    fn excluded_ranges_parse() {
        let t = "\nProtocol tcp Port Exclusion Ranges\n\nStart Port    End Port\n----------    --------\n      1025        1124\n     50000       50059     *\n\n* - Administered port exclusions.\n";
        assert_eq!(parse_excluded(t), vec![(1025, 1124), (50000, 50059)]);
    }

    #[test]
    fn duplicates_found() {
        let l = vec![("Apache".to_string(), 80), ("Nginx".to_string(), 80), ("MySQL".to_string(), 3306)];
        let d = duplicate_ports(&l);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].0, 80);
        assert_eq!(d[0].1, vec!["Apache", "Nginx"]);
    }

    #[test]
    fn php_extensions_missing() {
        let l = vec![("mysqli".to_string(), true), ("curl".to_string(), false), ("mbstring".to_string(), true)];
        assert_eq!(missing_exts(&l, KEY_EXTENSIONS), vec!["pdo_mysql", "openssl", "curl"]);
    }

    #[test]
    fn extension_dir_read() {
        assert_eq!(extension_dir_value("; extension_dir = \"x\"\nextension_dir = \"ext\" ; comment\n").as_deref(), Some("ext"));
        assert_eq!(extension_dir_value(";extension_dir = \"ext\"\n"), None);
    }

    #[test]
    fn hosts_marker_count() {
        let t = format!("127.0.0.1 localhost\n127.0.0.1      a.test    {m}\n127.0.0.1      b.test    {m}\n", m = sites::MARKER);
        assert_eq!(count_marked(&t), 2);
    }

    #[test]
    fn disk_thresholds() {
        assert_eq!(disk_level(10 * GB), Level::Ok);
        assert_eq!(disk_level(2 * GB), Level::Ok);
        assert_eq!(disk_level(GB), Level::Warn);
        assert_eq!(disk_level(GB / 4), Level::Error);
        let (pt, en) = (Lang::from_code("pt-BR"), Lang::from_code("en"));
        assert_eq!(gb(pt, 3 * GB / 2), "1,5");
        assert_eq!(gb(en, 3 * GB / 2), "1.5");
    }

    #[test]
    fn mysql_reasons() {
        let pt = Lang::from_code("pt-BR");
        assert_eq!(mysql_reason(pt, "ERROR 1045 (28000): Access denied for user 'root'"), tr(pt, "h_my_reason_pw"));
        assert_eq!(mysql_reason(pt, "ERROR 2003 (HY000): Can't connect to MySQL server"), tr(pt, "h_my_reason_conn"));
        assert_eq!(mysql_reason(pt, "boom\nx"), "boom");
    }

    #[test]
    fn counts_levels() {
        let l = vec![ok("a", ""), warn("b", "", "x"), err("c", "", "y"), ok("d", "")];
        assert_eq!(counts(&l), (2, 1, 1));
    }

    #[test]
    fn a_panic_inside_a_check_is_contained() {
        let mut out = Vec::new();
        guarded(Lang::from_code("en"), "x", &mut out, |o| {
            o.push(ok("before", ""));
            panic!("boom");
        });
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].level, Level::Warn);
    }

    #[test]
    fn english_findings_have_no_portuguese() {
        let en = Lang::from_code("en");
        let mut out = Vec::new();
        guarded(en, "Disk", &mut out, |_| panic!("boom"));
        let listeners = [ports::Listener { port: 80, addrs: vec![], pid: 4242, program: "skype.exe".into() }];
        out.push(port_finding(en, "Apache", 80, "httpd.exe", &listeners, &[]));
        for x in &out {
            let all = format!("{} {} {}", x.title, x.detail, x.fix.clone().unwrap_or_default());
            for pt in ["Não", "não", "porta ", "Tente", "Feche", "Servidor", "está"] {
                assert!(!all.contains(pt), "{pt:?} in {all}");
            }
            assert!(!all.contains('?'), "missing key: {all}");
        }
        assert!(out[1].title.contains("port 80") && out[1].detail.contains("skype.exe (PID 4242)"), "{:?}", out[1]);
    }
}
