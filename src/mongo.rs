//! MongoDB Community Server (and its shell, mongosh), from MongoDB's own download feed.
//!   Server  https://downloads.mongodb.org/current.json: the newest production release of a major line (X.0), the
//!           Windows x86_64 "base" (Community) ZIP. Rapid releases (8.1, 8.2...) are skipped: they are short-lived and
//!           their data cannot go back to the previous major.
//!   Shell   the latest mongosh release on GitHub, the ZIP from downloads.mongodb.com.
//! Stopping: `mongod --shutdown` exists only on Linux, so a polite stop runs mongosh with
//! `db.adminCommand({shutdown:1})` when mongosh is installed; without it (or if it does not work in 8 seconds) the
//! program is ended, which is safe because MongoDB journals its writes.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::i18n::{Lang, tr};
use crate::install::{Latest, X86, exists, json, ver_key};

pub const FEED: &str = "https://downloads.mongodb.org/current.json";

fn no_32bit(lang: Lang, tool: &str) -> String {
    format!("{tool} {}", tr(lang, "log_no_32bit"))
}

/// The version to offer from MongoDB's feed (`current.json`), with its Windows ZIP.
pub fn pick_server(feed: &serde_json::Value) -> Result<Latest, String> {
    let list = feed["versions"].as_array().ok_or("current.json: no versions")?;
    let stable: Vec<&serde_json::Value> = list
        .iter()
        .filter(|r| {
            r["production_release"].as_bool() == Some(true)
                && r["development_release"].as_bool() != Some(true)
                && r["release_candidate"].as_bool() != Some(true)
                && r["continuous_release"].as_bool() != Some(true)
                && r["version"].as_str().is_some()
        })
        .collect();
    fn key(r: &serde_json::Value) -> Vec<u32> {
        ver_key(r["version"].as_str().unwrap_or(""))
    }
    // A major line (X.0.y) first; if the feed has none, the newest production release.
    let best = stable
        .iter()
        .filter(|r| key(r).get(1) == Some(&0))
        .max_by_key(|r| key(r))
        .or_else(|| stable.iter().max_by_key(|r| key(r)))
        .ok_or("no production MongoDB release in current.json")?;
    let version = best["version"].as_str().unwrap_or("?").to_string();
    let url = best["downloads"]
        .as_array()
        .and_then(|d| d.iter().find(|x| x["target"].as_str() == Some("windows") && x["arch"].as_str() == Some("x86_64") && x["edition"].as_str() == Some("base")))
        .and_then(|d| d["archive"]["url"].as_str())
        .ok_or_else(|| format!("MongoDB {version} has no Windows x86_64 ZIP in current.json"))?;
    // The ZIP's top folder is named by MongoDB ("*": whatever the single folder is), renamed to the version folder.
    Ok(Latest { version: version.clone(), url: url.to_string(), top: "*".into(), folder: format!("mongodb-{version}") })
}

pub fn latest_server(lang: Lang) -> Result<Latest, String> {
    if X86 {
        return Err(no_32bit(lang, "MongoDB"));
    }
    pick_server(&json(FEED)?)
}

/// mongosh from the tag of the GitHub release ("v2.5.1").
pub fn shell_from_tag(tag: &str) -> Latest {
    let v = tag.trim_start_matches('v').to_string();
    Latest { version: v.clone(), url: format!("https://downloads.mongodb.com/compass/mongosh-{v}-win32-x64.zip"), top: "*".into(), folder: format!("mongosh-{v}") }
}

pub fn latest_shell(lang: Lang) -> Result<Latest, String> {
    if X86 {
        return Err(no_32bit(lang, "mongosh"));
    }
    let v = json("https://api.github.com/repos/mongodb-js/mongosh/releases/latest")?;
    let l = shell_from_tag(v["tag_name"].as_str().ok_or("no mongosh release found")?);
    if exists(&l.url) { Ok(l) } else { Err(format!("{}: not found", l.url)) }
}

/// The newest installed mongosh.exe (<root>\bin\mongosh\<version>\bin\mongosh.exe).
pub fn mongosh_exe(root: &str) -> Option<PathBuf> {
    let mut vs = crate::install::installed(root, "mongosh");
    vs.sort_by_key(|v| ver_key(v));
    vs.iter().rev().map(|v| Path::new(root).join("bin").join("mongosh").join(v).join(r"bin\mongosh.exe")).find(|p| p.is_file())
}

/// The arguments of `mongod` for one server: its own data folder and port, only this PC, a log file in the data folder.
pub fn server_args(data: &str, port: u16) -> Vec<String> {
    vec![
        "--dbpath".into(),
        data.into(),
        "--port".into(),
        port.to_string(),
        "--bind_ip".into(),
        "127.0.0.1".into(),
        "--logpath".into(),
        Path::new(data).join("mongod.log").to_string_lossy().into_owned(),
        "--logappend".into(),
    ]
}

/// Polite stop through mongosh. False when mongosh is not installed (the caller then ends the program).
pub fn shutdown(root: &str, port: u16) -> bool {
    let Some(sh) = mongosh_exe(root) else { return false };
    let mut c = Command::new(sh);
    c.args(["--quiet", "--host", "127.0.0.1", "--port", &port.to_string(), "--eval", "db.adminCommand({shutdown:1})"]);
    // mongosh reports a lost connection when the server goes down: the answer does not matter, the port does.
    let _ = crate::ports::output_timeout(c, Duration::from_secs(15));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // current.json, trimmed to what the code reads.
    const FEED_SAMPLE: &str = r#"{"versions":[
      {"version":"8.3.17","production_release":true,"development_release":false,"release_candidate":false,"continuous_release":false,
       "downloads":[{"target":"windows","arch":"x86_64","edition":"base","archive":{"url":"https://fastdl.mongodb.org/windows/mongodb-windows-x86_64-8.3.17.zip"}}]},
      {"version":"9.0.8","production_release":true,"development_release":false,"release_candidate":false,"continuous_release":false,
       "downloads":[
         {"target":"rhel80","arch":"x86_64","edition":"base","archive":{"url":"https://fastdl.mongodb.org/linux/mongodb-linux-x86_64-rhel80-9.0.8.tgz"}},
         {"target":"windows","arch":"x86_64","edition":"enterprise","archive":{"url":"https://downloads.mongodb.com/windows/mongodb-windows-x86_64-enterprise-9.0.8.zip"}},
         {"target":"windows","arch":"x86_64","edition":"base","archive":{"url":"https://fastdl.mongodb.org/windows/mongodb-windows-x86_64-9.0.8.zip"}}]},
      {"version":"9.1.0-rc0","production_release":false,"development_release":false,"release_candidate":true,"continuous_release":false,"downloads":[]},
      {"version":"8.0.38","production_release":true,"development_release":false,"release_candidate":false,"continuous_release":false,
       "downloads":[{"target":"windows","arch":"x86_64","edition":"base","archive":{"url":"https://fastdl.mongodb.org/windows/mongodb-windows-x86_64-8.0.38.zip"}}]}
    ]}"#;

    #[test]
    fn newest_major_line_with_its_community_zip() {
        let l = pick_server(&serde_json::from_str(FEED_SAMPLE).unwrap()).unwrap();
        assert_eq!(l.version, "9.0.8");
        assert_eq!(l.url, "https://fastdl.mongodb.org/windows/mongodb-windows-x86_64-9.0.8.zip");
        assert_eq!(l.folder, "mongodb-9.0.8");
        assert_eq!(l.top, "*");
    }

    #[test]
    fn rapid_releases_and_candidates_are_skipped() {
        let feed = r#"{"versions":[
          {"version":"8.3.17","production_release":true,"downloads":[{"target":"windows","arch":"x86_64","edition":"base","archive":{"url":"u83"}}]},
          {"version":"8.0.38","production_release":true,"downloads":[{"target":"windows","arch":"x86_64","edition":"base","archive":{"url":"u80"}}]}]}"#;
        assert_eq!(pick_server(&serde_json::from_str(feed).unwrap()).unwrap().version, "8.0.38");
        // No major line at all: the newest production release is better than nothing.
        let only_rapid = r#"{"versions":[{"version":"8.3.17","production_release":true,"downloads":[{"target":"windows","arch":"x86_64","edition":"base","archive":{"url":"u"}}]}]}"#;
        assert_eq!(pick_server(&serde_json::from_str(only_rapid).unwrap()).unwrap().version, "8.3.17");
    }

    #[test]
    fn feed_without_a_windows_zip_is_an_error() {
        let feed = r#"{"versions":[{"version":"9.0.8","production_release":true,"downloads":[{"target":"rhel80","arch":"x86_64","edition":"base","archive":{"url":"x"}}]}]}"#;
        assert!(pick_server(&serde_json::from_str(feed).unwrap()).is_err());
        assert!(pick_server(&serde_json::from_str("{}").unwrap()).is_err());
    }

    #[test]
    fn mongosh_asset_from_the_release_tag() {
        let l = shell_from_tag("v2.5.1");
        assert_eq!(l.version, "2.5.1");
        assert_eq!(l.url, "https://downloads.mongodb.com/compass/mongosh-2.5.1-win32-x64.zip");
        assert_eq!(l.folder, "mongosh-2.5.1");
    }

    #[test]
    fn server_arguments() {
        let a = server_args(r"C:\ninho\data\mongodb", 27018);
        let joined = a.join(" ");
        assert!(joined.contains("--dbpath C:\\ninho\\data\\mongodb"));
        assert!(joined.contains("--port 27018"));
        assert!(joined.contains("--bind_ip 127.0.0.1"));
        assert!(joined.contains("mongod.log"));
    }
}
