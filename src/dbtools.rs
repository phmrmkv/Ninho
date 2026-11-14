//! MySQL backup / restore (DDEV-style snapshots). Everything here blocks, so the caller runs it on a thread.
//! Backups are plain `.sql` files: std has no gzip and Ninho takes no new crates.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::services::{hide_window, version_dir};
use crate::settings::Settings;

/// Schemas that belong to the server; never listed, never written to.
const SYSTEM_SCHEMAS: [&str; 4] = ["mysql", "sys", "information_schema", "performance_schema"];

pub struct Snapshot {
    pub file: PathBuf,
    pub database: String,
    /// "2026-11-13 21:40", read from the file name.
    pub when: String,
    pub bytes: u64,
}

fn is_system(db: &str) -> bool {
    SYSTEM_SCHEMAS.iter().any(|n| n.eq_ignore_ascii_case(db))
}

/// Names go inside backticks and into file names, so keep them to a safe alphabet.
fn valid_name(db: &str) -> bool {
    !db.is_empty() && db.len() <= 64 && db.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '$'))
}

fn check_db(db: &str) -> Result<(), String> {
    if !valid_name(db) {
        return Err(format!("invalid database name '{db}' (use letters, digits, _ - $)"));
    }
    if is_system(db) {
        return Err(format!("'{db}' is a system schema"));
    }
    Ok(())
}

fn backups_dir(s: &Settings, id: &str) -> PathBuf {
    // The id only comes from the app, but it becomes a folder name: keep it plain.
    let safe: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).collect();
    Path::new(&s.root).join("backups").join(safe)
}

/// A client command (`mysql.exe` / `mysqldump.exe`) wired to the server; password via env, never argv.
fn client(s: &Settings, id: &str, exe: &str, password: &str) -> Result<Command, String> {
    let v = version_dir(s, id).ok_or("MySQL is not installed")?;
    let path = v.join("bin").join(exe);
    if !path.is_file() {
        return Err(format!("{exe} not found in {}", v.join("bin").display()));
    }
    let mut c = Command::new(path);
    c.args(["-h", "127.0.0.1", "-u", "root"]);
    c.arg(format!("--port={}", s.svc(id).port));
    if !password.is_empty() {
        c.env("MYSQL_PWD", password);
    }
    hide_window(&mut c);
    Ok(c)
}

fn stderr_of(out: &std::process::Output) -> String {
    let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if e.is_empty() { format!("exited with {}", out.status) } else { e }
}

/// Databases on one MySQL server, without system schemas (mysql, sys, information_schema, performance_schema).
pub fn databases(s: &Settings, id: &str, password: &str) -> Result<Vec<String>, String> {
    let mut c = client(s, id, "mysql.exe", password)?;
    // -N: no header, -B: one name per line.
    c.args(["-N", "-B", "-e", "SHOW DATABASES"]);
    let out = c.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(stderr_of(&out));
    }
    let mut list: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !is_system(l))
        .collect();
    list.sort_by_key(|n| n.to_lowercase());
    Ok(list)
}

/// Dump one database to `<root>\backups\<id>\<db>-YYYYMMDD-HHMMSS.sql` (plain text) via mysqldump
/// `--single-transaction --routines --events --hex-blob --default-character-set=utf8mb4`. Returns the file.
pub fn backup(s: &Settings, id: &str, db: &str, password: &str) -> Result<PathBuf, String> {
    check_db(db)?;
    let dir = backups_dir(s, id);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let file = dir.join(format!("{db}-{}.sql", stamp_now()));
    let out_file = File::create(&file).map_err(|e| format!("cannot create {}: {e}", file.display()))?;

    let mut c = client(s, id, "mysqldump.exe", password)?;
    c.args(["--single-transaction", "--routines", "--events", "--hex-blob", "--default-character-set=utf8mb4"]);
    c.arg(db);
    // The dump streams straight to disk; only stderr is captured.
    let res = c.stdin(Stdio::null()).stdout(Stdio::from(out_file)).stderr(Stdio::piped()).output();
    match res {
        Ok(o) if o.status.success() => Ok(file),
        Ok(o) => {
            // A half-written dump would look like a good snapshot later: remove it.
            let _ = fs::remove_file(&file);
            Err(stderr_of(&o))
        }
        Err(e) => {
            let _ = fs::remove_file(&file);
            Err(e.to_string())
        }
    }
}

/// Snapshots (`.sql`) in `<root>\backups\<id>`, newest first.
pub fn snapshots(s: &Settings, id: &str) -> Vec<Snapshot> {
    let Ok(rd) = fs::read_dir(backups_dir(s, id)) else { return Vec::new() };
    let mut keyed: Vec<(String, Snapshot)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_string_lossy().into_owned();
            let (database, stamp) = parse_snapshot_name(&name)?;
            let bytes = e.metadata().ok()?.len();
            let when = pretty_stamp(&stamp);
            Some((stamp, Snapshot { file: path, database, when, bytes }))
        })
        .collect();
    // The stamp is YYYYMMDD-HHMMSS, so text order is time order.
    keyed.sort_by(|a, b| b.0.cmp(&a.0));
    keyed.into_iter().map(|(_, snap)| snap).collect()
}

/// Restore a snapshot (`.sql` only) into `db` (CREATE DATABASE IF NOT EXISTS first), piping into mysql.exe.
pub fn restore(s: &Settings, id: &str, file: &Path, db: &str, password: &str) -> Result<(), String> {
    load_sql(s, id, file, db, password)
}

/// Import any `.sql` file the person picks into `db` (same as restore).
pub fn import(s: &Settings, id: &str, file: &Path, db: &str, password: &str) -> Result<(), String> {
    load_sql(s, id, file, db, password)
}

fn load_sql(s: &Settings, id: &str, file: &Path, db: &str, password: &str) -> Result<(), String> {
    check_db(db)?;
    let ext_ok = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("sql"));
    if !ext_ok {
        return Err("only plain .sql files are supported (unzip .gz/.zip first)".into());
    }
    // Open first: a missing file should fail before the database is created.
    let input = File::open(file).map_err(|e| format!("cannot open {}: {e}", file.display()))?;

    let mut c = client(s, id, "mysql.exe", password)?;
    c.args(["--default-character-set=utf8mb4", "-e"]);
    c.arg(format!("CREATE DATABASE IF NOT EXISTS `{db}` CHARACTER SET utf8mb4"));
    let out = c.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(stderr_of(&out));
    }

    let mut c = client(s, id, "mysql.exe", password)?;
    c.arg("--default-character-set=utf8mb4");
    c.arg(db);
    // The file feeds stdin directly, so a big dump never sits in memory.
    let out = c.stdin(Stdio::from(input)).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(stderr_of(&out)) }
}

// ---- time and names (the pure parts are unit-tested) ----

/// The PC's offset from UTC in minutes, asked to Windows once (std has no time zones).
fn utc_offset_min() -> i64 {
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-Command", "[int][TimeZoneInfo]::Local.GetUtcOffset([DateTime]::Now).TotalMinutes"]);
        hide_window(&mut c);
        c.output().ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok()).unwrap_or(0)
    })
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (Hinnant's civil_from_days).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// "YYYYMMDD-HHMMSS" for a local-time seconds count since the Unix epoch.
fn stamp_from_secs(local_secs: i64) -> String {
    let (y, mo, d) = civil_from_days(local_secs.div_euclid(86_400));
    let t = local_secs.rem_euclid(86_400);
    format!("{y:04}{mo:02}{d:02}-{:02}{:02}{:02}", t / 3600, t / 60 % 60, t % 60)
}

fn stamp_now() -> String {
    let utc = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    stamp_from_secs(utc + utc_offset_min() * 60)
}

/// "20261113-214000" to "2026-11-13 21:40".
fn pretty_stamp(stamp: &str) -> String {
    format!("{}-{}-{} {}:{}", &stamp[0..4], &stamp[4..6], &stamp[6..8], &stamp[9..11], &stamp[11..13])
}

/// "my-db-20261113-214000.sql" to ("my-db", "20261113-214000"). The db name may itself contain '-'.
fn parse_snapshot_name(name: &str) -> Option<(String, String)> {
    let stem = name.strip_suffix(".sql").or_else(|| name.strip_suffix(".SQL"))?;
    if stem.len() < 17 || !stem.is_char_boundary(stem.len() - 16) {
        return None;
    }
    let (db, tail) = stem.split_at(stem.len() - 16);
    let stamp = tail.strip_prefix('-')?;
    let b = stamp.as_bytes();
    let shape_ok = b.len() == 15 && b[8] == b'-' && b.iter().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit());
    (shape_ok && valid_name(db)).then(|| (db.to_string(), stamp.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_name("my_db-1$"));
        assert!(!valid_name(""));
        assert!(!valid_name("a b"));
        assert!(!valid_name("a`b"));
        assert!(!valid_name("a;drop"));
        assert!(!valid_name(&"x".repeat(65)));
        assert!(check_db("Mysql").is_err());
        assert!(check_db("shop").is_ok());
    }

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(stamp_from_secs(1_700_000_000), "20231114-221320");
        assert_eq!(stamp_from_secs(0), "19700101-000000");
    }

    #[test]
    fn snapshot_names() {
        assert_eq!(parse_snapshot_name("shop-20261113-214000.sql"), Some(("shop".into(), "20261113-214000".into())));
        assert_eq!(parse_snapshot_name("my-db-20261113-214000.sql"), Some(("my-db".into(), "20261113-214000".into())));
        assert_eq!(parse_snapshot_name("shop-20261113-214000.sql.gz"), None);
        assert_eq!(parse_snapshot_name("shop.sql"), None);
        assert_eq!(parse_snapshot_name("-20261113-214000.sql"), None);
        assert_eq!(parse_snapshot_name("shop-2026111a-214000.sql"), None);
        assert_eq!(pretty_stamp("20261113-214000"), "2026-11-13 21:40");
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Makes a throwaway database, backs it up, restores the backup into a second one, then drops both.
    #[test]
    #[ignore]
    fn backup_and_restore_round_trip() {
        let mut s = crate::settings::Settings::default();
        s.root = "C:/laragon".into();
        crate::services::mysql_exec(&s, "mysql", "CREATE DATABASE IF NOT EXISTS ninho_backup_test; CREATE TABLE IF NOT EXISTS ninho_backup_test.t (id INT PRIMARY KEY, name VARCHAR(40)); REPLACE INTO ninho_backup_test.t VALUES (1, 'one'), (2, 'two')", "").expect("setup");
        let file = backup(&s, "mysql", "ninho_backup_test", "").expect("backup");
        assert!(std::fs::metadata(&file).unwrap().len() > 100);
        assert!(snapshots(&s, "mysql").iter().any(|x| x.file == file));
        restore(&s, "mysql", &file, "ninho_restore_test", "").expect("restore");
        let after = databases(&s, "mysql", "").unwrap();
        crate::services::mysql_exec(&s, "mysql", "DROP DATABASE ninho_restore_test; DROP DATABASE ninho_backup_test", "").expect("drop");
        assert!(after.iter().any(|d| d == "ninho_restore_test"));
        eprintln!("backup: {}", file.display());
    }
}
