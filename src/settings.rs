use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::services::{SERVICES, def};

/// One server the person runs. The first of each kind has `id == key` ("mysql", "apache"...); extra ones
/// ("mysql-2", "redis-2"...) are added with "New server" and run side by side on their own port.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ServiceCfg {
    /// Kind of server (a key of SERVICES).
    pub key: String,
    /// Unique name of this server inside Ninho.
    #[serde(default)]
    pub id: String,
    /// Name shown on screen; empty means the kind's name.
    #[serde(default)]
    pub label: String,
    pub enabled: bool,
    pub port: u16,
    /// Version folder chosen by the person; empty means "the newest one".
    #[serde(default)]
    pub version: String,
    /// Extra MySQL: its own data folder. Extra PHP server: the folder it serves. Empty for the first of each kind.
    #[serde(default)]
    pub data_dir: String,
}

impl ServiceCfg {
    pub fn base(key: &str) -> ServiceCfg {
        let d = def(key);
        ServiceCfg {
            key: key.into(),
            id: key.into(),
            label: String::new(),
            enabled: d.default_on,
            port: d.default_port,
            version: String::new(),
            data_dir: String::new(),
        }
    }
    pub fn is_extra(&self) -> bool {
        self.id != self.key
    }
    pub fn title(&self) -> String {
        if self.label.is_empty() { def(&self.key).name.to_string() } else { self.label.clone() }
    }
}

/// What a site in the sites folder runs with when Ninho serves it on its own (Sites tab).
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct SiteCfg {
    pub folder: String,
    /// PHP version folder for this site; empty = the one chosen for the PHP module.
    #[serde(default)]
    pub php: String,
    /// Port of its own PHP server; 0 = not chosen yet.
    #[serde(default)]
    pub port: u16,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Settings {
    pub root: String,
    pub doc_root: String,
    pub hostname_format: String,
    pub auto_vhosts: bool,
    pub start_all_on_launch: bool,
    pub run_at_startup: bool,
    pub language: Lang,
    #[serde(default)]
    pub ignore_list: Vec<String>,
    pub services: Vec<ServiceCfg>,
    #[serde(default)]
    pub sites: Vec<SiteCfg>,
    /// HTTPS for the .test sites (local certificate authority, Apache on 443).
    #[serde(default)]
    pub https: bool,
    /// "light", "dark" or "system" (follows Windows).
    #[serde(default = "system_theme")]
    pub theme: String,
    /// Closing the window keeps Ninho in the tray, next to the clock.
    #[serde(default = "yes")]
    pub close_to_tray: bool,
    /// Open hidden in the tray (with "Run when Windows starts", the servers start quietly).
    #[serde(default)]
    pub start_hidden: bool,
    /// Look for a new Ninho on GitHub when it opens.
    #[serde(default = "yes")]
    pub check_updates: bool,
}

fn yes() -> bool {
    true
}

fn system_theme() -> String {
    "system".into()
}

/// Folder of ninho.exe: Ninho is portable, its settings live beside it.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

impl Default for Settings {
    fn default() -> Self {
        // Portable: the folder of ninho.exe when it holds a bin folder; else an existing local server folder; else
        // the folder of ninho.exe itself (the first "Install" creates bin there).
        let here = exe_dir();
        let root = if here.join("bin").is_dir() {
            here.to_string_lossy().into_owned()
        } else if Path::new(r"C:\laragon\bin").is_dir() {
            r"C:\laragon".to_string()
        } else {
            here.to_string_lossy().into_owned()
        };
        let doc_root = format!(r"{root}\www");
        Settings {
            root,
            doc_root,
            hostname_format: "{name}.test".into(),
            auto_vhosts: true,
            start_all_on_launch: false,
            run_at_startup: false,
            // First start: the language Windows uses.
            language: Lang::system(),
            ignore_list: Vec::new(),
            services: SERVICES.iter().map(|s| ServiceCfg::base(s.key)).collect(),
            sites: Vec::new(),
            https: false,
            theme: system_theme(),
            close_to_tray: true,
            start_hidden: false,
            check_updates: true,
        }
    }
}

impl Settings {
    /// Settings beside ninho.exe (portable).
    pub fn file() -> PathBuf {
        exe_dir().join("ninho.json")
    }

    /// Where older versions kept the settings; read once while ninho.json does not exist yet.
    fn old_file() -> PathBuf {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base).join("Ninho").join("settings.json")
    }

    /// Has Ninho been set up before (a settings file exists)? False on the very first start.
    pub fn exists() -> bool {
        Self::file().is_file() || Self::old_file().is_file()
    }

    pub fn load() -> Settings {
        let text = std::fs::read_to_string(Self::file()).or_else(|_| std::fs::read_to_string(Self::old_file()));
        let mut s: Settings = text
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        // Older files have no id: the first of each kind is the kind itself.
        for c in &mut s.services {
            if c.id.is_empty() {
                c.id = c.key.clone();
            }
        }
        // One base entry per known kind, in the known order, then the extra servers (of known kinds only).
        let mut fixed: Vec<ServiceCfg> = SERVICES
            .iter()
            .map(|d| s.services.iter().find(|c| c.id == d.key).cloned().unwrap_or_else(|| ServiceCfg::base(d.key)))
            .collect();
        for c in &s.services {
            if c.is_extra() && SERVICES.iter().any(|d| d.key == c.key) && !fixed.iter().any(|f| f.id == c.id) {
                fixed.push(c.clone());
            }
        }
        s.services = fixed;
        s
    }

    pub fn save(&self) -> std::io::Result<()> {
        let f = Self::file();
        if let Some(dir) = f.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(f, serde_json::to_string_pretty(self).unwrap_or_default())
    }

    /// The server with this id ("mysql", "mysql-2"...).
    pub fn svc(&self, id: &str) -> &ServiceCfg {
        self.services.iter().find(|c| c.id == id).expect("known server")
    }

    /// A free id for a new server of this kind: "<key>-2", "<key>-3"...
    pub fn next_id(&self, key: &str) -> String {
        (2..).map(|n| format!("{key}-{n}")).find(|id| !self.services.iter().any(|c| &c.id == id)).unwrap()
    }
}

/// Valid TCP ports for a server.
pub fn port_ok(p: u32) -> bool {
    (2..=65535).contains(&p)
}

/// Project and database names: letters, digits, '-', '.', '_'.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
}

/// Hostname format must contain {name} and stay a valid hostname.
pub fn hostname_format_check(fmt: &str) -> Result<(), &'static str> {
    if !fmt.contains("{name}") {
        return Err("bad_format");
    }
    if !name_ok(&fmt.replace("{name}", "")) {
        return Err("bad_hostname");
    }
    Ok(())
}

pub fn set_run_at_startup(on: bool) {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let mut cmd = std::process::Command::new("reg");
    if on {
        let exe = std::env::current_exe().unwrap_or_default();
        cmd.args(["add", key, "/v", "Ninho", "/t", "REG_SZ", "/d"])
            .arg(format!("\"{}\"", exe.display()))
            .arg("/f");
    } else {
        cmd.args(["delete", key, "/v", "Ninho", "/f"]);
    }
    crate::services::hide_window(&mut cmd);
    let _ = cmd.output();
}
