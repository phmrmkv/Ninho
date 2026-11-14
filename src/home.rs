//! The page at http://localhost: every site of the www folder, the servers, Mailpit, Adminer and phpinfo.
//! It lives in `<root>\etc\ninho-home` (never in the person's www folder); Apache's (and Nginx's) `localhost` server
//! points at that folder. The page is a static file written again at every refresh and every server start.
use std::path::{Path, PathBuf};

use crate::i18n::{Lang, tr};
use crate::install;
use crate::services;
use crate::settings::Settings;
use crate::sites;

/// Where the page and phpinfo.php are written.
pub fn page_dir(s: &Settings) -> PathBuf {
    Path::new(&s.root).join("etc").join("ninho-home")
}

/// The web server `localhost` answers from: Apache, else Nginx when only that one is on.
fn web_port(s: &Settings) -> u16 {
    if !s.svc("apache").enabled && s.svc("nginx").enabled { s.svc("nginx").port } else { s.svc("apache").port }
}

/// http://localhost, with the port when it is not 80.
pub fn base_url(s: &Settings) -> String {
    let p = web_port(s);
    if p == 80 { "http://localhost".into() } else { format!("http://localhost:{p}") }
}

/// The address of the home page itself.
pub fn url(s: &Settings) -> String {
    format!("{}/", base_url(s))
}

/// A site's address: its .test host when auto vhosts are on, else localhost/<folder>.
fn site_url(s: &Settings, site: &sites::Site) -> String {
    let port = web_port(s);
    if s.auto_vhosts {
        if s.https {
            return format!("https://{}", site.host);
        }
        if port == 80 { format!("http://{}", site.host) } else { format!("http://{}:{port}", site.host) }
    } else {
        format!("{}/{}", base_url(s), site.folder)
    }
}

pub struct SiteRow {
    pub folder: String,
    pub url: String,
    pub kind: String,
    pub php: String,
}

pub struct ServerRow {
    pub name: String,
    pub version: String,
    pub port: u16,
}

pub struct Page {
    pub lang: Lang,
    pub doc_root: String,
    pub sites: Vec<SiteRow>,
    pub servers: Vec<ServerRow>,
    pub mail: Option<String>,
    pub adminer: Option<String>,
    /// Address of the person's own index file of the www root, when there is one.
    pub user_index: Option<String>,
}

pub fn esc(t: &str) -> String {
    let mut o = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

const CSS: &str = r#":root{color-scheme:light dark;--bg:#eef2f5;--surface:#fff;--fg:#1c2a33;--muted:#4f5f69;--line:#d9e1e6;--brand:#1b7880;--soft:#d5f3eb;--ok:#0c7a63}
@media (prefers-color-scheme:dark){:root{--bg:#12181c;--surface:#1a2227;--fg:#e6eef1;--muted:#9fb0b8;--line:#34424a;--brand:#3cc7d1;--soft:#14403a;--ok:#5fdcbf}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif}
main{max-width:860px;margin:0 auto;padding:24px 16px 40px}h1{font-size:1.6rem;margin:0 0 4px}h2{font-size:1.05rem;margin:28px 0 10px}
p{margin:0 0 8px}.muted{color:var(--muted);font-size:.9rem;overflow-wrap:anywhere}a{color:var(--brand)}
ul{list-style:none;margin:0;padding:0;display:grid;gap:8px}li{background:var(--surface);border:1px solid var(--line);border-radius:8px;padding:10px 14px;display:flex;flex-wrap:wrap;align-items:center;gap:6px 10px}
li a{font-weight:600;overflow-wrap:anywhere}.badge{font-size:.78rem;border:1px solid var(--line);border-radius:99px;padding:1px 9px;color:var(--muted)}
.kind{background:var(--soft);color:var(--ok);border-color:transparent}.sp{flex:1}.note{background:var(--surface);border:1px solid var(--line);border-left:4px solid var(--brand);border-radius:8px;padding:10px 14px;margin-top:16px}
footer{margin-top:32px}"#;

/// The whole page, as one static HTML file with inline CSS and no outside requests.
pub fn render(p: &Page) -> String {
    let l = p.lang;
    let mut h = String::new();
    h.push_str(&format!(
        "<!doctype html>\n<html lang=\"{}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<title>Ninho</title>\n<style>{CSS}</style>\n</head>\n<body>\n<main>\n",
        esc(l.code())
    ));
    h.push_str(&format!("<h1>Ninho</h1>\n<p class=\"muted\">{}</p>\n", esc(&p.doc_root)));
    if let Some(link) = &p.user_index {
        h.push_str(&format!("<div class=\"note\">{} <a href=\"{}\">{}</a></div>\n", esc(tr(l, "home_your_files")), esc(link), esc(link)));
    }
    h.push_str(&format!("<h2>{}</h2>\n", esc(tr(l, "home_sites"))));
    if p.sites.is_empty() {
        h.push_str(&format!("<p class=\"muted\">{}</p>\n", esc(tr(l, "home_no_sites"))));
    } else {
        h.push_str("<ul>\n");
        for x in &p.sites {
            h.push_str(&format!("<li><a href=\"{}\">{}</a>", esc(&x.url), esc(&x.folder)));
            if !x.kind.is_empty() {
                h.push_str(&format!("<span class=\"badge kind\">{}</span>", esc(&x.kind)));
            }
            if !x.php.is_empty() {
                h.push_str(&format!("<span class=\"badge\">PHP {}</span>", esc(&x.php)));
            }
            h.push_str(&format!("<span class=\"sp\"></span><span class=\"muted\">{}</span></li>\n", esc(&x.url)));
        }
        h.push_str("</ul>\n");
    }
    h.push_str(&format!("<h2>{}</h2>\n<ul>\n", esc(tr(l, "home_servers"))));
    for v in &p.servers {
        let ver = if v.version.is_empty() { String::new() } else { format!("<span class=\"badge\">{}</span>", esc(&v.version)) };
        h.push_str(&format!("<li><strong>{}</strong>{ver}<span class=\"sp\"></span><span class=\"muted\">:{}</span></li>\n", esc(&v.name), v.port));
    }
    h.push_str("</ul>\n");
    h.push_str(&format!("<h2>{}</h2>\n<ul>\n", esc(tr(l, "home_tools"))));
    if let Some(m) = &p.mail {
        h.push_str(&format!("<li><a href=\"{}\">{}</a></li>\n", esc(m), esc(tr(l, "home_mail"))));
    }
    if let Some(a) = &p.adminer {
        h.push_str(&format!("<li><a href=\"{}\">{}</a></li>\n", esc(a), esc(tr(l, "home_adminer"))));
    }
    h.push_str("<li><a href=\"phpinfo.php\">phpinfo()</a></li>\n</ul>\n");
    h.push_str(&format!("<footer class=\"muted\">{}</footer>\n</main>\n</body>\n</html>\n", esc(tr(l, "home_footer"))));
    h
}

/// Everything the page shows, read from the settings and the folders.
pub fn collect(s: &Settings) -> Page {
    let root = Path::new(&s.doc_root);
    let php_global = services::version_label(s, "php").map(|v| install::short_ver(&v)).unwrap_or_default();
    let sites_rows = sites::list(s)
        .iter()
        .map(|x| {
            let kind = sites::kind(&root.join(&x.folder));
            let php = match kind {
                "Laravel" | "WordPress" | "Symfony" | "PHP" => match s.sites.iter().find(|c| c.folder == x.folder) {
                    Some(c) if !c.php.is_empty() => install::short_ver(&c.php),
                    _ => php_global.clone(),
                },
                _ => String::new(),
            };
            SiteRow { folder: x.folder.clone(), url: site_url(s, x), kind: kind.to_string(), php }
        })
        .collect();
    let servers = s
        .services
        .iter()
        .filter(|c| c.enabled)
        .filter_map(|c| {
            let v = services::version_label(s, &c.id)?;
            Some(ServerRow { name: c.title(), version: install::short_ver(&v), port: c.port })
        })
        .collect();
    let mail = s
        .services
        .iter()
        .find(|c| c.key == "mailpit" && c.enabled && services::version_label(s, &c.id).is_some())
        .map(|c| format!("http://localhost:{}", services::mailpit_web_port(c)));
    let adminer = Path::new(&s.root).join(r"bin\adminer\index.php").is_file().then(|| "http://127.0.0.1:8099/?server=127.0.0.1&username=root".to_string());
    let user_index = ["index.php", "index.html", "index.htm"].iter().any(|f| root.join(f).is_file()).then(|| format!("{}/_www/", base_url(s)));
    Page { lang: s.language, doc_root: s.doc_root.clone(), sites: sites_rows, servers, mail, adminer, user_index }
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The Apache `localhost` server: Ninho's page, the person's folders still at localhost/<folder>, and their own
/// files of the www root at localhost/_www/.
pub fn apache_block(home: &str, www: &str, port: u16, folders: &[String]) -> Vec<String> {
    let mut n = vec![
        String::new(),
        "# Ninho home page (http://localhost). The sites below keep working at localhost/<folder>.".to_string(),
        format!("<VirtualHost *:{port}>"),
        "    ServerName localhost".to_string(),
        format!("    DocumentRoot \"{home}\""),
        format!("    <Directory \"{home}\">"),
        "        Require all granted".to_string(),
        "    </Directory>".to_string(),
        format!("    Alias /_www \"{www}\""),
        format!("    <Directory \"{www}\">"),
        "        Require all granted".to_string(),
        "        DirectoryIndex index.php index.html index.htm".to_string(),
        "    </Directory>".to_string(),
    ];
    for f in folders {
        n.push(format!("    Alias /{f} \"{www}/{f}\""));
    }
    n.push("</VirtualHost>".into());
    n
}

/// The lines for conf/ninho.conf (empty when the option is off).
pub fn apache_lines(s: &Settings) -> Vec<String> {
    if !s.home_page {
        return Vec::new();
    }
    let folders: Vec<String> = sites::list(s).into_iter().map(|x| x.folder).collect();
    apache_block(&slash(&page_dir(s)), &slash(Path::new(&s.doc_root)), s.svc("apache").port, &folders)
}

/// The Nginx `localhost` server, same idea.
pub fn nginx_block(home: &str, www: &str, port: u16, folders: &[String]) -> String {
    let mut t = format!(
        "# Ninho home page (http://localhost). Written again at every refresh.\r\nserver {{\r\n    listen {port};\r\n    server_name localhost;\r\n    root \"{home}\";\r\n    index index.html;\r\n\r\n    location ~ \\.php$ {{\r\n        include snippets/fastcgi-php.conf;\r\n        fastcgi_pass php_upstream;\r\n    }}\r\n\r\n    location ^~ /_www/ {{\r\n        alias \"{www}/\";\r\n        index index.php index.html;\r\n        autoindex on;\r\n    }}\r\n"
    );
    for f in folders {
        t.push_str(&format!(
            "\r\n    location ^~ /{f}/ {{\r\n        root \"{www}\";\r\n        index index.php index.html;\r\n        autoindex on;\r\n        location ~ \\.php$ {{\r\n            include snippets/fastcgi-php.conf;\r\n            fastcgi_pass php_upstream;\r\n        }}\r\n    }}\r\n"
        ));
    }
    t.push_str("}\r\n");
    t
}

const NGINX_FILE: &str = "00-ninho-home.conf";

/// Write the page (and phpinfo.php) and, when Nginx is on, its `localhost` server file. With the option off it writes
/// nothing and removes the Nginx file. Only rewrites a file whose text changed.
pub fn write(s: &Settings) {
    let nginx_dir = Path::new(&s.root).join(r"etc\nginx\sites-enabled");
    if !s.home_page {
        let _ = std::fs::remove_file(nginx_dir.join(NGINX_FILE));
        return;
    }
    let dir = page_dir(s);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    put(&dir.join("index.html"), &render(&collect(s)));
    put(&dir.join("phpinfo.php"), "<?php phpinfo();\n");
    if s.kind("nginx").is_some_and(|c| c.enabled) && nginx_dir.is_dir() {
        let folders: Vec<String> = sites::list(s).into_iter().map(|x| x.folder).collect();
        put(&nginx_dir.join(NGINX_FILE), &nginx_block(&slash(&dir), &slash(Path::new(&s.doc_root)), s.kind("nginx").map_or(8080, |c| c.port), &folders));
    }
}

fn put(path: &Path, text: &str) {
    if std::fs::read_to_string(path).ok().as_deref() != Some(text) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(sites: Vec<SiteRow>) -> Page {
        Page {
            lang: Lang::from_code("en"),
            doc_root: r"C:\www".into(),
            sites,
            servers: vec![ServerRow { name: "Apache".into(), version: "2.4".into(), port: 80 }],
            mail: Some("http://localhost:8025".into()),
            adminer: None,
            user_index: None,
        }
    }

    fn set_apache_port(s: &mut Settings, port: u16) {
        s.services.iter_mut().find(|c| c.id == "apache").unwrap().port = port;
    }

    #[test]
    fn escapes_folder_names() {
        let h = render(&page(vec![SiteRow { folder: "a<b>&\"c".into(), url: "http://x\"><script>".into(), kind: "PHP".into(), php: "8.3".into() }]));
        assert!(!h.contains("<script>"));
        assert!(h.contains("a&lt;b&gt;&amp;&quot;c"));
        assert!(h.contains("PHP 8.3"));
    }

    #[test]
    fn links_with_and_without_auto_vhosts() {
        let mut s = Settings::default();
        let site = sites::Site { folder: "blog".into(), host: "blog.test".into() };
        s.auto_vhosts = true;
        s.https = false;
        set_apache_port(&mut s, 80);
        assert_eq!(site_url(&s, &site), "http://blog.test");
        s.auto_vhosts = false;
        assert_eq!(site_url(&s, &site), "http://localhost/blog");
        set_apache_port(&mut s, 8080);
        assert_eq!(site_url(&s, &site), "http://localhost:8080/blog");
        s.auto_vhosts = true;
        assert_eq!(site_url(&s, &site), "http://blog.test:8080");
    }

    #[test]
    fn page_has_no_outside_requests_and_follows_language() {
        let h = render(&page(vec![]));
        assert!(!h.contains("src=\"http") && !h.contains("rel=\"stylesheet\""));
        assert!(h.contains("prefers-color-scheme:dark") && h.contains("width=device-width"));
        assert!(h.contains("phpinfo.php") && h.contains("http://localhost:8025"));
        let mut p = page(vec![]);
        p.lang = Lang::from_code("pt-BR");
        assert!(render(&p).contains("lang=\"pt-BR\""));
        assert_ne!(render(&p), h);
    }

    #[test]
    fn note_about_the_users_own_index() {
        let mut p = page(vec![]);
        p.user_index = Some("http://localhost/_www/".into());
        assert!(render(&p).contains("href=\"http://localhost/_www/\""));
    }

    #[test]
    fn apache_lines_point_at_home_and_keep_folders() {
        let l = apache_block("C:/n/etc/ninho-home", "C:/www", 80, &["blog".into(), "shop".into()]).join("\n");
        assert!(l.contains("<VirtualHost *:80>") && l.contains("ServerName localhost"));
        assert!(l.contains("DocumentRoot \"C:/n/etc/ninho-home\""));
        assert!(l.contains("Alias /blog \"C:/www/blog\"") && l.contains("Alias /shop \"C:/www/shop\""));
        assert!(l.contains("Alias /_www \"C:/www\""));
        assert!(l.trim_end().ends_with("</VirtualHost>"));
        let mut s = Settings::default();
        s.home_page = false;
        assert!(apache_lines(&s).is_empty());
    }

    #[test]
    fn nginx_block_points_at_home() {
        let t = nginx_block("C:/n/etc/ninho-home", "C:/www", 8080, &["blog".into()]);
        assert!(t.contains("listen 8080;") && t.contains("server_name localhost;"));
        assert!(t.contains("root \"C:/n/etc/ninho-home\";"));
        assert!(t.contains("location ^~ /blog/") && t.contains("root \"C:/www\";"));
    }

    #[test]
    fn writes_into_its_own_folder_only() {
        let tmp = std::env::temp_dir().join(format!("ninho-home-{}", std::process::id()));
        let www = tmp.join("www");
        std::fs::create_dir_all(www.join("blog")).unwrap();
        std::fs::write(www.join("index.php"), "x").unwrap();
        let mut s = Settings::default();
        s.root = tmp.to_string_lossy().into_owned();
        s.doc_root = www.to_string_lossy().into_owned();
        write(&s);
        let html = std::fs::read_to_string(page_dir(&s).join("index.html")).unwrap();
        assert!(html.contains("blog") && html.contains("/_www/"));
        assert!(page_dir(&s).join("phpinfo.php").is_file());
        assert_eq!(std::fs::read_dir(&www).unwrap().count(), 2); // blog + index.php, nothing added
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
