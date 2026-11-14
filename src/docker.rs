//! Docker containers in the Ports tab. When `docker` is on the PATH and its daemon answers, `docker ps` tells which
//! container publishes which port, so the Ports list can say "Docker: web (nginx:alpine)" instead of the bare
//! com.docker.backend.exe / wslrelay.exe that Windows reports, and offer to stop the container. Without Docker (no
//! program, no daemon, a hung answer) the list is just empty and nothing is shown or logged.
use std::process::Command;
use std::time::Duration;

use crate::ports::output_timeout;

/// One published port of a container: host side → container side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    pub host_ip: String,
    pub host_port: u16,
    pub container_port: u16,
    pub proto: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub ports: Vec<Published>,
}

impl Container {
    /// "Docker: web (nginx:alpine)".
    pub fn label(&self) -> String {
        format!("Docker: {} ({})", self.name, self.image)
    }
}

/// `"0.0.0.0:8080->80/tcp, :::8080->80/tcp, 5432/tcp"` → the published ports (the unpublished "5432/tcp" is skipped).
/// Ranges ("0.0.0.0:8000-8002->8000-8002/tcp") become one entry per port.
pub fn parse_ports(text: &str) -> Vec<Published> {
    let mut out = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let Some((left, right)) = part.split_once("->") else { continue };
        let (cports, proto) = right.split_once('/').unwrap_or((right, "tcp"));
        // Left: "0.0.0.0:8080", ":::8080", "[::1]:8080", "127.0.0.1:8000-8002" (an address and a port or a range).
        let Some((ip, hports)) = left.rsplit_once(':') else { continue };
        let (Some(h), Some(c)) = (range(hports), range(cports)) else { continue };
        for (i, hp) in (h.0..=h.1).enumerate() {
            let cp = c.0.saturating_add(i as u16).min(c.1.max(c.0));
            out.push(Published { host_ip: ip.trim_matches(['[', ']']).to_string(), host_port: hp, container_port: cp, proto: proto.to_string() });
        }
    }
    out
}

/// "8080" → (8080, 8080); "8000-8002" → (8000, 8002).
fn range(t: &str) -> Option<(u16, u16)> {
    match t.split_once('-') {
        Some((a, b)) => {
            let (a, b) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
            (a <= b).then_some((a, b))
        }
        None => t.trim().parse().ok().map(|p| (p, p)),
    }
}

/// The output of `docker ps --format "{{json .}}"`: one JSON object per line. Lines that are not JSON are ignored.
pub fn parse_ps(text: &str) -> Vec<Container> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok())
        .filter_map(|v| {
            let text = |k: &str| v[k].as_str().unwrap_or("").to_string();
            let id = text("ID");
            if id.is_empty() {
                return None;
            }
            // A container can have several names ("a,b"): the first one.
            let name = text("Names").split(',').next().unwrap_or("").to_string();
            Some(Container { id, name, image: text("Image"), ports: parse_ports(&text("Ports")) })
        })
        .collect()
}

/// Running containers; empty when Docker is missing, stopped or slow (4 seconds at most).
pub fn running() -> Vec<Container> {
    let mut c = Command::new("docker");
    c.args(["ps", "--format", "{{json .}}"]);
    match output_timeout(c, Duration::from_secs(4)) {
        Some(o) if o.status.success() => parse_ps(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// `docker stop <id>`; the container is stopped, not deleted.
pub fn stop(id: &str) -> Result<(), String> {
    let mut c = Command::new("docker");
    c.args(["stop", id]);
    match output_timeout(c, Duration::from_secs(40)) {
        Some(o) if o.status.success() => Ok(()),
        Some(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        None => Err("docker did not answer".into()),
    }
}

/// Programs Windows reports for a published port of Docker (Docker Desktop's backend and WSL's relay).
pub fn is_relay(program: &str) -> bool {
    let p = program.to_ascii_lowercase();
    p.contains("docker") || p == "wslrelay.exe" || p == "vpnkit.exe" || p == "wslhost.exe"
}

/// The container that publishes a host port, if any.
pub fn owner(list: &[Container], port: u16) -> Option<&Container> {
    list.iter().find(|c| c.ports.iter().any(|p| p.host_port == port && p.proto == "tcp"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_mapping_text() {
        let p = parse_ports("0.0.0.0:8080->80/tcp, :::8080->80/tcp");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0], Published { host_ip: "0.0.0.0".into(), host_port: 8080, container_port: 80, proto: "tcp".into() });
        assert_eq!(p[1].host_ip, "::");
        assert_eq!(p[1].host_port, 8080);
    }

    #[test]
    fn unpublished_ports_ranges_and_loopback() {
        let p = parse_ports("127.0.0.1:5433->5432/tcp, 6379/tcp, [::1]:9000->9000/udp");
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].host_ip.as_str(), p[0].host_port, p[0].container_port), ("127.0.0.1", 5433, 5432));
        assert_eq!((p[1].host_ip.as_str(), p[1].proto.as_str()), ("::1", "udp"));
        let r = parse_ports("0.0.0.0:8000-8002->9000-9002/tcp");
        assert_eq!(r.iter().map(|x| (x.host_port, x.container_port)).collect::<Vec<_>>(), vec![(8000, 9000), (8001, 9001), (8002, 9002)]);
        assert!(parse_ports("").is_empty());
        assert!(parse_ports("garbage->x/tcp").is_empty());
    }

    const PS: &str = concat!(
        r#"{"Command":"\"/docker-entrypoint.…\"","ID":"3f1c2d9a7b10","Image":"nginx:alpine","Names":"web","Ports":"0.0.0.0:8080->80/tcp, :::8080->80/tcp","State":"running","Status":"Up 2 hours"}"#,
        "\n",
        r#"{"ID":"aa11bb22cc33","Image":"postgres:17","Names":"db,legacy-alias","Ports":"127.0.0.1:5433->5432/tcp","State":"running"}"#,
        "\n",
        r#"{"ID":"dd44ee55ff66","Image":"busybox","Names":"idle","Ports":"","State":"running"}"#,
        "\n",
        "not json at all\n",
    );

    #[test]
    fn docker_ps_json() {
        let list = parse_ps(PS);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "web");
        assert_eq!(list[0].label(), "Docker: web (nginx:alpine)");
        assert_eq!(list[1].name, "db");
        assert!(list[2].ports.is_empty());
        assert_eq!(owner(&list, 8080).map(|c| c.id.as_str()), Some("3f1c2d9a7b10"));
        assert_eq!(owner(&list, 5433).map(|c| c.name.as_str()), Some("db"));
        assert!(owner(&list, 5432).is_none());
    }

    #[test]
    fn docker_relay_programs() {
        assert!(is_relay("com.docker.backend.exe"));
        assert!(is_relay("wslrelay.exe"));
        assert!(!is_relay("mysqld.exe"));
    }
}
