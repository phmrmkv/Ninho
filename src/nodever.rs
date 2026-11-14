//! Several Node.js versions side by side (like nvm or Herd) and one version per project.
//! Versions live in <root>\bin\nodejs\node-v<ver>-win-x64; a project asks for one with `.nvmrc` or `.node-version`.
//! Official site only: nodejs.org/dist (index.json lists the releases, each one has a win-x64 ZIP).
use std::path::{Path, PathBuf};

/// "win-x64", or "win-x86" when Ninho is built as 32-bit (Node.js has 32-bit Windows builds up to version 22).
#[cfg(windows)]
pub const ARCH: &str = if cfg!(target_arch = "x86") { "win-x86" } else { "win-x64" };
/// Linux and macOS (preview): the official builds for this system and processor.
#[cfg(target_os = "linux")]
pub const ARCH: &str = if cfg!(target_arch = "aarch64") { "linux-arm64" } else { "linux-x64" };
#[cfg(target_os = "macos")]
pub const ARCH: &str = if cfg!(target_arch = "aarch64") { "darwin-arm64" } else { "darwin-x64" };

/// The archive nodejs.org has for this system: ZIP on Windows, tar.gz on Linux and macOS.
pub const ARCHIVE: &str = if cfg!(windows) { "zip" } else { "tar.gz" };

/// How index.json names this build in "files": "win-x64-zip", "linux-x64", "osx-arm64-tar".
pub fn index_key() -> String {
    if cfg!(windows) {
        format!("{ARCH}-zip")
    } else if cfg!(target_os = "macos") {
        format!("{}-tar", ARCH.replace("darwin", "osx"))
    } else {
        ARCH.to_string()
    }
}

/// Linux and macOS: the archive keeps node, npm and npx in bin/; links at the top of the version folder make it
/// look like the Windows one (node beside the folder name), so PATH and the checks are the same everywhere.
#[cfg(unix)]
pub fn link_bins(d: &Path) {
    for b in ["node", "npm", "npx", "corepack"] {
        let target = Path::new("bin").join(b);
        if d.join(&target).exists() && !d.join(b).exists() {
            let _ = std::os::unix::fs::symlink(&target, d.join(b));
        }
    }
}
use std::process::Command;
use std::time::Duration;

use crate::install::ver_key;
use crate::services::{Shared, hide_window};
use crate::i18n::tr;
use crate::settings::Settings;

/// How many LTS lines `available()` offers (older ones are end of life).
const LTS_LINES: usize = 4;

fn nodejs_dir(s: &Settings) -> PathBuf {
    Path::new(&s.root).join("bin").join("nodejs")
}

fn major(v: &str) -> u32 {
    ver_key(v).first().copied().unwrap_or(0)
}

/// "26.0.0-rc.1", "25.0.0-nightly2026...", "-alpha", "-beta": not offered, only releases.
pub fn is_prerelease(v: &str) -> bool {
    let low = v.to_ascii_lowercase();
    low.contains('-') || ["alpha", "beta", "rc", "nightly"].iter().any(|w| low.contains(w))
}

// ---------- what exists on nodejs.org ----------

/// Newest release of each LTS line plus the newest "Current" (the first entry), from index.json text:
/// (version without "v", lts codename or "", is_current). Newest first.
fn parse_index(text: &[u8]) -> Result<Vec<(String, String, bool)>, String> {
    let v: serde_json::Value = serde_json::from_slice(text).map_err(|e| format!("index.json: {e}"))?;
    let all = v.as_array().ok_or("index.json is not a list")?;
    // Only releases that have a Windows x64 ZIP (very old ones and some odd builds do not).
    let mut rel: Vec<(String, String)> = all
        .iter()
        .filter(|r| r["files"].as_array().is_some_and(|f| f.iter().any(|x| x.as_str() == Some(index_key().as_str()))))
        .filter_map(|r| {
            let ver = r["version"].as_str()?.trim_start_matches('v').to_string();
            if is_prerelease(&ver) {
                return None;
            }
            Some((ver, r["lts"].as_str().unwrap_or("").to_string()))
        })
        .collect();
    // The site lists newest first, but do not depend on it.
    rel.sort_by_key(|(v, _)| std::cmp::Reverse(ver_key(v)));
    let newest = rel.first().ok_or("no Windows x64 release in index.json")?.0.clone();
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<(String, String, bool)> = Vec::new();
    for (ver, lts) in &rel {
        let is_cur = *ver == newest;
        if lts.is_empty() {
            // The newest Current is only worth offering while no LTS line has caught up with it.
            if is_cur {
                out.push((ver.clone(), String::new(), true));
            }
        } else if !seen.contains(lts) && seen.len() < LTS_LINES {
            seen.push(lts.clone());
            out.push((ver.clone(), lts.clone(), is_cur));
        }
    }
    Ok(out)
}

/// Newest release of each LTS line plus the newest "Current", from https://nodejs.org/dist/index.json:
/// (version like "24.11.0", lts codename or "", is_current).
pub fn available() -> Result<Vec<(String, String, bool)>, String> {
    let url = "https://nodejs.org/dist/index.json";
    let mut c = Command::new(crate::platform::curl());
    c.args(["-L", "-f", "-s", "-S", "-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Ninho", url]);
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("{url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    parse_index(&out.stdout)
}

// ---------- what is installed ----------

/// "node-v26.13.1-win-x64" → "26.13.1".
fn folder_version(name: &str) -> Option<&str> {
    let v = name.strip_prefix("node-v")?.strip_suffix(&format!("-{ARCH}"))?;
    (ver_key(v).len() >= 2).then_some(v)
}

/// Installed versions ("26.13.1"), newest first.
pub fn installed(s: &Settings) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(nodejs_dir(s))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().join(crate::platform::exe("node")).is_file())
                .filter_map(|e| folder_version(&e.file_name().to_string_lossy()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|s| std::cmp::Reverse(ver_key(s)));
    v
}

/// Folder of an installed version, to put first on a project's PATH.
pub fn dir_of(s: &Settings, version: &str) -> Option<PathBuf> {
    let d = nodejs_dir(s).join(format!("node-v{}-{ARCH}", version.trim_start_matches('v')));
    d.join(crate::platform::exe("node")).is_file().then_some(d)
}

// ---------- install ----------

/// Download and unpack one version (zip node-v<ver>-win-x64.zip, its top folder is the version folder).
/// Progress shows in `shared.installing` under the id "node-<ver>"; the caller refreshes the screen afterwards.
pub fn install(s: &Settings, shared: &Shared, version: &str) -> Result<PathBuf, String> {
    let version = version.trim().trim_start_matches('v');
    // The version goes into a URL and a path: digits and dots only.
    if version.is_empty() || ver_key(version).len() != 3 || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(format!("{} {version}", tr(s.language, "log_not_node_version")));
    }
    if let Some(d) = dir_of(s, version) {
        return Ok(d);
    }
    let id = format!("node-{version}");
    shared.installing.lock().unwrap().insert(id.clone(), "…".into());
    let r = install_inner(s, shared, version, &id);
    shared.installing.lock().unwrap().remove(&id);
    r
}

fn install_inner(s: &Settings, shared: &Shared, version: &str, id: &str) -> Result<PathBuf, String> {
    let lang = s.language;
    let name = format!("node-v{version}-{ARCH}");
    let url = format!("https://nodejs.org/dist/v{version}/{name}.{ARCHIVE}");
    let tmp = Path::new(&s.root).join("tmp").join("ninho-downloads");
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let zip = tmp.join(format!("{name}.{ARCHIVE}"));
    shared.say(format!("Node.js {version}: {} {url}", tr(lang, "log_downloading")));

    let mut c = Command::new(crate::platform::curl());
    c.args(["-L", "-f", "-s", "-S", "--retry", "2", "-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Ninho"]);
    c.arg("-o").arg(&zip).arg(&url);
    hide_window(&mut c);
    let mut child = c.spawn().map_err(|e| e.to_string())?;
    loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            if st.success() {
                break;
            }
            let _ = std::fs::remove_file(&zip);
            return Err(format!("{} {url}", tr(lang, "log_download_failed")));
        }
        let mb = std::fs::metadata(&zip).map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
        shared.installing.lock().unwrap().insert(id.into(), format!("{mb:.0} MB"));
        std::thread::sleep(Duration::from_millis(700));
    }

    shared.installing.lock().unwrap().insert(id.into(), "unpacking".into());
    let kind = nodejs_dir(s);
    std::fs::create_dir_all(&kind).map_err(|e| e.to_string())?;
    let mut c = Command::new(crate::platform::tar());
    c.arg("-xf").arg(&zip).arg("-C").arg(&kind);
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&zip);
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let d = kind.join(&name);
    #[cfg(unix)]
    link_bins(&d);
    if !d.join(crate::platform::exe("node")).is_file() {
        return Err(format!("{} {name}", tr(lang, "log_node_exe_missing")));
    }
    shared.say(format!("Node.js {version}: {} {}", tr(lang, "log_installed_in"), d.display()));
    Ok(d)
}

// ---------- what a project asks for ----------

/// Normalize the text of an .nvmrc: "v20" → "20", "LTS/Iron" → "lts/iron", "stable"/"latest"/"current" → "node".
fn normalize(raw: &str) -> Option<String> {
    let line = raw.lines().map(|l| l.split('#').next().unwrap_or("").trim()).find(|l| !l.is_empty())?;
    let w = line.to_ascii_lowercase();
    let w = w.trim_start_matches('v');
    Some(match w {
        "node" | "stable" | "latest" | "current" => "node".to_string(),
        _ => w.to_string(),
    })
}

/// The version a project asks for: `.nvmrc` or `.node-version` in its folder ("v20", "20.11.1", "lts/iron", "lts/*",
/// "node"), normalized ("20", "20.11.1", "lts/iron", "lts/*", "node").
pub fn wanted(dir: &Path) -> Option<String> {
    [".nvmrc", ".node-version"].iter().find_map(|f| normalize(&std::fs::read_to_string(dir.join(f)).ok()?))
}

/// Pick from `installed` (any order) what `wanted` means. `lts` lists (codename in lower case, major) of the LTS lines
/// and is only called for "lts/..." (it may hit the network).
fn pick(installed: &[String], wanted: &str, lts: impl FnOnce() -> Vec<(String, u32)>) -> Option<String> {
    let newest = |mut v: Vec<&String>| -> Option<String> {
        v.sort_by_key(|s| std::cmp::Reverse(ver_key(s)));
        v.first().map(|s| s.to_string())
    };
    let w = wanted.trim().to_ascii_lowercase();
    let w = w.trim_start_matches('v');
    if w == "node" {
        return newest(installed.iter().collect());
    }
    if let Some(name) = w.strip_prefix("lts/") {
        let lines = lts();
        let majors: Vec<u32> = if name == "*" {
            lines.iter().map(|l| l.1).collect()
        } else {
            lines.iter().filter(|l| l.0 == name).map(|l| l.1).collect()
        };
        return newest(installed.iter().filter(|v| majors.contains(&major(v))).collect());
    }
    // "20", "20.11" or "20.11.1": a prefix of the numbers, newest installed that fits.
    let want = ver_key(w);
    if want.is_empty() {
        return None;
    }
    newest(installed.iter().filter(|v| ver_key(v).starts_with(&want)).collect())
}

/// The installed version that satisfies `wanted` (a normalized value from `wanted()`), if any. Only "lts/..." goes to the
/// network, and only when something is installed to match.
pub fn resolve(s: &Settings, wanted: &str) -> Option<String> {
    let inst = installed(s);
    if inst.is_empty() {
        return None;
    }
    pick(&inst, wanted, || {
        available().unwrap_or_default().into_iter().filter(|a| !a.1.is_empty()).map(|a| (a.1.to_ascii_lowercase(), major(&a.0))).collect()
    })
}

/// Write `.nvmrc` with the version (e.g. "24.11.0"). A `.node-version` already there is left alone, but `wanted()` reads
/// `.nvmrc` first, so the new value wins.
pub fn set_for_project(dir: &Path, version: &str) -> std::io::Result<()> {
    std::fs::write(dir.join(".nvmrc"), format!("{}\n", version.trim().trim_start_matches('v')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_releases_are_not_offered() {
        for v in ["26.0.0-rc.1", "26.0.0-beta.2", "25.0.0-alpha.3", "26.1.0-nightly20261101"] {
            assert!(is_prerelease(v), "{v}");
        }
        for v in ["26.13.1", "24.11.0"] {
            assert!(!is_prerelease(v), "{v}");
        }
    }

    const INDEX: &str = r#"[
      {"version":"v25.2.0","date":"2026-11-01","files":["win-x64-zip","linux-x64"],"lts":false},
      {"version":"v25.1.0","date":"2026-10-20","files":["win-x64-zip"],"lts":false},
      {"version":"v24.11.1","date":"2026-11-02","files":["win-x64-zip"],"lts":"Krypton"},
      {"version":"v24.11.0","date":"2026-10-28","files":["win-x64-zip"],"lts":"Krypton"},
      {"version":"v22.21.1","date":"2026-10-30","files":["win-x64-zip"],"lts":"Jod"},
      {"version":"v22.20.0","date":"2026-09-01","files":["win-x64-zip"],"lts":"Jod"},
      {"version":"v20.19.5","date":"2026-09-02","files":["win-x64-zip"],"lts":"Iron"},
      {"version":"v18.20.8","date":"2026-03-01","files":["linux-x64"],"lts":"Hydrogen"},
      {"version":"v18.20.7","date":"2026-02-01","files":["win-x64-zip"],"lts":"Hydrogen"}
    ]"#;

    /// The fixtures are written with the Windows names; on Linux and macOS this system's build takes their place
    /// (and the "other system" build stays another one).
    fn here(index: &str) -> String {
        index.replace("linux-x64", "other-x64").replace("win-x64-zip", &index_key())
    }

    #[test]
    fn index_gives_newest_per_line_and_current() {
        let a = parse_index(here(INDEX).as_bytes()).unwrap();
        assert_eq!(
            a,
            vec![
                ("25.2.0".to_string(), String::new(), true),
                ("24.11.1".to_string(), "Krypton".to_string(), false),
                ("22.21.1".to_string(), "Jod".to_string(), false),
                ("20.19.5".to_string(), "Iron".to_string(), false),
                ("18.20.7".to_string(), "Hydrogen".to_string(), false),
            ]
        );
    }

    #[test]
    fn index_skips_releases_without_a_windows_zip_and_marks_lts_that_is_newest() {
        let a = parse_index(here(r#"[{"version":"v26.0.0","files":["win-x64-zip"],"lts":"Next"},{"version":"v25.9.0","files":["linux-x64"],"lts":false}]"#).as_bytes()).unwrap();
        assert_eq!(a, vec![("26.0.0".to_string(), "Next".to_string(), true)]);
        assert!(parse_index(b"[]").is_err());
        assert!(parse_index(b"nope").is_err());
    }

    #[test]
    fn nvmrc_values_are_normalized() {
        assert_eq!(normalize("v20\n").as_deref(), Some("20"));
        assert_eq!(normalize("20.11.1\r\n").as_deref(), Some("20.11.1"));
        assert_eq!(normalize("  \n# pinned\nlts/Iron # comment\n").as_deref(), Some("lts/iron"));
        assert_eq!(normalize("lts/*").as_deref(), Some("lts/*"));
        assert_eq!(normalize("stable").as_deref(), Some("node"));
        assert_eq!(normalize("node").as_deref(), Some("node"));
        assert_eq!(normalize("\n  \n"), None);
    }

    fn inst() -> Vec<String> {
        ["26.13.1", "24.11.0", "24.2.0", "22.21.1", "20.11.1"].map(String::from).to_vec()
    }
    fn lts() -> Vec<(String, u32)> {
        vec![("krypton".into(), 24), ("jod".into(), 22), ("iron".into(), 20)]
    }

    #[test]
    fn resolve_matches_prefixes_and_newest() {
        let none = || -> Vec<(String, u32)> { panic!("no network needed") };
        assert_eq!(pick(&inst(), "24", none).as_deref(), Some("24.11.0"));
        assert_eq!(pick(&inst(), "24.2", none).as_deref(), Some("24.2.0"));
        assert_eq!(pick(&inst(), "20.11.1", none).as_deref(), Some("20.11.1"));
        assert_eq!(pick(&inst(), "v22", none).as_deref(), Some("22.21.1"));
        assert_eq!(pick(&inst(), "20.12", none), None);
        assert_eq!(pick(&inst(), "18", none), None);
        assert_eq!(pick(&inst(), "node", none).as_deref(), Some("26.13.1"));
        assert_eq!(pick(&inst(), "abc", none), None);
        assert_eq!(pick(&[], "node", none), None);
    }

    #[test]
    fn resolve_lts_lines() {
        assert_eq!(pick(&inst(), "lts/iron", lts).as_deref(), Some("20.11.1"));
        assert_eq!(pick(&inst(), "lts/krypton", lts).as_deref(), Some("24.11.0"));
        assert_eq!(pick(&inst(), "lts/hydrogen", lts), None);
        // 26 is not an LTS line, so the newest LTS one wins.
        assert_eq!(pick(&inst(), "lts/*", lts).as_deref(), Some("24.11.0"));
        // Offline: no LTS data, no answer.
        assert_eq!(pick(&inst(), "lts/*", Vec::new), None);
    }

    #[test]
    fn folder_names_and_project_file() {
        assert_eq!(folder_version(&format!("node-v26.13.1-{ARCH}")), Some("26.13.1"));
        assert_eq!(folder_version(&format!("node-v26.13.1-{ARCH}.zip")), None);
        assert_eq!(folder_version("npm"), None);
        let d = std::env::temp_dir().join(format!("nodever-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        set_for_project(&d, "v24.11.0").unwrap();
        assert_eq!(std::fs::read_to_string(d.join(".nvmrc")).unwrap(), "24.11.0\n");
        assert_eq!(wanted(&d).as_deref(), Some("24.11.0"));
        std::fs::write(d.join(".nvmrc"), "").unwrap();
        std::fs::write(d.join(".node-version"), "v20\n").unwrap();
        assert_eq!(wanted(&d).as_deref(), Some("20"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
