//! Every TCP port something is listening on in this PC, with the program behind it (netstat and tasklist on
//! Windows, ss on Linux, lsof on macOS: see platform).
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use crate::services::hide_window;

#[derive(Clone, Debug)]
pub struct Listener {
    pub port: u16,
    /// Address it listens on: 127.0.0.1 (only this PC), 0.0.0.0 / [::] (the whole network)...
    pub addrs: Vec<String>,
    pub pid: u32,
    /// Program name (mysqld.exe...), or empty when the system does not tell.
    pub program: String,
}

/// Run a program with a time limit (hidden window, no input): its output, or None when it cannot start, fails to
/// finish in time (it is then ended) or cannot be read.
pub fn output_timeout(mut c: Command, limit: Duration) -> Option<Output> {
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_window(&mut c);
    let child = c.spawn().ok()?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(limit) {
        Ok(Ok(o)) => Some(o),
        Ok(Err(_)) => None,
        Err(_) => {
            let _ = kill(pid);
            None
        }
    }
}

/// Ports in LISTENING state (TCP over IPv4 and IPv6), one entry per port and program, sorted by port.
pub fn listening() -> Vec<Listener> {
    crate::platform::listening()
}

/// Who is listening on one port, if anyone.
pub fn owner(list: &[Listener], port: u16) -> Option<&Listener> {
    list.iter().find(|l| l.port == port)
}

/// Stop one program by its PID (and the programs it started).
pub fn kill(pid: u32) -> Result<(), String> {
    crate::platform::kill(pid)
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
        135 | 445 | 139 | 2179 | 2869 | 5040 | 5357 | 7680 | 13508 | 49664..=49699 if cfg!(windows) => "Windows",
        _ => "",
    }
}

/// Parts of the system itself: stopping them crashes or locks the PC, so the Ports tab never offers it.
pub fn is_system(program: &str, pid: u32) -> bool {
    crate::platform::is_system(program, pid)
}
