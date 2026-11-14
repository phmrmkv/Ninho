//! Local HTTPS for the .test sites: one private certificate authority made by Ninho, trusted by the current Windows
//! user only, and one certificate per site signed by it. Uses the openssl.exe that Apache Lounge ships in
//! <apache>\bin, so nothing extra is installed.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::services::hide_window;
use crate::settings::Settings;

const CA_NAME: &str = "Ninho Local CA";
const CA_KEY: &str = "ninho-ca.key";
const CA_CRT: &str = "ninho-ca.crt";
const CA_SERIAL: &str = "ninho-ca.srl";
const CNF: &str = "ninho-openssl.cnf";
/// Re-issue a site certificate when it ends within this many seconds (30 days).
const RENEW_SECS: u64 = 30 * 24 * 3600;

/// Folder with the CA and site certificates: <root>\etc\ssl.
pub fn dir(s: &Settings) -> PathBuf {
    Path::new(&s.root).join("etc").join("ssl")
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// A host goes into file names and into a config file, so only plain hostname characters are accepted.
fn host_ok(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 100
        && !host.starts_with(['.', '-'])
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
}

/// Our own minimal openssl config. The shipped openssl.cnf varies between builds (and may add extensions of its own
/// that clash with ours), so the same file is used everywhere via OPENSSL_CONF.
fn cnf_text() -> String {
    format!(
        "[req]\r\ndistinguished_name = dn\r\nx509_extensions = v3_ca\r\nprompt = no\r\n\r\n[dn]\r\nCN = {CA_NAME}\r\n\r\n[v3_ca]\r\nbasicConstraints = critical,CA:TRUE\r\nkeyUsage = critical,keyCertSign,cRLSign\r\nsubjectKeyIdentifier = hash\r\n"
    )
}

fn ensure_cnf(s: &Settings) -> Result<PathBuf, String> {
    let d = dir(s);
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    let f = d.join(CNF);
    let text = cnf_text();
    if std::fs::read_to_string(&f).ok().as_deref() != Some(text.as_str()) {
        std::fs::write(&f, text).map_err(|e| format!("{}: {e}", f.display()))?;
    }
    Ok(f)
}

/// Run openssl with our config; Ok(stdout) on exit code 0, else Err(stderr).
fn run(openssl: &Path, cnf: &Path, args: &[&OsStr]) -> Result<String, String> {
    let mut c = Command::new(openssl);
    c.args(args).env("OPENSSL_CONF", cnf);
    // Next to its own folder so it finds nothing odd in the current directory.
    if let Some(p) = openssl.parent() {
        c.current_dir(p);
    }
    hide_window(&mut c);
    let out = c.output().map_err(|e| format!("{}: {e}", openssl.display()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("openssl: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn o(s: &str) -> &OsStr {
    OsStr::new(s)
}

/// Create Ninho's local certificate authority once (ninho-ca.key, ninho-ca.crt, 10 years, CN "Ninho Local CA").
pub fn ensure_ca(s: &Settings, openssl: &Path) -> Result<PathBuf, String> {
    let cnf = ensure_cnf(s)?;
    let d = dir(s);
    let (key, crt) = (d.join(CA_KEY), d.join(CA_CRT));
    // Both files or neither: a lone half is useless, and a new key means every site certificate is re-issued.
    if key.is_file() && crt.is_file() {
        return Ok(crt);
    }
    run(openssl, &cnf, &[o("genrsa"), o("-out"), key.as_os_str(), o("2048")])?;
    let made = run(
        openssl,
        &cnf,
        &[
            o("req"), o("-x509"), o("-new"), o("-nodes"), o("-sha256"), o("-days"), o("3650"),
            o("-key"), key.as_os_str(), o("-subj"), o(&format!("/CN={CA_NAME}")), o("-out"), crt.as_os_str(),
        ],
    );
    if let Err(e) = made {
        let _ = std::fs::remove_file(&crt);
        let _ = std::fs::remove_file(&key);
        return Err(e);
    }
    Ok(crt)
}

/// SHA-1 of the CA as Windows shows it, read with certutil (its label is translated, but always has "(sha1)").
fn ca_thumbprint(crt: &Path) -> Option<String> {
    let mut c = Command::new("certutil");
    c.arg("-dump").arg(crt);
    hide_window(&mut c);
    let out = c.output().ok()?;
    parse_thumbprint(&String::from_utf8_lossy(&out.stdout))
}

fn parse_thumbprint(dump: &str) -> Option<String> {
    let line = dump.lines().find(|l| l.to_ascii_lowercase().contains("(sha1)"))?;
    let hex: String = line.split_once(':')?.1.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (hex.len() == 40).then(|| hex.to_ascii_uppercase())
}

/// Is the CA in the current user's Trusted Root store?
pub fn ca_trusted(s: &Settings) -> bool {
    let crt = dir(s).join(CA_CRT);
    if !crt.is_file() {
        return false;
    }
    // By thumbprint when possible, so an older CA with the same name does not count.
    let what = ca_thumbprint(&crt).unwrap_or_else(|| CA_NAME.to_string());
    let mut c = Command::new("certutil");
    c.args(["-user", "-store", "Root", &what]);
    hide_window(&mut c);
    match c.output() {
        Ok(o) => o.status.success() && String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains(&CA_NAME.to_ascii_lowercase()),
        Err(_) => false,
    }
}

/// Add the CA to the CURRENT USER's Trusted Root store. Windows shows its own confirmation dialog and this waits for
/// the answer. Only the user store, never the machine store, and nothing runs elevated.
pub fn trust_ca(s: &Settings) -> Result<(), String> {
    let crt = dir(s).join(CA_CRT);
    if !crt.is_file() {
        return Err(format!("{}: not created yet", crt.display()));
    }
    let mut c = Command::new("certutil");
    c.args(["-user", "-addstore", "Root"]).arg(&crt);
    // Not hidden-window sensitive: the confirmation dialog is shown by Windows itself.
    hide_window(&mut c);
    let out = c.output().map_err(|e| format!("certutil: {e}"))?;
    if ca_trusted(s) {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Err(if msg.is_empty() { "The certificate was not added (confirmation refused?)".into() } else { msg })
    }
}

/// Extension file of a site certificate: the names the browser checks (SAN) and what the key may be used for.
fn ext_text(host: &str) -> String {
    format!(
        "authorityKeyIdentifier = keyid,issuer\r\nbasicConstraints = CA:FALSE\r\nkeyUsage = digitalSignature,keyEncipherment\r\nextendedKeyUsage = serverAuth\r\nsubjectAltName = @alt\r\n\r\n[alt]\r\nDNS.1 = {host}\r\nDNS.2 = *.{host}\r\nDNS.3 = localhost\r\nIP.1 = 127.0.0.1\r\n"
    )
}

fn modified(p: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Certificate for one site host (and *.host, localhost, 127.0.0.1), signed by the CA, 825 days. Re-issued only when
/// missing, older than the CA, or ending in less than 30 days.
pub fn ensure_site_cert(s: &Settings, openssl: &Path, host: &str) -> Result<(PathBuf, PathBuf), String> {
    if !host_ok(host) {
        return Err(format!("Invalid host name: {host}"));
    }
    let ca_crt = ensure_ca(s, openssl)?;
    let d = dir(s);
    let ca_key = d.join(CA_KEY);
    let (crt, key) = (d.join(format!("{host}.crt")), d.join(format!("{host}.key")));
    if crt.is_file() && key.is_file() {
        let newer_than_ca = match (modified(&crt), modified(&ca_crt)) {
            (Some(c), Some(a)) => c >= a,
            _ => true,
        };
        // -checkend exits 0 when the certificate is still valid after that many seconds.
        let cnf = ensure_cnf(s)?;
        let valid = run(openssl, &cnf, &[o("x509"), o("-checkend"), o(&RENEW_SECS.to_string()), o("-noout"), o("-in"), crt.as_os_str()]).is_ok();
        if newer_than_ca && valid {
            return Ok((crt, key));
        }
    }
    let cnf = ensure_cnf(s)?;
    let (csr, ext, serial) = (d.join(format!("{host}.csr")), d.join(format!("{host}.ext")), d.join(CA_SERIAL));
    std::fs::write(&ext, ext_text(host)).map_err(|e| format!("{}: {e}", ext.display()))?;
    let made = (|| {
        run(openssl, &cnf, &[o("genrsa"), o("-out"), key.as_os_str(), o("2048")])?;
        run(openssl, &cnf, &[o("req"), o("-new"), o("-key"), key.as_os_str(), o("-subj"), o(&format!("/CN={host}")), o("-out"), csr.as_os_str()])?;
        run(
            openssl,
            &cnf,
            &[
                o("x509"), o("-req"), o("-in"), csr.as_os_str(), o("-CA"), ca_crt.as_os_str(), o("-CAkey"), ca_key.as_os_str(),
                o("-CAserial"), serial.as_os_str(), o("-CAcreateserial"), o("-days"), o("825"), o("-sha256"),
                o("-extfile"), ext.as_os_str(), o("-out"), crt.as_os_str(),
            ],
        )
    })();
    // Scratch files go either way.
    let _ = std::fs::remove_file(&csr);
    let _ = std::fs::remove_file(&ext);
    if let Err(e) = made {
        let _ = std::fs::remove_file(&crt);
        return Err(e);
    }
    Ok((crt, key))
}

/// Apache vhost text for HTTPS on `port` (443) for a site dir and host, pointing at its cert/key (forward slashes).
pub fn apache_ssl_vhost(site_dir: &str, host: &str, port: u16, crt: &Path, key: &Path) -> String {
    let (crt, key) = (slash(crt), slash(key));
    format!(
        "<VirtualHost *:{port}> \r\n    DocumentRoot \"{site_dir}\"\r\n    ServerName {host}\r\n    ServerAlias *.{host}\r\n    SSLEngine on\r\n    SSLCertificateFile \"{crt}\"\r\n    SSLCertificateKeyFile \"{key}\"\r\n    <Directory \"{site_dir}\">\r\n        AllowOverride All\r\n        Require all granted\r\n    </Directory>\r\n</VirtualHost>\r\n"
    )
}

/// Lines Apache needs once for SSL: LoadModule ssl_module, socache_shmcb_module, `Listen <port>`, SSLSessionCache.
pub fn apache_ssl_lines(port: u16) -> Vec<String> {
    vec![
        "LoadModule ssl_module modules/mod_ssl.so".into(),
        "LoadModule socache_shmcb_module modules/mod_socache_shmcb.so".into(),
        format!("Listen {port}"),
        "SSLSessionCache \"shmcb:logs/ssl_scache(512000)\"".into(),
        "SSLSessionCacheTimeout 300".into(),
    ]
}

/// "notAfter=Nov 13 12:00:00 2028 GMT" → "2028-11-13". The raw text is returned when it has another shape.
fn parse_enddate(out: &str) -> String {
    let raw = out.trim().strip_prefix("notAfter=").unwrap_or(out.trim());
    let p: Vec<&str> = raw.split_whitespace().collect();
    // Month, day, time, year (the day can be padded with a double space, which split_whitespace absorbs).
    if p.len() >= 4 {
        const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        if let (Some(m), Ok(d), Ok(y)) = (M.iter().position(|m| *m == p[0]), p[1].parse::<u32>(), p[3].parse::<u32>()) {
            return format!("{y:04}-{:02}-{d:02}", m + 1);
        }
    }
    raw.to_string()
}

/// Certificates in dir(s) with host and expiry date, for a list in the UI.
pub fn list(s: &Settings, openssl: &Path) -> Vec<(String, String)> {
    let Ok(cnf) = ensure_cnf(s) else { return Vec::new() };
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir(s))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("crt")))
                .filter(|p| p.file_name().is_some_and(|n| !n.eq_ignore_ascii_case(CA_CRT)))
                .filter_map(|p| {
                    let host = p.file_stem()?.to_string_lossy().into_owned();
                    let end = run(openssl, &cnf, &[o("x509"), o("-enddate"), o("-noout"), o("-in"), p.as_os_str()]).ok()?;
                    Some((host, parse_enddate(&end)))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vhost_has_ssl_and_forward_slashes() {
        let t = apache_ssl_vhost("C:/laragon/www/app", "app.test", 443, Path::new(r"C:\l\etc\ssl\app.test.crt"), Path::new(r"C:\l\etc\ssl\app.test.key"));
        assert!(t.starts_with("<VirtualHost *:443>"));
        assert!(t.contains("ServerName app.test\r\n"));
        assert!(t.contains("ServerAlias *.app.test"));
        assert!(t.contains("SSLEngine on"));
        assert!(t.contains("SSLCertificateFile \"C:/l/etc/ssl/app.test.crt\""));
        assert!(t.contains("SSLCertificateKeyFile \"C:/l/etc/ssl/app.test.key\""));
        assert!(!t.contains("etc\\ssl"));
        assert!(t.trim_end().ends_with("</VirtualHost>"));
    }

    #[test]
    fn ssl_lines_cover_modules_and_port() {
        let l = apache_ssl_lines(8443);
        assert!(l.iter().any(|x| x.starts_with("LoadModule ssl_module")));
        assert!(l.iter().any(|x| x.starts_with("LoadModule socache_shmcb_module")));
        assert!(l.contains(&"Listen 8443".to_string()));
        assert!(l.iter().any(|x| x.starts_with("SSLSessionCache ")));
    }

    #[test]
    fn ext_has_san_for_host_wildcard_localhost_ip() {
        let t = ext_text("shop.test");
        assert!(t.contains("subjectAltName = @alt"));
        assert!(t.contains("DNS.1 = shop.test"));
        assert!(t.contains("DNS.2 = *.shop.test"));
        assert!(t.contains("DNS.3 = localhost"));
        assert!(t.contains("IP.1 = 127.0.0.1"));
        assert!(t.contains("CA:FALSE"));
    }

    #[test]
    fn enddate_is_parsed() {
        assert_eq!(parse_enddate("notAfter=Nov 13 12:00:00 2028 GMT\r\n"), "2028-11-13");
        assert_eq!(parse_enddate("notAfter=Feb  3 01:02:03 2027 GMT"), "2027-02-03");
        assert_eq!(parse_enddate("weird"), "weird");
    }

    #[test]
    fn thumbprint_is_parsed_in_any_language() {
        let en = "Cert Hash(sha1): 0a 1b 2c 3d 4e 5f 60 71 82 93 a4 b5 c6 d7 e8 f9 01 12 23 34\r\n";
        let pt = "Hash do Certificado(sha1): 0a1b2c3d4e5f60718293a4b5c6d7e8f901122334\r\n";
        assert_eq!(parse_thumbprint(en).as_deref(), Some("0A1B2C3D4E5F60718293A4B5C6D7E8F901122334"));
        assert_eq!(parse_thumbprint(pt).as_deref(), Some("0A1B2C3D4E5F60718293A4B5C6D7E8F901122334"));
        assert_eq!(parse_thumbprint("nothing"), None);
    }

    #[test]
    fn host_names_are_checked() {
        assert!(host_ok("my-app.test"));
        assert!(!host_ok(""));
        assert!(!host_ok("a b.test"));
        assert!(!host_ok("a\r\nb"));
        assert!(!host_ok(".test"));
    }
}

/// Real openssl run, only on a PC that has Apache's openssl in C:\laragon (cargo test -- --ignored).
#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore]
    fn makes_ca_and_site_cert() {
        let Some(openssl) = std::fs::read_dir(r"C:\laragon\bin\apache").ok().and_then(|rd| {
            rd.filter_map(|e| e.ok()).map(|e| e.path().join("bin").join("openssl.exe")).find(|p| p.is_file())
        }) else {
            return;
        };
        let root = std::env::temp_dir().join("ninho-certs-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut s = Settings::default();
        s.root = root.to_string_lossy().into_owned();
        let ca = ensure_ca(&s, &openssl).expect("CA");
        assert!(ca.is_file());
        let (crt, key) = ensure_site_cert(&s, &openssl, "blog.test").expect("site cert");
        assert!(crt.is_file() && key.is_file());
        let listed = list(&s, &openssl);
        assert!(listed.iter().any(|(h, end)| h == "blog.test" && end.len() == 10), "{listed:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod live_apache {
    use super::*;

    /// Apache accepts the SSL lines and an HTTPS vhost made by this module (httpd -t), without starting it.
    #[test]
    #[ignore]
    fn apache_accepts_ssl_config() {
        let Some(apache) = std::fs::read_dir(r"C:\laragon\bin\apache").ok().and_then(|rd| {
            rd.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.join("bin").join("openssl.exe").is_file())
        }) else {
            return;
        };
        let openssl = apache.join("bin").join("openssl.exe");
        let root = std::env::temp_dir().join("ninho-ssl-apache-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("www").join("blog")).unwrap();
        let mut s = Settings::default();
        s.root = root.to_string_lossy().into_owned();
        ensure_ca(&s, &openssl).unwrap();
        let (crt, key) = ensure_site_cert(&s, &openssl, "blog.test").unwrap();
        let site = root.join("www").join("blog").to_string_lossy().replace('\\', "/");
        let mut conf = apache_ssl_lines(4443).join("\r\n");
        conf.push_str("\r\n");
        conf.push_str(&apache_ssl_vhost(&site, "blog.test", 4443, &crt, &key));
        let file = root.join("ssl-test.conf");
        std::fs::write(&file, conf).unwrap();
        let out = std::process::Command::new(apache.join("bin").join("httpd.exe"))
            .current_dir(&apache)
            .stdin(std::process::Stdio::null())
            .args(["-t", "-c"])
            .arg(format!("Include \"{}\"", file.to_string_lossy().replace('\\', "/")))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
        let _ = std::fs::remove_dir_all(&root);
        assert!(text.contains("Syntax OK"), "{text}");
    }
}
