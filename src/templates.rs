//! Project templates for "New project": built-in ones (Laragon's "Quick app") plus the person's own
//! from `<root>\etc\ninho-templates.json`. Pure string/file work, so it is testable without a window.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::settings::{Settings, name_ok};

/// Windows' own tools: Git Bash's tar cannot unzip, and these ship with every Windows 10+.
const CURL: &str = r"C:\Windows\System32\curl.exe";
const TAR: &str = r"C:\Windows\System32\tar.exe";

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Template {
    pub name: String,
    /// "composer" | "zip" | "git" | "blank" | "npm"
    pub kind: String,
    /// Composer package, URL or command line, depending on `kind`.
    pub source: String,
    /// Folder the web server should serve inside the project ("public", "web"); empty = the project folder.
    #[serde(default)]
    pub public_dir: String,
    /// Offer to create a database with the project's name.
    #[serde(default)]
    pub create_db: bool,
}

fn t(name: &str, kind: &str, source: &str, public_dir: &str, create_db: bool) -> Template {
    Template { name: name.into(), kind: kind.into(), source: source.into(), public_dir: public_dir.into(), create_db }
}

/// The templates that ship with Ninho. "From a ZIP URL" and "From a git repository" have an empty source:
/// the person types it (see `needs_source`).
pub fn builtin() -> Vec<Template> {
    vec![
        t("Vazio", "blank", "", "", false),
        t("WordPress", "zip", "https://wordpress.org/latest.zip", "", true),
        t("Laravel", "composer", "laravel/laravel", "public", true),
        // The skeleton is bare; command() prints the tip to add "webapp".
        t("Symfony", "composer", "symfony/skeleton", "public", true),
        t("Drupal", "composer", "drupal/recommended-project", "web", true),
        t("CodeIgniter 4", "composer", "codeigniter4/appstarter", "public", true),
        t("Slim", "composer", "slim/slim-skeleton", "public", false),
        // A blank project whose source names the file to write.
        t("HTML estático", "blank", "index.html", "", false),
        t("Vite + React", "npm", "npm create vite@latest {name} -- --template react", "", false),
        t("Vite + Vue", "npm", "npm create vite@latest {name} -- --template vue", "", false),
        t("A partir de um ZIP", "zip", "", "", false),
        t("A partir de um repositório git", "git", "", "", false),
    ]
}

const EXAMPLE: &str = r#"[
  {
    "name": "Meu Laravel",
    "kind": "composer",
    "source": "laravel/laravel",
    "public_dir": "public",
    "create_db": true
  }
]
"#;

fn kind_ok(k: &str) -> bool {
    matches!(k, "composer" | "zip" | "git" | "blank" | "npm")
}

/// Parses the person's file. Entries with an empty name or an unknown kind are dropped so a typo can't break the list.
pub fn parse_user(json: &str) -> Result<Vec<Template>, serde_json::Error> {
    let list: Vec<Template> = serde_json::from_str(json)?;
    Ok(list.into_iter().filter(|x| !x.name.trim().is_empty() && kind_ok(&x.kind)).collect())
}

/// Built-ins plus the person's own. The file is created with an example the first time and never overwritten;
/// a broken file just means "built-ins only".
pub fn all(s: &Settings) -> Vec<Template> {
    let mut out = builtin();
    let dir = Path::new(&s.root).join("etc");
    let file = dir.join("ninho-templates.json");
    if !file.exists() {
        let _ = std::fs::create_dir_all(&dir);
        // create_new: if two windows race, neither overwrites the other's edits.
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create_new(true).open(&file) {
            let _ = std::io::Write::write_all(&mut f, EXAMPLE.as_bytes());
        }
    }
    if let Ok(text) = std::fs::read_to_string(&file) {
        if let Ok(mine) = parse_user(text.trim_start_matches('\u{feff}')) {
            for m in mine {
                // Same name as a built-in: the person's version wins.
                if let Some(i) = out.iter().position(|b| b.name.eq_ignore_ascii_case(&m.name)) {
                    out[i] = m;
                } else {
                    out.push(m);
                }
            }
        }
    }
    out
}

/// True when the person must type the URL (ZIP / git entries with no source of their own).
pub fn needs_source(t: &Template) -> bool {
    matches!(t.kind.as_str(), "zip" | "git") && t.source.trim().is_empty()
}

/// The typed source when given, else the template's own.
fn effective<'a>(t: &'a Template, over: Option<&'a str>) -> &'a str {
    match over.map(str::trim) {
        Some(o) if !o.is_empty() => o,
        _ => t.source.trim(),
    }
}

/// Only characters that need no quoting in a cmd.exe line: no & | < > ^ " % or spaces, because cmd would
/// treat them as syntax and run something else.
fn safe_chars(s: &str, extra: &str) -> bool {
    s.chars().all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c) || extra.contains(c))
}

fn https_ok(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    url.len() <= 500 && !rest.is_empty() && !rest.starts_with('/') && safe_chars(rest, ":/?=+@,#")
}

/// Validates name and source before anything is run. Short English messages, for the log or an error line.
pub fn check(t: &Template, name: &str, source_override: Option<&str>) -> Result<(), &'static str> {
    // A leading dot would let ".." escape the sites folder.
    if !name_ok(name) || name.starts_with('.') {
        return Err("invalid project name");
    }
    let src = effective(t, source_override);
    match t.kind.as_str() {
        "blank" => Ok(()),
        "zip" | "git" => {
            if src.is_empty() {
                Err("missing URL")
            } else if !https_ok(src) {
                Err("the URL must start with https:// and use only plain characters")
            } else {
                Ok(())
            }
        }
        "composer" => {
            if src.is_empty() || !src.contains('/') || !safe_chars(src, "/:") {
                Err("invalid composer package")
            } else {
                Ok(())
            }
        }
        "npm" => {
            let first = src.split_whitespace().next().unwrap_or("");
            if !matches!(first, "npm" | "npx") {
                Err("the command must start with npm or npx")
            } else if !safe_chars(&src.replace(' ', ""), "{}@/:=+,") {
                Err("the command has characters that are not allowed")
            } else {
                Ok(())
            }
        }
        _ => Err("unknown template kind"),
    }
}

/// Line for a visible terminal window, run with the sites folder as current directory. None = nothing to run
/// (blank/static: use `create_direct`). The caller adds its own "done" message after it. A bad name or source
/// gives a line that only prints the reason, so nothing unchecked ever reaches cmd.
pub fn command(t: &Template, name: &str, source_override: Option<&str>) -> Option<String> {
    if let Err(e) = check(t, name, source_override) {
        return Some(format!("echo Ninho - {e}"));
    }
    let src = effective(t, source_override);
    match t.kind.as_str() {
        "composer" => {
            let mut c = format!("composer create-project {src} {name} --prefer-dist --no-interaction");
            if src.starts_with("symfony/skeleton") {
                c.push_str(&format!(" && echo. && echo Tip - for a full website run: cd {name} then composer require webapp"));
            }
            Some(c)
        }
        "git" => Some(format!("git clone {src} {name}")),
        "npm" => Some(src.replace("{name}", name)),
        "zip" => {
            let zip = format!(".ninho-{name}.zip");
            let tmp = format!(".ninho-{name}.tmp");
            // One top folder in the archive (WordPress: "wordpress") becomes the project; otherwise the
            // extracted files do. No double quotes anywhere: they break the `cmd /k` wrapper.
            let ps = format!(
                "$ErrorActionPreference='Stop'; $d=@(Get-ChildItem -LiteralPath '{tmp}' -Force); \
                 if($d.Count -eq 1 -and $d[0].PSIsContainer){{Move-Item -LiteralPath $d[0].FullName -Destination '{name}'; Remove-Item -LiteralPath '{tmp}'}} \
                 else{{Move-Item -LiteralPath '{tmp}' -Destination '{name}'}}"
            );
            Some(format!(
                "{CURL} -L -f --retry 2 -o {zip} {src} && mkdir {tmp} && {TAR} -xf {zip} -C {tmp} && powershell -NoProfile -Command {ps} && del {zip}"
            ))
        }
        _ => None,
    }
}

const WELCOME_PHP: &str = "<?php $projeto = '{name}'; ?>\n<!doctype html>\n<html lang=\"pt-BR\">\n<head><meta charset=\"utf-8\"><title>{name}</title></head>\n<body style=\"font-family:sans-serif;max-width:40em;margin:4em auto\">\n<h1>Projeto {name} criado pelo Ninho</h1>\n<p>PHP <?= PHP_VERSION ?> funcionando. Edite este arquivo (index.php) e comece.</p>\n</body>\n</html>\n";

const WELCOME_HTML: &str = "<!doctype html>\n<html lang=\"pt-BR\">\n<head><meta charset=\"utf-8\"><title>{name}</title></head>\n<body style=\"font-family:sans-serif;max-width:40em;margin:4em auto\">\n<h1>Projeto {name} criado pelo Ninho</h1>\n<p>Edite este arquivo (index.html) e comece.</p>\n</body>\n</html>\n";

/// Creates a blank (index.php) or static (index.html) project folder. An existing file is never overwritten.
pub fn create_direct(t: &Template, dir: &Path) -> std::io::Result<()> {
    if t.kind != "blank" {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "template is not created directly"));
    }
    std::fs::create_dir_all(dir)?;
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // The caller checks the name; dropping markup/quote characters is only a second belt.
    let safe: String = name.chars().filter(|c| !"<>&'\"".contains(*c)).collect();
    let (file, body) = if t.source.trim().ends_with(".html") { ("index.html", WELCOME_HTML) } else { ("index.php", WELCOME_PHP) };
    let path = dir.join(file);
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, body.replace("{name}", &safe))
}

/// Tools the template needs that are not in the given PATH string (";"-separated). curl and tar come with Windows.
pub fn missing_tools(t: &Template, path_env: &str) -> Vec<&'static str> {
    let on_path = |files: &[&str]| {
        path_env
            .split(';')
            .filter(|d| !d.trim().is_empty())
            .any(|d| files.iter().any(|f| Path::new(d.trim()).join(f).is_file()))
    };
    let mut out = Vec::new();
    match t.kind.as_str() {
        "composer" if !on_path(&["composer.bat", "composer.cmd", "composer.exe"]) => out.push("composer"),
        "git" if !on_path(&["git.exe", "git.cmd"]) => out.push("git"),
        "npm" => {
            if !on_path(&["npm.cmd", "npm.exe"]) {
                out.push("npm");
            }
            if !on_path(&["node.exe"]) {
                out.push("node");
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(n: &str) -> Template {
        builtin().into_iter().find(|x| x.name == n).unwrap()
    }
    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ninho-tpl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn composer_commands() {
        assert_eq!(
            command(&find("Laravel"), "blog", None).unwrap(),
            "composer create-project laravel/laravel blog --prefer-dist --no-interaction"
        );
        assert!(command(&find("Drupal"), "d", None).unwrap().starts_with("composer create-project drupal/recommended-project d "));
        let sf = command(&find("Symfony"), "s", None).unwrap();
        assert!(sf.contains("symfony/skeleton s") && sf.contains("webapp"));
    }

    #[test]
    fn zip_command_uses_system_tools() {
        let c = command(&find("WordPress"), "wp", None).unwrap();
        assert!(c.starts_with(r"C:\Windows\System32\curl.exe -L -f"));
        assert!(c.contains(r"C:\Windows\System32\tar.exe -xf .ninho-wp.zip -C .ninho-wp.tmp"));
        assert!(c.contains("https://wordpress.org/latest.zip") && c.contains("Destination 'wp'") && !c.contains('"'));
    }

    #[test]
    fn git_and_npm_commands() {
        let g = find("A partir de um repositório git");
        assert!(needs_source(&g));
        assert_eq!(command(&g, "app", Some("https://github.com/a/b.git")).unwrap(), "git clone https://github.com/a/b.git app");
        assert_eq!(command(&find("Vite + Vue"), "ui", None).unwrap(), "npm create vite@latest ui -- --template vue");
    }

    #[test]
    fn blank_has_no_command_and_bad_input_is_blocked() {
        assert!(command(&find("Vazio"), "x", None).is_none());
        let g = find("A partir de um repositório git");
        for bad in ["http://a.com/x.git", "https://a.com/x&calc", "https://a.com/x y", "file:///c:/x", "https://"] {
            assert!(command(&g, "x", Some(bad)).unwrap().starts_with("echo Ninho"), "{bad}");
        }
        assert!(command(&find("Laravel"), "a&b", None).unwrap().starts_with("echo Ninho"));
        assert!(command(&find("Laravel"), "..", None).unwrap().starts_with("echo Ninho"));
        assert!(command(&t("e", "composer", "a/b && calc", "", false), "x", None).unwrap().starts_with("echo Ninho"));
        assert!(command(&t("e", "npm", "calc", "", false), "x", None).unwrap().starts_with("echo Ninho"));
        assert!(command(&t("e", "npm", "npm run x & calc", "", false), "x", None).unwrap().starts_with("echo Ninho"));
    }

    #[test]
    fn user_json_round_trip() {
        let mine = vec![t("Mine", "composer", "a/b:^1.0", "web", true)];
        let json = serde_json::to_string_pretty(&mine).unwrap();
        assert_eq!(parse_user(&json).unwrap(), mine);
        // Omitted fields default, bad entries are dropped.
        let got = parse_user(
            r#"[{"name":"A","kind":"git","source":"https://x.io/r.git"},{"name":"","kind":"git","source":""},{"name":"B","kind":"weird","source":""}]"#,
        )
        .unwrap();
        assert_eq!(got.len(), 1);
        assert!(got[0].public_dir.is_empty() && !got[0].create_db);
        assert_eq!(parse_user(EXAMPLE).unwrap().len(), 1);
    }

    #[test]
    fn all_creates_example_once_and_merges() {
        let root = tmp("all");
        let s = Settings { root: root.to_string_lossy().into(), doc_root: String::new(), ..Settings::default() };
        assert_eq!(all(&s).len(), builtin().len() + 1);
        let file = root.join("etc").join("ninho-templates.json");
        std::fs::write(&file, r#"[{"name":"laravel","kind":"composer","source":"x/y"}]"#).unwrap();
        let second = all(&s); // not overwritten; same name replaces the built-in
        assert_eq!(second.len(), builtin().len());
        assert_eq!(second.iter().find(|x| x.name == "laravel").unwrap().source, "x/y");
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(all(&s).len(), builtin().len());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn direct_creation() {
        let root = tmp("direct");
        create_direct(&find("Vazio"), &root.join("a")).unwrap();
        assert!(std::fs::read_to_string(root.join("a/index.php")).unwrap().contains("criado pelo Ninho"));
        create_direct(&find("HTML estático"), &root.join("b")).unwrap();
        assert!(std::fs::read_to_string(root.join("b/index.html")).unwrap().contains("Projeto b criado pelo Ninho"));
        assert!(create_direct(&find("Laravel"), &root.join("c")).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_tools_with_fake_path() {
        let a = tmp("pa");
        let b = tmp("pb");
        std::fs::write(a.join("composer.bat"), "").unwrap();
        std::fs::write(b.join("git.exe"), "").unwrap();
        let path = format!("{};{};", a.display(), b.display());
        assert!(missing_tools(&find("Laravel"), &path).is_empty());
        assert!(missing_tools(&find("A partir de um repositório git"), &path).is_empty());
        assert_eq!(missing_tools(&find("Vite + React"), &path), vec!["npm", "node"]);
        assert_eq!(missing_tools(&find("Laravel"), &b.to_string_lossy()), vec!["composer"]);
        assert!(missing_tools(&find("WordPress"), "").is_empty());
        assert!(missing_tools(&find("Vazio"), "").is_empty());
        std::fs::write(a.join("npm.cmd"), "").unwrap();
        std::fs::write(a.join("node.exe"), "").unwrap();
        assert!(missing_tools(&find("Vite + Vue"), &path).is_empty());
        let _ = (std::fs::remove_dir_all(a), std::fs::remove_dir_all(b));
    }
}
