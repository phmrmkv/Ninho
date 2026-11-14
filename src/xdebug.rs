//! Xdebug on/off for one installed PHP folder: downloads the matching php_xdebug.dll from xdebug.org into ext\ and
//! switches `zend_extension=xdebug` in php.ini. Xdebug names its Windows builds
//! `php_xdebug-<version>-<php minor>-<ts|nts>-<vs16|vs17>-x86_64.dll`, so the PHP facts decide the file.
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::i18n::{Lang, tr};
use crate::services::{Shared, hide_window};

/// The facts about a PHP build that pick the right Xdebug DLL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhpInfo {
    /// "8.5"
    pub minor: String,
    /// Thread safe build (Apache module / Laragon default) or not.
    pub ts: bool,
    /// "vs16" or "vs17"
    pub vs: String,
    pub x64: bool,
}

/// Settings written under `[xdebug]` when Xdebug is switched on.
const BLOCK: [&str; 3] = ["xdebug.mode=debug,develop", "xdebug.start_with_request=trigger", "xdebug.client_port=9003"];

fn win(tool: &str) -> PathBuf {
    // Windows' own curl, always there on Windows 10+.
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    Path::new(&sys).join("System32").join(tool)
}

fn get_text(url: &str) -> Result<String, String> {
    let mut c = Command::new(win("curl.exe"));
    c.args(["-L", "-f", "-s", "-S", "-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Ninho", url]);
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("{url}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn download(lang: Lang, url: &str, to: &Path) -> Result<(), String> {
    let mut c = Command::new(win("curl.exe"));
    c.args(["-L", "-f", "-s", "-S", "--retry", "2", "-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Ninho"]);
    c.arg("-o").arg(to).arg(url);
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(format!("{} {url}: {}", tr(lang, "log_download_failed"), String::from_utf8_lossy(&out.stderr).trim())) }
}

/// Numbers of "3.5.3" in order, for picking the newest.
fn ver_key(v: &str) -> Vec<u32> {
    v.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
}

/// "8.5.12-1" → "8.5"
fn minor_of(version: &str) -> Option<String> {
    let mut p = version.split(['.', '-']);
    let (a, b) = (p.next()?, p.next()?);
    if a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit()) && !a.is_empty() && !b.is_empty() {
        Some(format!("{a}.{b}"))
    } else {
        None
    }
}

/// The value after `=>` of a `php -i` line that starts with `key =>`.
fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| l.strip_prefix(key)?.trim_start().strip_prefix("=>").map(str::trim))
}

/// Read the facts from the text of `php -i`.
fn parse_info(text: &str) -> Option<PhpInfo> {
    let minor = minor_of(field(text, "PHP Version")?)?;
    let ts = field(text, "Thread Safety")?.eq_ignore_ascii_case("enabled");
    // "Zend Extension Build => API420250925,TS,VS17" is exact; "Compiler => Visual C++ 2022" is the fallback.
    let vs = field(text, "Zend Extension Build")
        .and_then(|b| b.split(',').map(str::trim).find(|p| p.len() == 4 && p.to_ascii_lowercase().starts_with("vs")))
        .map(str::to_ascii_lowercase)
        .or_else(|| {
            let c = field(text, "Compiler")?;
            if c.contains("2022") {
                Some("vs17".to_string())
            } else if c.contains("2019") {
                Some("vs16".to_string())
            } else {
                None
            }
        })?;
    let x64 = field(text, "Architecture").map(|a| a.eq_ignore_ascii_case("x64")).unwrap_or(true);
    Some(PhpInfo { minor, ts, vs, x64 })
}

/// Fallback when php.exe cannot run: read "php-8.5.12-1-Win32-vs17-x64" / "php-8.3.36-nts-Win32-vs16-x64".
fn info_from_name(name: &str) -> Option<PhpInfo> {
    let l = name.to_ascii_lowercase();
    let minor = minor_of(l.strip_prefix("php-")?)?;
    let vs = l.split(['-', '_']).find(|p| p.len() == 4 && p.starts_with("vs") && p[2..].chars().all(|c| c.is_ascii_digit()))?.to_string();
    Some(PhpInfo { minor, ts: !l.contains("nts"), vs, x64: !l.contains("x86") || l.contains("x64") })
}

/// PHP facts needed to pick the right Xdebug DLL. Asks php.exe (it knows its own build); the folder name is the fallback.
pub fn php_info(lang: Lang, php_dir: &Path) -> Result<PhpInfo, String> {
    let exe = php_dir.join("php.exe");
    if !exe.is_file() {
        return Err(format!("{} {}", tr(lang, "log_php_exe_missing"), php_dir.display()));
    }
    // -n: no php.ini, so a broken ini or Xdebug itself cannot add warnings or slow the call down.
    let mut c = Command::new(&exe);
    c.arg("-n").arg("-i");
    hide_window(&mut c);
    if let Ok(out) = c.output() {
        if let Some(i) = parse_info(&String::from_utf8_lossy(&out.stdout)) {
            return Ok(i);
        }
    }
    let name = php_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    info_from_name(&name).ok_or_else(|| format!("could not tell the PHP version of {}", php_dir.display()))
}

fn dll_path(php_dir: &Path) -> PathBuf {
    php_dir.join("ext").join("php_xdebug.dll")
}

/// File name Xdebug publishes for this PHP and Xdebug version.
fn dll_name(xdebug: &str, i: &PhpInfo) -> String {
    format!("php_xdebug-{xdebug}-{}-{}-{}-x86_64.dll", i.minor, if i.ts { "ts" } else { "nts" }, i.vs)
}

/// Split "php_xdebug-3.5.3-8.5-ts-vs17-x86_64.dll" into (xdebug version, PhpInfo).
fn parse_dll_name(name: &str) -> Option<(String, PhpInfo)> {
    let stem = name.strip_prefix("php_xdebug-")?.strip_suffix("-x86_64.dll")?;
    let p: Vec<&str> = stem.split('-').collect();
    if p.len() != 4 || !matches!(p[2], "ts" | "nts") || !p[3].starts_with("vs") {
        return None;
    }
    Some((p[0].to_string(), PhpInfo { minor: p[1].to_string(), ts: p[2] == "ts", vs: p[3].to_string(), x64: true }))
}

/// Newest stable Xdebug version that has a build for this PHP, found in the links of a download page.
/// Pre-releases (3.6.0alpha1, rc...) are skipped: only plain numbers count.
fn newest_for(page: &str, i: &PhpInfo) -> Option<String> {
    page.match_indices("php_xdebug-")
        .filter_map(|(at, _)| {
            let rest = &page[at..];
            let end = rest.find(".dll")? + 4;
            parse_dll_name(&rest[..end])
        })
        .filter(|(v, f)| v.chars().all(|c| c.is_ascii_digit() || c == '.') && f.minor == i.minor && f.ts == i.ts && f.vs == i.vs)
        .map(|(v, _)| v)
        .max_by_key(|v| ver_key(v))
}

/// Is the xdebug DLL present in ext\ and is `zend_extension=xdebug` active in php.ini?
pub fn status(php_dir: &Path) -> (bool, bool) {
    let installed = dll_path(php_dir).is_file();
    let on = std::fs::read_to_string(php_dir.join("php.ini")).map(|t| t.lines().any(|l| parse_ext(l) == Some(true))).unwrap_or(false);
    (installed, on)
}

/// Download the matching DLL from xdebug.org into ext\php_xdebug.dll. Returns the Xdebug version.
pub fn install(php_dir: &Path, shared: &Shared, lang: Lang) -> Result<String, String> {
    let info = php_info(lang, php_dir)?;
    if !info.x64 {
        return Err(tr(lang, "log_xdebug_64").into());
    }
    let no_build = || format!("{} {}", tr(lang, "log_xdebug_no_build"), info.minor);
    shared.say(format!("Xdebug: {} {} ({}, {})...", tr(lang, "log_xdebug_looking"), info.minor, if info.ts { "TS" } else { "NTS" }, info.vs));
    // The main page lists the current releases; the old ones are on the historical page.
    let mut version = newest_for(&get_text("https://xdebug.org/download")?, &info);
    if version.is_none() {
        version = newest_for(&get_text("https://xdebug.org/download/historical")?, &info);
    }
    let version = version.ok_or_else(no_build)?;
    let url = format!("https://xdebug.org/files/{}", dll_name(&version, &info));
    let ext = php_dir.join("ext");
    if !ext.is_dir() {
        return Err(format!("{} {}", tr(lang, "log_no_ext_folder"), php_dir.display()));
    }
    // Download beside the target and rename, so a failed download never leaves a half DLL that PHP would try to load.
    let part = ext.join("php_xdebug.dll.part");
    shared.say(format!("Xdebug {version}: {} {url}", tr(lang, "log_downloading")));
    let res = download(lang, &url, &part).and_then(|_| {
        let len = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let mut mz = [0u8; 2];
        let is_pe = std::fs::File::open(&part).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut mz)).is_ok() && &mz == b"MZ";
        if len < 100_000 || !is_pe { Err(tr(lang, "log_not_dll").to_string()) } else { Ok(()) }
    });
    if let Err(e) = res {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    // Fails when a running PHP (php-cgi, Apache) holds the old DLL.
    if let Err(e) = std::fs::rename(&part, dll_path(php_dir)) {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{} {e}", tr(lang, "log_xdebug_replace_failed")));
    }
    shared.say(format!("Xdebug {version}: {} {}", tr(lang, "log_installed_in"), ext.display()));
    Ok(version)
}

/// For a `zend_extension` line (commented or not) that loads Xdebug: Some(is it active). Anything else: None.
fn parse_ext(line: &str) -> Option<bool> {
    let t = line.trim();
    let (on, rest) = match t.strip_prefix(';') {
        Some(r) => (false, r.trim_start()),
        None => (true, t),
    };
    let v = rest.strip_prefix("zend_extension")?.trim_start().strip_prefix('=')?.trim().trim_matches('"').to_ascii_lowercase();
    // "xdebug", "php_xdebug.dll", "C:\...\php_xdebug.dll"; prose lines have spaces.
    if v.is_empty() || v.contains(' ') || !v.contains("xdebug") {
        return None;
    }
    Some(on)
}

fn is_active(line: &str) -> bool {
    !line.trim_start().starts_with(';')
}

/// Turn Xdebug on/off in php.ini. Every other line stays as it was; repeating the call changes nothing.
pub fn set(php_dir: &Path, on: bool) -> Result<(), String> {
    let ini = php_dir.join("php.ini");
    let text = std::fs::read_to_string(&ini).map_err(|e| format!("{}: {e}", ini.display()))?;
    let new = edit_ini(&text, on);
    if new != text {
        std::fs::write(&ini, new).map_err(|e| format!("{}: {e}", ini.display()))?;
    }
    Ok(())
}

/// The php.ini text with Xdebug switched on or off.
fn edit_ini(text: &str, on: bool) -> String {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut seen = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| match parse_ext(l) {
            // First Xdebug line becomes the one line; any further ones are commented, two copies would not load.
            Some(_) if on && !seen => {
                seen = true;
                "zend_extension=xdebug".to_string()
            }
            Some(_) if is_active(l) => format!(";{}", l.trim_start()),
            _ => l.to_string(),
        })
        .collect();
    if on {
        let has_header = lines.iter().any(|l| l.trim().eq_ignore_ascii_case("[xdebug]"));
        if !has_header {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push("[xdebug]".into());
        }
        if !seen {
            lines.push("zend_extension=xdebug".into());
        }
        if !has_header {
            lines.extend(BLOCK.iter().map(|s| s.to_string()));
        }
    }
    lines.join(nl) + nl
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHP_I: &str = "phpinfo()\nPHP Version => 8.5.12\n\nSystem => Windows NT PC 10.0 build 26300 (Windows 11) AMD64\nCompiler => Visual C++ 2022\nArchitecture => x64\nThread Safety => enabled\nZend Extension Build => API420250925,TS,VS17\nPHP Extension Build => API20250925,TS,VS17\n";

    #[test]
    fn parses_php_i() {
        let i = parse_info(PHP_I).unwrap();
        assert_eq!(i, PhpInfo { minor: "8.5".into(), ts: true, vs: "vs17".into(), x64: true });
        let nts = PHP_I.replace("enabled", "disabled").replace("TS,VS17", "NTS,VS16").replace("8.5.12", "8.3.36");
        let i = parse_info(&nts).unwrap();
        assert_eq!((i.minor.as_str(), i.ts, i.vs.as_str()), ("8.3", false, "vs16"));
        // Without the build line the compiler name decides.
        let no_build: String = PHP_I.lines().filter(|l| !l.contains("Build")).collect::<Vec<_>>().join("\n");
        assert_eq!(parse_info(&no_build).unwrap().vs, "vs17");
        assert!(parse_info("nothing useful").is_none());
    }

    #[test]
    fn reads_folder_names() {
        let i = info_from_name("php-8.5.12-1-Win32-vs17-x64").unwrap();
        assert_eq!((i.minor.as_str(), i.ts, i.vs.as_str(), i.x64), ("8.5", true, "vs17", true));
        let i = info_from_name("php-8.3.36-nts-Win32-vs16-x64").unwrap();
        assert_eq!((i.minor.as_str(), i.ts, i.vs.as_str()), ("8.3", false, "vs16"));
        assert!(info_from_name("my-php").is_none());
    }

    #[test]
    fn builds_and_matches_dll_names() {
        let i = PhpInfo { minor: "8.5".into(), ts: true, vs: "vs17".into(), x64: true };
        let n = dll_name("3.5.3", &i);
        assert_eq!(n, "php_xdebug-3.5.3-8.5-ts-vs17-x86_64.dll");
        let (v, f) = parse_dll_name(&n).unwrap();
        assert_eq!((v.as_str(), f), ("3.5.3", i));
        assert!(parse_dll_name("php_xdebug-3.5.3-8.5-ts-vs17-x86.dll").is_none());
    }

    #[test]
    fn picks_newest_stable_for_this_php() {
        let page = "<a href='files/php_xdebug-3.5.1-8.5-ts-vs17-x86_64.dll'>x</a>\
            <a href='files/php_xdebug-3.5.3-8.5-ts-vs17-x86_64.dll'>x</a>\
            <a href='files/php_xdebug-3.5.3-8.5-nts-vs17-x86_64.dll'>x</a>\
            <a href='files/php_xdebug-3.6.0alpha1-8.5-ts-vs17-x86_64.dll'>x</a>\
            <a href='files/php_xdebug-3.5.3-8.3-ts-vs16-x86_64.dll'>x</a>\
            <a href='files/php_xdebug-3.10.0-8.5-ts-vs17-x86_64.dll'>x</a>";
        let mk = |m: &str, ts: bool, vs: &str| PhpInfo { minor: m.into(), ts, vs: vs.into(), x64: true };
        assert_eq!(newest_for(page, &mk("8.5", true, "vs17")).as_deref(), Some("3.10.0"));
        assert_eq!(newest_for(page, &mk("8.5", false, "vs17")).as_deref(), Some("3.5.3"));
        assert_eq!(newest_for(page, &mk("8.6", true, "vs17")), None);
        assert_eq!(newest_for(page, &mk("8.5", true, "vs16")), None);
    }

    #[test]
    fn ini_round_trip_is_idempotent() {
        let base = "[PHP]\r\nmemory_limit=512M\r\n;zend_extension=opcache\r\nextension=mysqli\r\n";
        let on = edit_ini(base, true);
        assert!(on.contains("\r\nzend_extension=xdebug\r\n"));
        assert!(on.contains("[xdebug]\r\nzend_extension=xdebug\r\nxdebug.mode=debug,develop\r\nxdebug.start_with_request=trigger\r\nxdebug.client_port=9003\r\n"));
        assert!(on.starts_with(base), "other lines stay as they were");
        assert_eq!(edit_ini(&on, true), on);
        let off = edit_ini(&on, false);
        assert!(off.contains(";zend_extension=xdebug\r\n"));
        assert!(!off.lines().any(|l| parse_ext(l) == Some(true)));
        assert_eq!(edit_ini(&off, false), off);
        // Turning it on again reuses the commented line and does not repeat the block.
        let again = edit_ini(&off, true);
        assert_eq!(again, on);
        assert_eq!(again.matches("[xdebug]").count(), 1);
    }

    #[test]
    fn ini_edit_handles_existing_lines() {
        // The old DLL style, active twice: one stays, the other is commented.
        let t = "zend_extension=\"C:\\x\\php_xdebug.dll\"\nzend_extension=xdebug\n;zend_extension=opcache\n";
        let on = edit_ini(t, true);
        assert_eq!(on.lines().filter(|l| parse_ext(l) == Some(true)).count(), 1);
        assert!(on.contains(";zend_extension=opcache\n"));
        let off = edit_ini(t, false);
        assert!(!off.lines().any(|l| parse_ext(l) == Some(true)));
        // Prose that mentions the word is not touched.
        assert_eq!(parse_ext("; use zend_extension = xdebug for debugging"), None);
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Downloads the matching DLL for the installed PHP 8.5 (cargo test -- --ignored). php.ini is not touched.
    #[test]
    #[ignore]
    fn installs_for_local_php() {
        let dir = std::path::PathBuf::from("C:/laragon/bin/php/php-8.5.12-1-Win32-vs17-x64");
        if !dir.join("php.exe").is_file() {
            return;
        }
        let shared = crate::services::Shared::new();
        let v = install(&dir, &shared, crate::i18n::Lang::from_code("en")).expect("install");
        assert!(!v.is_empty());
        assert!(status(&dir).0);
    }
}
