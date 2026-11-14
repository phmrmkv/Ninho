//! Root password of MySQL / MariaDB: change it from Ninho and remember it, protected by Windows (DPAPI, this user
//! only), so the polite stop, the database tools, the importer and the project database check keep working.
//! Passwords never go on a command line: the current one travels in `MYSQL_PWD`, the new one through stdin.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::services::{hide_window, version_dir};
use crate::settings::Settings;

// ---- base64 (std has none; Ninho takes no new crates) ----

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.trim().trim_end_matches('=').bytes() {
        let v = B64.iter().position(|&b| b == c)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
        }
    }
    Some(out)
}

// ---- Windows DPAPI ----

#[cfg(windows)]
mod dpapi {
    use std::ffi::c_void;

    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(input: *const Blob, desc: *const u16, entropy: *const Blob, reserved: *mut c_void, prompt: *mut c_void, flags: u32, out: *mut Blob) -> i32;
        fn CryptUnprotectData(input: *const Blob, desc: *mut *mut u16, entropy: *const Blob, reserved: *mut c_void, prompt: *mut c_void, flags: u32, out: *mut Blob) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(mem: *mut c_void) -> *mut c_void;
    }

    /// Extra secret mixed into the key: another program of the same user cannot decrypt it by accident.
    const ENTROPY: &[u8] = b"Ninho root password";
    /// CRYPTPROTECT_UI_FORBIDDEN: never show a window. Flags 0 otherwise = scope of the current user.
    const UI_FORBIDDEN: u32 = 1;

    fn blob(data: &[u8]) -> Blob {
        Blob { len: data.len() as u32, data: data.as_ptr() as *mut u8 }
    }

    fn take(out: Blob) -> Vec<u8> {
        // SAFETY: Windows allocated `out.data` with `out.len` bytes; it is copied, then released with LocalFree.
        unsafe {
            let v = std::slice::from_raw_parts(out.data, out.len as usize).to_vec();
            LocalFree(out.data as *mut c_void);
            v
        }
    }

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
        let (input, entropy) = (blob(plain), blob(ENTROPY));
        let mut out = Blob { len: 0, data: std::ptr::null_mut() };
        // SAFETY: the blobs point at slices that live through the call; `out` is filled by Windows on success.
        let ok = unsafe { CryptProtectData(&input, std::ptr::null(), &entropy, std::ptr::null_mut(), std::ptr::null_mut(), UI_FORBIDDEN, &mut out) };
        if ok == 0 { Err("Windows could not protect the password (CryptProtectData)".into()) } else { Ok(take(out)) }
    }

    pub fn unprotect(sealed: &[u8]) -> Result<Vec<u8>, String> {
        let (input, entropy) = (blob(sealed), blob(ENTROPY));
        let mut out = Blob { len: 0, data: std::ptr::null_mut() };
        // SAFETY: same as `protect`; the description string Windows may return is not requested (null).
        let ok = unsafe { CryptUnprotectData(&input, std::ptr::null_mut(), &entropy, std::ptr::null_mut(), std::ptr::null_mut(), UI_FORBIDDEN, &mut out) };
        if ok == 0 { Err("Windows could not unlock the saved password (another user or another PC?)".into()) } else { Ok(take(out)) }
    }
}

#[cfg(not(windows))]
mod dpapi {
    pub fn protect(_: &[u8]) -> Result<Vec<u8>, String> {
        Err("password protection needs Windows".into())
    }
    pub fn unprotect(_: &[u8]) -> Result<Vec<u8>, String> {
        Err("password protection needs Windows".into())
    }
}

/// Password to text safe for ninho.json: DPAPI (current user), then base64.
pub fn protect(plain: &str) -> Result<String, String> {
    dpapi::protect(plain.as_bytes()).map(|b| b64_encode(&b))
}

/// Back from `protect`. Fails on a damaged value, or one made by another Windows user.
pub fn unprotect(text: &str) -> Result<String, String> {
    let sealed = b64_decode(text).ok_or("the saved password is damaged")?;
    let plain = dpapi::unprotect(&sealed)?;
    String::from_utf8(plain).map_err(|_| "the saved password is damaged".to_string())
}

// ---- the password Ninho remembers for one server ----

/// The remembered root password of server `id` ("" = none remembered, or it cannot be unlocked).
pub fn stored(s: &Settings, id: &str) -> String {
    match s.services.iter().find(|c| c.id == id) {
        Some(c) if !c.root_pw_dpapi.is_empty() => unprotect(&c.root_pw_dpapi).unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn has_stored(s: &Settings, id: &str) -> bool {
    s.services.iter().any(|c| c.id == id && !c.root_pw_dpapi.is_empty())
}

/// What to log in with: what the person typed, else the remembered password.
pub fn effective(s: &Settings, id: &str, typed: &str) -> String {
    if typed.is_empty() { stored(s, id) } else { typed.to_string() }
}

/// Put the password for `id` in the command's environment (never on its command line). "" typed = the remembered one.
pub fn set_env(cmd: &mut Command, s: &Settings, id: &str, typed: &str) {
    let pw = effective(s, id, typed);
    if !pw.is_empty() {
        cmd.env("MYSQL_PWD", pw);
    }
}

// ---- running the server's own client ----

/// `mysql.exe`, or `mariadb.exe` (newer MariaDB has no mysql.exe), in a version folder's bin.
pub fn client_exe(version_dir: &Path) -> Option<PathBuf> {
    ["mysql", "mariadb"].iter().map(|n| version_dir.join("bin").join(crate::platform::exe(n))).find(|p| p.is_file())
}

fn base_cmd(s: &Settings, id: &str, password: &str) -> Result<Command, String> {
    let v = version_dir(s, id).ok_or("the server is not installed")?;
    let exe = client_exe(&v).ok_or_else(|| format!("mysql.exe not found in {}", v.join("bin").display()))?;
    let mut c = Command::new(exe);
    c.arg("--no-defaults").args(["-h", "127.0.0.1", "-u", "root", "--protocol=tcp", "--default-character-set=utf8mb4"]);
    c.arg(format!("--port={}", s.svc(id).port));
    if !password.is_empty() {
        c.env("MYSQL_PWD", password);
    }
    hide_window(&mut c);
    Ok(c)
}

fn err_text(out: &std::process::Output) -> String {
    let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if e.is_empty() { format!("exited with {}", out.status) } else { e }
}

/// Lines a query prints (no header), with this exact password.
fn query_with(s: &Settings, id: &str, password: &str, sql: &str) -> Result<Vec<String>, String> {
    let mut c = base_cmd(s, id, password)?;
    c.args(["-N", "-B", "-e", sql]);
    let out = c.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(err_text(&out));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

/// Names of the databases of a MySQL / MariaDB server (all of them, system ones included), with the remembered
/// password when `typed` is empty.
pub fn list_databases(s: &Settings, id: &str, typed: &str) -> Result<Vec<String>, String> {
    query_with(s, id, &effective(s, id, typed), "SHOW DATABASES")
}

// ---- SQL ----

/// `'...'` for a string in SQL text. Backslash and quote are doubled. NUL cannot be written (checked before).
pub fn sql_literal(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
}

/// The SQL that sets the root password for every `root` account (`hosts`: "localhost", "127.0.0.1", "::1"...).
/// The first statement drops NO_BACKSLASH_ESCAPES for this session, so the doubled backslashes mean what they say.
pub fn alter_sql(hosts: &[String], new_password: &str) -> String {
    let mut sql = String::from(
        "SET SESSION sql_mode = TRIM(BOTH ',' FROM REPLACE(CONCAT(',', @@SESSION.sql_mode, ','), ',NO_BACKSLASH_ESCAPES,', ','));\n",
    );
    let hosts: Vec<String> = if hosts.is_empty() { vec!["localhost".into()] } else { hosts.to_vec() };
    for h in hosts {
        sql.push_str(&format!("ALTER USER 'root'@{} IDENTIFIED BY {};\n", sql_literal(&h), sql_literal(new_password)));
    }
    sql
}

/// A password for the Windows MySQL / MariaDB client has to be printable ASCII: it reaches the client through the
/// environment, in the PC's ANSI code page, where accents and other scripts come out as different characters.
pub fn check(password: &str) -> bool {
    password.bytes().all(|b| (0x20..0x7f).contains(&b))
}

/// Why a change failed, so the window can say it in the person's language.
#[derive(Debug, PartialEq)]
pub enum Fail {
    /// Not plain printable ASCII (see `check`).
    BadChar,
    /// The current password is wrong (ERROR 1045).
    WrongCurrent,
    /// The server does not answer (ERROR 2003).
    NoConnect,
    NotInstalled,
    Other(String),
}

pub fn classify(stderr: &str) -> Fail {
    let e = stderr.to_ascii_lowercase();
    if e.contains("1045") || e.contains("access denied") {
        Fail::WrongCurrent
    } else if e.contains("2003") || e.contains("can't connect") || e.contains("cannot connect") {
        Fail::NoConnect
    } else if e.contains("not installed") {
        Fail::NotInstalled
    } else {
        Fail::Other(stderr.trim().to_string())
    }
}

/// Change the root password of a running server. `current` empty = the remembered one (or none).
/// Checks the result by logging in with the new password.
pub fn change(s: &Settings, id: &str, current: &str, new: &str) -> Result<(), Fail> {
    if !check(new) {
        return Err(Fail::BadChar);
    }
    let cur = effective(s, id, current);
    let hosts = query_with(s, id, &cur, "SELECT Host FROM mysql.user WHERE User = 'root'").map_err(|e| classify(&e))?;
    let sql = alter_sql(&hosts, new);
    let mut c = base_cmd(s, id, &cur).map_err(|e| classify(&e))?;
    // The new password goes through stdin: it is never on a command line.
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| Fail::Other(e.to_string()))?;
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(sql.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| Fail::Other(e.to_string()))?;
    if !out.status.success() {
        return Err(classify(&err_text(&out)));
    }
    query_with(s, id, new, "SELECT 1").map(|_| ()).map_err(|e| Fail::Other(format!("login with the new password failed: {e}")))
}

// ---- the window ----

use std::sync::{Arc, Mutex};

type Slot = Arc<Mutex<Option<Result<(), Fail>>>>;

/// State of the "Change root password" window.
#[derive(Default)]
pub struct PwWin {
    pub open: bool,
    pub id: String,
    pub current: String,
    pub new: String,
    pub repeat: String,
    pub remember: bool,
    pub err: Option<String>,
    pub busy: bool,
    pub slot: Slot,
    /// What was sent, to remember it when the server says yes.
    pub sent_new: String,
    pub sent_remember: bool,
}

impl PwWin {
    pub fn open_for(&mut self, id: &str) {
        *self = PwWin { open: true, id: id.to_string(), remember: true, ..Default::default() };
    }
}

/// Set (or clear) the remembered password of server `id`. Returns false when Windows could not protect it.
pub fn remember(s: &mut Settings, id: &str, password: Option<&str>) -> bool {
    let sealed = match password {
        Some(p) if !p.is_empty() => match protect(p) {
            Ok(t) => t,
            Err(_) => return false,
        },
        _ => String::new(),
    };
    if let Some(c) = s.services.iter_mut().find(|c| c.id == id) {
        c.root_pw_dpapi = sealed;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        for s in ["", "a", "ab", "abc", "abcd", "senha com ç e ÿ \u{0}\u{1}"] {
            assert_eq!(b64_decode(&b64_encode(s.as_bytes())).unwrap(), s.as_bytes(), "{s:?}");
        }
        assert_eq!(b64_encode(b"Man"), "TWFu");
        assert_eq!(b64_encode(b"Ma"), "TWE=");
        assert_eq!(b64_encode(b"M"), "TQ==");
        assert!(b64_decode("not base64!").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_round_trip() {
        for pw in ["", "x", "s3nh@ 'quote' \\ \"dq\" ç ÿ 日本語 \n;--"] {
            let sealed = protect(pw).unwrap();
            // The saved text never holds the password itself.
            if pw.len() > 4 {
                assert!(!sealed.contains(pw));
                assert!(!b64_decode(&sealed).unwrap().windows(pw.len()).any(|w| w == pw.as_bytes()));
            }
            assert_eq!(unprotect(&sealed).unwrap(), pw);
        }
        assert!(unprotect("AAAA").is_err(), "garbage is refused");
        assert!(unprotect("%%%").is_err());
    }

    #[test]
    fn sql_escaping() {
        assert_eq!(sql_literal("abc"), "'abc'");
        assert_eq!(sql_literal("it's"), "'it''s'");
        assert_eq!(sql_literal(r"a\b"), r"'a\\b'");
        assert_eq!(sql_literal(r"\'"), r"'\\'''");
        assert_eq!(sql_literal(""), "''");
        assert_eq!(sql_literal("a;b--c\"d"), "'a;b--c\"d'");
        assert_eq!(sql_literal("ç日本"), "'ç日本'");
    }

    #[test]
    fn alter_statements() {
        let sql = alter_sql(&["localhost".into(), "127.0.0.1".into(), "::1".into()], "p'w\\");
        assert!(sql.contains("ALTER USER 'root'@'localhost' IDENTIFIED BY 'p''w\\\\';"));
        assert!(sql.contains("ALTER USER 'root'@'127.0.0.1' IDENTIFIED BY 'p''w\\\\';"));
        assert!(sql.contains("ALTER USER 'root'@'::1'"));
        assert_eq!(sql.matches("ALTER USER").count(), 3);
        assert!(sql.starts_with("SET SESSION sql_mode"));
        // No account found: still root@localhost.
        assert!(alter_sql(&[], "x").contains("'root'@'localhost'"));
        // A host cannot break out of its quotes either.
        assert!(alter_sql(&["a'; DROP".into()], "x").contains("'a''; DROP'"));
    }

    #[test]
    fn only_plain_ascii_passwords() {
        assert!(check("") && check("abc 123 !@#$%^&*()_+-=[]{};:'\",.<>/?`~|\\"));
        for bad in ["açúcar", "日本", "tab\there", "nul\0", "line\nbreak", "\u{7f}"] {
            assert!(!check(bad), "{bad:?}");
        }
    }

    #[test]
    fn error_kinds() {
        assert_eq!(classify("ERROR 1045 (28000): Access denied for user 'root'@'localhost' (using password: YES)"), Fail::WrongCurrent);
        assert_eq!(classify("ERROR 2003 (HY000): Can't connect to MySQL server on '127.0.0.1:3306' (10061)"), Fail::NoConnect);
        assert_eq!(classify("the server is not installed"), Fail::NotInstalled);
        assert_eq!(classify("ERROR 1064 syntax"), Fail::Other("ERROR 1064 syntax".into()));
    }

    #[test]
    fn effective_prefers_typed() {
        let s = Settings::default();
        assert_eq!(effective(&s, "mysql", "typed"), "typed");
        assert_eq!(effective(&s, "mysql", ""), "");
    }

    #[cfg(windows)]
    #[test]
    fn remembered_password_is_used() {
        let mut s = Settings::default();
        assert!(remember(&mut s, "mysql", Some("abc'def")));
        assert!(has_stored(&s, "mysql"));
        assert_eq!(effective(&s, "mysql", ""), "abc'def");
        assert_eq!(effective(&s, "mysql", "other"), "other");
        // ninho.json holds the sealed text, not the password.
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("root_pw_dpapi") && !json.contains("abc'def"));
        assert!(remember(&mut s, "mysql", None));
        assert!(!has_stored(&s, "mysql"));
        assert!(!serde_json::to_string(&s).unwrap().contains("root_pw_dpapi"));
    }
}

/// Starts a throwaway server (its own temp data folder, a free port), changes its root password, then checks that
/// everything Ninho does with it still works, including the polite stop. Run: `cargo test --release live_pw -- --ignored --nocapture`.
#[cfg(test)]
mod live_pw {
    use super::*;
    use crate::services::{self, Shared, port_open};
    use crate::settings::ServiceCfg;

    fn run(key: &str, id: &str, port: u16) {
        let dir = std::env::temp_dir().join(format!("ninho-live-{id}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Settings::default();
        s.root = r"C:\laragon".into();
        let mut c = ServiceCfg::base(key);
        (c.id, c.port, c.data_dir, c.enabled) = (id.into(), port, dir.to_string_lossy().into_owned(), true);
        s.services.push(c);
        let shared = Shared::new();
        services::start(&shared, &s, id);
        assert!(port_open(port), "server did not start: {:?}", shared.log.lock().unwrap());

        let pw = r#"p'w\;"&x%y ~`^"#;
        // Fresh server: no password. Wrong "current" is refused only once there is one.
        change(&s, id, "", pw).expect("first change");
        assert_eq!(classify(&list_databases(&s, id, "").unwrap_err()), Fail::WrongCurrent, "old empty password must be refused");
        assert!(list_databases(&s, id, pw).is_ok(), "the exact new password works");
        assert_eq!(change(&s, id, "wrong", "zzz").unwrap_err(), Fail::WrongCurrent);

        // Remembered: every Ninho tool works without typing it.
        assert!(remember(&mut s, id, Some(pw)));
        assert!(list_databases(&s, id, "").is_ok());
        services::mysql_exec(&s, id, "CREATE DATABASE IF NOT EXISTS ninho_pw_live", "").expect("mysql_exec uses the remembered password");
        crate::project::create_database(&s, id, "shop").expect("project database");
        let dbs = list_databases(&s, id, "").unwrap();
        assert!(dbs.iter().any(|d| d == "shop") && dbs.iter().any(|d| d == "ninho_pw_live"), "{dbs:?}");

        // Change again, from the remembered one (current left empty), to another awkward password.
        let pw2 = r#"second\'--;#"#;
        change(&s, id, "", pw2).expect("second change");
        assert!(list_databases(&s, id, pw).is_err());
        assert!(remember(&mut s, id, Some(pw2)));
        assert!(list_databases(&s, id, "").is_ok());

        // Polite stop with the password (mysqladmin / mariadb-admin), not a kill.
        services::stop(&shared, &s, id);
        let log = shared.log.lock().unwrap().join("\n");
        assert!(!port_open(port), "still answering: {log}");
        assert!(!log.contains(crate::i18n::tr(s.language, "log_mysql_force_stop")), "had to kill it: {log}");
        eprintln!("{key}: password change, remembered use and polite stop all worked");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore]
    fn live_pw_mysql() {
        run("mysql", "mysql-livepw", 33999);
    }

    #[test]
    #[ignore]
    fn live_pw_mariadb() {
        run("mariadb", "mariadb-livepw", 33998);
    }
}
