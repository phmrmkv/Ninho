//! Windows: the behavior Ninho always had (moved here unchanged from app.rs, ports.rs, services.rs, settings.rs,
//! i18n.rs and selfinstall.rs).
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ports::Listener;

/// No black console window for the programs Ninho starts.
pub fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

fn system32(tool: &str) -> PathBuf {
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    Path::new(&sys).join("System32").join(tool)
}

/// Windows' own curl (no TLS code of our own).
pub fn curl() -> PathBuf {
    system32("curl.exe")
}

/// Windows' own tar (Git's tar cannot open ZIP files).
pub fn tar() -> PathBuf {
    system32("tar.exe")
}

/// A path inside a command line typed in a terminal.
pub fn quote(p: &str) -> String {
    format!("\"{p}\"")
}

/// A folder, a file or a web page, with the program Windows uses for it.
pub fn open(target: &str) {
    let _ = Command::new("explorer").arg(target).spawn();
}

/// A text file (config, log) in Notepad.
pub fn open_text(file: &Path) {
    let _ = Command::new("notepad").arg(file).spawn();
}

/// A terminal in `dir`, with `path` as PATH.
pub fn terminal(dir: &str, path: &str) {
    let _ = Command::new("cmd").args(["/c", "start", "", "cmd", "/k", &format!("cd /d \"{dir}\"")]).env("PATH", path).spawn();
}

/// A terminal titled `title` that runs `script` and stays open.
pub fn terminal_run(title: &str, script: &str, cwd: Option<&Path>, path: Option<&str>) {
    let mut c = Command::new("cmd");
    c.args(["/c", "start", title, "cmd", "/k", script]);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    if let Some(p) = path {
        c.env("PATH", p);
    }
    let _ = c.spawn();
}

/// VS Code in `dir`. False when it could not start.
pub fn code(dir: &Path, path: &str) -> bool {
    let mut c = Command::new("cmd");
    c.args(["/c", "code", "."]).current_dir(dir).env("PATH", path);
    hide_window(&mut c);
    c.spawn().is_ok()
}

fn run(cmd: &str, args: &[&str]) -> String {
    let mut c = Command::new(cmd);
    c.args(args);
    hide_window(&mut c);
    c.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// PID → program name, from `tasklist /fo csv /nh`.
pub fn programs() -> HashMap<u32, String> {
    run("tasklist", &["/fo", "csv", "/nh"])
        .lines()
        .filter_map(|l| {
            let cols: Vec<&str> = l.split("\",\"").map(|c| c.trim_matches('"')).collect();
            Some((cols.get(1)?.trim().parse().ok()?, cols.first()?.to_string()))
        })
        .collect()
}

/// Ports in LISTENING state (TCP over IPv4 and IPv6), one entry per port and program, sorted by port.
pub fn listening() -> Vec<Listener> {
    let names = programs();
    let mut by: HashMap<(u16, u32), Listener> = HashMap::new();
    for line in run("netstat", &["-ano", "-p", "TCP"]).lines().chain(run("netstat", &["-ano", "-p", "TCPv6"]).lines()) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Proto  Local  Foreign  State  PID  — the state word changes with the Windows language, so look at the
        // foreign address instead: a listening socket has none (0.0.0.0:0 or [::]:0).
        if cols.len() != 5 || !cols[0].starts_with("TCP") || !(cols[2].ends_with(":0")) {
            continue;
        }
        let Some((addr, port)) = cols[1].rsplit_once(':') else { continue };
        let (Ok(port), Ok(pid)) = (port.parse::<u16>(), cols[4].parse::<u32>()) else { continue };
        let e = by.entry((port, pid)).or_insert_with(|| Listener {
            port,
            addrs: Vec::new(),
            pid,
            program: names.get(&pid).cloned().unwrap_or_default(),
        });
        if !e.addrs.iter().any(|a| a == addr) {
            e.addrs.push(addr.to_string());
        }
    }
    let mut out: Vec<Listener> = by.into_values().collect();
    out.sort_by_key(|l| (l.port, l.pid));
    out
}

/// Stop one program by its PID (and the programs it started).
pub fn kill(pid: u32) -> Result<(), String> {
    let mut c = Command::new("taskkill");
    c.args(["/F", "/T", "/PID", &pid.to_string()]);
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

/// Parts of Windows itself: stopping them crashes or locks the PC, so the Ports tab never offers it.
pub fn is_system(program: &str, pid: u32) -> bool {
    const CORE: &[&str] = &[
        "system", "svchost.exe", "lsass.exe", "wininit.exe", "services.exe", "csrss.exe", "smss.exe", "winlogon.exe",
        "spoolsv.exe", "vmms.exe", "lsaiso.exe", "dwm.exe", "explorer.exe", "memory compression", "registry",
    ];
    pid <= 4 || CORE.contains(&program.to_ascii_lowercase().as_str())
}

/// Word shown for the system's own ports in the Ports tab.
pub const SYSTEM_WORD: &str = "Windows";

/// The hosts file.
pub fn hosts_path() -> String {
    let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{win}\System32\drivers\etc\hosts")
}

/// The Windows hosts file in Notepad as administrator (Windows asks for permission; saving needs it).
/// Returns a line for the activity list when the system cannot open it for editing (never on Windows).
pub fn edit_hosts() -> Option<String> {
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-Command", "Start-Process notepad -Verb RunAs -ArgumentList \"$env:SystemRoot\\System32\\drivers\\etc\\hosts\""]);
    hide_window(&mut c);
    let _ = c.spawn();
    None
}

/// Puts `new_text` (already saved in `tmp_file`) in the hosts file with administrator rights: Windows asks for
/// permission. Ok(true) when it was copied, Ok(false) when the person said no.
pub fn copy_hosts_as_admin(tmp_file: &Path, hosts: &str) -> Result<bool, String> {
    let ps = format!(
        "Start-Process -FilePath cmd -Verb RunAs -WindowStyle Hidden -Wait -ArgumentList '/c copy /y \"{}\" \"{}\"'",
        tmp_file.display(),
        hosts
    );
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-Command", &ps]);
    hide_window(&mut c);
    Ok(matches!(c.output(), Ok(o) if o.status.success()))
}

/// The PC's offset from UTC in minutes, asked to Windows once (std has no time zones).
pub fn utc_offset_min() -> i64 {
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-Command", "[int][TimeZoneInfo]::Local.GetUtcOffset([DateTime]::Now).TotalMinutes"]);
        hide_window(&mut c);
        c.output().ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok()).unwrap_or(0)
    })
}

/// A plain Windows message box (used by --uninstall-app and crashes, which run without the main window).
pub fn message(title: &str, text: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: *mut core::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (t, c) = (w(text), w(title));
    unsafe {
        MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 0x40);
    }
}

/// The language Windows shows its menus in ("pt-BR"), for the first start.
pub fn locale() -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
    }
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    (n > 1).then(|| String::from_utf16_lossy(&buf[..(n - 1) as usize]))
}

/// Start Ninho with Windows (the user's Run key), or not.
pub fn set_run_at_startup(on: bool) {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let mut cmd = Command::new("reg");
    if on {
        let exe = std::env::current_exe().unwrap_or_default();
        cmd.args(["add", key, "/v", "Ninho", "/t", "REG_SZ", "/d"]).arg(format!("\"{}\"", exe.display())).arg("/f");
    } else {
        cmd.args(["delete", key, "/v", "Ninho", "/f"]);
    }
    hide_window(&mut cmd);
    let _ = cmd.output();
}

/// Windows' own "choose a folder" dialog. Blocks until it closes: call it on a thread.
pub fn pick_folder(start: &str) -> Option<String> {
    let start = start.replace('\'', "''");
    let ps = format!("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.SelectedPath = '{start}'; $d.ShowNewFolderButton = $true; if ($d.ShowDialog() -eq 'OK') {{ $d.SelectedPath }}");
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-STA", "-Command", &ps]);
    hide_window(&mut c);
    let out = c.output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() { None } else { Some(p) }
}

/// Windows' own "open file" dialog for a .sql file (no extra crate needed).
pub fn pick_sql_file() -> Option<PathBuf> {
    let ps = "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Filter = 'SQL (*.sql)|*.sql'; if ($d.ShowDialog() -eq 'OK') { $d.FileName }";
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-STA", "-Command", ps]);
    hide_window(&mut c);
    let out = c.output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() { None } else { Some(PathBuf::from(p)) }
}

/// Fonts for the writing systems egui does not ship (CJK, Devanagari): Windows' own, by script.
pub fn font_files(script: &str) -> Vec<PathBuf> {
    let names: &[&str] = match script {
        "ja" => &["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"],
        "ko" => &["malgun.ttf", "gulim.ttc"],
        "zh-CN" => &["msyh.ttc", "simsun.ttc"],
        "zh-TW" => &["msjh.ttc", "mingliu.ttc"],
        "hi" => &["Nirmala.ttc", "Nirmala.ttf", "mangal.ttf"],
        // Fallback for Latin letters egui does not draw (Vietnamese).
        "latin" => &["segoeui.ttf", "arial.ttf", "tahoma.ttf"],
        _ => &[],
    };
    let fonts = Path::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("Fonts");
    names.iter().map(|f| fonts.join(f)).collect()
}
