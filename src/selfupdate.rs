//! Self-update from the GitHub releases of paulohmaciel/Ninho.
//! Flow: check() -> download_and_swap() -> restart(). A running exe can be renamed on Windows but not
//! overwritten, so the new build is swapped in by renames and the old file is deleted on the next start.

use crate::services::hide_window;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const API: &str = "https://api.github.com/repos/paulohmaciel/Ninho/releases/latest";
const AGENT: &str = "Ninho-updater";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// "0.6.0" (no leading v).
    pub version: String,
    pub notes: String,
    pub exe_url: String,
    pub sums_url: Option<String>,
    /// The release page on GitHub (fallback for a manual download).
    pub page: String,
}

fn win(tool: &str) -> PathBuf {
    // Windows' own curl, same as install.rs (no TLS code of our own).
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    Path::new(&sys).join("System32").join(tool)
}

fn asset_name() -> &'static str {
    if cfg!(target_arch = "x86") { "Ninho-x86.exe" } else { "Ninho-x64.exe" }
}

/// Numbers of "v0.6.0" in order; padded with zeros when compared, so 0.6 == 0.6.0.
fn ver_key(v: &str) -> Vec<u32> {
    v.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
}

fn is_newer(candidate: &str, current: &str) -> bool {
    let (mut a, mut b) = (ver_key(candidate), ver_key(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// GET with curl; returns (HTTP status, body). Network failures become readable messages.
fn http_get(url: &str, api: bool) -> Result<(u16, Vec<u8>), String> {
    let mut c = Command::new(win("curl.exe"));
    c.args(["-L", "-s", "-S", "--max-time", "30", "-A", AGENT]);
    if api {
        c.args(["-H", "Accept: application/vnd.github+json"]);
    }
    // The status goes after the body on its own line, because -f would hide GitHub's error text.
    c.args(["-w", "\n%{http_code}"]).arg(url);
    hide_window(&mut c);
    let out = c.output().map_err(|e| format!("Could not run curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Could not reach GitHub (check the internet connection): {}", err.trim()));
    }
    let mut body = out.stdout;
    let cut = body.iter().rposition(|&b| b == b'\n').ok_or("Empty answer from GitHub")?;
    let code = String::from_utf8_lossy(&body[cut + 1..]).trim().parse::<u16>().unwrap_or(0);
    body.truncate(cut);
    Ok((code, body))
}

fn status_error(code: u16) -> String {
    match code {
        403 | 429 => "GitHub's request limit was reached; try again in a while.".into(),
        c => format!("GitHub answered HTTP {c}."),
    }
}

/// Reads the "latest release" JSON. Ok(None) for drafts and prereleases; Err when `asset` is missing.
fn parse_release(v: &serde_json::Value, asset: &str) -> Result<Option<Release>, String> {
    if v["draft"].as_bool().unwrap_or(false) || v["prerelease"].as_bool().unwrap_or(false) {
        return Ok(None);
    }
    let tag = v["tag_name"].as_str().ok_or("Unexpected answer from GitHub (no tag_name)")?;
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let url_of = |name: &str| {
        assets.iter().find(|a| a["name"].as_str() == Some(name)).and_then(|a| a["browser_download_url"].as_str()).map(String::from)
    };
    let Some(exe_url) = url_of(asset) else {
        return Err(format!("Release {tag} has no {asset} file yet; try again later."));
    };
    Ok(Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v["body"].as_str().unwrap_or("").trim().to_string(),
        exe_url,
        sums_url: url_of("SHA256SUMS.txt"),
        page: v["html_url"].as_str().unwrap_or("https://github.com/paulohmaciel/Ninho/releases").to_string(),
    }))
}

/// The newest published release, if it is newer than this build.
/// A 404 (the repository has no release yet) is "nothing newer": Ok(None), not an error, because for the user
/// it means the same thing and a red error on every start would be noise.
pub fn check() -> Result<Option<Release>, String> {
    let (code, body) = http_get(API, true)?;
    match code {
        200 => {}
        404 => return Ok(None),
        c => return Err(status_error(c)),
    }
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|e| format!("Unexpected answer from GitHub: {e}"))?;
    Ok(parse_release(&v, asset_name())?.filter(|r| is_newer(&r.version, env!("CARGO_PKG_VERSION"))))
}

/// `<hex>  <name>` lines (sha256sum format; a `*` before the name marks binary mode).
fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hex, rest) = l.trim().split_once(char::is_whitespace)?;
        (rest.trim_start().trim_start_matches('*') == name && hex.len() == 64).then(|| hex.to_ascii_lowercase())
    })
}

/// "<dir>\Ninho.exe" + ".new" -> "<dir>\Ninho.exe.new" (keeps the real name, whatever the user renamed it to).
fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut s = exe.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Downloads the new exe beside the running one, verifies it and swaps it in. Returns the new exe's path.
/// `progress` receives the bytes downloaded so far (about every half second).
pub fn download_and_swap(r: &Release, progress: &dyn Fn(u64)) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not find the running exe: {e}"))?;
    let (new, old) = (sibling(&exe, ".new"), sibling(&exe, ".old"));
    let manual = format!("Download it from {} and replace the file by hand.", r.page);

    // Fetch the checksum list first: a missing or broken one should stop us before the big download.
    // Without it nothing can be checked, so nothing is installed.
    let u = r.sums_url.as_ref().ok_or_else(|| format!("This release has no SHA256SUMS.txt, so it cannot be checked. {manual}"))?;
    let expected = {
        let (code, body) = http_get(u, false)?;
        if code != 200 {
            return Err(format!("Could not read SHA256SUMS.txt: {}", status_error(code)));
        }
        let name = r.exe_url.rsplit('/').next().unwrap_or(asset_name());
        Some(sum_for(&String::from_utf8_lossy(&body), name).ok_or_else(|| format!("{name} is not listed in SHA256SUMS.txt"))?)
    };

    // A folder we cannot write to (Program Files) fails here, before any download.
    let _ = std::fs::remove_file(&new);
    if let Err(e) = std::fs::File::create(&new) {
        let dir = exe.parent().map(|p| p.display().to_string()).unwrap_or_default();
        return Err(format!("Ninho cannot write to its own folder ({dir}): {e}. {manual}"));
    }
    if let Err(e) = fetch_and_verify(r, &new, expected.as_deref(), progress) {
        let _ = std::fs::remove_file(&new);
        return Err(e);
    }

    // A leftover .old from an earlier update may still be locked if that copy is running; then we cannot swap.
    if old.exists() && std::fs::remove_file(&old).is_err() {
        let _ = std::fs::remove_file(&new);
        return Err("An older Ninho is still running from a previous update; close it and try again.".into());
    }
    if let Err(e) = std::fs::rename(&exe, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(format!("Could not move the running exe aside: {e}. {manual}"));
    }
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe); // put the working copy back
        let _ = std::fs::remove_file(&new);
        return Err(format!("Could not put the new exe in place: {e}. {manual}"));
    }
    Ok(exe)
}

fn fetch_and_verify(r: &Release, to: &Path, expected: Option<&str>, progress: &dyn Fn(u64)) -> Result<(), String> {
    let mut c = Command::new(win("curl.exe"));
    c.args(["-L", "-f", "-s", "-S", "--retry", "2", "-A", AGENT]).arg("-o").arg(to).arg(&r.exe_url);
    hide_window(&mut c);
    let mut child = c.spawn().map_err(|e| format!("Could not run curl: {e}"))?;
    // curl writes straight to the file, so its size is the progress.
    loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            if !st.success() {
                return Err(format!("Download failed ({}); check the internet connection.", r.exe_url));
            }
            break;
        }
        progress(std::fs::metadata(to).map(|m| m.len()).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(500));
    }
    let size = std::fs::metadata(to).map(|m| m.len()).unwrap_or(0);
    progress(size);
    if size < 1_048_576 {
        return Err("The downloaded file is too small to be Ninho; the download was discarded.".into());
    }
    let mut head = [0u8; 2];
    std::io::Read::read_exact(&mut std::fs::File::open(to).map_err(|e| e.to_string())?, &mut head).map_err(|e| e.to_string())?;
    if &head != b"MZ" {
        return Err("The downloaded file is not a Windows program; the download was discarded.".into());
    }
    if let Some(want) = expected
        && sha256_file(to)? != want
    {
        return Err("The downloaded file does not match its SHA-256 checksum; it was discarded.".into());
    }
    Ok(())
}

/// Starts the new exe a couple of seconds from now, so this process has closed (and released any single-instance
/// lock) by then. The caller must exit right after this returns Ok.
/// `start_again`: ids of the servers that were running, started again by the new copy (it waits for this one to close).
pub fn restart(new_exe: &Path, start_again: &[String]) -> Result<(), String> {
    let mut c = Command::new("cmd");
    c.env("NINHO_START_AFTER_UPDATE", start_again.join(","));
    let line = format!("ping -n 2 127.0.0.1 >nul & start \"\" \"{}\" --after-update", new_exe.display());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // raw_arg: cmd does not understand Rust's \" quoting (same as selfinstall.rs).
        c.raw_arg(format!("/c \"{line}\""));
    }
    #[cfg(not(windows))]
    c.args(["/c", &line]);
    if let Some(dir) = new_exe.parent() {
        c.current_dir(dir);
    }
    hide_window(&mut c);
    c.spawn().map(|_| ()).map_err(|e| format!("Could not start the new version: {e}"))
}

/// Deletes the Ninho.exe.old left by the last update. Call at every start; errors are ignored
/// (the old copy may still be closing, and the next start tries again).
pub fn cleanup_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(sibling(&exe, ".old"));
    }
}

// ---- SHA-256 (FIPS 180-4), so the update needs no crate and no PowerShell/certutil ----

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
    0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
    0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
    0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
    0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

struct Sha256 {
    h: [u32; 8],
    buf: Vec<u8>,
    len: u64,
}

impl Sha256 {
    fn new() -> Self {
        Sha256 {
            h: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19],
            buf: Vec::with_capacity(64),
            len: 0,
        }
    }

    fn block(&mut self, b: &[u8]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b_, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b_) ^ (a & c) ^ (b_ & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b_;
            b_ = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in self.h.iter_mut().zip([a, b_, c, d, e, f, g, h]) {
            *x = x.wrapping_add(y);
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.len += data.len() as u64;
        if !self.buf.is_empty() {
            let take = (64 - self.buf.len()).min(data.len());
            self.buf.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.buf.len() == 64 {
                let b = std::mem::take(&mut self.buf);
                self.block(&b);
            }
        }
        while data.len() >= 64 {
            self.block(&data[..64]);
            data = &data[64..];
        }
        self.buf.extend_from_slice(data);
    }

    fn finish(mut self) -> String {
        // 0x80, zeros up to 56 mod 64, then the bit length.
        let bits = self.len * 8;
        let mut pad = vec![0x80u8];
        pad.resize(1 + (119 - self.buf.len() % 64) % 64, 0);
        pad.extend_from_slice(&bits.to_be_bytes());
        self.update(&pad);
        self.h.iter().map(|x| format!("{x:08x}")).collect()
    }
}

#[cfg(test)]
fn sha256_hex(data: &[u8]) -> String {
    let mut s = Sha256::new();
    s.update(data);
    s.finish()
}

fn sha256_file(p: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let (mut s, mut chunk) = (Sha256::new(), vec![0u8; 256 * 1024]);
    loop {
        let n = f.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(s.finish());
        }
        s.update(&chunk[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r###"{
      "url": "https://api.github.com/repos/paulohmaciel/Ninho/releases/123456",
      "html_url": "https://github.com/paulohmaciel/Ninho/releases/tag/v0.6.0",
      "tag_name": "v0.6.0",
      "name": "v0.6.0",
      "draft": false,
      "prerelease": false,
      "body": "## What's Changed\n* Self-update by @paulohmaciel\n\n**Full Changelog**: https://github.com/paulohmaciel/Ninho/compare/v0.5.0...v0.6.0\n",
      "assets": [
        {"name": "Ninho-x64.exe", "size": 9123456, "browser_download_url": "https://github.com/paulohmaciel/Ninho/releases/download/v0.6.0/Ninho-x64.exe"},
        {"name": "Ninho-x86.exe", "size": 8123456, "browser_download_url": "https://github.com/paulohmaciel/Ninho/releases/download/v0.6.0/Ninho-x86.exe"},
        {"name": "SHA256SUMS.txt", "size": 160, "browser_download_url": "https://github.com/paulohmaciel/Ninho/releases/download/v0.6.0/SHA256SUMS.txt"}
      ]
    }"###;

    #[test]
    fn sha256_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // The classic million-'a' vector (about 1 MB), fed in odd-sized pieces to exercise the buffering.
        let mut s = Sha256::new();
        let a = vec![b'a'; 1_000_000];
        for piece in a.chunks(777) {
            s.update(piece);
        }
        assert_eq!(s.finish(), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
        // Lengths around the padding boundaries must agree between one-shot and byte-by-byte feeding.
        for n in [55usize, 56, 57, 63, 64, 65, 119, 120] {
            let d = vec![7u8; n];
            let mut s = Sha256::new();
            d.iter().for_each(|b| s.update(&[*b]));
            assert_eq!(s.finish(), sha256_hex(&d), "length {n}");
        }
    }

    #[test]
    fn hashes_a_file() {
        let p = std::env::temp_dir().join("ninho-selfupdate-test.bin");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn parses_the_api_answer() {
        let v: serde_json::Value = serde_json::from_str(SAMPLE).unwrap();
        let r = parse_release(&v, "Ninho-x64.exe").unwrap().unwrap();
        assert_eq!(r.version, "0.6.0");
        assert!(r.exe_url.ends_with("/v0.6.0/Ninho-x64.exe"));
        assert!(r.sums_url.unwrap().ends_with("SHA256SUMS.txt"));
        assert!(r.notes.starts_with("## What's Changed"));
        assert_eq!(r.page, "https://github.com/paulohmaciel/Ninho/releases/tag/v0.6.0");
        let r86 = parse_release(&v, "Ninho-x86.exe").unwrap().unwrap();
        assert!(r86.exe_url.ends_with("Ninho-x86.exe"));
    }

    #[test]
    fn skips_drafts_prereleases_and_reports_missing_assets() {
        let mut v: serde_json::Value = serde_json::from_str(SAMPLE).unwrap();
        assert!(parse_release(&v, "Ninho-arm64.exe").is_err());
        v["prerelease"] = true.into();
        assert_eq!(parse_release(&v, "Ninho-x64.exe").unwrap(), None);
        v["prerelease"] = false.into();
        v["draft"] = true.into();
        assert_eq!(parse_release(&v, "Ninho-x64.exe").unwrap(), None);
        assert!(parse_release(&serde_json::json!({"message": "Not Found"}), "x").is_err());
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.6.0", "0.5.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.5.0", "0.5.0"));
        assert!(!is_newer("v0.5", "0.5.0"));
        assert!(!is_newer("0.4.9", "0.5.0"));
        assert!(is_newer("0.5.1", "0.5"));
    }

    #[test]
    fn reads_sha256sums() {
        let sums = "b3a1c9f0d2e4a6b8c0d2e4f60718293a4b5c6d7e8f90a1b2c3d4e5f607182939  Ninho-x64.exe\n\
                    0A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4E5F60718293A4B5C6D7E8F9 *Ninho-x86.exe\n\
                    short  Other.exe\n";
        assert_eq!(sum_for(sums, "Ninho-x64.exe").unwrap(), "b3a1c9f0d2e4a6b8c0d2e4f60718293a4b5c6d7e8f90a1b2c3d4e5f607182939");
        assert_eq!(sum_for(sums, "Ninho-x86.exe").unwrap(), "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9");
        assert_eq!(sum_for(sums, "Other.exe"), None); // not a 64-hex checksum
        assert_eq!(sum_for(sums, "Missing.exe"), None);
    }

    #[test]
    fn sibling_keeps_the_real_name() {
        assert_eq!(sibling(Path::new(r"C:\a\Ninho.exe"), ".new"), PathBuf::from(r"C:\a\Ninho.exe.new"));
        assert_eq!(sibling(Path::new(r"C:\a\meu-ninho.exe"), ".old"), PathBuf::from(r"C:\a\meu-ninho.exe.old"));
    }
}
