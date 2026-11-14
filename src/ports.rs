//! Every TCP port something is listening on in this PC (netstat), with the program behind it (tasklist).
use std::collections::HashMap;
use std::process::Command;

use crate::services::hide_window;

#[derive(Clone, Debug)]
pub struct Listener {
    pub port: u16,
    /// Address it listens on: 127.0.0.1 (only this PC), 0.0.0.0 / [::] (the whole network)...
    pub addrs: Vec<String>,
    pub pid: u32,
    /// Program name (mysqld.exe...), or empty when Windows does not tell.
    pub program: String,
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

/// Who is listening on one port, if anyone.
pub fn owner(list: &[Listener], port: u16) -> Option<&Listener> {
    list.iter().find(|l| l.port == port)
}

/// Stop one program by its PID (and the programs it started).
pub fn kill(pid: u32) -> Result<(), String> {
    let mut c = Command::new("taskkill");
    c.args(["/F", "/T", "/PID", &pid.to_string()]);
    hide_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

/// A short, friendly guess of what usually lives on a port.
pub fn hint(port: u16) -> &'static str {
    match port {
        80 | 8080 | 8000 | 8008 | 8888 => "web",
        443 | 8443 => "web (https)",
        3306 | 3307 | 3308 => "MySQL",
        33060 => "MySQL X",
        5432 => "PostgreSQL",
        6379 => "Redis",
        11211 => "Memcached",
        1025 | 8025 => "Mailpit",
        3000 | 3001 | 3333 | 3334 | 3335 | 5173 | 4200 => "Node / Vite",
        8188 => "ComfyUI",
        27017 => "MongoDB",
        135 | 445 | 139 | 2179 | 2869 | 5040 | 5357 | 7680 | 13508 | 49664..=49699 => "Windows",
        _ => "",
    }
}

/// Parts of Windows itself: stopping them crashes or locks the PC, so the Ports tab never offers it.
pub fn is_system(program: &str, pid: u32) -> bool {
    const CORE: &[&str] = &[
        "system", "svchost.exe", "lsass.exe", "wininit.exe", "services.exe", "csrss.exe", "smss.exe", "winlogon.exe",
        "spoolsv.exe", "vmms.exe", "lsaiso.exe", "dwm.exe", "explorer.exe", "memory compression", "registry",
    ];
    pid <= 4 || CORE.contains(&program.to_ascii_lowercase().as_str())
}
