//! PHP extensions in php.ini: list them and switch one on or off (the `extension=` lines, with or without `;`).
use std::path::Path;

/// Extension name of a line like `extension=mysqli`, `;extension=php_gd.dll` or `extension = "zip"`; and whether it is on.
fn parse(line: &str) -> Option<(String, bool)> {
    let t = line.trim();
    let (on, rest) = match t.strip_prefix(';') {
        Some(r) => (false, r.trim_start()),
        None => (true, t),
    };
    let rest = rest.strip_prefix("extension")?.trim_start().strip_prefix('=')?;
    let mut name = rest.trim().trim_matches('"').to_string();
    // Old style "php_gd.dll" → "gd". Lines that are prose, not a setting, have spaces.
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    if let Some(n) = name.strip_prefix("php_") {
        name = n.to_string();
    }
    if let Some(n) = name.strip_suffix(".dll") {
        name = n.to_string();
    }
    Some((name, on))
}

/// Every extension php.ini mentions, alphabetically, with its state. An extension listed twice counts as on if any
/// of its lines is on.
pub fn list(ini: &Path) -> Vec<(String, bool)> {
    let Ok(text) = std::fs::read_to_string(ini) else { return Vec::new() };
    let mut out: Vec<(String, bool)> = Vec::new();
    for (name, on) in text.lines().filter_map(parse) {
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some(e) => e.1 |= on,
            None => out.push((name, on)),
        }
    }
    out.sort();
    out
}

/// Switch one extension on or off, keeping every other line exactly as it was.
pub fn set(ini: &Path, ext: &str, on: bool) -> std::io::Result<()> {
    let text = std::fs::read_to_string(ini)?;
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut done = false;
    let lines: Vec<String> = text
        .lines()
        .map(|l| match parse(l) {
            Some((name, _)) if name == ext && !done => {
                done = true;
                if on { format!("extension={ext}") } else { format!(";extension={ext}") }
            }
            _ => l.to_string(),
        })
        .collect();
    let mut out = lines.join(nl) + nl;
    if !done && on {
        out.push_str(&format!("extension={ext}{nl}"));
    }
    std::fs::write(ini, out)
}
