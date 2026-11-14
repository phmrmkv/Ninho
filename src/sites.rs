use std::path::Path;
use std::sync::Arc;

use crate::services::Shared;
use crate::i18n::tr;
use crate::settings::{Settings, name_ok};

/// Marks the hosts lines Ninho owns, so it only ever rewrites its own lines.
pub const MARKER: &str = "#ninho local";
/// The marker starts at this column, so the hosts file stays aligned.
const MARKER_COLUMN: usize = 50;

#[derive(Clone)]
pub struct Site {
    pub folder: String,
    pub host: String,
}

/// What kind of project a site folder holds, from the files at its top.
pub fn kind(dir: &Path) -> &'static str {
    let has = |f: &str| dir.join(f).exists();
    if has("artisan") {
        "Laravel"
    } else if has("wp-config.php") || has("wp-load.php") {
        "WordPress"
    } else if dir.join("bin").join("console").exists() && has("composer.json") {
        "Symfony"
    } else if has("package.json") && !has("index.php") && !has("composer.json") {
        "Node.js"
    } else if has("index.php") || has("composer.json") {
        "PHP"
    } else if has("index.html") {
        "HTML"
    } else {
        ""
    }
}

/// Folder a PHP server should serve: public/ for Laravel and Symfony, the site itself otherwise.
pub fn web_root(dir: &Path) -> std::path::PathBuf {
    if dir.join("public").join("index.php").is_file() { dir.join("public") } else { dir.to_path_buf() }
}

pub fn hostname(fmt: &str, folder: &str) -> String {
    fmt.replace("{name}", folder)
}

/// Folders of the document root that become sites.
pub fn list(s: &Settings) -> Vec<Site> {
    let key = format!("sites|{}|{}|{}", s.doc_root, s.hostname_format, s.ignore_list.join("|"));
    crate::services::memo(key, || list_now(s))
}

fn list_now(s: &Settings) -> Vec<Site> {
    let mut out: Vec<Site> = std::fs::read_dir(&s.doc_root)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n != "New folder" && name_ok(n))
                .filter(|n| !s.ignore_list.iter().any(|i| i.eq_ignore_ascii_case(n)))
                .map(|n| Site { host: hostname(&s.hostname_format, &n), folder: n })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|x| x.folder.to_lowercase());
    out
}

pub fn url(s: &Settings, site: &Site) -> String {
    if s.https && s.auto_vhosts {
        return format!("https://{}", site.host);
    }
    let port = s.svc("apache").port;
    if s.auto_vhosts {
        if port == 80 { format!("http://{}", site.host) } else { format!("http://{}:{port}", site.host) }
    } else if port == 80 {
        format!("http://localhost/{}", site.folder)
    } else {
        format!("http://localhost:{port}/{}", site.folder)
    }
}

fn apache_vhost(site_dir: &str, host: &str, port: u16) -> String {
    format!(
        "<VirtualHost *:{port}> \r\n    DocumentRoot \"{site_dir}\"\r\n    ServerName {host}\r\n    ServerAlias *.{host}\r\n    <Directory \"{site_dir}\">\r\n        AllowOverride All\r\n        Require all granted\r\n    </Directory>\r\n</VirtualHost>\r\n\r\n# This file is made automatically. Remove the [auto.] prefix to keep your own changes.\r\n"
    )
}

fn nginx_server(site_dir: &str, host: &str, port: u16) -> String {
    format!(
        "server {{\r\n    listen {port};\r\n    server_name {host} *.{host};\r\n    root \"{site_dir}\";\r\n\r\n    index index.html index.htm index.php;\r\n\r\n    location / {{\r\n        try_files $uri $uri/ /index.php$is_args$args;\r\n        autoindex on;\r\n    }}\r\n\r\n    location ~ \\.php$ {{\r\n        include snippets/fastcgi-php.conf;\r\n        fastcgi_pass php_upstream;\r\n    }}\r\n\r\n    charset utf-8;\r\n\r\n    location = /favicon.ico {{ access_log off; log_not_found off; }}\r\n    location = /robots.txt  {{ access_log off; log_not_found off; }}\r\n    location ~ /\\.ht {{\r\n        deny all;\r\n    }}\r\n}}\r\n\r\n# This file is made automatically. Remove the [auto.] prefix to keep your own changes.\r\n"
    )
}

/// Write `auto.<host>.conf` for each site and remove the ones whose folder is gone.
/// `ssl` adds the HTTPS part of a site's Apache file (site dir, host) when HTTPS is on.
fn write_confs(dir: &Path, sites: &[Site], doc_root: &str, port: u16, nginx: bool, ssl: Option<&dyn Fn(&str, &str) -> Option<String>>) -> usize {
    if !dir.is_dir() {
        return 0;
    }
    let mut written = 0;
    let wanted: Vec<String> = sites.iter().map(|x| format!("auto.{}.conf", x.host)).collect();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("auto.") && n.ends_with(".conf") && !wanted.contains(&n) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    for site in sites {
        // A file without the prefix belongs to the person.
        if dir.join(format!("{}.conf", site.host)).exists() {
            continue;
        }
        let site_dir = format!("{}/{}", doc_root.replace('\\', "/").trim_end_matches('/'), site.folder);
        let mut text = if nginx {
            nginx_server(&site_dir, &site.host, port)
        } else {
            apache_vhost(&site_dir, &site.host, port)
        };
        if let Some(extra) = ssl.and_then(|f| f(&site_dir, &site.host)) {
            text.push_str("\r\n");
            text.push_str(&extra);
        }
        let path = dir.join(format!("auto.{}.conf", site.host));
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            if std::fs::write(&path, text).is_ok() {
                written += 1;
            }
        }
    }
    written
}

fn hosts_path() -> String {
    crate::platform::hosts_path()
}

/// `127.0.0.1` + 6 spaces + host, padded to column 50, then the marker.
fn hosts_line(host: &str) -> String {
    let head = format!("127.0.0.1      {host}");
    let pad = MARKER_COLUMN.saturating_sub(head.len()).max(1);
    format!("{head}{}{MARKER}", " ".repeat(pad))
}

/// New hosts text, or None when nothing changes.
fn new_hosts(current: &str, sites: &[Site]) -> Option<String> {
    let mut kept: Vec<&str> = current.lines().filter(|l| !l.contains(MARKER)).collect();
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    let ours: Vec<String> = sites.iter().map(|x| hosts_line(&x.host)).collect();
    let old_ours: Vec<&str> = current.lines().filter(|l| l.contains(MARKER)).collect();
    if old_ours.iter().map(|l| l.trim_end()).eq(ours.iter().map(|l| l.as_str())) {
        return None;
    }
    let mut text = kept.join("\r\n");
    text.push_str("\r\n");
    for l in &ours {
        text.push_str(l);
        text.push_str("\r\n");
    }
    Some(text)
}

fn update_hosts(shared: &Arc<Shared>, s: &Settings, sites: &[Site]) {
    let lang = s.language;
    let path = hosts_path();
    let Ok(current) = std::fs::read_to_string(&path) else {
        shared.say(tr(lang, "log_hosts_read_failed"));
        return;
    };
    let Some(text) = new_hosts(&current, sites) else { return };
    // Linux and macOS write their hosts file with plain line ends.
    let text = if cfg!(windows) { text } else { text.replace("\r\n", "\n") };
    if std::fs::write(&path, &text).is_ok() {
        shared.say(tr(lang, "log_hosts_updated"));
        return;
    }
    // Needs administrator rights: write a copy and ask Windows to put it in place.
    let tmp = Path::new(&s.root).join("tmp");
    let _ = std::fs::create_dir_all(&tmp);
    let tmp_file = tmp.join("hosts.ninho.tmp");
    if std::fs::write(&tmp_file, &text).is_err() {
        shared.say(tr(lang, "log_hosts_tmp_failed"));
        return;
    }
    if cfg!(windows) {
        shared.say(tr(lang, "log_hosts_asking"));
    }
    match crate::platform::copy_hosts_as_admin(&tmp_file, &path) {
        Ok(true) => shared.say(tr(lang, "log_hosts_updated")),
        Ok(false) => shared.say(tr(lang, "log_hosts_refused")),
        // Linux and macOS: Ninho never asks for the administrator password; it shows the command instead.
        Err(cmd) => shared.say(format!("{} {cmd}", tr(lang, "log_hosts_run"))),
    }
}

/// Make the sites' web server files and hosts entries match the document root.
pub fn refresh(shared: &Arc<Shared>, s: &Settings, verbose: bool) {
    crate::home::write(s);
    if !s.auto_vhosts {
        return;
    }
    let sites = list(s);
    let root = Path::new(&s.root);
    // HTTPS: a certificate per site, signed by Ninho's local authority (openssl from the Apache folder).
    let openssl = crate::services::version_dir(s, "apache").map(|v| v.join("bin").join(crate::platform::exe("openssl"))).filter(|p| p.is_file());
    let ssl_ok = s.https && openssl.as_ref().is_some_and(|o| match crate::certs::ensure_ca(s, o) {
        Ok(_) => true,
        Err(e) => {
            shared.say(format!("HTTPS: {e}"));
            false
        }
    });
    let make_ssl = |site_dir: &str, host: &str| -> Option<String> {
        let o = openssl.as_ref()?;
        match crate::certs::ensure_site_cert(s, o, host) {
            Ok((crt, key)) => Some(crate::certs::apache_ssl_vhost(site_dir, host, 443, &crt, &key)),
            Err(e) => {
                shared.say(format!("HTTPS {host}: {e}"));
                None
            }
        }
    };
    let ssl: Option<&dyn Fn(&str, &str) -> Option<String>> = if ssl_ok { Some(&make_ssl) } else { None };
    let a = write_confs(&root.join("etc").join("apache2").join("sites-enabled"), &sites, &s.doc_root, s.svc("apache").port, false, ssl);
    let n = if s.svc("nginx").enabled {
        write_confs(&root.join("etc").join("nginx").join("sites-enabled"), &sites, &s.doc_root, s.svc("nginx").port, true, None)
    } else {
        0
    };
    // Caddy has one file for all the sites, written again at every start; here when the sites change meanwhile.
    if s.svc("caddy").enabled && crate::services::version_dir(s, "caddy").is_some() {
        let _ = crate::caddy::write_current(s);
    }
    if verbose || a + n > 0 {
        shared.say(format!("{} {}, {} {}", tr(s.language, "log_sites_found"), sites.len(), tr(s.language, "log_files_updated"), a + n));
    }
    update_hosts(shared, s, &sites);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_pads_to_column_50() {
        let l = hosts_line("blog.test");
        assert_eq!(l.find(MARKER), Some(50));
        assert!(l.starts_with("127.0.0.1      blog.test"));
    }

    #[test]
    fn keeps_other_lines() {
        let cur = "# comment\r\n127.0.0.1 localhost\r\n";
        let sites = vec![Site { folder: "a".into(), host: "a.test".into() }];
        let t = new_hosts(cur, &sites).unwrap();
        assert!(t.contains("127.0.0.1 localhost"));
        assert!(t.contains("a.test"));
        assert!(new_hosts(&t, &sites).is_none());
    }
}
