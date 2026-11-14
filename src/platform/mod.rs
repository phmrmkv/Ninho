//! What differs between Windows, Linux and macOS, behind one set of functions. Windows (`windows.rs`) is the main
//! target and keeps exactly the behavior Ninho always had; Linux and macOS (`unix.rs`, a preview) do the same jobs
//! with the system's own tools: `ss`/`lsof` for ports, `kill`, `xdg-open`/`open`, a terminal emulator, a lock socket
//! for "one Ninho at a time", and the servers already installed by the package manager (`systools.rs`).
//!
//! Every function exists on every system with the same signature, so the rest of the code calls it without #[cfg].

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(unix)]
pub mod systools;

/// Program file name: "mysqld" → "mysqld.exe" on Windows, "mysqld" on Linux and macOS.
pub fn exe(base: &str) -> String {
    format!("{base}{}", std::env::consts::EXE_SUFFIX)
}

/// Separator of the folders in PATH.
pub const PATH_SEP: &str = if cfg!(windows) { ";" } else { ":" };

/// The file that swallows output ("NUL" on Windows).
pub const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// `root` + the parts, written the way this system writes paths. On Windows this is exactly the old
/// `format!(r"{root}\a\b")`, so the strings saved in ninho.json and shown on screen do not change.
pub fn join(root: &str, parts: &[&str]) -> String {
    let sep = if cfg!(windows) { "\\" } else { "/" };
    let mut s = root.to_string();
    for p in parts {
        s.push_str(sep);
        s.push_str(p);
    }
    s
}

/// A path for a config file or a log line, with this system's separators: Windows turns '/' into '\'
/// (what Ninho always did with my.ini's datadir); Linux and macOS keep the path as written.
pub fn native_slashes(p: &str) -> String {
    if cfg!(windows) { p.replace('/', "\\") } else { p.to_string() }
}

/// Linux, macOS or Windows, for messages and the update file names.
pub fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Windows"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_names_and_joins() {
        if cfg!(windows) {
            assert_eq!(exe("mysqld"), "mysqld.exe");
            assert_eq!(join(r"C:\Ninho", &["data", "mysql-2"]), r"C:\Ninho\data\mysql-2");
            assert_eq!(native_slashes("C:/Ninho/data"), r"C:\Ninho\data");
        } else {
            assert_eq!(exe("mysqld"), "mysqld");
            assert_eq!(join("/home/a/Ninho", &["data", "mysql-2"]), "/home/a/Ninho/data/mysql-2");
            assert_eq!(native_slashes("/home/a/Ninho/data"), "/home/a/Ninho/data");
        }
    }
}
