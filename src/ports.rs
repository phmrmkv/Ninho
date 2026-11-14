//! Every TCP port something is listening on in this PC, with the program behind it (netstat and tasklist on
//! Windows, ss on Linux, lsof on macOS: see platform).
#[derive(Clone, Debug)]
pub struct Listener {
    pub port: u16,
    /// Address it listens on: 127.0.0.1 (only this PC), 0.0.0.0 / [::] (the whole network)...
    pub addrs: Vec<String>,
    pub pid: u32,
    /// Program name (mysqld.exe...), or empty when the system does not tell.
    pub program: String,
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
