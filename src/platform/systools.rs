//! Linux and macOS (preview): the servers come from the system's package manager (apt, dnf, pacman, Homebrew), not
//! from Windows ZIPs. "Install" in Ninho finds the program (PATH and the usual places), and links it with its
//! command line tools into <root>/bin/<kind>/<kind>-<version>-system, the same layout the Windows builds use. Ninho
//! then runs it as this user, with its own config files, data folders and logs under its root, on its own ports.
//! Nothing of the system's own server (its config in /etc, its data in /var/lib, its service) is touched.
//!
//! What is missing gets a hint with the package to install. Mailpit and Node.js are downloaded from their official
//! builds instead (install.rs), like on Windows.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::i18n::{Lang, tr};
use crate::services::Shared;
use crate::settings::Settings;

/// One server kind: where its main program and command line tools come from, and what to install when it is missing.
struct Spec {
    key: &'static str,
    /// Names the main program may have, in order of preference.
    main: &'static [&'static str],
    /// Link inside the version folder → the names its target may have (the first link is the main program).
    links: &'static [(&'static str, &'static [&'static str])],
    apt: &'static str,
    dnf: &'static str,
    brew: &'static str,
}

const SPECS: &[Spec] = &[
    Spec { key: "apache", main: &["httpd", "apache2"], links: &[("bin/httpd", &["httpd", "apache2"])], apt: "apache2 libapache2-mod-php", dnf: "httpd php", brew: "httpd" },
    Spec {
        key: "mysql",
        main: &["mysqld"],
        links: &[("bin/mysqld", &["mysqld"]), ("bin/mysql", &["mysql"]), ("bin/mysqladmin", &["mysqladmin"]), ("bin/mysqldump", &["mysqldump"])],
        apt: "mysql-server",
        dnf: "mysql-server",
        brew: "mysql@8.4",
    },
    Spec {
        key: "mariadb",
        main: &["mariadbd", "mysqld"],
        links: &[
            ("bin/mariadbd", &["mariadbd", "mysqld"]),
            ("bin/mariadb", &["mariadb", "mysql"]),
            ("bin/mariadb-admin", &["mariadb-admin", "mysqladmin"]),
            ("bin/mariadb-dump", &["mariadb-dump", "mysqldump"]),
            ("bin/mariadb-install-db", &["mariadb-install-db", "mysql_install_db"]),
        ],
        apt: "mariadb-server",
        dnf: "mariadb-server",
        brew: "mariadb",
    },
    Spec {
        key: "postgres",
        main: &["postgres"],
        links: &[("bin/postgres", &["postgres"]), ("bin/initdb", &["initdb"]), ("bin/pg_ctl", &["pg_ctl"]), ("bin/psql", &["psql"]), ("bin/pg_dump", &["pg_dump"])],
        apt: "postgresql",
        dnf: "postgresql-server",
        brew: "postgresql@17",
    },
    Spec { key: "php", main: &["php"], links: &[("php", &["php"])], apt: "php-cli php-mysql php-curl php-gd php-intl php-mbstring php-xml php-zip", dnf: "php-cli php-mysqlnd", brew: "php" },
    Spec { key: "redis", main: &["redis-server", "valkey-server"], links: &[("redis-server", &["redis-server", "valkey-server"]), ("redis-cli", &["redis-cli", "valkey-cli"])], apt: "redis-server", dnf: "valkey", brew: "redis" },
    Spec { key: "memcached", main: &["memcached"], links: &[("memcached", &["memcached"])], apt: "memcached", dnf: "memcached", brew: "memcached" },
];

fn spec(key: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.key == key)
}

/// Kinds Ninho takes from the system on Linux and macOS (the others are downloaded, or not offered yet).
pub fn from_system(key: &str) -> bool {
    spec(key).is_some()
}

/// The command that installs a kind: "sudo apt install mariadb-server" on Debian and Ubuntu, dnf on Fedora,
/// brew on macOS (Ninho never runs it: it needs the administrator password).
pub fn install_command(key: &str) -> String {
    let Some(sp) = spec(key) else { return String::new() };
    if cfg!(target_os = "macos") {
        format!("brew install {}", sp.brew)
    } else if Path::new("/usr/bin/dnf").is_file() && !Path::new("/usr/bin/apt").is_file() {
        format!("sudo dnf install {}", sp.dnf)
    } else if Path::new("/usr/bin/pacman").is_file() && !Path::new("/usr/bin/apt").is_file() {
        format!("sudo pacman -S {}", sp.dnf.split_whitespace().next().unwrap_or(sp.key))
    } else {
        format!("sudo apt install {}", sp.apt)
    }
}

/// "Not on this computer: install it with ..." for the activity list and the Download window.
pub fn missing_text(lang: Lang, key: &str) -> String {
    tr(lang, "unix_not_found").replace("{cmd}", &install_command(key))
}

/// Folders where packages put server programs that are not always on PATH (sbin, Homebrew's keg-only formulas,
/// Debian's versioned PostgreSQL), newest version first where there are several.
fn search_dirs() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for d in ["/usr/sbin", "/usr/local/sbin", "/usr/local/bin", "/usr/bin", "/sbin", "/opt/homebrew/bin", "/opt/homebrew/sbin"] {
        out.push(PathBuf::from(d));
    }
    let mut versioned: Vec<PathBuf> = Vec::new();
    for parent in ["/opt/homebrew/opt", "/usr/local/opt", "/usr/lib/postgresql", "/usr/local/pgsql"] {
        if let Ok(rd) = std::fs::read_dir(parent) {
            versioned.extend(rd.filter_map(|e| e.ok()).flat_map(|e| [e.path().join("bin"), e.path().join("sbin")]));
        }
    }
    if let Ok(rd) = std::fs::read_dir("/usr") {
        versioned.extend(rd.filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with("pgsql-")).map(|e| e.path().join("bin")));
    }
    versioned.push(PathBuf::from("/Applications/Postgres.app/Contents/Versions/latest/bin"));
    versioned.sort_by_key(|p| std::cmp::Reverse(crate::install::ver_key(&p.to_string_lossy())));
    out.extend(versioned);
    let mut seen: Vec<PathBuf> = Vec::new();
    out.retain(|p| p.is_dir() && !seen.contains(p) && {
        seen.push(p.clone());
        true
    });
    out
}

fn run_text(exe: &Path, args: &[&str]) -> String {
    Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map(|o| format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
        .unwrap_or_default()
}

/// The first dotted version number in a program's `--version` text ("Ver 8.0.39-0ubuntu" → "8.0.39",
/// "Apache/2.4.58" → "2.4.58", "v=7.2.4" → "7.2.4").
pub fn find_version(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|t| t.trim_matches('.'))
        .find(|t| t.contains('.') && t.starts_with(|c: char| c.is_ascii_digit()) && crate::install::ver_key(t).len() >= 2)
        .map(str::to_string)
}

fn version_of(key: &str, exe: &Path) -> String {
    let args: &[&str] = match key {
        "apache" => &["-v"],
        "memcached" => &["-V"],
        _ => &["--version"],
    };
    find_version(&run_text(exe, args)).unwrap_or_else(|| "0.0".into())
}

/// Is this mysqld really MariaDB (Debian links mysqld to mariadbd)?
fn is_mariadb(exe: &Path) -> bool {
    run_text(exe, &["--version"]).to_ascii_lowercase().contains("mariadb")
}

/// The main program of a kind on this system, if any.
fn find_main(sp: &Spec) -> Option<PathBuf> {
    let dirs = search_dirs();
    for name in sp.main {
        for d in &dirs {
            let p = d.join(name);
            if !p.is_file() {
                continue;
            }
            match (sp.key, *name) {
                ("mysql", _) if is_mariadb(&p) => continue,
                ("mariadb", "mysqld") if !is_mariadb(&p) => continue,
                _ => return Some(p),
            }
        }
    }
    None
}

/// A command line tool of the same package: beside the main program first (its real folder too), then anywhere.
fn find_sibling(main: &Path, names: &[&str]) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = main.parent() {
        dirs.push(d.to_path_buf());
    }
    if let Some(d) = std::fs::canonicalize(main).ok().and_then(|r| r.parent().map(Path::to_path_buf)) {
        dirs.push(d);
    }
    // Debian keeps mysqld in /usr/sbin and the clients in /usr/bin.
    if let Some(d) = main.parent().and_then(Path::parent) {
        dirs.push(d.join("bin"));
    }
    dirs.extend(search_dirs());
    names.iter().find_map(|n| dirs.iter().map(|d| d.join(n)).find(|p| p.is_file()))
}

/// The version the system has of a kind, as the Download window shows it, or the hint to install it.
pub fn latest(lang: Lang, key: &str) -> Result<String, String> {
    let sp = spec(key).ok_or_else(|| format!("{key}: {}", tr(lang, "log_no_auto_download")))?;
    let main = find_main(sp).ok_or_else(|| missing_text(lang, key))?;
    Ok(version_of(key, &main))
}

/// Folder name of a linked version: "mysql-8.0.39-system".
pub fn folder_name(key: &str, version: &str) -> String {
    let base = if key == "apache" { "httpd" } else { key };
    format!("{base}-{version}-system")
}

/// "Install" on Linux and macOS: links the system's program into <root>/bin/<kind>/<kind>-<version>-system and
/// writes Ninho's own config beside it. Returns (version, version folder); install.rs then finishes as on Windows
/// (data folder, httpd.conf includes...).
pub fn link(shared: &Shared, s: &Settings, key: &str, kind_dir: &Path) -> Result<(String, PathBuf), String> {
    let lang = s.language;
    let sp = spec(key).ok_or_else(|| format!("{key}: {}", tr(lang, "log_no_auto_download")))?;
    let main = find_main(sp).ok_or_else(|| missing_text(lang, key))?;
    let version = version_of(key, &main);
    let vdir = kind_dir.join(folder_name(key, &version));
    if vdir.join(sp.links[0].0).exists() {
        return Err(format!("{} {version}", tr(lang, "log_version_present")));
    }
    shared.say(format!("{key}: {} {}", tr(lang, "unix_using_system"), main.display()));
    for (i, (rel, names)) in sp.links.iter().enumerate() {
        let target = if i == 0 { Some(main.clone()) } else { find_sibling(&main, names) };
        let Some(target) = target else { continue };
        let link = vdir.join(rel);
        if let Some(d) = link.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&target, &link).map_err(|e| format!("{}: {e}", link.display()))?;
    }
    match key {
        "mysql" => mysql_ini(s, kind_dir, &vdir, &version)?,
        "apache" => apache_conf(&vdir, &vdir.join("bin/httpd"))?,
        _ => {}
    }
    Ok((version, vdir))
}

/// my.ini for the system MySQL: Ninho's own data folder (one per series, like on Windows), only on this PC, no
/// X protocol (its socket would go to /run/mysqld, which belongs to root). A my.ini of an older linked version
/// is copied instead, so the data and settings carry over.
fn mysql_ini(s: &Settings, kind_dir: &Path, vdir: &Path, version: &str) -> Result<(), String> {
    let ini = vdir.join("my.ini");
    if ini.is_file() {
        return Ok(());
    }
    let older = std::fs::read_dir(kind_dir).ok().and_then(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path().join("my.ini")).filter(|p| p.is_file()).max());
    if let Some(old) = older {
        return std::fs::copy(&old, &ini).map(|_| ()).map_err(|e| e.to_string());
    }
    let nums = crate::install::ver_key(version);
    let series = format!("mysql-{}.{}", nums.first().copied().unwrap_or(8), nums.get(1).copied().unwrap_or(0));
    let data = Path::new(&s.root).join("data").join(series);
    let text = format!(
        "[client]\nport=3306\n\n[mysqld]\ndatadir=\"{}\"\nport=3306\nbind-address=127.0.0.1\nmysqlx=OFF\nmax_allowed_packet=512M\nexplicit_defaults_for_timestamp=1\n",
        data.display()
    );
    std::fs::write(&ini, text).map_err(|e| e.to_string())
}

/// Where a system Apache keeps its modules (mod_mime.so...).
fn apache_modules_dir(httpd: &Path) -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();
    if let Some(prefix) = std::fs::canonicalize(httpd).ok().and_then(|r| r.parent().and_then(Path::parent).map(Path::to_path_buf)) {
        for rel in ["lib/httpd/modules", "lib64/httpd/modules", "modules", "libexec/apache2", "lib/apache2/modules"] {
            c.push(prefix.join(rel));
        }
    }
    for d in [
        "/usr/lib/apache2/modules",
        "/usr/lib64/httpd/modules",
        "/usr/lib/httpd/modules",
        "/opt/homebrew/lib/httpd/modules",
        "/usr/local/lib/httpd/modules",
        "/usr/libexec/apache2",
    ] {
        c.push(PathBuf::from(d));
    }
    c.into_iter().find(|d| d.join("mod_mime.so").is_file())
}

/// The system's mod_php (libphp8.3.so on Debian, libphp.so from Homebrew), if one is installed.
fn php_module(modules: &Path) -> Option<PathBuf> {
    let mut dirs = vec![modules.to_path_buf()];
    for d in ["/opt/homebrew/opt/php/lib/httpd/modules", "/usr/local/opt/php/lib/httpd/modules"] {
        dirs.push(PathBuf::from(d));
    }
    dirs.iter().find_map(|d| {
        let mut found: Vec<PathBuf> = std::fs::read_dir(d).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| {
            let f = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            f.starts_with("libphp") && f.ends_with(".so")
        }).collect();
        found.sort();
        found.pop()
    })
}

/// httpd.conf for the system Apache: everything under the version folder (pid, logs), Ninho's port, the modules a
/// PHP site needs, and the system's mod_php when present. apache_setup (services.rs) then points it at the sites
/// folder and includes conf/ninho.conf, as on Windows. An httpd.conf already there (edited by the person) is kept.
fn apache_conf(vdir: &Path, httpd: &Path) -> Result<(), String> {
    for d in ["conf", "logs", "htdocs"] {
        std::fs::create_dir_all(vdir.join(d)).map_err(|e| e.to_string())?;
    }
    let conf = vdir.join("conf").join("httpd.conf");
    if conf.is_file() {
        return Ok(());
    }
    let built_in = run_text(httpd, &["-l"]);
    let modules = apache_modules_dir(httpd);
    let php = modules.as_deref().and_then(php_module);
    let mut l: Vec<String> = vec![
        "# Written once by Ninho for the system Apache. Edit freely: Ninho only fixes the lines it needs.".into(),
        format!("Define SRVROOT \"{}\"", vdir.display()),
        "ServerRoot \"${SRVROOT}\"".into(),
        "Listen 8080".into(),
        "PidFile \"${SRVROOT}/logs/httpd.pid\"".into(),
        "ErrorLog \"${SRVROOT}/logs/error.log\"".into(),
        "LogLevel warn".into(),
    ];
    if let Some(m) = &modules {
        let has_mpm = ["prefork.c", "event.c", "worker.c"].iter().any(|x| built_in.contains(x));
        let mut wanted: Vec<&str> = Vec::new();
        if !has_mpm {
            // mod_php is not thread safe: it needs prefork.
            wanted.push(if php.is_some() || !m.join("mod_mpm_event.so").is_file() { "mpm_prefork" } else { "mpm_event" });
        }
        wanted.extend(["unixd", "authz_core", "authz_host", "authn_core", "dir", "mime", "log_config", "alias", "rewrite", "headers", "env", "setenvif", "autoindex"]);
        for name in wanted {
            let file = m.join(format!("mod_{name}.so"));
            if file.is_file() && !built_in.contains(&format!("mod_{name}.c")) {
                l.push(format!("LoadModule {name}_module \"{}\"", file.display()));
            }
        }
    }
    if let Some(p) = &php {
        let f = p.file_name().unwrap_or_default().to_string_lossy();
        let id = if f.contains("php7") { "php7_module" } else if f.contains("php5") { "php5_module" } else { "php_module" };
        l.push(format!("LoadModule {id} \"{}\"", p.display()));
        l.push("<FilesMatch \"\\.php$\">".into());
        l.push("    SetHandler application/x-httpd-php".into());
        l.push("</FilesMatch>".into());
    }
    if let Some(mime) = ["/etc/mime.types", "/etc/apache2/mime.types", "/etc/httpd/conf/mime.types", "/opt/homebrew/etc/httpd/mime.types", "/usr/local/etc/httpd/mime.types"]
        .iter()
        .find(|p| Path::new(p).is_file())
    {
        l.push(format!("TypesConfig \"{mime}\""));
    }
    l.extend([
        "#ServerName localhost".to_string(),
        "LogFormat \"%h %l %u %t \\\"%r\\\" %>s %b\" common".into(),
        "CustomLog \"${SRVROOT}/logs/access.log\" common".into(),
        "DocumentRoot \"${SRVROOT}/htdocs\"".into(),
        "<Directory \"${SRVROOT}/htdocs\">".into(),
        "    Require all granted".into(),
        "</Directory>".into(),
    ]);
    std::fs::write(&conf, l.join("\n") + "\n").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_out_of_version_texts() {
        assert_eq!(find_version("/usr/sbin/mysqld  Ver 8.0.39-0ubuntu0.24.04.2 for Linux on x86_64").as_deref(), Some("8.0.39"));
        assert_eq!(find_version("Server version: Apache/2.4.58 (Ubuntu)\nServer built: 2024").as_deref(), Some("2.4.58"));
        assert_eq!(find_version("Redis server v=7.0.15 sha=00000000:0 malloc=jemalloc-5.3.0").as_deref(), Some("7.0.15"));
        assert_eq!(find_version("postgres (PostgreSQL) 16.4 (Ubuntu 16.4-0ubuntu0.24.04.2)").as_deref(), Some("16.4"));
        assert_eq!(find_version("PHP 8.3.6 (cli) (built: Apr 15 2024)").as_deref(), Some("8.3.6"));
        assert_eq!(find_version("no version here"), None);
        assert_eq!(folder_name("apache", "2.4.58"), "httpd-2.4.58-system");
        assert_eq!(folder_name("mysql", "8.0.39"), "mysql-8.0.39-system");
    }

    #[test]
    fn every_kind_has_an_install_command() {
        for k in ["apache", "mysql", "mariadb", "postgres", "php", "redis", "memcached"] {
            assert!(from_system(k));
            let c = install_command(k);
            assert!(c.starts_with("brew install ") || c.starts_with("sudo "), "{c}");
        }
        assert!(!from_system("nodejs"));
        assert!(!from_system("mailpit"));
    }
}
