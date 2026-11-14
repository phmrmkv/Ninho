//! "Share on the internet": a Cloudflare quick tunnel (no account) that gives a local site a public
//! https://<random>.trycloudflare.com address while it runs. cloudflared.exe lives in <root>\bin\cloudflared.
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::services::{Shared, hide_window};
use crate::i18n::tr;
use crate::settings::Settings;

#[cfg(windows)]
const DOWNLOAD: &str = if cfg!(target_arch = "x86") {
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-386.exe"
} else {
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe"
};
/// Linux: a single program, like on Windows.
#[cfg(target_os = "linux")]
const DOWNLOAD: &str = if cfg!(target_arch = "aarch64") {
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-arm64"
} else {
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64"
};
/// macOS: a .tgz with the program inside, unpacked after the download.
#[cfg(target_os = "macos")]
const DOWNLOAD: &str = if cfg!(target_arch = "aarch64") {
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-arm64.tgz"
} else {
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-amd64.tgz"
};
/// How many output lines are kept for error messages.
const KEEP_LINES: usize = 20;

fn exe_path(s: &Settings) -> PathBuf {
    Path::new(&s.root).join("bin").join("cloudflared").join(crate::platform::exe("cloudflared"))
}

/// The system's curl (Windows' own on Windows 10+, same choice as install.rs: it handles TLS and redirects).
fn curl() -> PathBuf {
    crate::platform::curl()
}

/// True when the exe starts and answers --version (a half-downloaded or blocked file does not).
fn runs(exe: &Path) -> bool {
    let mut c = Command::new(exe);
    c.arg("--version").stdin(Stdio::null());
    hide_window(&mut c);
    c.output().map(|o| o.status.success()).unwrap_or(false)
}

/// Path of cloudflared.exe in <root>\bin\cloudflared, if installed.
pub fn installed(s: &Settings) -> Option<PathBuf> {
    let p = exe_path(s);
    p.is_file().then_some(p)
}

/// Download the newest cloudflared for Windows from the official GitHub release. An exe that already runs is kept.
pub fn install(s: &Settings, shared: &Shared) -> Result<PathBuf, String> {
    let lang = s.language;
    let exe = exe_path(s);
    if exe.is_file() && runs(&exe) {
        shared.say(format!("cloudflared: {}", tr(lang, "log_already_installed")));
        return Ok(exe);
    }
    let dir = exe.parent().ok_or("bad cloudflared folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Download beside the final name and rename after, so a failed download never leaves a broken exe in place.
    let part = dir.join("cloudflared.exe.part");
    let _ = std::fs::remove_file(&part);
    shared.say(format!("cloudflared: {} {DOWNLOAD}", tr(lang, "log_downloading")));
    let mut c = Command::new(curl());
    c.args(["-L", "-f", "-s", "-S", "--retry", "2", "-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Ninho"]);
    c.arg("-o").arg(&part).arg(DOWNLOAD).stdin(Stdio::null());
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{} {}", tr(lang, "log_download_failed"), String::from_utf8_lossy(&out.stderr).trim()));
    }
    // The real file is tens of MB; a tiny one is an error page.
    if std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) < 1_000_000 {
        let _ = std::fs::remove_file(&part);
        return Err(tr(lang, "log_cf_too_small").into());
    }
    #[cfg(target_os = "macos")]
    {
        // The .tgz holds the program: unpack it beside the download, then the rename below puts it in place.
        let tgz = dir.join("cloudflared.tgz");
        std::fs::rename(&part, &tgz).map_err(|e| e.to_string())?;
        let out = Command::new(crate::platform::tar()).arg("-xzf").arg(&tgz).arg("-C").arg(dir).output().map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&tgz);
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        std::fs::rename(dir.join("cloudflared"), &part).map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::rename(&part, &exe).map_err(|e| format!("{} ({e})", tr(lang, "log_cf_place_failed")))?;
    if !runs(&exe) {
        return Err(tr(lang, "log_cf_not_running").into());
    }
    shared.say(format!("cloudflared: {} {}", tr(lang, "log_installed_in"), exe.display()));
    Ok(exe)
}

/// The https://<name>.trycloudflare.com address in a log line, if any. Ignores api.trycloudflare.com (cloudflared's
/// own endpoint, which shows up in error lines).
pub fn extract_url(line: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = line[from..].find("https://") {
        let start = from + i;
        let rest = &line[start..];
        let end = rest.find(|c: char| c.is_whitespace() || matches!(c, '|' | '"' | '\'' | '<' | '>' | ',')).unwrap_or(rest.len());
        let cand = rest[..end].trim_end_matches(['.', '/']);
        if let Some(host) = cand.strip_prefix("https://") {
            if host.ends_with(".trycloudflare.com") && host != "api.trycloudflare.com" && host.len() > ".trycloudflare.com".len() {
                return Some(cand.to_string());
            }
        }
        from = start + "https://".len();
    }
    None
}

/// Read a process pipe line by line (lossy UTF-8) until it closes.
fn pump<R: Read + Send + 'static>(r: R, url: Arc<Mutex<Option<String>>>, lines: Arc<Mutex<Vec<String>>>) {
    std::thread::spawn(move || {
        for raw in BufReader::new(r).split(b'\n') {
            let Ok(raw) = raw else { break };
            let line = String::from_utf8_lossy(&raw).trim_end_matches('\r').to_string();
            if line.trim().is_empty() {
                continue;
            }
            if let Some(u) = extract_url(&line) {
                let mut g = url.lock().unwrap();
                if g.is_none() {
                    *g = Some(u);
                }
            }
            let mut l = lines.lock().unwrap();
            l.push(line);
            if l.len() > KEEP_LINES {
                let extra = l.len() - KEEP_LINES;
                l.drain(..extra);
            }
        }
    });
}

/// A running quick tunnel.
pub struct Tunnel {
    child: Child,
    url: Arc<Mutex<Option<String>>>,
    lines: Arc<Mutex<Vec<String>>>,
}

/// Start a tunnel to 127.0.0.1:<local_port>. For a .test site pass its host name as `host_header` and Apache's port
/// as `local_port`, so Apache picks the right virtual host. The public URL shows up a few seconds later.
pub fn start(s: &Settings, local_port: u16, host_header: Option<&str>) -> Result<Tunnel, String> {
    let exe = installed(s).ok_or_else(|| tr(s.language, "log_cf_not_installed"))?;
    let mut c = Command::new(&exe);
    c.args(["tunnel", "--url", &format!("http://127.0.0.1:{local_port}"), "--no-autoupdate"]);
    if let Some(h) = host_header.filter(|h| !h.is_empty()) {
        c.args(["--http-host-header", h]);
    }
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_window(&mut c);
    let mut child = c.spawn().map_err(|e| format!("{} {e}", tr(s.language, "log_cf_start_failed")))?;
    let url = Arc::new(Mutex::new(None));
    let lines = Arc::new(Mutex::new(Vec::new()));
    // cloudflared logs to stderr; stdout is read too so a full pipe can never block it.
    if let Some(o) = child.stdout.take() {
        pump(o, url.clone(), lines.clone());
    }
    if let Some(e) = child.stderr.take() {
        pump(e, url.clone(), lines.clone());
    }
    Ok(Tunnel { child, url, lines })
}

impl Tunnel {
    /// The https://….trycloudflare.com address once known.
    pub fn url(&self) -> Option<String> {
        self.url.lock().unwrap().clone()
    }

    /// Process still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Kill the process and wait for it (safe to call again).
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// The last ~20 output lines, for error messages.
    pub fn last_lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_url_in_boxed_log_line() {
        let l = "2026-11-13T23:50:01Z INF |  https://quiet-river-1234.trycloudflare.com   |";
        assert_eq!(extract_url(l).as_deref(), Some("https://quiet-river-1234.trycloudflare.com"));
    }

    #[test]
    fn finds_url_at_end_of_line_and_with_trailing_dot() {
        assert_eq!(extract_url("see https://a-b-c.trycloudflare.com").as_deref(), Some("https://a-b-c.trycloudflare.com"));
        assert_eq!(extract_url("at https://a-b-c.trycloudflare.com.").as_deref(), Some("https://a-b-c.trycloudflare.com"));
    }

    #[test]
    fn ignores_other_urls_and_api_endpoint() {
        assert_eq!(extract_url("INF Requesting new quick Tunnel on trycloudflare.com..."), None);
        assert_eq!(extract_url("ERR Post \"https://api.trycloudflare.com/tunnel\": EOF"), None);
        assert_eq!(extract_url("see https://developers.cloudflare.com/docs"), None);
        assert_eq!(extract_url("https://.trycloudflare.com"), None);
    }

    #[test]
    fn skips_api_then_finds_real_url() {
        let l = "https://api.trycloudflare.com/x and https://odd-fox-77.trycloudflare.com ok";
        assert_eq!(extract_url(l).as_deref(), Some("https://odd-fox-77.trycloudflare.com"));
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Installs cloudflared and opens a quick tunnel to a throwaway page for a few seconds (cargo test -- --ignored).
    #[test]
    #[ignore]
    fn opens_a_public_link() {
        let mut s = crate::settings::Settings::default();
        s.root = "C:/laragon".into();
        let shared = crate::services::Shared::new();
        install(&s, &shared).expect("install");
        let dir = std::env::temp_dir().join("ninho-share-test");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("index.php"), "ok").unwrap();
        let mut php = std::process::Command::new("C:/laragon/bin/php/php-8.5.12-1-Win32-vs17-x64/php.exe")
            .args(["-S", "127.0.0.1:8199", "-t"]).arg(&dir).spawn().expect("php");
        let mut t = start(&s, 8199, None).expect("start");
        let mut url = None;
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            url = t.url();
            if url.is_some() || !t.alive() { break; }
        }
        t.stop();
        let _ = php.kill();
        assert!(url.as_deref().is_some_and(|u| u.contains(".trycloudflare.com")), "{:?} {:?}", url, t.last_lines());
        eprintln!("link: {}", url.unwrap());
    }
}
