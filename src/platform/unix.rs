//! Linux and macOS (preview): the same jobs as windows.rs, with the tools every Linux and macOS already has.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::ports::Listener;

/// Programs started from a window have no console to hide on Linux and macOS.
pub fn hide_window(cmd: &mut Command) {
    let _ = cmd;
}

/// The system's curl (every Linux desktop and macOS ships one).
pub fn curl() -> PathBuf {
    PathBuf::from("curl")
}

/// The system's tar (reads .tar.gz, .tar.xz and, on macOS, .zip).
pub fn tar() -> PathBuf {
    PathBuf::from("tar")
}

/// A word for sh: in single quotes, with any single quote closed, escaped and reopened.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A path inside a command line typed in a terminal.
pub fn quote(p: &str) -> String {
    sh_quote(p)
}

fn spawn_quiet(mut c: Command) -> bool {
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    c.spawn().is_ok()
}

/// A folder, a file or a web page, with the program the desktop uses for it.
pub fn open(target: &str) {
    let mut c = Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" });
    c.arg(target);
    spawn_quiet(c);
}

/// A text file (config, log) in the desktop's text editor.
pub fn open_text(file: &Path) {
    let mut c = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg("-t");
        c
    } else {
        Command::new("xdg-open")
    };
    c.arg(file);
    spawn_quiet(c);
}

/// A terminal window running `line` in sh. macOS: Terminal.app. Linux: $TERMINAL, then the usual emulators.
fn spawn_terminal(title: &str, line: &str) -> bool {
    if cfg!(target_os = "macos") {
        let esc = line.replace('\\', "\\\\").replace('"', "\\\"");
        let mut c = Command::new("osascript");
        c.args(["-e", &format!("tell application \"Terminal\" to do script \"{esc}\""), "-e", "tell application \"Terminal\" to activate"]);
        return spawn_quiet(c);
    }
    // Keep the terminal open after the command, like `cmd /k` on Windows.
    let line = format!("{line}; exec \"${{SHELL:-/bin/sh}}\"");
    let mut tries: Vec<(String, Vec<String>)> = Vec::new();
    if let Ok(t) = std::env::var("TERMINAL") {
        if !t.trim().is_empty() {
            tries.push((t, vec!["-e".into()]));
        }
    }
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
    tries.push(("x-terminal-emulator".into(), s(&["-e"])));
    tries.push(("gnome-terminal".into(), s(&["--title", title, "--"])));
    tries.push(("konsole".into(), s(&["-e"])));
    tries.push(("xfce4-terminal".into(), s(&["-T", title, "-x"])));
    tries.push(("kitty".into(), s(&["--title", title])));
    tries.push(("alacritty".into(), s(&["-t", title, "-e"])));
    tries.push(("foot".into(), s(&["-T", title])));
    tries.push(("xterm".into(), s(&["-T", title, "-e"])));
    for (prog, pre) in tries {
        let mut c = Command::new(&prog);
        c.args(&pre).args(["sh", "-c", &line]);
        if spawn_quiet(c) {
            return true;
        }
    }
    false
}

fn shell_line(script: &str, cwd: Option<&Path>, path: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(p) = path {
        parts.push(format!("export PATH={}", sh_quote(p)));
    }
    if let Some(d) = cwd {
        parts.push(format!("cd {}", sh_quote(&d.to_string_lossy())));
    }
    if !script.is_empty() {
        parts.push(script.to_string());
    }
    if parts.is_empty() {
        parts.push("true".into());
    }
    parts.join("; ")
}

/// A terminal in `dir`, with `path` as PATH.
pub fn terminal(dir: &str, path: &str) {
    spawn_terminal("Ninho", &shell_line("", Some(Path::new(dir)), Some(path)));
}

/// A terminal titled `title` that runs `script` and stays open.
pub fn terminal_run(title: &str, script: &str, cwd: Option<&Path>, path: Option<&str>) {
    spawn_terminal(title, &shell_line(script, cwd, path));
}

/// VS Code in `dir`. False when it could not start.
pub fn code(dir: &Path, path: &str) -> bool {
    let mut c = Command::new("code");
    c.arg(".").current_dir(dir).env("PATH", path);
    spawn_quiet(c)
}

fn run(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// PID → program name (only the processes this user may see).
pub fn programs() -> HashMap<u32, String> {
    run("ps", &["-A", "-o", "pid=", "-o", "comm="])
        .lines()
        .filter_map(|l| {
            let (pid, name) = l.trim().split_once(char::is_whitespace)?;
            let name = name.trim();
            // macOS gives the full path of the program; the name is enough.
            Some((pid.parse().ok()?, name.rsplit('/').next().unwrap_or(name).to_string()))
        })
        .collect()
}

fn add(by: &mut HashMap<(u16, u32), Listener>, addr: &str, port: u16, pid: u32, program: &str) {
    let e = by.entry((port, pid)).or_insert_with(|| Listener { port, addrs: Vec::new(), pid, program: program.to_string() });
    let addr = addr.trim_start_matches('[').trim_end_matches(']');
    let addr = if addr == "*" { "0.0.0.0" } else { addr };
    let addr = addr.split('%').next().unwrap_or(addr);
    if !e.addrs.iter().any(|a| a == addr) {
        e.addrs.push(addr.to_string());
    }
}

/// `ss -ltnpH` (Linux): "LISTEN 0 151 127.0.0.1:3306 0.0.0.0:* users:(("mysqld",pid=1234,fd=23))".
/// Programs of other users (root's sshd...) have no "users:" part: PID 0, no name.
pub fn parse_ss(text: &str) -> Vec<Listener> {
    let mut by: HashMap<(u16, u32), Listener> = HashMap::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 || cols[0] != "LISTEN" {
            continue;
        }
        let Some((addr, port)) = cols[3].rsplit_once(':') else { continue };
        let Ok(port) = port.parse::<u16>() else { continue };
        let users = line.find("users:((").map(|i| &line[i..]).unwrap_or("");
        let mut found = false;
        for part in users.split("(\"").skip(1) {
            let Some((name, rest)) = part.split_once('"') else { continue };
            let pid = rest.split("pid=").nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|p| p.parse().ok());
            if let Some(pid) = pid {
                add(&mut by, addr, port, pid, name);
                found = true;
            }
        }
        if !found {
            add(&mut by, addr, port, 0, "");
        }
    }
    let mut out: Vec<Listener> = by.into_values().collect();
    out.sort_by_key(|l| (l.port, l.pid));
    out
}

/// `lsof -nP -iTCP -sTCP:LISTEN -F pcn` (macOS): "p<pid>", "c<program>", then "n<addr>:<port>" per socket.
pub fn parse_lsof(text: &str) -> Vec<Listener> {
    let mut by: HashMap<(u16, u32), Listener> = HashMap::new();
    let (mut pid, mut prog) = (0u32, String::new());
    for line in text.lines() {
        let (tag, val) = line.split_at(line.len().min(1));
        match tag {
            "p" => pid = val.parse().unwrap_or(0),
            "c" => prog = val.to_string(),
            "n" => {
                if let Some((addr, port)) = val.rsplit_once(':') {
                    if let Ok(port) = port.parse::<u16>() {
                        add(&mut by, addr, port, pid, &prog);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out: Vec<Listener> = by.into_values().collect();
    out.sort_by_key(|l| (l.port, l.pid));
    out
}

/// Ports in LISTEN state (TCP over IPv4 and IPv6), one entry per port and program, sorted by port.
pub fn listening() -> Vec<Listener> {
    if cfg!(target_os = "macos") {
        parse_lsof(&run("lsof", &["-nP", "+c0", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"]))
    } else {
        parse_ss(&run("ss", &["-ltnpH"]))
    }
}

fn alive(pid: u32) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

/// Stop one program by its PID: asked politely (TERM), forced (KILL) if it still runs after 5 seconds.
pub fn kill(pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err("no PID".into());
    }
    let out = Command::new("kill").args(["-TERM", &pid.to_string()]).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    for _ in 0..20 {
        if !alive(pid) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
    Ok(())
}

/// Parts of the system (or programs of another user, which have no PID here): the Ports tab never offers to stop them.
pub fn is_system(program: &str, pid: u32) -> bool {
    const CORE: &[&str] = &["systemd", "systemd-resolve", "systemd-resolved", "init", "launchd", "sshd", "cupsd", "rpcbind", "avahi-daemon", "mdnsresponder", "rapportd", "controlcenter", "sharingd"];
    pid <= 1 || CORE.contains(&program.to_ascii_lowercase().as_str())
}

/// Word shown for the system's own ports in the Ports tab.
pub const SYSTEM_WORD: &str = if cfg!(target_os = "macos") { "macOS" } else { "Linux" };

/// The hosts file.
pub fn hosts_path() -> String {
    "/etc/hosts".into()
}

/// The command that edits the hosts file (it needs the administrator password, which Ninho never asks for).
pub fn edit_hosts() -> Option<String> {
    Some(if cfg!(target_os = "macos") { "sudo nano /etc/hosts".into() } else { "sudoedit /etc/hosts".into() })
}

/// Writing /etc/hosts needs root: Ninho does not ask for the password, the activity list shows the command instead.
pub fn copy_hosts_as_admin(tmp_file: &Path, hosts: &str) -> Result<bool, String> {
    Err(format!("sudo cp {} {}", sh_quote(&tmp_file.to_string_lossy()), hosts))
}

/// "-0300" → -180.
pub fn parse_utc_offset(z: &str) -> Option<i64> {
    let z = z.trim();
    let (sign, digits) = match z.as_bytes().first()? {
        b'-' => (-1, &z[1..]),
        b'+' => (1, &z[1..]),
        _ => (1, z),
    };
    if digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let h: i64 = digits[..2].parse().ok()?;
    let m: i64 = digits[2..].parse().ok()?;
    Some(sign * (h * 60 + m))
}

/// The PC's offset from UTC in minutes, asked to `date` once (std has no time zones).
pub fn utc_offset_min() -> i64 {
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| parse_utc_offset(&run("date", &["+%z"])).unwrap_or(0))
}

/// No message box on Linux and macOS: the text goes to the terminal (and the crash log already has it).
pub fn message(title: &str, text: &str) {
    eprintln!("{title}: {text}");
}

/// "pt_BR.UTF-8" → "pt-BR"; "C" and "POSIX" say nothing.
pub fn locale_code(raw: &str) -> Option<String> {
    let base = raw.split(['.', '@']).next()?.trim();
    if base.is_empty() || base == "C" || base == "POSIX" {
        return None;
    }
    Some(base.replace('_', "-"))
}

/// The language of the desktop ("pt-BR"), for the first start: LC_ALL, LC_MESSAGES, LANG; on macOS also the
/// system's own setting.
pub fn locale() -> Option<String> {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(c) = std::env::var(var).ok().as_deref().and_then(locale_code) {
            return Some(c);
        }
    }
    if cfg!(target_os = "macos") {
        return locale_code(run("defaults", &["read", "-g", "AppleLocale"]).trim());
    }
    None
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// Ninho's own folder when it is not portable: ~/Ninho.
pub fn default_root() -> PathBuf {
    home().join("Ninho")
}

/// The desktop entry that starts Ninho (Linux): the same file serves the menu and the autostart folder.
pub fn desktop_entry(exe: &Path, icon: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Ninho\nComment=Local server: Apache, MySQL, PHP, Node.js and Mailpit\nExec=\"{}\"\nIcon={}\nTerminal=false\nCategories=Development;\n",
        exe.display(),
        icon.display()
    )
}

/// Start Ninho with the session (Linux: ~/.config/autostart; macOS: a LaunchAgent), or not.
pub fn set_run_at_startup(on: bool) {
    let exe = std::env::current_exe().unwrap_or_default();
    if cfg!(target_os = "macos") {
        let file = home().join("Library/LaunchAgents/io.github.phmrmkv.ninho.plist");
        if on {
            let plist = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>io.github.phmrmkv.ninho</string><key>ProgramArguments</key><array><string>{}</string></array><key>RunAtLoad</key><true/></dict></plist>\n",
                exe.display()
            );
            if let Some(d) = file.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(file, plist);
        } else {
            let _ = std::fs::remove_file(file);
        }
        return;
    }
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
    let file = config.join("autostart").join("ninho.desktop");
    if on {
        let _ = std::fs::create_dir_all(config.join("autostart"));
        let _ = std::fs::write(file, desktop_entry(&exe, Path::new("ninho")));
    } else {
        let _ = std::fs::remove_file(file);
    }
}

fn first_line(mut c: Command) -> Option<String> {
    c.stdin(Stdio::null()).stderr(Stdio::null());
    let out = c.output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().trim_end_matches('/').to_string();
    (out.status.success() && !p.is_empty()).then_some(p)
}

/// The desktop's "choose a folder" dialog (zenity or kdialog on Linux, the Finder's on macOS). Blocks: call it on a thread.
pub fn pick_folder(start: &str) -> Option<String> {
    if cfg!(target_os = "macos") {
        let mut c = Command::new("osascript");
        c.args(["-e", "POSIX path of (choose folder)"]);
        return first_line(c);
    }
    let mut z = Command::new("zenity");
    z.args(["--file-selection", "--directory", &format!("--filename={start}/")]);
    if let Ok(true) = which_runs("zenity") {
        return first_line(z);
    }
    let mut k = Command::new("kdialog");
    k.args(["--getexistingdirectory", start]);
    first_line(k)
}

/// The desktop's "open file" dialog for a .sql file.
pub fn pick_sql_file() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let mut c = Command::new("osascript");
        c.args(["-e", "POSIX path of (choose file)"]);
        return first_line(c).map(PathBuf::from);
    }
    if let Ok(true) = which_runs("zenity") {
        let mut z = Command::new("zenity");
        z.args(["--file-selection", "--file-filter=SQL | *.sql"]);
        return first_line(z).map(PathBuf::from);
    }
    let mut k = Command::new("kdialog");
    k.args(["--getopenfilename", ".", "*.sql"]);
    first_line(k).map(PathBuf::from)
}

/// Is `prog` on PATH?
fn which_runs(prog: &str) -> Result<bool, String> {
    Ok(find_on_path(prog).is_some())
}

/// The first `name` in PATH that is a file.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(name)).find(|f| f.is_file()))
}

/// Fonts for the writing systems egui does not ship (CJK, Devanagari), from the usual Linux and macOS places.
pub fn font_files(script: &str) -> Vec<PathBuf> {
    let names: &[&str] = match (script, cfg!(target_os = "macos")) {
        ("ja", true) => &["/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc", "/Library/Fonts/Arial Unicode.ttf"],
        ("ko", true) => &["/System/Library/Fonts/AppleSDGothicNeo.ttc", "/Library/Fonts/Arial Unicode.ttf"],
        ("zh-CN" | "zh-TW", true) => &["/System/Library/Fonts/Hiragino Sans GB.ttc", "/System/Library/Fonts/STHeiti Light.ttc", "/Library/Fonts/Arial Unicode.ttf"],
        ("hi", true) => &["/System/Library/Fonts/Kohinoor.ttc", "/Library/Fonts/Arial Unicode.ttf"],
        ("ja" | "ko" | "zh-CN" | "zh-TW", false) => &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-sans-cjk-vf-fonts/NotoSansCJK-VF.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        ],
        ("hi", false) => &[
            "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSansDevanagari-Regular.ttf",
            "/usr/share/fonts/truetype/lohit-devanagari/Lohit-Devanagari.ttf",
        ],
        _ => &[],
    };
    names.iter().map(PathBuf::from).collect()
}

// ---------------------------------------------------------------- one Ninho at a time

/// A local socket in the user's runtime folder: the first Ninho listens on it, a second one connects, says "show"
/// and quits. A socket left by a crash is noticed (nobody answers) and replaced.
fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()).unwrap_or_else(std::env::temp_dir);
    let user = std::env::var("USER").unwrap_or_default();
    dir.join(if user.is_empty() { "ninho.sock".to_string() } else { format!("ninho-{user}.sock") })
}

static LISTENER: std::sync::Mutex<Option<std::os::unix::net::UnixListener>> = std::sync::Mutex::new(None);

/// True when another Ninho is already open: it was asked to show its window, and this copy should quit.
/// `wait`: right after an update the old copy is still closing; wait up to 30 s for it instead of quitting.
pub fn already_running(wait: bool) -> bool {
    use std::io::Write;
    use std::os::unix::net::{UnixListener, UnixStream};
    let path = socket_path();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match UnixStream::connect(&path) {
            Ok(mut s) => {
                if wait && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    continue;
                }
                let _ = s.write_all(b"show\n");
                return true;
            }
            Err(_) => break,
        }
    }
    let _ = std::fs::remove_file(&path);
    if let Ok(l) = UnixListener::bind(&path) {
        *LISTENER.lock().unwrap() = Some(l);
    }
    false
}

/// In the first copy: a thread that waits for "show yourself" and raises `flag` (the UI then shows the window).
pub fn listen(flag: std::sync::Arc<std::sync::atomic::AtomicBool>, ctx: eframe::egui::Context) {
    let Some(l) = LISTENER.lock().unwrap().take() else { return };
    std::thread::spawn(move || {
        for s in l.incoming() {
            if s.is_ok() {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                ctx.request_repaint();
            }
        }
    });
}

/// Called when Ninho closes: the socket file goes too.
pub fn release_single() {
    let _ = std::fs::remove_file(socket_path());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ss_lines() {
        let t = "LISTEN 0      151        127.0.0.1:3306       0.0.0.0:*    users:((\"mysqld\",pid=1234,fd=23))\n\
                 LISTEN 0      511          [::]:8080           [::]:*    users:((\"httpd\",pid=50,fd=4),(\"httpd\",pid=51,fd=4))\n\
                 LISTEN 0      4096   127.0.0.53%lo:53         0.0.0.0:*\n";
        let l = parse_ss(t);
        assert_eq!(l.len(), 4);
        assert_eq!((l[0].port, l[0].pid, l[0].program.as_str()), (53, 0, ""));
        assert_eq!(l[0].addrs, vec!["127.0.0.53"]);
        assert_eq!((l[1].port, l[1].pid, l[1].program.as_str()), (3306, 1234, "mysqld"));
        assert_eq!((l[2].port, l[2].pid, l[2].program.as_str()), (8080, 50, "httpd"));
        assert_eq!(l[2].addrs, vec!["::"]);
        assert_eq!(l[3].pid, 51);
    }

    #[test]
    fn lsof_lines() {
        let t = "p321\ncmysqld\nf20\nn127.0.0.1:3306\nf21\nn[::1]:3306\np9\ncredis-server\nf6\nn*:6379\n";
        let l = parse_lsof(t);
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].port, l[0].pid, l[0].program.as_str()), (3306, 321, "mysqld"));
        assert_eq!(l[0].addrs, vec!["127.0.0.1", "::1"]);
        assert_eq!((l[1].port, l[1].program.as_str()), (6379, "redis-server"));
        assert_eq!(l[1].addrs, vec!["0.0.0.0"]);
    }

    #[test]
    fn offsets_and_locales() {
        assert_eq!(parse_utc_offset("-0300\n"), Some(-180));
        assert_eq!(parse_utc_offset("+0530"), Some(330));
        assert_eq!(parse_utc_offset("x"), None);
        assert_eq!(locale_code("pt_BR.UTF-8").as_deref(), Some("pt-BR"));
        assert_eq!(locale_code("zh_TW"), Some("zh-TW".into()));
        assert_eq!(locale_code("C.UTF-8"), None);
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
    }
}
