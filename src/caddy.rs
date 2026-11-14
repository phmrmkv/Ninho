//! Caddy as one more web server beside Apache and Nginx, from GitHub (caddyserver/caddy, the windows_amd64 ZIP).
//!   Port      2015 (Caddy's classic default), plain HTTP, only on this PC: Apache keeps 80, and Caddy's automatic HTTPS
//!             is off so it never asks to install its certificate authority. A site is served on it as
//!             http://<name>.test:2015 (the hosts file is the same one Apache uses).
//!   Config    <root>\etc\caddy\Caddyfile, written again at every start: one block per site folder, like the Apache and
//!             Nginx files, plus localhost for the whole sites folder.
//!   PHP       Caddy talks FastCGI to php-cgi.exe of the PHP chosen in Ninho; Ninho starts a few php-cgi workers beside
//!             Caddy (127.0.0.1, ports 9074 and up) and stops them with it. Windows' php-cgi answers one request per
//!             worker at a time, so a few workers are used and Caddy spreads the requests among them.
//!   Stopping  `caddy stop` (the admin endpoint on 127.0.0.1:2019), the program is ended if it does not close.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::i18n::{Lang, tr};
use crate::install::{Latest, X86, json};
use crate::services::{self, Shared, hide_window};
use crate::settings::Settings;
use crate::sites::Site;

/// First port of the php-cgi workers and how many there are.
pub const PHP_BASE_PORT: u16 = 9074;
pub const PHP_WORKERS: u16 = 4;
/// Caddy's admin endpoint (`caddy stop` talks to it).
pub const ADMIN: &str = "127.0.0.1:2019";

pub fn caddyfile_path(root: &str) -> PathBuf {
    Path::new(root).join(r"etc\caddy\Caddyfile")
}

pub fn log_path(root: &str) -> PathBuf {
    Path::new(root).join(r"etc\caddy\caddy.log")
}

/// The Latest to download from the JSON of GitHub's "latest release" call.
pub fn pick(release: &serde_json::Value, x86: bool, lang: Lang) -> Result<Latest, String> {
    if x86 {
        // Caddy publishes amd64, arm64 and others for Windows, no 32-bit x86 build.
        return Err(format!("Caddy {}", tr(lang, "log_no_32bit")));
    }
    let tag = release["tag_name"].as_str().ok_or("no Caddy release found")?;
    let version = tag.trim_start_matches('v');
    let want = format!("caddy_{version}_windows_amd64.zip");
    let url = release["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(want.as_str())))
        .and_then(|x| x["browser_download_url"].as_str())
        .ok_or_else(|| format!("{want} is not in the release {tag}"))?;
    Ok(Latest { version: version.to_string(), url: url.to_string(), top: String::new(), folder: format!("caddy-{version}") })
}

pub fn latest(lang: Lang) -> Result<Latest, String> {
    pick(&json("https://api.github.com/repos/caddyserver/caddy/releases/latest")?, X86, lang)
}

fn slash(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_string()
}

/// The Caddyfile: global options, localhost for the whole sites folder, one block per site (and its subdomains).
/// `php` is true when php-cgi workers are available. Each site comes with whether it has an index.php at its top: then
/// every address that is not a file goes to it (Laravel, WordPress...); a site of plain files is just served, so its
/// index.html shows. `data` is Caddy's own storage folder (it would use the user profile otherwise).
pub fn caddyfile(sites: &[(Site, bool)], doc_root: &str, port: u16, php: bool, data: &str) -> String {
    let mut t = String::new();
    t.push_str("# Written by Ninho at every Caddy start; put your own sites in another file and `import` it from a copy.\n");
    t.push_str(&format!(
        "{{\n\tauto_https off\n\tadmin {ADMIN}\n\tdefault_bind 127.0.0.1\n\tpersist_config off\n\tstorage file_system \"{}\"\n}}\n\n",
        slash(data)
    ));
    let ups: Vec<String> = (0..PHP_WORKERS).map(|i| format!("127.0.0.1:{}", PHP_BASE_PORT + i)).collect();
    let block = |addr: &str, dir: &str, front: bool| {
        let fastcgi = if !php {
            "\t# PHP is not installed (or has no php-cgi.exe): .php files are not run.\n".to_string()
        } else if front {
            format!("\tphp_fastcgi {}\n", ups.join(" "))
        } else {
            format!("\tphp_fastcgi {} {{\n\t\ttry_files {{path}} {{path}}/index.php {{path}}\n\t}}\n", ups.join(" "))
        };
        format!("{addr} {{\n\troot * \"{dir}\"\n\tencode gzip\n{fastcgi}\tfile_server browse\n}}\n\n")
    };
    let root = slash(doc_root);
    t.push_str(&block(&format!("http://localhost:{port}, http://127.0.0.1:{port}"), &root, false));
    for (s, front) in sites {
        let addr = format!("http://{h}:{port}, http://*.{h}:{port}", h = s.host);
        t.push_str(&block(&addr, &format!("{root}/{}", s.folder), *front));
    }
    t
}

/// Write the Caddyfile for the current sites.
pub fn write_caddyfile(s: &Settings, php: bool) -> Result<PathBuf, String> {
    let path = caddyfile_path(&s.root);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let sites: Vec<(Site, bool)> = if s.auto_vhosts {
        crate::sites::list(s)
            .into_iter()
            .map(|x| {
                let front = Path::new(&s.doc_root).join(&x.folder).join("index.php").is_file();
                (x, front)
            })
            .collect()
    } else {
        Vec::new()
    };
    let data = Path::new(&s.root).join(r"etc\caddy\data");
    std::fs::write(&path, caddyfile(&sites, &s.doc_root, s.kind("caddy").map_or(2015, |c| c.port), php, &data.to_string_lossy())).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Write the Caddyfile for the sites as they are now (PHP found as it is now too).
pub fn write_current(s: &Settings) -> Result<PathBuf, String> {
    write_caddyfile(s, php_cgi(s).is_some())
}

/// php-cgi.exe of the PHP chosen in Ninho.
fn php_cgi(s: &Settings) -> Option<PathBuf> {
    services::version_dir(s, "php").map(|v| v.join("php-cgi.exe")).filter(|p| p.is_file())
}

/// Before Caddy starts: its Caddyfile, and the php-cgi workers it forwards .php files to.
pub fn prepare(shared: &Arc<Shared>, s: &Settings) {
    let lang = s.language;
    let cgi = php_cgi(s);
    if let Err(e) = write_current(s) {
        shared.say(format!("Caddy: {} {e}", tr(lang, "log_caddy_conf_failed")));
    }
    let Some(cgi) = cgi else {
        shared.say(format!("Caddy: {}", tr(lang, "log_caddy_no_php")));
        return;
    };
    for i in 0..PHP_WORKERS {
        let port = PHP_BASE_PORT + i;
        let key = format!("caddy-php-{i}");
        // A worker left by an earlier run (another Ninho window, a crash) is simply used again.
        if services::port_open(port) {
            continue;
        }
        let mut c = Command::new(&cgi);
        c.args(["-b", &format!("127.0.0.1:{port}")]);
        // No limit of requests per worker (Windows' php-cgi quits after 500 by default).
        c.env("PHP_FCGI_MAX_REQUESTS", "0");
        c.current_dir(cgi.parent().unwrap());
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        hide_window(&mut c);
        match c.spawn() {
            Ok(ch) => {
                shared.children.lock().unwrap().insert(key, ch);
            }
            Err(e) => {
                shared.say(format!("Caddy: {} {e}", tr(lang, "log_caddy_php_failed")));
                return;
            }
        }
    }
}

/// `caddy stop` (asks the running Caddy to quit through its admin endpoint).
pub fn polite_stop(exe: &Path) {
    let mut c = Command::new(exe);
    c.args(["stop", "--address", ADMIN]);
    let _ = crate::ports::output_timeout(c, Duration::from_secs(8));
}

/// Ends the php-cgi workers started with Caddy: the ones this Ninho holds, and any php-cgi still listening on their
/// ports (started by another run of Ninho, e.g. the command line).
pub fn stop_helpers(shared: &Shared) {
    let keys: Vec<String> = shared.children.lock().unwrap().keys().filter(|k| k.starts_with("caddy-php-")).cloned().collect();
    for k in keys {
        if let Some(mut ch) = shared.children.lock().unwrap().remove(&k) {
            let _ = ch.kill();
            let _ = ch.wait();
        }
    }
    let ports = PHP_BASE_PORT..PHP_BASE_PORT + PHP_WORKERS;
    for l in crate::ports::listening().iter().filter(|l| ports.contains(&l.port) && l.program.eq_ignore_ascii_case("php-cgi.exe")) {
        let _ = crate::ports::kill(l.pid);
    }
}

/// `caddy run` arguments.
pub fn run_args(caddyfile: &Path) -> Vec<String> {
    vec!["run".into(), "--config".into(), caddyfile.to_string_lossy().into_owned(), "--adapter".into(), "caddyfile".into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    // GitHub's releases/latest answer, trimmed to what the code reads.
    const RELEASE: &str = r#"{"tag_name":"v2.11.9","prerelease":false,"assets":[
      {"name":"caddy_2.11.9_linux_amd64.tar.gz","browser_download_url":"https://github.com/caddyserver/caddy/releases/download/v2.11.9/caddy_2.11.9_linux_amd64.tar.gz"},
      {"name":"caddy_2.11.9_windows_amd64.zip.sig","browser_download_url":"https://github.com/caddyserver/caddy/releases/download/v2.11.9/caddy_2.11.9_windows_amd64.zip.sig"},
      {"name":"caddy_2.11.9_windows_amd64.zip","browser_download_url":"https://github.com/caddyserver/caddy/releases/download/v2.11.9/caddy_2.11.9_windows_amd64.zip"},
      {"name":"caddy_2.11.9_windows_arm64.zip","browser_download_url":"https://github.com/caddyserver/caddy/releases/download/v2.11.9/caddy_2.11.9_windows_arm64.zip"}]}"#;

    #[test]
    fn windows_zip_from_the_release() {
        let v: serde_json::Value = serde_json::from_str(RELEASE).unwrap();
        let l = pick(&v, false, Lang::from_code("en")).unwrap();
        assert_eq!(l.version, "2.11.9");
        assert_eq!(l.url, "https://github.com/caddyserver/caddy/releases/download/v2.11.9/caddy_2.11.9_windows_amd64.zip");
        assert_eq!(l.folder, "caddy-2.11.9");
        assert!(l.top.is_empty());
    }

    #[test]
    fn no_32bit_build_and_missing_asset() {
        let v: serde_json::Value = serde_json::from_str(RELEASE).unwrap();
        assert!(pick(&v, true, Lang::from_code("en")).is_err());
        let none: serde_json::Value = serde_json::from_str(r#"{"tag_name":"v2.12.0","assets":[]}"#).unwrap();
        assert!(pick(&none, false, Lang::from_code("en")).unwrap_err().contains("caddy_2.12.0_windows_amd64.zip"));
        assert!(pick(&serde_json::json!({}), false, Lang::from_code("en")).is_err());
    }

    /// blog has an index.php (front controller), shop is plain files.
    fn sites() -> Vec<(Site, bool)> {
        vec![
            (Site { folder: "blog".into(), host: "blog.test".into() }, true),
            (Site { folder: "shop".into(), host: "shop.test".into() }, false),
        ]
    }

    const WORKERS: &str = "127.0.0.1:9074 127.0.0.1:9075 127.0.0.1:9076 127.0.0.1:9077";

    #[test]
    fn caddyfile_serves_every_site_with_php() {
        let t = caddyfile(&sites(), r"C:\ninho\www", 2015, true, r"C:\ninho\etc\caddy\data");
        assert!(t.contains("auto_https off"));
        assert!(t.contains("default_bind 127.0.0.1"));
        assert!(t.contains("persist_config off"));
        assert!(t.contains("storage file_system \"C:/ninho/etc/caddy/data\""));
        assert!(t.contains("http://localhost:2015, http://127.0.0.1:2015 {"));
        assert!(t.contains("root * \"C:/ninho/www\""));
        assert!(t.contains("http://blog.test:2015, http://*.blog.test:2015 {"));
        assert!(t.contains("root * \"C:/ninho/www/shop\""));
        // The site with an index.php sends every unknown address to it; the others (and localhost) do not.
        assert_eq!(t.matches(&format!("php_fastcgi {WORKERS}\n")).count(), 1);
        assert_eq!(t.matches(&format!("php_fastcgi {WORKERS} {{\n\t\ttry_files {{path}} {{path}}/index.php {{path}}\n\t}}")).count(), 2);
        assert_eq!(t.matches("file_server browse").count(), 3);
        // Braces are balanced.
        assert_eq!(t.matches('{').count(), t.matches('}').count());
    }

    #[test]
    fn caddyfile_without_php_or_sites() {
        let t = caddyfile(&[], "C:/www/", 8081, false, "C:/d");
        assert!(!t.contains("php_fastcgi"));
        assert!(t.contains("PHP is not installed"));
        assert!(t.contains("http://localhost:8081"));
        assert!(t.contains("root * \"C:/www\""));
    }

    /// With NINHO_TEST_CADDYFILE=<path> this writes a sample Caddyfile there, to try it against a real Caddy.
    #[test]
    #[ignore]
    fn write_sample_for_a_real_caddy() {
        let out = std::env::var("NINHO_TEST_CADDYFILE").unwrap();
        let root = std::env::var("NINHO_TEST_WWW").unwrap();
        let sites = vec![
            (Site { folder: "blog".into(), host: "blog.test".into() }, true),
            (Site { folder: "plain".into(), host: "plain.test".into() }, false),
        ];
        std::fs::write(out, caddyfile(&sites, &root, 2115, true, &format!("{root}/../etc/caddy/data"))).unwrap();
    }

    #[test]
    fn run_command_line() {
        let a = run_args(Path::new(r"C:\n\etc\caddy\Caddyfile"));
        assert_eq!(a[0], "run");
        assert!(a.contains(&"--adapter".to_string()));
    }
}
