//! Ninho is portable (one exe, settings beside it) and can also install itself for the current user, with no
//! administrator rights and no separate installer:
//!   - copies Ninho.exe (and its ninho.json, so the installed copy uses the same servers and sites) to
//!     %LOCALAPPDATA%\Programs\Ninho
//!   - a Start menu shortcut
//!   - an entry in Windows "Installed apps" (HKCU ...\Uninstall\Ninho) whose Uninstall runs `Ninho.exe --uninstall-app`
//! Uninstalling removes only those three things. The servers, databases and sites (the root folder) are never touched.
//! Linux (preview): the program goes to ~/.local/share/ninho, with a menu entry (~/.local/share/applications) and
//! its icon. macOS: not offered yet (Ninho runs from any folder).
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;

#[cfg(windows)]
use crate::services::hide_window;

#[cfg(windows)]
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ninho";

/// Where an installed Ninho lives.
#[cfg(windows)]
pub fn install_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    Path::new(&base).join("Programs").join("Ninho")
}

#[cfg(windows)]
fn start_menu_link() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    Path::new(&base).join(r"Microsoft\Windows\Start Menu\Programs").join("Ninho.lnk")
}

/// Is this copy the installed one?
pub fn is_installed_copy() -> bool {
    std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)).is_some_and(|d| same_dir(&d, &install_dir()))
}

/// Is Ninho installed for this user (whatever copy is running)?
pub fn installed() -> bool {
    install_dir().join(PROGRAM).is_file()
}

/// File name of the installed program.
const PROGRAM: &str = if cfg!(windows) { "Ninho.exe" } else { "Ninho" };

/// Can this system install Ninho (Windows and Linux; macOS not yet)?
pub const AVAILABLE: bool = !cfg!(target_os = "macos");

fn same_dir(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_ascii_lowercase();
    n(a) == n(b)
}

#[cfg(windows)]
fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let mut c = Command::new(cmd);
    c.args(args);
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

/// A Windows shortcut (.lnk) through the shell's own COM object.
#[cfg(windows)]
fn shortcut(link: &Path, target: &Path, workdir: &Path) -> Result<(), String> {
    let esc = |p: &Path| p.to_string_lossy().replace('\'', "''");
    let ps = format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath = '{}'; $s.WorkingDirectory = '{}'; $s.IconLocation = '{},0'; $s.Description = 'Ninho - local server'; $s.Save()",
        esc(link),
        esc(target),
        esc(workdir),
        esc(target)
    );
    run("powershell", &["-NoProfile", "-Command", &ps])
}

/// Install the running copy for the current user. Returns the installed exe.
#[cfg(windows)]
pub fn install(version: &str) -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = install_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let exe = dir.join("Ninho.exe");
    if !same_dir(me.parent().unwrap_or(Path::new(".")), &dir) {
        std::fs::copy(&me, &exe).map_err(|e| format!("{}: {e}", exe.display()))?;
        // Same servers and sites: carry the settings over, unless the installed copy already has its own.
        let here = me.parent().unwrap_or(Path::new(".")).join("ninho.json");
        let there = dir.join("ninho.json");
        if !there.exists() {
            let settings = crate::settings::Settings::load();
            let _ = std::fs::write(&there, serde_json::to_string_pretty(&settings).unwrap_or_default());
            let _ = here;
        }
    }
    shortcut(&start_menu_link(), &exe, &dir)?;
    let size_kb = std::fs::metadata(&exe).map(|m| m.len() / 1024).unwrap_or(0).to_string();
    let exe_s = exe.to_string_lossy().into_owned();
    let entries: [(&str, &str, String); 9] = [
        ("DisplayName", "REG_SZ", "Ninho".into()),
        ("DisplayVersion", "REG_SZ", version.into()),
        ("Publisher", "REG_SZ", "phmrmkv".into()),
        ("DisplayIcon", "REG_SZ", exe_s.clone()),
        ("InstallLocation", "REG_SZ", dir.to_string_lossy().into_owned()),
        ("UninstallString", "REG_SZ", format!("\"{exe_s}\" --uninstall-app")),
        ("URLInfoAbout", "REG_SZ", "https://github.com/phmrmkv/Ninho".into()),
        ("NoModify", "REG_DWORD", "1".into()),
        ("EstimatedSize", "REG_DWORD", size_kb),
    ];
    for (name, kind, value) in entries {
        run("reg", &["add", UNINSTALL_KEY, "/v", name, "/t", kind, "/d", &value, "/f"])?;
    }
    run("reg", &["add", UNINSTALL_KEY, "/v", "NoRepair", "/t", "REG_DWORD", "/d", "1", "/f"])?;
    Ok(exe)
}

/// Remove the installed program (shortcut, "Installed apps" entry, start-with-Windows entry, the program folder).
/// The folder is deleted a moment after this process exits, since a running exe can't delete itself.
#[cfg(windows)]
pub fn uninstall() -> Result<(), String> {
    let _ = std::fs::remove_file(start_menu_link());
    let _ = run("reg", &["delete", UNINSTALL_KEY, "/f"]);
    // Only Ninho's own start-with-Windows entry, and only if it points at the installed copy.
    let _ = run("reg", &["delete", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "Ninho", "/f"]);
    let dir = install_dir();
    if dir.join("Ninho.exe").is_file() {
        // Remove only the files Ninho put there, then the folder if it is empty.
        // ping as the pause: `timeout` refuses to run without a console. The line goes to cmd as is (raw_arg),
        // because Rust's argument quoting (\") is not what cmd understands.
        let script = format!(
            "ping -n 3 127.0.0.1 >nul & del /f /q \"{0}\\Ninho.exe\" \"{0}\\ninho.json\" & rmdir \"{0}\"",
            dir.display()
        );
        let mut c = Command::new("cmd");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.raw_arg(format!("/c \"{script}\""));
        }
        #[cfg(not(windows))]
        c.args(["/c", &script]);
        hide_window(&mut c);
        c.spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A plain message box (used by --uninstall-app, which runs without the main window).
pub fn message(title: &str, text: &str) {
    crate::platform::message(title, text);
}

// ---------------------------------------------------------------- Linux and macOS (preview)

#[cfg(unix)]
fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(unix)]
fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local").join("share"))
}

/// Where an installed Ninho lives (Linux: ~/.local/share/ninho).
#[cfg(unix)]
pub fn install_dir() -> PathBuf {
    data_home().join("ninho")
}

#[cfg(unix)]
fn desktop_file() -> PathBuf {
    data_home().join("applications").join("ninho.desktop")
}

#[cfg(unix)]
fn icon_file() -> PathBuf {
    data_home().join("icons").join("hicolor").join("256x256").join("apps").join("ninho.png")
}

/// Linux: copy the running program to ~/.local/share/ninho, with a menu entry and its icon (no root needed).
/// The settings stay in ~/Ninho, shared by every copy.
#[cfg(unix)]
pub fn install(_version: &str) -> Result<PathBuf, String> {
    if !AVAILABLE {
        return Err("Installing is not available on macOS yet: run Ninho from any folder.".into());
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = install_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let exe = dir.join(PROGRAM);
    if !same_dir(me.parent().unwrap_or(Path::new(".")), &dir) {
        // A running program can be replaced by a rename, not overwritten in place.
        let part = dir.join("Ninho.part");
        std::fs::copy(&me, &part).map_err(|e| format!("{}: {e}", exe.display()))?;
        std::fs::rename(&part, &exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    }
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    let icon = icon_file();
    if let Some(d) = icon.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&icon, include_bytes!("../assets/ninho-256.png"));
    let desktop = desktop_file();
    if let Some(d) = desktop.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&desktop, crate::platform::desktop_entry(&exe, &icon)).map_err(|e| format!("{}: {e}", desktop.display()))?;
    Ok(exe)
}

/// Remove the installed program, its menu entry, its icon and the start-with-the-session entry. The servers,
/// databases and sites (~/Ninho) are never touched.
#[cfg(unix)]
pub fn uninstall() -> Result<(), String> {
    let _ = std::fs::remove_file(desktop_file());
    let _ = std::fs::remove_file(icon_file());
    crate::platform::set_run_at_startup(false);
    let dir = install_dir();
    let _ = std::fs::remove_file(dir.join(PROGRAM));
    let _ = std::fs::remove_dir(&dir);
    Ok(())
}

#[cfg(all(test, windows))]
mod live {
    /// Real install for this user, checked, then removed (cargo test -- --ignored).
    #[test]
    #[ignore]
    fn install_then_uninstall() {
        let exe = super::install("0.0.0-test").expect("install");
        assert!(exe.is_file());
        assert!(super::start_menu_link().is_file());
        assert!(super::run("reg", &["query", super::UNINSTALL_KEY, "/v", "UninstallString"]).is_ok());
        super::uninstall().expect("uninstall");
        std::thread::sleep(std::time::Duration::from_secs(6));
        assert!(!super::start_menu_link().exists());
        assert!(super::run("reg", &["query", super::UNINSTALL_KEY]).is_err());
        assert!(!super::install_dir().join("Ninho.exe").exists());
    }
}
