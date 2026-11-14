//! Command line, for scripts and terminals:
//!   Ninho.exe status [--json]        every server with port, state and PID
//!   Ninho.exe start [id...]          start the given servers (default: all active ones)
//!   Ninho.exe stop [id...]           stop them (default: all active ones)
//!   Ninho.exe reload                 rewrite the .test sites and restart Apache if it runs
//!   Ninho.exe --install tool...      download the newest stable tools (php, mysql, nodejs, mailpit, redis, apache)
//! Ninho is a window program, so it borrows the console of the terminal that called it to print.
use std::sync::Arc;

use crate::services::{self, Shared, State};
use crate::settings::Settings;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn AttachConsole(process_id: u32) -> i32;
}

/// Print to the calling terminal (when there is one).
fn attach() {
    #[cfg(windows)]
    unsafe {
        // ATTACH_PARENT_PROCESS
        AttachConsole(u32::MAX);
    }
}

fn shared_for(s: &Settings) -> Arc<Shared> {
    let shared = Shared::new();
    {
        let mut ports = shared.ports.lock().unwrap();
        for c in &s.services {
            ports.insert(c.id.clone(), (c.port, services::image(&c.key)));
        }
    }
    shared.refresh();
    shared
}

fn pick(s: &Settings, args: &[String]) -> Vec<String> {
    if args.is_empty() {
        s.services.iter().filter(|c| c.enabled).map(|c| c.id.clone()).collect()
    } else {
        args.iter().filter(|a| s.services.iter().any(|c| &c.id == *a)).cloned().collect()
    }
}

fn state_word(st: State) -> &'static str {
    match st {
        State::Running => "running",
        State::Stopped => "stopped",
        State::Busy => "busy",
        State::Foreign => "port-taken",
    }
}

/// Runs a command when the arguments ask for one; returns false to open the window instead.
pub fn run(args: &[String]) -> bool {
    let Some(cmd) = args.get(1).map(String::as_str) else { return false };
    if !matches!(cmd, "status" | "start" | "stop" | "reload" | "help" | "--help") {
        return false;
    }
    attach();
    let s = Settings::load();
    let rest: Vec<String> = args[2..].iter().filter(|a| !a.starts_with("--")).cloned().collect();
    let json = args.iter().any(|a| a == "--json");
    let shared = shared_for(&s);
    match cmd {
        "status" => {
            let rows: Vec<serde_json::Value> = s
                .services
                .iter()
                .map(|c| {
                    let st = shared.state(&c.id);
                    serde_json::json!({
                        "id": c.id, "name": c.title(), "port": c.port, "enabled": c.enabled,
                        "state": state_word(st), "version": services::version_label(&s, &c.id),
                        "pids": services::pids(&shared, &s, &c.id),
                    })
                })
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
            } else {
                println!("{:<12} {:<7} {:<11} {:<8} VERSION", "SERVER", "PORT", "STATE", "PID");
                for r in &rows {
                    let pid = r["pids"].as_array().and_then(|a| a.first()).map(|p| p.to_string()).unwrap_or_else(|| "-".into());
                    println!(
                        "{:<12} {:<7} {:<11} {:<8} {}",
                        r["id"].as_str().unwrap_or(""),
                        r["port"].as_u64().unwrap_or(0),
                        r["state"].as_str().unwrap_or(""),
                        pid,
                        r["version"].as_str().unwrap_or("not installed")
                    );
                }
            }
        }
        "start" => {
            for id in pick(&s, &rest) {
                if services::version_dir(&s, &id).is_some() {
                    services::start(&shared, &s, &id);
                }
            }
        }
        "stop" => {
            for id in pick(&s, &rest) {
                if shared.state(&id) == State::Running {
                    services::stop(&shared, &s, &id);
                }
            }
        }
        "reload" => {
            crate::sites::refresh(&shared, &s, true);
            if shared.state("apache") == State::Running {
                services::stop(&shared, &s, "apache");
                services::start(&shared, &s, "apache");
            }
        }
        _ => {
            println!("Ninho {}: status [--json] | start [id...] | stop [id...] | reload | --install tool...", env!("CARGO_PKG_VERSION"));
        }
    }
    if !json {
        for line in shared.log.lock().unwrap().iter() {
            println!("{line}");
        }
    }
    true
}
