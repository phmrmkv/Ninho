//! `Ninho.exe mcp`: a Model Context Protocol server over stdio, so AI coding agents (Claude Code, Cursor...)
//! can see and control the local servers. JSON-RPC 2.0, one message per line (MCP stdio transport).
//! stdout carries ONLY protocol messages; anything else goes to stderr. No AttachConsole here: an agent
//! launches us with pipes, and those are already our std handles.
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::services::{self, Shared, State};
use crate::settings::Settings;
use crate::sites;

const PROTOCOL: &str = "2025-06-18";
const DEFAULT_LINES: usize = 100;
const MAX_LINES: usize = 1000;
/// Reading more than this from the end of a log is never needed for MAX_LINES lines.
const MAX_TAIL_BYTES: u64 = 4 * 1024 * 1024;

/// Serve MCP on stdin/stdout until stdin closes.
pub fn serve() {
    let s = Settings::load();
    let shared = shared_for(&s);
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(msg) => handle(&msg, &s, &shared),
            Err(e) => {
                eprintln!("ninho mcp: bad JSON: {e}");
                Some(error(Value::Null, -32700, "Parse error"))
            }
        };
        if let Some(r) = reply {
            // One message per line; serde_json never emits raw newlines.
            if writeln!(out, "{r}").and_then(|_| out.flush()).is_err() {
                break; // the agent went away
            }
        }
    }
}

/// Same wiring as the command line: watch every configured port, then look at them once.
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

fn error(id: Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// One request in, at most one response out. Notifications (no id) never get a reply.
pub fn handle(msg: &Value, s: &Settings, shared: &Arc<Shared>) -> Option<Value> {
    let id = msg.get("id").cloned()?;
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        // A response to us (we never send requests) needs no answer; anything else is malformed.
        return msg.get("result").or(msg.get("error")).is_none().then(|| error(id, -32600, "Invalid request"));
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => ok(
            id,
            json!({
                "protocolVersion": PROTOCOL,
                "serverInfo": {"name": "ninho", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"tools": {}},
                "instructions": "Ninho runs local dev servers (Apache, MySQL, PHP, Node.js...). Call status first, then start/stop/restart by server id.",
            }),
        ),
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({"tools": tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            if !tools().iter().any(|t| t["name"] == name) {
                return Some(error(id, -32602, &format!("Unknown tool: {name}")));
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            // Tool failures are results with isError, so the agent can read and react to them.
            ok(id, match call(name, &args, s, shared) {
                Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
                Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
            })
        }
        _ => error(id, -32601, &format!("Method not found: {method}")),
    })
}

fn tools() -> Vec<Value> {
    let ids = json!({"type": "array", "items": {"type": "string"}});
    vec![
        json!({"name": "status", "description": "State of every server: id, name, port, state (running/stopped/busy/port-taken), version and PIDs.",
               "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "start", "description": "Start servers by id; without ids, starts every enabled server. Waits until each answers on its port.",
               "inputSchema": {"type": "object", "properties": {"ids": ids.clone()}}}),
        json!({"name": "stop", "description": "Stop servers by id; without ids, stops every enabled server.",
               "inputSchema": {"type": "object", "properties": {"ids": ids}}}),
        json!({"name": "restart", "description": "Stop and start one server (for example after changing its config).",
               "inputSchema": {"type": "object", "properties": {"id": {"type": "string", "description": "Server id, e.g. apache or mysql-2"}}, "required": ["id"]}}),
        json!({"name": "list_sites", "description": "Local sites (folders of the document root) with their URL and project kind.",
               "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "read_log", "description": "Last lines of a server's log file.",
               "inputSchema": {"type": "object", "properties": {
                   "id": {"type": "string", "description": "Server id, e.g. apache or mysql"},
                   "lines": {"type": "integer", "minimum": 1, "maximum": MAX_LINES, "default": DEFAULT_LINES}},
                   "required": ["id"]}}),
    ]
}

fn call(name: &str, args: &Value, s: &Settings, shared: &Arc<Shared>) -> Result<String, String> {
    match name {
        "status" => {
            shared.refresh();
            Ok(pretty(&json!(status_rows(s, shared))))
        }
        "start" | "stop" => {
            let ids = pick(s, args)?;
            let mut log = Vec::new();
            for id in &ids {
                log.extend(act(name, id, s, shared));
            }
            Ok(summary(s, shared, &ids, log))
        }
        "restart" => {
            let id = required_id(s, args)?;
            let mut log = act("stop", &id, s, shared);
            log.extend(act("start", &id, s, shared));
            Ok(summary(s, shared, &[id], log))
        }
        "list_sites" => {
            let rows: Vec<Value> = sites::list(s)
                .iter()
                .map(|x| {
                    let kind = sites::kind(&Path::new(&s.doc_root).join(&x.folder));
                    json!({"folder": x.folder, "url": sites::url(s, x), "kind": if kind.is_empty() { Value::Null } else { json!(kind) }})
                })
                .collect();
            Ok(pretty(&json!(rows)))
        }
        "read_log" => {
            let id = required_id(s, args)?;
            let n = match args.get("lines") {
                None | Some(Value::Null) => DEFAULT_LINES,
                Some(v) => match v.as_u64() {
                    Some(n) if (1..=MAX_LINES as u64).contains(&n) => n as usize,
                    _ => return Err(format!("lines must be an integer from 1 to {MAX_LINES}")),
                },
            };
            let file = services::log_file(s, &id).ok_or_else(|| format!("{id} has no log file (not installed, never started, or it writes none)"))?;
            let text = tail(&file, n).map_err(|e| format!("could not read {}: {e}", file.display()))?;
            Ok(if text.is_empty() { "(log is empty)".to_string() } else { text })
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn status_rows(s: &Settings, shared: &Shared) -> Vec<Value> {
    s.services
        .iter()
        .map(|c| {
            json!({
                "id": c.id, "name": c.title(), "port": c.port, "enabled": c.enabled,
                "state": state_word(shared.state(&c.id)), "version": services::version_label(s, &c.id),
                "pids": services::pids(shared, s, &c.id),
            })
        })
        .collect()
}

fn state_word(st: State) -> &'static str {
    match st {
        State::Running => "running",
        State::Stopped => "stopped",
        State::Busy => "busy",
        State::Foreign => "port-taken",
    }
}

/// `Settings::svc` panics on an unknown id, so every id from the agent is checked first.
fn known(s: &Settings, id: &str) -> Result<(), String> {
    if s.services.iter().any(|c| c.id == id) {
        Ok(())
    } else {
        let all: Vec<&str> = s.services.iter().map(|c| c.id.as_str()).collect();
        Err(format!("Unknown server id \"{id}\". Known ids: {}", all.join(", ")))
    }
}

fn required_id(s: &Settings, args: &Value) -> Result<String, String> {
    let id = args.get("id").and_then(Value::as_str).ok_or("missing required argument: id (string)")?;
    known(s, id)?;
    Ok(id.to_string())
}

/// The ids asked for; none means every enabled server (same rule as the command line).
fn pick(s: &Settings, args: &Value) -> Result<Vec<String>, String> {
    match args.get("ids") {
        None | Some(Value::Null) => Ok(s.services.iter().filter(|c| c.enabled).map(|c| c.id.clone()).collect()),
        Some(Value::Array(a)) => {
            let mut out = Vec::new();
            for v in a {
                let id = v.as_str().ok_or("ids must be an array of strings")?;
                known(s, id)?;
                out.push(id.to_string());
            }
            Ok(out)
        }
        Some(_) => Err("ids must be an array of strings".into()),
    }
}

/// Run one start or stop and return what Ninho said about it. The log is ours alone here, so clearing it is safe.
fn act(what: &str, id: &str, s: &Settings, shared: &Arc<Shared>) -> Vec<String> {
    shared.log.lock().unwrap().clear();
    if what == "start" {
        if services::version_dir(s, id).is_some() {
            services::start(shared, s, id);
        } else {
            shared.say(format!("{id}: not installed, skipped"));
        }
    } else if shared.state(id) == State::Running {
        services::stop(shared, s, id);
    } else {
        shared.say(format!("{id}: not running, nothing to stop"));
    }
    shared.log.lock().unwrap().clone()
}

fn summary(s: &Settings, shared: &Arc<Shared>, ids: &[String], log: Vec<String>) -> String {
    shared.refresh();
    let rows: Vec<Value> = status_rows(s, shared).into_iter().filter(|r| ids.iter().any(|i| r["id"] == i.as_str())).collect();
    pretty(&json!({"servers": rows, "log": log}))
}

/// Last `n` lines of a file, reading only its end so a huge log stays cheap.
fn tail(path: &Path, n: usize) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(MAX_TAIL_BYTES);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    // Logs may hold non-UTF-8 bytes, and a cut at the start may split a character.
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // probably a partial line
    }
    let from = lines.len().saturating_sub(n);
    Ok(lines[from..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    }

    fn run(msg: Value, s: &Settings) -> Option<Value> {
        handle(&msg, s, &Shared::new())
    }

    /// Settings whose root is a fresh temp folder holding a fake Apache with a 5-line error log.
    fn fake(tag: &str) -> Settings {
        let root = std::env::temp_dir().join(format!("ninho-mcp-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let d = services::def("apache");
        let ver = root.join("bin").join(d.dir).join("httpd-2.4.0");
        let exe = ver.join(d.exe);
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        std::fs::create_dir_all(ver.join("logs")).unwrap();
        std::fs::write(ver.join("logs").join("error.log"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        Settings { root: root.to_string_lossy().into_owned(), doc_root: root.join("www").to_string_lossy().into_owned(), ..Settings::default() }
    }

    fn text(r: &Value) -> String {
        r["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
    }

    #[test]
    fn initialize_answers_with_protocol_and_server_info() {
        let r = run(req("initialize", json!({"protocolVersion": "2025-06-18"})), &Settings::default()).unwrap();
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["serverInfo"]["name"], "ninho");
        assert_eq!(r["result"]["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
        assert!(r["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn tools_list_has_every_tool_with_an_object_schema() {
        let r = run(req("tools/list", Value::Null), &Settings::default()).unwrap();
        let list = r["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = list.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["status", "start", "stop", "restart", "list_sites", "read_log"]);
        for t in list {
            assert_eq!(t["inputSchema"]["type"], "object");
            assert!(t["description"].is_string());
        }
    }

    #[test]
    fn unknown_method_is_32601_and_ping_works() {
        let r = run(req("nope/nothing", Value::Null), &Settings::default()).unwrap();
        assert_eq!(r["error"]["code"], -32601);
        assert_eq!(r["id"], 1);
        assert!(run(req("ping", Value::Null), &Settings::default()).unwrap()["result"].is_object());
    }

    #[test]
    fn notifications_get_no_reply() {
        let n = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(run(n, &Settings::default()).is_none());
        let unknown = json!({"jsonrpc": "2.0", "method": "notifications/whatever"});
        assert!(run(unknown, &Settings::default()).is_none());
    }

    #[test]
    fn unknown_tool_is_invalid_params() {
        let r = run(req("tools/call", json!({"name": "format_disk"})), &Settings::default()).unwrap();
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn read_log_validates_arguments() {
        let s = fake("args");
        let call = |args: Value| run(req("tools/call", json!({"name": "read_log", "arguments": args})), &s).unwrap();
        for bad in [json!({}), json!({"id": "nope"}), json!({"id": "apache", "lines": 0}), json!({"id": "apache", "lines": 1001}), json!({"id": "apache", "lines": "5"})] {
            let r = call(bad.clone());
            assert_eq!(r["result"]["isError"], true, "{bad}");
        }
        assert!(text(&call(json!({"id": "nope"}))).contains("Known ids"));
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn read_log_returns_the_tail() {
        let s = fake("tail");
        let call = |args: Value| run(req("tools/call", json!({"name": "read_log", "arguments": args})), &s).unwrap();
        let r = call(json!({"id": "apache", "lines": 2}));
        assert!(r["result"]["isError"].is_null());
        assert_eq!(text(&r), "four\nfive");
        assert_eq!(text(&call(json!({"id": "apache"}))), "one\ntwo\nthree\nfour\nfive");
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn start_rejects_unknown_ids_without_panicking() {
        let r = run(req("tools/call", json!({"name": "start", "arguments": {"ids": ["ghost"]}})), &Settings::default()).unwrap();
        assert_eq!(r["result"]["isError"], true);
    }

    #[test]
    fn tail_keeps_only_the_last_lines() {
        let f = std::env::temp_dir().join(format!("ninho-mcp-tail-{}.log", std::process::id()));
        std::fs::write(&f, "a\nb\nc").unwrap();
        assert_eq!(tail(&f, 2).unwrap(), "b\nc");
        assert_eq!(tail(&f, 50).unwrap(), "a\nb\nc");
        let _ = std::fs::remove_file(&f);
    }
}
