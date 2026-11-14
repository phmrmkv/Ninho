use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Stroke, vec2,
};

use crate::i18n::{Lang, tr};
use crate::certs;
use crate::dbtools;
use crate::xdebug;
use crate::health;
use crate::icons;
use crate::install;
use crate::nodever;
use crate::phpini;
use crate::qr;
use crate::ports;
use crate::services::{self, SERVICES, Shared, State};
use crate::settings::{self, ServiceCfg, Settings, hostname_format_check, name_ok, port_ok};
use crate::share;
use crate::sites;
use crate::templates;
use crate::platform;
use crate::tray;

// ---------- Theme: semantic color tokens (light / dark), control sizes, one button recipe ----------
// Names follow the owner's design system: bg (page), surface (cards, fields), surface2 (hover, recessed), fg (text),
// muted (secondary text), line (borders), brand (selected item), accent (the main action, one per screen), and a
// mark + soft background per state. Text on accent is dark: white on this teal fails 4.5:1.
static DARK: AtomicBool = AtomicBool::new(false);
fn dark() -> bool {
    DARK.load(Ordering::Relaxed)
}
const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
fn pick(light: u32, dark_v: u32) -> Color32 {
    hex(if dark() { dark_v } else { light })
}
fn bg() -> Color32 { pick(0xeef2f5, 0x12181c) }
fn surface() -> Color32 { pick(0xffffff, 0x1a2227) }
fn surface2() -> Color32 { pick(0xf3f6f8, 0x232d33) }
fn fg() -> Color32 { pick(0x1c2a33, 0xe6eef1) }
fn muted() -> Color32 { pick(0x4f5f69, 0x9fb0b8) }
fn line() -> Color32 { pick(0xd9e1e6, 0x34424a) }
fn brand() -> Color32 { pick(0x1b7880, 0x3cc7d1) }
fn on_brand() -> Color32 { pick(0xffffff, 0x062226) }
fn accent() -> Color32 { pick(0x2fb3bd, 0x3cc7d1) }
fn on_accent() -> Color32 { hex(0x062226) }
fn ok() -> Color32 { pick(0x0c7a63, 0x5fdcbf) }
fn ok_soft() -> Color32 { pick(0xd5f3eb, 0x14403a) }
fn amber() -> Color32 { pick(0x8a5a00, 0xf2b544) }
fn amber_soft() -> Color32 { pick(0xfdf0d0, 0x3b3015) }
fn bad() -> Color32 { pick(0xb3322a, 0xff9087) }
fn bad_soft() -> Color32 { pick(0xfbe3e0, 0x47232a) }
fn bad_fill() -> Color32 { pick(0xb3322a, 0xff9087) }
fn on_bad() -> Color32 { pick(0xffffff, 0x3a0f0a) }
/// The activity list stays dark in both themes, like a terminal.
fn console_bg() -> Color32 { pick(0x1f2a30, 0x0b1013) }

/// Control heights for a desktop tool: row actions, the default, the main action of a screen. Radius 8 everywhere.
const CONTROL_SM: f32 = 32.0;
const CONTROL: f32 = 36.0;
const CONTROL_LG: f32 = 40.0;
const RADIUS: u8 = 8;

#[derive(Clone, Copy, PartialEq)]
enum Tone {
    /// The main action (accent, dark text).
    Primary,
    /// Every other action (surface, border).
    Neutral,
    /// Remove, stop a program (bad).
    Danger,
    /// The selected view in the side menu (brand).
    Nav,
}

/// The one button recipe: same radius, same text size per height, fixed height.
fn button<'a>(text: impl Into<String>, tone: Tone, height: f32) -> egui::Button<'a> {
    let (fill, text_col, stroke) = match tone {
        Tone::Primary => (accent(), on_accent(), accent()),
        Tone::Neutral => (surface(), fg(), line()),
        Tone::Danger => (bad_fill(), on_bad(), bad_fill()),
        Tone::Nav => (brand(), on_brand(), brand()),
    };
    let size = if height >= CONTROL_LG { 14.5 } else if height <= CONTROL_SM { 13.0 } else { 14.0 };
    let mut rt = RichText::new(text.into()).size(size).color(text_col);
    if tone != Tone::Neutral {
        rt = rt.strong();
    }
    egui::Button::new(rt).fill(fill).stroke(Stroke::new(1.0, stroke)).corner_radius(CornerRadius::same(RADIUS)).min_size(vec2(0.0, height))
}

/// Screen reader name for a button whose visible text repeats on every row ("Stop" -> "Stop MySQL").
fn named(r: egui::Response, label: impl Into<String>) -> egui::Response {
    let label = label.into();
    let on = r.enabled();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, on, label.clone()));
    r
}

/// Same, for a button of a group where one is the current choice (views, theme): the screen reader says on/off.
fn toggle_named(r: egui::Response, label: &str, selected: bool) -> egui::Response {
    let on = r.enabled();
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, on, selected, label));
    r
}

pub struct Shot {
    pub path: PathBuf,
    pub requested: bool,
    pub done: Arc<AtomicBool>,
}

pub struct NinhoApp {
    s: Settings,
    shared: Arc<Shared>,
    shot: Option<Shot>,
    frames: u32,
    /// 0 = servers, 1 = ports
    tab: u8,
    show_prefs: bool,
    prefs_tab: u8,
    draft: Settings,
    port_text: Vec<String>,
    prefs_err: Option<String>,
    show_project: bool,
    np_name: String,
    np_kind: usize,
    /// Templates shown in "Create project" (read when the window opens) and the URL a ZIP / git template needs.
    np_list: Vec<templates::Template>,
    np_source: String,
    np_db: bool,
    np_err: Option<String>,
    show_db: bool,
    db_target: String,
    db_name: String,
    db_pw: String,
    db_err: Option<String>,
    show_new: bool,
    nn_kind: String,
    nn_label: String,
    nn_port: String,
    nn_dir: String,
    nn_err: Option<String>,
    confirm_kill: Option<ports::Listener>,
    port_test: String,
    only_local: bool,
    show_updates: bool,
    logo: Option<egui::TextureHandle>,
    db: DbWin,
    /// Public links of shared sites (folder → running tunnel).
    tunnels: std::collections::HashMap<String, share::Tunnel>,
    share_busy: Arc<AtomicBool>,
    log_win: LogWin,
    /// Visuals already built for the current theme.
    styled: bool,
    /// "Install another Node.js version" window: the list from nodejs.org, filled on a thread.
    node_win: bool,
    /// "Check my environment": findings filled on a thread.
    health_win: bool,
    health: Arc<std::sync::Mutex<Option<Vec<health::Finding>>>>,
    node_list: Arc<std::sync::Mutex<Option<Result<Vec<(String, String, bool)>, String>>>>,
    tray: Option<tray::Tray>,
    /// Language whose fonts are loaded.
    fonts_for: Option<Lang>,
    /// Another Ninho was opened: show this window.
    show_req: Arc<AtomicBool>,
    /// A newer Ninho on GitHub (checked once on open), the update window and its download progress in bytes.
    update: Arc<std::sync::Mutex<Option<crate::selfupdate::Release>>>,
    update_win: bool,
    update_bytes: Arc<std::sync::atomic::AtomicU64>,
    update_state: Arc<std::sync::Mutex<String>>,
    /// Import from XAMPP, WampServer or Laragon: what was found, the MySQL root password when one is asked, busy flag.
    imp_win: bool,
    imp_found: Vec<crate::importer::Found>,
    imp_pass: String,
    imp_need_pass: Arc<AtomicBool>,
    imp_busy: Arc<AtomicBool>,
    /// A dump waiting for "load anyway": its file, its databases and the ones Ninho already has (would be replaced).
    imp_pending: Arc<std::sync::Mutex<Option<(PathBuf, Vec<String>, Vec<String>)>>>,
    /// The update is in place: restart into this exe once the servers are stopped.
    restart_to: Arc<std::sync::Mutex<Option<PathBuf>>>,
    /// Answer of "Check for a new Ninho" shown in a small window (None = closed), and "open the update window".
    upd_msg: Arc<std::sync::Mutex<Option<String>>>,
    upd_open: Arc<AtomicBool>,
    /// When each window, menu or list appeared (for the pop-in animation); `pops_ready` skips the very first frame.
    pops: std::collections::HashMap<egui::LayerId, std::time::Instant>,
    pops_ready: bool,
    /// First start: the welcome window (folder, language, what to download).
    welcome: bool,
    /// True while the user moves with Tab / arrows (the focus ring shows); a click turns it off.
    kbd_nav: bool,
    /// Open windows, the last one opened on top: Escape closes it.
    win_stack: Vec<u8>,
    wl_root: String,
    wl_pick: Vec<(&'static str, bool)>,
    /// "Quit" from the menu or the tray: really close instead of hiding in the tray.
    quitting: bool,
    hidden_note: bool,
    /// Folder chosen in Windows' folder dialog (which field asked, the folder), and whether a dialog is open.
    folder_pick: Arc<std::sync::Mutex<Option<(String, PathBuf)>>>,
    picking: Arc<AtomicBool>,
    /// Ports tab: the filter text, and the port last tested (Some(None) = what was typed is not a port).
    port_filter: String,
    port_tested: Option<Option<u16>>,
}

/// Live log viewer: the end of a log file, read again every second while the window is open.
#[derive(Default)]
struct LogWin {
    open: bool,
    title: String,
    file: PathBuf,
    filter: String,
    text: String,
    len: u64,
    last_read: Option<std::time::Instant>,
}

/// State of the Databases window (work runs on threads; results land in the shared slots).
#[derive(Default)]
struct DbWin {
    open: bool,
    target: String,
    password: String,
    list: Arc<std::sync::Mutex<Option<Result<Vec<String>, String>>>>,
    busy: Arc<std::sync::Mutex<String>>,
    import_db: String,
    confirm: Option<(PathBuf, String)>,
    /// Search text (shown with more than 10 databases) and how many backups the list shows (0 = the first 10).
    search: String,
    snap_shown: usize,
}

/// First free port from `from` upwards that no server here uses either.
fn free_port(s: &Settings, from: u16) -> u16 {
    (from..=65535).find(|p| !s.services.iter().any(|c| c.port == *p) && !services::port_open(*p)).unwrap_or(from)
}

impl NinhoApp {
    pub fn new(cc: &eframe::CreationContext<'_>, shot: Option<PathBuf>) -> Self {
        style(&cc.egui_ctx);
        let mut s = Settings::load();
        // NINHO_LANG=ja (any code) shows another language without saving it (used by the screenshot check).
        if let Ok(code) = std::env::var("NINHO_LANG") {
            s.language = Lang::from_code(&code);
        }
        let shared = Shared::new();
        sync_ports(&shared, &s);
        // First look right away, then keep watching in the background.
        shared.refresh();
        services::spawn_watcher(shared.clone(), cc.egui_ctx.clone());
        shared.say(tr(s.language, "welcome"));
        let app = NinhoApp {
            draft: s.clone(),
            port_text: Vec::new(),
            s,
            shared,
            shot: shot.map(|path| Shot { path, requested: false, done: Arc::new(AtomicBool::new(false)) }),
            frames: 0,
            // NINHO_TAB=ports opens on the Ports tab (used by the screenshot check).
            tab: match std::env::var("NINHO_TAB").as_deref() { Ok("ports") => 1, Ok("sites") => 2, _ => 0 },
            show_prefs: false,
            prefs_tab: 0,
            prefs_err: None,
            show_project: false,
            np_name: String::new(),
            np_kind: 0,
            np_list: Vec::new(),
            np_source: String::new(),
            np_db: true,
            np_err: None,
            show_db: false,
            db_target: "mysql".into(),
            db_name: String::new(),
            db_pw: String::new(),
            db_err: None,
            show_new: false,
            nn_kind: "mysql".into(),
            nn_label: String::new(),
            nn_port: String::new(),
            nn_dir: String::new(),
            nn_err: None,
            confirm_kill: None,
            port_test: String::new(),
            only_local: false,
            show_updates: false,
            logo: load_logo(&cc.egui_ctx),
            db: DbWin::default(),
            tunnels: std::collections::HashMap::new(),
            share_busy: Arc::new(AtomicBool::new(false)),
            log_win: LogWin::default(),
            styled: false,
            node_win: false,
            health_win: false,
            health: Arc::new(std::sync::Mutex::new(None)),
            node_list: Arc::new(std::sync::Mutex::new(None)),
            tray: None,
            fonts_for: None,
            show_req: Arc::new(AtomicBool::new(false)),
            update: Arc::new(std::sync::Mutex::new(None)),
            update_win: false,
            update_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            update_state: Arc::new(std::sync::Mutex::new(String::new())),
            imp_win: false,
            imp_found: Vec::new(),
            imp_pass: String::new(),
            imp_need_pass: Arc::new(AtomicBool::new(false)),
            imp_busy: Arc::new(AtomicBool::new(false)),
            imp_pending: Arc::new(std::sync::Mutex::new(None)),
            restart_to: Arc::new(std::sync::Mutex::new(None)),
            upd_msg: Arc::new(std::sync::Mutex::new(None)),
            upd_open: Arc::new(AtomicBool::new(false)),
            pops: std::collections::HashMap::new(),
            pops_ready: false,
            welcome: false,
            kbd_nav: false,
            win_stack: Vec::new(),
            wl_root: String::new(),
            wl_pick: vec![("apache", true), ("php", true), ("mysql", true), ("nodejs", false), ("mailpit", false)],
            quitting: false,
            hidden_note: false,
            folder_pick: Arc::new(std::sync::Mutex::new(None)),
            picking: Arc::new(AtomicBool::new(false)),
            port_filter: String::new(),
            port_tested: None,
        };
        // NINHO_TAB=updates opens the "Download and update" window (used by the screenshot check).
        let mut app = app;
        // NINHO_TAB=log:<id> opens that module's log viewer (used by the screenshot check).
        if let Some(id) = std::env::var("NINHO_TAB").ok().and_then(|t| t.strip_prefix("log:").map(String::from)) {
            if let Some(f) = app.s.services.iter().any(|c| c.id == id).then(|| services::log_file(&app.s, &id)).flatten() {
                app.log_win = LogWin { open: true, title: app.s.svc(&id).title(), file: f, ..Default::default() };
            }
        }
        if std::env::var("NINHO_TAB").as_deref() == Ok("project") {
            app.show_project = true;
        }
        // NINHO_TAB=prefs | prefs-services | db | dbtools | new | node | import opens that window (screenshot check).
        match std::env::var("NINHO_TAB").as_deref() {
            Ok("prefs") => app.open_prefs(),
            Ok("prefs-services") => {
                app.open_prefs();
                app.prefs_tab = 1;
            }
            Ok("db") => app.show_db = true,
            Ok("dbtools") => app.open_db_tools("mysql"),
            Ok("new") => app.open_new(),
            Ok("node") => app.open_node_win(),
            Ok("import") => app.open_import(),
            _ => {}
        }
        if std::env::var("NINHO_TAB").as_deref() == Ok("health") {
            app.run_health();
        }
        if std::env::var("NINHO_TAB").as_deref() == Ok("updates") {
            app.show_updates = true;
            app.check_updates();
        }
        if app.shot.is_none() {
            crate::single::listen(app.show_req.clone(), cc.egui_ctx.clone());
            if let Ok(ids) = std::env::var("NINHO_START_AFTER_UPDATE") {
                // SAFETY: still single-threaded here; the variable must not leak into servers started later.
                unsafe { std::env::remove_var("NINHO_START_AFTER_UPDATE") };
                for id in ids.split(',').filter(|i| !i.is_empty()) {
                    if app.s.services.iter().any(|c| c.id == id) {
                        services::start(&app.shared, &app.s, id);
                    }
                }
            }
            if app.s.check_updates {
                app.check_update(&cc.egui_ctx, false);
            }
            // Very first start: welcome window (it saves the settings, so it shows once).
            if !Settings::exists() {
                app.welcome = true;
                app.wl_root = app.s.root.clone();
                app.imp_found = crate::importer::find(Path::new(&app.s.root));
            }
        }
        if std::env::var("NINHO_TAB").as_deref() == Ok("welcome") {
            app.welcome = true;
            app.wl_root = app.s.root.clone();
        }
        if app.shot.is_none() {
            let texts = [app.t("tray_show"), app.t("start_all"), app.t("stop_all"), app.t("exit")];
            app.tray = tray::Tray::new(&cc.egui_ctx, texts);
            if app.s.start_hidden && app.tray.is_some() {
                cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        if app.shot.is_none() && app.s.start_all_on_launch {
            app.start_all();
        }
        app
    }

    fn t(&self, key: &str) -> &'static str {
        tr(self.s.language, key)
    }

    fn enabled_ids(&self) -> Vec<String> {
        self.s.services.iter().filter(|c| c.enabled).map(|c| c.id.clone()).collect()
    }

    fn any_running(&self) -> bool {
        self.enabled_ids().iter().any(|k| self.shared.state(k) == State::Running)
    }

    fn start_all(&self) {
        let (shared, s, ids) = (self.shared.clone(), self.s.clone(), self.enabled_ids());
        std::thread::spawn(move || {
            for id in ids {
                if services::version_dir(&s, &id).is_some() {
                    services::start(&shared, &s, &id);
                }
            }
        });
    }

    fn stop_all(&self) {
        let (shared, s, ids) = (self.shared.clone(), self.s.clone(), self.enabled_ids());
        std::thread::spawn(move || {
            for id in ids {
                if shared.state(&id) == State::Running {
                    services::stop(&shared, &s, &id);
                }
            }
        });
    }

    fn toggle(&self, id: String) {
        let (shared, s) = (self.shared.clone(), self.s.clone());
        let running = shared.state(&id) == State::Running;
        std::thread::spawn(move || {
            if running { services::stop(&shared, &s, &id) } else { services::start(&shared, &s, &id) }
        });
    }

    /// "Install" on a server row: the newest stable version of its kind, or the download page.
    fn install(&self, id: &str) {
        let key = self.s.svc(id).key.clone();
        if let Some(p) = install::page(&key) {
            self.shared.say(format!("{}: {} {p}", self.s.svc(id).title(), self.t("download_page")));
            open(p);
            return;
        }
        self.install_tool(&key, id);
    }

    fn install_tool(&self, tool: &str, row: &str) {
        self.shared.installing.lock().unwrap().insert(row.into(), "…".into());
        let (shared, s, tool, row) = (self.shared.clone(), self.s.clone(), tool.to_string(), row.to_string());
        std::thread::spawn(move || install::install(shared, s, tool, row));
    }

    /// Ask every official site for its newest stable version (in the background).
    fn check_updates(&self) {
        let lang = self.s.language;
        for (tool, _, _) in install::TOOLS {
            let (shared, tool) = (self.shared.clone(), tool.to_string());
            shared.latest.lock().unwrap().remove(&tool);
            std::thread::spawn(move || {
                let r = install::latest(lang, &tool).map(|l| l.version);
                shared.latest.lock().unwrap().insert(tool, r);
            });
        }
    }

    fn reload(&self) {
        let (shared, s) = (self.shared.clone(), self.s.clone());
        std::thread::spawn(move || {
            sites::refresh(&shared, &s, true);
            if shared.state("apache") == State::Running {
                services::stop(&shared, &s, "apache");
                services::start(&shared, &s, "apache");
                shared.say(format!("Apache: {}", tr(s.language, "log_apache_reloaded")));
            }
        });
    }

    fn terminal(&self) {
        let cmder = Path::new(&self.s.root).join("bin").join("cmder").join("Cmder.exe");
        if cfg!(windows) && cmder.is_file() {
            let _ = std::process::Command::new(cmder).args(["/START", &self.s.doc_root]).spawn();
        } else {
            platform::terminal(&self.s.doc_root, &services::tool_path(&self.s));
        }
    }

    fn save(&mut self) {
        sync_ports(&self.shared, &self.s);
        if let Err(e) = self.s.save() {
            self.shared.say(format!("{} {e}", self.t("log_prefs_save_failed")));
        }
        let shared = self.shared.clone();
        std::thread::spawn(move || shared.refresh());
    }

    fn open_prefs(&mut self) {
        self.draft = self.s.clone();
        self.port_text = self.draft.services.iter().map(|c| c.port.to_string()).collect();
        self.prefs_err = None;
        self.show_prefs = true;
    }

    fn save_prefs(&mut self) {
        let mut d = self.draft.clone();
        for (c, t) in d.services.iter_mut().zip(&self.port_text) {
            match t.trim().parse::<u32>() {
                Ok(p) if port_ok(p) => c.port = p as u16,
                _ => {
                    self.prefs_err = Some(format!("{} — {}: {t}", tr(d.language, "bad_port"), c.title()));
                    return;
                }
            }
        }
        // Two active servers can not share a port.
        for (i, a) in d.services.iter().enumerate() {
            if let Some(b) = d.services[i + 1..].iter().find(|b| a.enabled && b.enabled && b.port == a.port) {
                self.prefs_err = Some(format!("{} {}: {} / {}", tr(d.language, "same_port"), a.port, a.title(), b.title()));
                return;
            }
        }
        if let Err(k) = hostname_format_check(&d.hostname_format) {
            self.prefs_err = Some(tr(d.language, k).to_string());
            return;
        }
        if d.run_at_startup != self.s.run_at_startup {
            settings::set_run_at_startup(d.run_at_startup);
        }
        self.s = d;
        self.save();
        self.shared.say(self.t("saved"));
        self.show_prefs = false;
    }

    fn open_new(&mut self) {
        self.nn_kind = "mysql".into();
        self.nn_label.clear();
        self.nn_err = None;
        self.prefill_new();
        self.show_new = true;
    }

    /// Suggested port and folder for a new server of the chosen kind.
    fn prefill_new(&mut self) {
        let d = services::def(&self.nn_kind);
        self.nn_port = free_port(&self.s, d.default_port + 1).to_string();
        let id = self.s.next_id(&self.nn_kind);
        self.nn_dir = match d.key {
            "mysql" => platform::join(&self.s.root, &["data", &id]),
            "php" => self.s.doc_root.clone(),
            _ => String::new(),
        };
    }

    fn create_server(&mut self) {
        let port = match self.nn_port.trim().parse::<u32>() {
            Ok(p) if port_ok(p) => p as u16,
            _ => {
                self.nn_err = Some(self.t("bad_port").into());
                return;
            }
        };
        if let Some(c) = self.s.services.iter().find(|c| c.port == port) {
            self.nn_err = Some(format!("{} {port}: {}", self.t("same_port"), c.title()));
            return;
        }
        if self.nn_kind == "mysql" && self.nn_dir.trim().is_empty() {
            self.nn_err = Some(self.t("need_folder").into());
            return;
        }
        let id = self.s.next_id(&self.nn_kind);
        let n = id.rsplit('-').next().unwrap_or("2").to_string();
        let label = if self.nn_label.trim().is_empty() { format!("{} {n}", services::def(&self.nn_kind).name) } else { self.nn_label.trim().to_string() };
        let mut c = ServiceCfg::base(&self.nn_kind);
        c.id = id;
        c.label = label.clone();
        c.enabled = true;
        c.port = port;
        c.data_dir = self.nn_dir.trim().to_string();
        self.s.services.push(c);
        self.save();
        self.shared.say(format!("{} {label} ({} {port})", self.t("server_added"), self.t("port")));
        self.show_new = false;
    }

    fn remove_server(&mut self, id: &str) {
        let title = self.s.svc(id).title();
        self.s.services.retain(|c| c.id != id);
        self.save();
        self.shared.say(format!("{} {title}. {}", self.t("server_removed"), self.t("data_kept")));
    }

    fn create_project(&mut self) {
        let name = self.np_name.trim().to_string();
        if name.is_empty() {
            self.np_err = Some(self.t("need_name").into());
            return;
        }
        if !name_ok(&name) {
            self.np_err = Some(self.t("bad_name").into());
            return;
        }
        let dir = Path::new(&self.s.doc_root).join(&name);
        if dir.exists() {
            self.np_err = Some(self.t("exists").into());
            return;
        }
        let Some(t) = self.np_list.get(self.np_kind).cloned() else { return };
        let src = self.np_source.trim().to_string();
        let src_opt = if templates::needs_source(&t) { Some(src.as_str()) } else { None };
        if let Err(e) = templates::check(&t, &name, src_opt) {
            self.np_err = Some(e.to_string());
            return;
        }
        let path_env = services::tool_path(&self.s);
        let missing = templates::missing_tools(&t, &path_env);
        if !missing.is_empty() {
            self.np_err = Some(format!("{} {}", self.t("missing_tools"), missing.join(", ")));
            return;
        }
        let (shared, s, db) = (self.shared.clone(), self.s.clone(), self.np_db);
        match templates::command(&t, &name, src_opt) {
            None => {
                if let Err(e) = templates::create_direct(&t, &dir) {
                    self.np_err = Some(format!("{name}: {e}"));
                    return;
                }
                shared.say(format!("{name}: {}", self.t("project_created")));
            }
            Some(script) => {
                let blank_line = if cfg!(windows) { "echo." } else { "echo" };
                let script = format!("{script} && {blank_line} && echo {}", self.t("project_done"));
                platform::terminal_run(&format!("{} - {name}", t.name), &script, Some(Path::new(&s.doc_root)), Some(&path_env));
                shared.say(self.t("after_download"));
            }
        }
        std::thread::spawn(move || {
            if db && shared.state("mysql") == State::Running {
                match services::mysql_exec(&s, "mysql", &format!("CREATE DATABASE IF NOT EXISTS `{name}`"), "") {
                    Ok(()) => shared.say(format!("{name}: {}", tr(s.language, "log_db_created"))),
                    Err(e) => shared.say(format!("{name}: {} {e}", tr(s.language, "log_db_create_failed"))),
                }
            }
            sites::refresh(&shared, &s, true);
        });
        self.show_project = false;
    }

    fn create_db(&mut self) {
        let name = self.db_name.trim().to_string();
        if !name_ok(&name) {
            self.db_err = Some(self.t("bad_name").into());
            return;
        }
        let target = self.db_target.clone();
        if self.shared.state(&target) != State::Running {
            self.db_err = Some(format!("{} {}", self.t("start_first"), self.s.svc(&target).title()));
            return;
        }
        match services::mysql_exec(&self.s, &target, &format!("CREATE DATABASE `{name}`"), &self.db_pw) {
            Ok(()) => {
                self.shared.say(format!("{name}: {} ({})", self.t("log_db_created"), self.s.svc(&target).title()));
                self.show_db = false;
            }
            Err(e) => self.db_err = Some(format!("{name}: {} {e}", self.t("log_db_create_failed"))),
        }
    }

    // ---------- drawing: a control panel (module table with Start / Admin / Config / Logs, tool buttons on the side) ----------

    fn header(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top")
            .exact_size(64.0)
            .frame(egui::Frame::NONE.fill(surface()).inner_margin(Margin::symmetric(16, 0)).stroke(Stroke::new(1.0, line())))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(tex) = &self.logo {
                        ui.add(egui::Image::new(egui::load::SizedTexture::new(tex.id(), vec2(42.0, 42.0))).alt_text("Ninho"));
                    }
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        // Title and tagline close together, the pair centered in the bar.
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Ninho").size(21.0).strong().color(fg()));
                            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).size(12.5).color(muted()));
                        });
                        ui.label(RichText::new(self.t("control_panel")).size(12.0).color(muted()));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let enabled = self.enabled_ids();
                        let on = enabled.iter().filter(|k| self.shared.state(k) == State::Running).count();
                        let (col, bg) = if on == 0 { (muted(), surface2()) } else if on == enabled.len() { (ok(), ok_soft()) } else { (amber(), amber_soft()) };
                        ui.label(RichText::new(format!("  {on} / {} {}  ", enabled.len(), self.t("modules_on"))).size(13.0).strong().color(col).background_color(bg));
                        ui.add_space(12.0);
                        let newer = self.update.lock().unwrap().as_ref().map(|r| r.version.clone());
                        if let Some(v) = newer {
                            if ui.add(button(format!("{} {v}", self.t("update_available")), Tone::Primary, CONTROL_SM)).clicked() {
                                self.update_win = true;
                            }
                            ui.add_space(8.0);
                        }
                        // Theme: one button, a sun in the light theme and a moon in the dark one; a click switches (saved in ninho.json).
                        if self.theme_button(ui) {
                            self.s.theme = if dark() { "light" } else { "dark" }.into();
                            self.save();
                        }
                    });
                });
            });
    }

    /// Sun (light theme) or moon (dark theme), crossfading and turning when the theme changes. True when clicked.
    fn theme_button(&self, ui: &mut egui::Ui) -> bool {
        let size = vec2(CONTROL, CONTROL_SM);
        let (r, painter) = ui.allocate_painter(size, egui::Sense::click());
        let to = if dark() { self.t("theme_to_light") } else { self.t("theme_to_dark") };
        let r = named(r, to).on_hover_text(to);
        let rect = r.rect;
        let hot = r.hovered() || r.has_focus();
        let bg = if hot { surface2() } else { surface() };
        painter.rect(rect, CornerRadius::same(RADIUS), bg, Stroke::new(1.0, if hot { brand() } else { line() }), egui::StrokeKind::Inside);
        let t = ui.ctx().animate_bool_with_time(egui::Id::unique("theme-icon"), dark(), 0.35);
        let c = rect.center();
        let col = if hot { brand() } else { fg() };
        // Sun: a disc and 8 rays that shrink and turn away as the moon comes in.
        if t < 1.0 {
            let a = 1.0 - t;
            let sun = col.gamma_multiply(a);
            painter.circle_filled(c, 4.2 * (0.6 + 0.4 * a), sun);
            for i in 0..8 {
                let ang = i as f32 * std::f32::consts::FRAC_PI_4 + t * 1.2;
                let d = vec2(ang.cos(), ang.sin());
                painter.line_segment([c + d * 6.6, c + d * (6.6 + 2.6 * a)], Stroke::new(1.6, sun));
            }
        }
        // Moon: a disc with a bite taken by a disc of the button's own color.
        if t > 0.0 {
            let moon = col.gamma_multiply(t);
            let rad = 6.4 * (0.7 + 0.3 * t);
            painter.circle_filled(c, rad, moon);
            painter.circle_filled(c + vec2(3.4, -2.6) * (0.6 + 0.4 * t), rad * 0.82, bg);
        }
        r.clicked()
    }

    /// "Choose folder…" next to a folder field: Windows' folder dialog on a thread; the answer is put in the field named by
    /// \`tag\` (see apply_folder_pick).
    fn folder_button(&self, ui: &mut egui::Ui, tag: &str, start: &str) {
        self.folder_button_with(ui, tag, start, "choose_folder");
    }

    fn folder_button_with(&self, ui: &mut egui::Ui, tag: &str, start: &str, key: &str) {
        let busy = self.picking.load(Ordering::SeqCst);
        if ui.add_enabled(!busy, button(self.t(key), Tone::Neutral, CONTROL_SM)).clicked() {
            pick_folder(self.folder_pick.clone(), self.picking.clone(), tag, start, ui.ctx().clone());
        }
    }

    /// The folder the dialog returned goes where it was asked for.
    fn apply_folder_pick(&mut self) {
        let Some((tag, p)) = self.folder_pick.lock().unwrap().take() else { return };
        let mut path = p.to_string_lossy().to_string();
        if path.len() > 3 {
            path = path.trim_end_matches(['\\', '/']).to_string();
        }
        match tag.as_str() {
            "wl_root" => self.wl_root = path,
            "root" => self.draft.root = path,
            "doc_root" => self.draft.doc_root = path,
            "nn_dir" => self.nn_dir = path,
            "imp" => match crate::importer::find_in(Path::new(&self.s.root), &p) {
                Some(f) => {
                    if !self.imp_found.iter().any(|x| x.root == f.root) {
                        self.imp_found.push(f);
                    }
                }
                None => self.shared.say(format!("{}: {}", p.display(), self.t("import_none_here"))),
            },
            _ => {}
        }
    }

    /// "site", "sites" (and the third form Polish, Russian and Ukrainian need) for a count.
    fn sites_word(&self, n: usize) -> &'static str {
        let code = self.s.language.code();
        let slavic = matches!(code, "pl" | "ru" | "uk");
        let (m10, m100) = (n % 10, n % 100);
        let key = if n == 1 || (slavic && code != "pl" && m10 == 1 && m100 != 11) {
            "sites_count_one"
        } else if slavic && !((2..=4).contains(&m10) && !(12..=14).contains(&m100)) {
            "sites_count_many"
        } else {
            "sites_count"
        };
        self.t(key)
    }

    fn side_btn<'a>(&self, ui: &egui::Ui, text: &str) -> egui::Button<'a> {
        button(text, Tone::Neutral, CONTROL).min_size(vec2(ui.available_width(), CONTROL))
    }

    fn side_button(&self, ui: &mut egui::Ui, key: &str) -> bool {
        let b = self.side_btn(ui, self.t(key));
        ui.add(b).clicked()
    }

    /// The tool buttons on the right, like a classic control panel.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("tools")
            .exact_size(196.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(bg()).inner_margin(Margin::same(12)).stroke(Stroke::new(1.0, line())))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let running = self.any_running();
                let label = if running { self.t("stop_all") } else { self.t("start_all") };
                let big = button(label, if running { Tone::Danger } else { Tone::Primary }, CONTROL_LG).min_size(vec2(ui.available_width(), CONTROL_LG));
                if ui.add(big).clicked() {
                    if running { self.stop_all() } else { self.start_all() }
                }
                ui.add_space(4.0);
                // Views: Modules / Sites / Ports (the current one is highlighted).
                for (i, key) in [(0u8, "modules"), (2u8, "sites"), (1u8, "ports")] {
                    let on = self.tab == i;
                    let b = button(self.t(key), if on { Tone::Nav } else { Tone::Neutral }, CONTROL).min_size(vec2(ui.available_width(), CONTROL));
                    if toggle_named(ui.add(b), self.t(key), on).clicked() {
                        self.go_tab(i);
                    }
                }
                // The views above, the actions below.
                ui.add_space(2.0);
                ui.separator();
                ui.add_space(2.0);
                if self.side_button(ui, "updates") {
                    self.show_updates = true;
                    self.check_updates();
                }
                if self.side_button(ui, "terminal") {
                    self.terminal();
                }
                if self.side_button(ui, "root") {
                    open(&self.s.doc_root);
                }
                ui.add_space(4.0);
                egui::containers::menu::MenuButton::from_button(self.side_btn(ui, &format!("{} >", self.t("more")))).ui(ui, |ui| {
                    ui.set_min_width(260.0);
                    // Databases.
                    if ui.button(self.t("create_db")).clicked() {
                        self.db_name.clear();
                        self.db_err = None;
                        self.db_target = "mysql".into();
                        self.show_db = true;
                        ui.close();
                    }
                    if ui.button(self.t("db_tools")).clicked() {
                        self.open_db_tools("mysql");
                        ui.close();
                    }
                    if ui.button(self.t("import_title")).clicked() {
                        self.open_import();
                        ui.close();
                    }
                    ui.separator();
                    // Tools.
                    if ui.button(self.t("new_server")).clicked() {
                        self.open_new();
                        ui.close();
                    }
                    if ui.button(self.t("health")).clicked() {
                        self.run_health();
                        ui.close();
                    }
                    if ui.button(self.t("hosts_file")).clicked() {
                        if let Some(cmd) = platform::edit_hosts() {
                            self.shared.say(format!("{} {cmd}", self.t("log_hosts_run")));
                        }
                        ui.close();
                    }
                    if ui.button(self.t("open_log")).clicked() {
                        open(&Path::new(&self.s.root).join("tmp").to_string_lossy());
                        ui.close();
                    }
                    ui.separator();
                    // Ninho itself.
                    if ui.button(self.t("check_update")).clicked() {
                        let ctx = ui.ctx().clone();
                        *self.upd_msg.lock().unwrap() = Some(self.t("update_checking").to_string());
                        self.check_update(&ctx, true);
                        ui.close();
                    }
                });
                ui.add_space(4.0);
                if self.side_button(ui, "preferences") {
                    self.open_prefs();
                }
                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    if self.side_button(ui, "exit") {
                        self.quitting = true;
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    let n = sites::list(&self.s).len();
                    ui.label(RichText::new(format!("{n} {} · {}", self.sites_word(n), self.s.doc_root)).size(12.0).color(muted()));
                });
            });
    }

    /// Activity list at the bottom, with the time of each line.
    fn log_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(150.0)
            .size_range(80.0..=400.0)
            .frame(egui::Frame::NONE.fill(console_bg()).inner_margin(Margin::symmetric(12, 8)))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().id_salt("log").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    let log = self.shared.log.lock().unwrap().clone();
                    let lang = self.s.language;
                    for line in log.iter().rev().take(200).rev() {
                        let (time, msg) = line.split_at(line.len().min(8));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(time).font(FontId::monospace(12.0)).color(Color32::from_rgb(0x7f, 0x95, 0x9e)));
                            let low = msg.to_lowercase();
                            let (sad, glad) = crate::i18n::moods(lang);
                            let col = if low.contains("could not") || low.contains("failed") || low.contains("in use") || sad.iter().any(|w| low.contains(w.as_str())) { Color32::from_rgb(0xff, 0x9b, 0x8f) } else if low.contains("started") || low.contains("installed") || glad.iter().any(|w| low.contains(w.as_str())) { Color32::from_rgb(0x8f, 0xe3, 0xa5) } else { Color32::from_rgb(0xe6, 0xee, 0xf0) };
                            ui.label(RichText::new(msg.trim_start()).font(FontId::monospace(12.0)).color(col));
                        });
                    }
                });
            });
    }

    fn center(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(surface()).inner_margin(Margin::symmetric(16, 12)))
            .show(ui, |ui| {
                if self.tab == 1 {
                    ui.label(RichText::new(self.t("ports")).size(16.0).strong().color(fg()));
                    ui.add_space(4.0);
                    self.ports_view(ui);
                    return;
                }
                if self.tab == 2 {
                    self.sites_view(ui);
                    return;
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.t("modules")).size(16.0).strong().color(fg()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.link(RichText::new(format!("+ {}", self.t("new_server"))).size(12.5).color(brand())).clicked() {
                            self.open_new();
                        }
                        let hidden = self.s.services.iter().filter(|c| !c.enabled).count();
                        if hidden > 0 && ui.link(RichText::new(format!("+ {hidden} {}", self.t("hidden_modules"))).size(12.0).color(muted())).clicked() {
                            self.open_prefs();
                            self.prefs_tab = 1;
                        }
                    });
                });
                ui.add_space(6.0);
                self.modules_table(ui);
                ui.add_space(8.0);
                self.foreign_hints(ui);
                self.tools_line(ui);
            });
    }

    fn modules_table(&mut self, ui: &mut egui::Ui) {
        let ids: Vec<String> = self.s.services.iter().filter(|c| c.enabled).map(|c| c.id.clone()).collect();
        if ids.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new(self.t("no_modules")).color(muted()));
            ui.add_space(4.0);
            if ui.add(button(self.t("choose_servers"), Tone::Primary, CONTROL)).clicked() {
                self.open_prefs();
                self.prefs_tab = 1;
            }
            return;
        }
        let mut action: Option<(String, &'static str)> = None;
        egui::ScrollArea::both().id_salt("mods").auto_shrink([false, true]).max_height((ui.available_height() - 60.0).max(120.0)).show(ui, |ui| {
            egui::Grid::new("mods").num_columns(4).striped(true).spacing([14.0, 6.0]).min_row_height(40.0).show(ui, |ui| {
                for h in ["module", "pids", "ports_col", "actions"] {
                    ui.label(RichText::new(self.t(h)).strong().size(12.0).color(muted()));
                }
                ui.end_row();
                for id in &ids {
                    let c = self.s.svc(id).clone();
                    let st = self.shared.state(id);
                    let version = services::version_label(&self.s, id);
                    let installing = self.shared.installing.lock().unwrap().get(id).cloned();
                    // Module: icon, name and version, on green while it runs (red when another program holds its port).
                    let bg = match st {
                        State::Running => ok_soft(),
                        State::Foreign => bad_soft(),
                        State::Busy => amber_soft(),
                        State::Stopped => Color32::TRANSPARENT,
                    };
                    let row = egui::Frame::NONE.fill(bg).corner_radius(CornerRadius::same(6)).inner_margin(Margin::symmetric(6, 4)).show(ui, |ui| {
                        ui.set_min_width(220.0);
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                            icons::draw(ui.painter(), r, &c.key);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.label(RichText::new(c.title()).strong().size(14.0).color(fg()));
                                let sub = match (&installing, &version) {
                                    (Some(m), _) => format!("{} {m}", self.t("installing")),
                                    (None, Some(v)) if st == State::Stopped => format!("{} · {}", short_ver(v), self.t("stopped")),
                                    (None, Some(v)) => short_ver(v),
                                    (None, None) => self.t("not_installed").into(),
                                };
                                ui.label(RichText::new(sub).size(12.0).color(muted()));
                            });
                        });
                    });
                    let state_txt = match st {
                        State::Running => self.t("running"),
                        State::Stopped => self.t("stopped"),
                        State::Busy => self.t("working"),
                        State::Foreign => self.t("port_in_use"),
                    };
                    let row_label = format!("{}, {}, {state_txt}", c.title(), version.as_deref().map(short_ver).unwrap_or_else(|| self.t("not_installed").into()));
                    row.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, row_label.clone()));
                    let pids = services::pids(&self.shared, &self.s, id);
                    ui.label(RichText::new(if pids.is_empty() { "—".to_string() } else { pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ") }).monospace().size(12.0).color(fg()));
                    let mut ports_txt = c.port.to_string();
                    if c.key == "mailpit" {
                        ports_txt = format!("{ports_txt}, {}", services::mailpit_web_port(&c));
                    }
                    ui.label(RichText::new(ports_txt).monospace().size(12.0).color(if st == State::Foreign { bad() } else { fg() }));
                    ui.horizontal(|ui| {
                        let small = |t: &str, tone: Tone| button(t, tone, CONTROL_SM).min_size(vec2(84.0, CONTROL_SM));
                        if version.is_none() {
                            let label = if installing.is_some() { self.t("working") } else if install::page(&c.key).is_some() { self.t("download") } else { self.t("install") };
                            if named(ui.add_enabled(installing.is_none(), small(label, Tone::Primary)), format!("{label} {}", c.title())).clicked() {
                                action = Some((id.clone(), "install"));
                            }
                        } else {
                            let (label, tone) = match st {
                                State::Running => (self.t("stop"), Tone::Neutral),
                                State::Busy => (self.t("working"), Tone::Neutral),
                                State::Foreign => (self.t("see_ports"), Tone::Neutral),
                                State::Stopped => (self.t("start"), Tone::Primary),
                            };
                            if named(ui.add_enabled(st != State::Busy, small(label, tone)).on_hover_text(format!("{label} {}", c.title())), format!("{label} {}", c.title())).clicked() {
                                action = Some((id.clone(), if st == State::Foreign { "ports" } else { "toggle" }));
                            }
                        }
                        let can_admin = st == State::Running && matches!(c.key.as_str(), "apache" | "nginx" | "mysql" | "mariadb" | "postgres" | "php" | "mailpit" | "redis");
                        let adm = ui.add_enabled(can_admin, small(self.t("admin"), Tone::Neutral)).on_hover_text(self.t("admin_hint"));
                        let adm = if st == State::Running { adm } else { adm.on_disabled_hover_text(self.t("admin_off_hint")) };
                        if named(adm, format!("{} {}", self.t("admin"), c.title())).clicked() {
                            action = Some((id.clone(), "admin"));
                        }
                        let (cfg_btn, _) = egui::containers::menu::MenuButton::from_button(small(self.t("config"), Tone::Neutral)).ui(ui, |ui| self.config_menu(ui, id));
                        named(cfg_btn, format!("{} {}", self.t("config"), c.title()));
                        let log = services::log_file(&self.s, id);
                        if named(ui.add_enabled(log.is_some(), small(self.t("logs"), Tone::Neutral)).on_disabled_hover_text(self.t("no_log_yet")), format!("{} {}", self.t("logs"), c.title())).clicked() {
                            if let Some(f) = log {
                                self.log_win = LogWin { open: true, title: c.title(), file: f, ..Default::default() };
                            }
                        }
                    });
                    ui.end_row();
                }
            });
        });
        match action {
            Some((id, "install")) => self.install(&id),
            Some((id, "toggle")) => self.toggle(id),
            Some((_, "ports")) => self.tab = 1,
            Some((id, "admin")) => self.admin(&id),
            _ => {}
        }
    }

    /// Config menu of one module: version, its settings file, PHP extensions, port, folder.
    fn config_menu(&mut self, ui: &mut egui::Ui, id: &str) {
        ui.set_min_width(240.0);
        let c = self.s.svc(id).clone();
        let d = services::def(&c.key);
        let vs = services::versions(&self.s.root, d);
        ui.menu_button(self.t("version"), |ui| {
            let mut pick: Option<String> = None;
            if ui.radio(c.version.is_empty(), self.t("newest")).clicked() {
                pick = Some(String::new());
            }
            for v in vs.iter().rev() {
                if ui.radio(c.version == *v, v).clicked() {
                    pick = Some(v.clone());
                }
            }
            if vs.is_empty() {
                ui.label(RichText::new(self.t("not_installed")).color(muted()));
            }
            if let Some(v) = pick {
                if let Some(cfg) = self.s.services.iter_mut().find(|x| x.id == id) {
                    cfg.version = v;
                }
                self.save();
                self.shared.say(format!("{}: {} {}", c.title(), self.t("version_set"), services::version_label(&self.s, id).unwrap_or_default()));
                ui.close();
            }
        });
        if let Some(f) = services::config_file(&self.s, id) {
            let name = f.file_name().unwrap_or_default().to_string_lossy().into_owned();
            if ui.button(format!("{} {name}", self.t("open"))).clicked() {
                open_text(&f);
                ui.close();
            }
        }
        if c.key == "apache" {
            let mut on = self.s.https;
            if ui.checkbox(&mut on, self.t("https_sites")).on_hover_text(self.t("https_hint")).changed() {
                self.set_https(on);
                ui.close();
            }
        }
        if c.key == "php" {
            if let Some(v) = services::version_dir(&self.s, id) {
                let (have, on) = xdebug::status(&v);
                if !have {
                    if ui.button(self.t("xdebug_get")).clicked() {
                        let (shared, dir, lang) = (self.shared.clone(), v.clone(), self.s.language);
                        std::thread::spawn(move || match xdebug::install(&dir, &shared, lang) {
                            Ok(ver) => shared.say(format!("Xdebug {ver}: {}", tr(lang, "log_xdebug_installed"))),
                            Err(e) => shared.say(format!("Xdebug: {e}")),
                        });
                        ui.close();
                    }
                } else {
                    let mut x = on;
                    if ui.checkbox(&mut x, "Xdebug").on_hover_text(self.t("xdebug_hint")).changed() {
                        match xdebug::set(&v, x) {
                            Ok(()) => self.shared.say(format!("Xdebug {} {}", if x { self.t("ext_on") } else { self.t("ext_off") }, self.t("restart_php"))),
                            Err(e) => self.shared.say(format!("Xdebug: {e}")),
                        }
                    }
                }
            }
            if let Some(v) = services::version_dir(&self.s, id) {
                let ini = v.join("php.ini");
                ui.menu_button(self.t("extensions"), |ui| {
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (ext, on) in phpini::list(&ini) {
                            let mut checked = on;
                            if ui.checkbox(&mut checked, &ext).changed() {
                                match phpini::set(&ini, &ext, checked) {
                                    Ok(()) => self.shared.say(format!("PHP: {ext} {} {}", if checked { self.t("ext_on") } else { self.t("ext_off") }, self.t("restart_php"))),
                                    Err(e) => self.shared.say(format!("PHP: {} {e}", self.t("log_phpini_change_failed"))),
                                }
                            }
                        }
                    });
                });
            }
        }
        if c.key == "mysql" && ui.button(self.t("db_tools")).clicked() {
            self.open_db_tools(id);
            ui.close();
        }
        if ui.button(self.t("change_port")).clicked() {
            self.open_prefs();
            self.prefs_tab = 1;
            ui.close();
        }
        if let Some(v) = services::version_dir(&self.s, id) {
            if ui.button(self.t("explore")).clicked() {
                open(&v.to_string_lossy());
                ui.close();
            }
        }
        if c.is_extra() && self.shared.state(id) != State::Running {
            ui.separator();
            if ui.button(self.t("remove")).on_hover_text(self.t("data_kept")).clicked() {
                self.remove_server(id);
                ui.close();
            }
        }
    }

    /// Warnings for modules whose port another program holds, with a one-click fix.
    fn foreign_hints(&mut self, ui: &mut egui::Ui) {
        let mut change: Option<(String, u16)> = None;
        for c in self.s.services.clone().iter().filter(|c| c.enabled) {
            if self.shared.state(&c.id) != State::Foreign {
                continue;
            }
            let foreign = self.shared.foreign.lock().unwrap().get(&c.id).cloned().unwrap_or_default();
            let program = foreign.split(" (PID").next().unwrap_or("").to_string();
            let pid = foreign.split("PID ").nth(1).and_then(|p| p.trim_end_matches(')').parse::<u32>().ok()).unwrap_or(0);
            let suggestion = self.suggest_port(c);
            let why = if ports::is_system(&program, pid) {
                format!("{}: {} {} {}", c.title(), self.t("taken_windows_1"), c.port, self.t("taken_windows_2"))
            } else {
                format!("{}: {} {} {} {program}.", c.title(), self.t("taken_other_1"), c.port, self.t("taken_other_2"))
            };
            egui::Frame::NONE.fill(amber_soft()).stroke(Stroke::new(1.0, line())).corner_radius(CornerRadius::same(6)).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{why} {} {suggestion} {}", self.t("taken_fix_1"), self.t("taken_fix_2"))).size(12.0).color(fg()));
                    let b = button(format!("{} {suggestion}", self.t("use_port")), Tone::Primary, CONTROL_SM);
                    if ui.add(b).clicked() {
                        change = Some((c.id.clone(), suggestion));
                    }
                });
            });
            ui.add_space(4.0);
        }
        if let Some((id, p)) = change {
            self.set_port(&id, p);
        }
    }

    /// The command line tools the Terminal button puts on the PATH.
    fn tools_line(&mut self, ui: &mut egui::Ui) {
        let mut parts = Vec::new();
        for (tool, name, dir) in install::TOOLS.iter().filter(|t| matches!(t.0, "php" | "nodejs" | "mysql")) {
            if let Some(v) = install::installed(&self.s.root, dir).last() {
                let _ = tool;
                parts.push(format!("{name} {}", short_ver(v)));
            }
        }
        if !parts.is_empty() {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::draw(ui.painter(), r, "nodejs");
                ui.label(RichText::new(format!("{}: {}", self.t("in_terminal"), parts.join(" · "))).size(12.0).color(muted()));
            });
        }
    }

    /// Tray: clicks from its menu, close-to-tray, and the status dot.
    fn tray_step(&mut self, ctx: &egui::Context) {
        let asks = self.tray.as_ref().map(|t| t.asks()).unwrap_or_default();
        for a in asks {
            match a {
                tray::Ask::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                tray::Ask::StartAll => self.start_all(),
                tray::Ask::StopAll => self.stop_all(),
                tray::Ask::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if self.tray.is_some() && self.s.close_to_tray && !self.quitting && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            if !self.hidden_note {
                self.shared.say(self.t("tray_note"));
                self.hidden_note = true;
            }
        }
        let enabled = self.enabled_ids();
        let on = enabled.iter().filter(|k| self.shared.state(k) == State::Running).count();
        let dot = if on == 0 { tray::Dot::AllOff } else if on == enabled.len() { tray::Dot::AllOn } else { tray::Dot::SomeOn };
        let tip = format!("Ninho · {on} / {} {}", enabled.len(), self.t("modules_on"));
        if let Some(t) = self.tray.as_mut() {
            t.status(dot, tip);
        }
    }

    /// Settings of one site (made on first use, with a free port for its own PHP server).
    fn site_cfg(&mut self, folder: &str) -> settings::SiteCfg {
        if let Some(c) = self.s.sites.iter().find(|c| c.folder == folder) {
            if c.port != 0 {
                return c.clone();
            }
        }
        let used: Vec<u16> = self.s.sites.iter().map(|c| c.port).collect();
        let port = (8100..9000).find(|p| !used.contains(p) && !self.s.services.iter().any(|c| c.port == *p) && !services::port_open(*p)).unwrap_or(8100);
        let cfg = settings::SiteCfg { folder: folder.into(), php: String::new(), port };
        self.s.sites.retain(|c| c.folder != folder);
        self.s.sites.push(cfg.clone());
        self.save();
        cfg
    }

    /// Sites tab: every folder of the sites folder, what it is, its addresses and its own PHP server.
    fn sites_view(&mut self, ui: &mut egui::Ui) {
        let list = sites::list(&self.s);
        let apache_on = self.shared.state("apache") == State::Running;
        let php_versions = services::versions(&self.s.root, services::def("php"));
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.t("sites")).size(16.0).strong().color(fg()));
            ui.label(RichText::new(format!("{} · {}", list.len(), self.s.doc_root)).size(12.0).color(muted()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(button(self.t("reload"), Tone::Neutral, CONTROL_SM)).clicked() {
                    self.reload();
                }
                if ui.add(button(format!("+ {}", self.t("quick_app")), Tone::Neutral, CONTROL_SM)).clicked() {
                    self.np_name.clear();
                    self.np_err = None;
                    self.show_project = true;
                }
            });
        });
        ui.label(RichText::new(if apache_on { self.t("sites_hint_on") } else { self.t("sites_hint_off") }).size(12.0).color(muted()));
        ui.add_space(6.0);
        if list.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new(fill(self.t("no_sites"), &[("doc_root", &self.s.doc_root)])).color(muted()));
            ui.add_space(4.0);
            if ui.add(button(self.t("create_project"), Tone::Primary, CONTROL)).clicked() {
                self.np_name.clear();
                self.np_err = None;
                self.show_project = true;
            }
            return;
        }
        let mut action: Option<(String, &'static str)> = None;
        let mut set_php: Option<(String, String)> = None;
        let cfgs: Vec<settings::SiteCfg> = list.iter().map(|x| self.site_cfg(&x.folder)).collect();
        egui::ScrollArea::both().id_salt("sites").auto_shrink([false, false]).show(ui, |ui| {
            // One line per cell: long addresses and paths never wrap letter by letter.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            egui::Grid::new("sitesgrid").num_columns(3).striped(true).spacing([14.0, 6.0]).min_row_height(40.0).show(ui, |ui| {
                for h in ["site", "php_server", "actions"] {
                    ui.label(RichText::new(self.t(h)).strong().size(12.0).color(muted()));
                }
                ui.end_row();
                for (idx, site) in list.iter().enumerate() {
                    let dir = Path::new(&self.s.doc_root).join(&site.folder);
                    let kind = sites::kind(&dir);
                    let cfg = cfgs[idx].clone();
                    let serving = self.shared.children.lock().unwrap().contains_key(&format!("site:{}", site.folder)) && services::port_open(cfg.port);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&site.folder).strong().size(14.0).color(fg()));
                            if !kind.is_empty() {
                                ui.label(RichText::new(format!(" {kind} ")).size(12.0).color(on_brand()).background_color(brand()));
                            }
                        });
                        let url = sites::url(&self.s, site);
                        if ui.add_enabled(apache_on, egui::Link::new(RichText::new(&url).size(12.5))).clicked() {
                            open(&url);
                        }
                        if serving {
                            let local = format!("http://127.0.0.1:{}", cfg.port);
                            if ui.link(RichText::new(&local).size(12.0)).clicked() {
                                open(&local);
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        // A Node.js project has no PHP version to pick.
                        if kind != "Node.js" {
                            let shown = if cfg.php.is_empty() { services::version_label(&self.s, "php").map(|v| short_ver(&v)).unwrap_or_else(|| "—".into()) } else { short_ver(&cfg.php) };
                            egui::ComboBox::from_id_salt(format!("php-{}", site.folder)).width(84.0).selected_text(format!("PHP {shown}")).show_ui(ui, |ui| {
                                if ui.selectable_label(cfg.php.is_empty(), self.t("same_as_module")).clicked() {
                                    set_php = Some((site.folder.clone(), String::new()));
                                }
                                for v in php_versions.iter().rev() {
                                    if ui.selectable_label(cfg.php == *v, short_ver(v)).clicked() {
                                        set_php = Some((site.folder.clone(), v.clone()));
                                    }
                                }
                            });
                        }
                        let b = if serving { button(self.t("unserve"), Tone::Neutral, CONTROL_SM) } else { button(self.t("serve"), Tone::Primary, CONTROL_SM) }.min_size(vec2(132.0, CONTROL_SM));
                        let serve_label = format!("{} {}", if serving { self.t("unserve") } else { self.t("serve") }, site.folder);
                        if named(ui.add_enabled(!php_versions.is_empty(), b).on_hover_text(format!("{} {}", self.t("serve_hint"), cfg.port)), serve_label).clicked() {
                            action = Some((site.folder.clone(), if serving { "unserve" } else { "serve" }));
                        }
                    });
                    ui.horizontal(|ui| {
                        if named(ui.add(button(self.t("open_site"), Tone::Neutral, CONTROL_SM)), format!("{} {}", self.t("open_site"), site.folder)).clicked() {
                            action = Some((site.folder.clone(), "open"));
                        }
                        let sharing = self.tunnels.contains_key(&site.folder);
                        let label = if sharing { self.t("unshare") } else { self.t("share") };
                        if named(ui.add_enabled(!self.share_busy.load(Ordering::SeqCst), button(label, Tone::Neutral, CONTROL_SM)).on_hover_text(self.t("share_hint")), format!("{label} {}", site.folder)).clicked() {
                            action = Some((site.folder.clone(), if sharing { "unshare" } else { "share" }));
                        }
                        // Less used actions in a menu, so the row fits.
                        let (more_btn, _) = egui::containers::menu::MenuButton::from_button(button(format!("{} >", self.t("more")), Tone::Neutral, CONTROL_SM)).ui(ui, |ui| {
                            for (key, what) in [("folder", "folder"), ("terminal", "terminal"), ("code", "code")] {
                                if ui.button(self.t(key)).clicked() {
                                    action = Some((site.folder.clone(), what));
                                    ui.close();
                                }
                            }
                            ui.separator();
                            // Node.js version of this project (.nvmrc), like nvm.
                            let site_dir = Path::new(&self.s.doc_root).join(&site.folder);
                            let current = nodever::wanted(&site_dir).unwrap_or_default();
                            ui.menu_button(format!("{} {}", self.t("node_version"), if current.is_empty() { "—".into() } else { current.clone() }), |ui| {
                                for v in nodever::installed(&self.s) {
                                    if ui.radio(current == v, &v).clicked() {
                                        match nodever::set_for_project(&site_dir, &v) {
                                            Ok(()) => self.shared.say(format!("{}: Node.js {v} (.nvmrc)", site.folder)),
                                            Err(e) => self.shared.say(format!("{}: .nvmrc ({e})", site.folder)),
                                        }
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                if ui.button(self.t("node_more")).clicked() {
                                    self.open_node_win();
                                    ui.close();
                                }
                            });
                        });
                        named(more_btn, format!("{} {}", self.t("more"), site.folder));
                    });
                    // Public link of a shared site, on its own row under the site.
                    let tunnel = self.tunnels.get_mut(&site.folder).map(|t| (t.url(), t.alive(), t.last_lines().last().cloned().unwrap_or_default()));
                    if let Some((url, alive, last)) = tunnel {
                        ui.end_row();
                        ui.label("");
                        ui.horizontal(|ui| {
                            match (&url, alive) {
                                (Some(u), true) => {
                                    ui.label(RichText::new(self.t("public_link")).size(12.0).color(muted()));
                                    if ui.link(RichText::new(u).size(12.5)).clicked() {
                                        open(u);
                                    }
                                    if ui.add(button(self.t("copy"), Tone::Neutral, CONTROL_SM)).clicked() {
                                        ui.ctx().copy_text(u.clone());
                                    }
                                    // QR code to open the link on a phone.
                                    if let Some(code) = qr::encode(u) {
                                        egui::containers::menu::MenuButton::from_button(button(self.t("qr"), Tone::Neutral, CONTROL_SM)).ui(ui, |ui| {
                                            let (r, qr_resp) = ui.allocate_exact_size(vec2(220.0, 220.0), Sense::hover());
                                            qr_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, format!("{} {u}", self.t("qr"))));
                                            qr::paint(&code, ui.painter(), r);
                                            ui.label(RichText::new(self.t("qr_hint")).size(12.0).color(muted()));
                                        });
                                    }
                                }
                                (None, true) => {
                                    ui.label(RichText::new(self.t("share_waiting")).size(12.0).color(amber()));
                                }
                                (_, false) => {
                                    ui.label(RichText::new(format!("{} {last}", self.t("share_failed"))).size(12.0).color(bad()));
                                }
                            }
                        });
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((folder, v)) = set_php {
            if let Some(c) = self.s.sites.iter_mut().find(|c| c.folder == folder) {
                c.php = v;
            }
            self.save();
            self.shared.say(format!("{folder}: {} {}", self.t("site_php_set"), self.t("restart_site")));
        }
        if let Some((folder, what)) = action {
            let dir = Path::new(&self.s.doc_root).join(&folder);
            let cfg = self.site_cfg(&folder);
            match what {
                "serve" => self.serve_site(&folder, &cfg),
                "unserve" => {
                    if let Some(mut ch) = self.shared.children.lock().unwrap().remove(&format!("site:{folder}")) {
                        let _ = ch.kill();
                        let _ = ch.wait();
                    }
                    self.shared.say(format!("{folder}: {}", self.t("site_stopped")));
                }
                "open" => {
                    let serving = self.shared.children.lock().unwrap().contains_key(&format!("site:{folder}"));
                    if serving {
                        open(&format!("http://127.0.0.1:{}", cfg.port));
                    } else if self.shared.state("apache") == State::Running {
                        open(&sites::url(&self.s, &sites::Site { host: sites::hostname(&self.s.hostname_format, &folder), folder: folder.clone() }));
                    } else {
                        self.serve_site(&folder, &cfg);
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        open(&format!("http://127.0.0.1:{}", cfg.port));
                    }
                }
                "share" => self.share_site(&folder, &cfg),
                "unshare" => {
                    if let Some(mut t) = self.tunnels.remove(&folder) {
                        t.stop();
                    }
                    self.shared.say(format!("{folder}: {}", self.t("unshared")));
                }
                "folder" => open(&dir.to_string_lossy()),
                "terminal" => platform::terminal(&dir.display().to_string(), &self.site_path(&cfg)),
                _ => {
                    if !platform::code(&dir, &self.site_path(&cfg)) {
                        self.shared.say(self.t("no_code"));
                    }
                }
            }
        }
    }

    /// Share a site on the internet through a Cloudflare quick tunnel (no account): its own PHP server when it runs,
    /// else Apache with the site's .test name as host. cloudflared is downloaded on first use.
    fn share_site(&mut self, folder: &str, cfg: &settings::SiteCfg) {
        let own = self.shared.children.lock().unwrap().contains_key(&format!("site:{folder}"));
        let (port, host) = if own {
            (cfg.port, None)
        } else if self.shared.state("apache") == State::Running {
            (self.s.svc("apache").port, Some(sites::hostname(&self.s.hostname_format, folder)))
        } else {
            self.serve_site(folder, cfg);
            (cfg.port, None)
        };
        if share::installed(&self.s).is_none() {
            // Download it on a thread, then start the tunnel on the next click.
            let (s, shared, busy) = (self.s.clone(), self.shared.clone(), self.share_busy.clone());
            busy.store(true, Ordering::SeqCst);
            shared.say(self.t("share_installing"));
            std::thread::spawn(move || {
                match share::install(&s, &shared) {
                    Ok(_) => shared.say(format!("cloudflared: {}", tr(s.language, "log_cf_installed_share"))),
                    Err(e) => shared.say(format!("cloudflared: {} {e}", tr(s.language, "log_cf_install_failed"))),
                }
                busy.store(false, Ordering::SeqCst);
            });
            return;
        }
        match share::start(&self.s, port, host.as_deref()) {
            Ok(t) => {
                self.tunnels.insert(folder.to_string(), t);
                self.shared.say(format!("{folder}: {}", self.t("share_started")));
            }
            Err(e) => self.shared.say(format!("{folder}: {e}")),
        }
    }

    /// PATH for one site: its own PHP version and its Node.js version (.nvmrc) first, then the usual tools.
    fn site_path(&self, cfg: &settings::SiteCfg) -> String {
        let mut first = Vec::new();
        if !cfg.php.is_empty() {
            first.push(Path::new(&self.s.root).join("bin").join("php").join(&cfg.php).display().to_string());
        }
        let dir = Path::new(&self.s.doc_root).join(&cfg.folder);
        if let Some(node) = nodever::wanted(&dir).and_then(|w| nodever::resolve(&self.s, &w)).and_then(|v| nodever::dir_of(&self.s, &v)) {
            first.push(node.display().to_string());
        }
        first.push(services::tool_path(&self.s));
        first.join(";")
    }

    fn run_health(&mut self) {
        self.health_win = true;
        let (slot, s) = (self.health.clone(), self.s.clone());
        *slot.lock().unwrap() = None;
        std::thread::spawn(move || {
            let r = health::check(&s);
            *slot.lock().unwrap() = Some(r);
        });
    }

    /// Findings of the environment check: problems first, each with what to do.
    fn health_window(&mut self, ctx: &egui::Context) {
        let mut open = self.health_win;
        let list = self.health.lock().unwrap().clone();
        let mut again = false;
        egui::Window::new(self.t("health")).open(&mut open).collapsible(false).resizable(true).default_size(vec2(640.0, 480.0)).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            match &list {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(self.t("checking")).color(muted()));
                    });
                }
                Some(items) => {
                    let (n_ok, n_warn, n_err) = health::counts(items);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("  {n_err} {}  ", self.t("h_errors"))).strong().color(bad()).background_color(bad_soft()));
                        ui.label(RichText::new(format!("  {n_warn} {}  ", self.t("h_warnings"))).strong().color(amber()).background_color(amber_soft()));
                        ui.label(RichText::new(format!("  {n_ok} {}  ", self.t("h_ok"))).strong().color(ok()).background_color(ok_soft()));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(button(self.t("check_again"), Tone::Neutral, CONTROL_SM)).clicked() {
                                again = true;
                            }
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        let mut sorted: Vec<&health::Finding> = items.iter().collect();
                        sorted.sort_by_key(|f| match f.level { health::Level::Error => 0, health::Level::Warn => 1, health::Level::Ok => 2 });
                        for f in sorted {
                            let (mark, col, soft) = match f.level {
                                health::Level::Error => (self.t("h_error"), bad(), bad_soft()),
                                health::Level::Warn => (self.t("h_warning"), amber(), amber_soft()),
                                health::Level::Ok => (self.t("h_ok_tag"), ok(), ok_soft()),
                            };
                            egui::Frame::NONE.fill(surface()).stroke(Stroke::new(1.0, line())).corner_radius(CornerRadius::same(RADIUS)).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!(" {mark} ")).size(12.0).strong().color(col).background_color(soft));
                                    ui.label(RichText::new(&f.title).strong().color(fg()));
                                });
                                if !f.detail.is_empty() {
                                    ui.label(RichText::new(&f.detail).size(12.5).color(muted()));
                                }
                                if let Some(fix) = &f.fix {
                                    ui.label(RichText::new(format!("{} {fix}", self.t("h_fix"))).size(12.5).color(fg()));
                                }
                            });
                            ui.add_space(4.0);
                        }
                    });
                }
            }
        });
        self.health_win = open;
        if again {
            self.run_health();
        }
    }

    /// First start: pick the language, the Ninho folder (programs, databases, sites) and what to download now.
    fn welcome_window(&mut self, ctx: &egui::Context) {
        let mut go = false;
        let mut later = false;
        egui::Window::new(self.t("welcome_title")).collapsible(false).resizable(false).default_width(520.0).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(RichText::new(self.t("welcome_intro")).size(14.0).color(fg()));
            ui.add_space(8.0);
            egui::Grid::new("wl").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                ui.label(self.t("language"));
                let mut lang = self.s.language;
                egui::ComboBox::from_id_salt("wl-lang").width(240.0).selected_text(lang.label()).show_ui(ui, |ui| {
                    for l in Lang::all() {
                        ui.selectable_value(&mut lang, l, l.label());
                    }
                });
                if lang != self.s.language {
                    self.s.language = lang;
                }
                ui.end_row();
                ui.label(self.t("welcome_folder"));
                ui.horizontal(|ui| {
                    ui.add_sized([240.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.wl_root));
                    self.folder_button(ui, "wl_root", &self.wl_root.clone());
                });
                ui.end_row();
            });
            ui.label(RichText::new(self.t("welcome_folder_hint")).size(12.0).color(muted()));
            ui.add_space(8.0);
            ui.label(RichText::new(self.t("welcome_pick")).strong().color(fg()));
            ui.horizontal_wrapped(|ui| {
                for (tool, on) in self.wl_pick.iter_mut() {
                    let name = install::TOOLS.iter().find(|t| t.0 == *tool).map(|t| t.1).unwrap_or(tool);
                    ui.checkbox(on, name);
                }
            });
            ui.label(RichText::new(self.t("welcome_pick_hint")).size(12.0).color(muted()));
            if !self.imp_found.is_empty() {
                ui.add_space(8.0);
                let names: Vec<String> = self.imp_found.iter().map(|f| format!("{} ({})", f.kind.name(), f.root.display())).collect();
                ui.label(RichText::new(format!("{} {}", self.t("import_found"), names.join(", "))).color(fg()));
                ui.label(RichText::new(self.t("import_after_start")).size(12.0).color(muted()));
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add(button(self.t("welcome_go"), Tone::Primary, CONTROL_LG)).clicked() {
                    go = true;
                }
                if ui.add(button(self.t("welcome_later"), Tone::Neutral, CONTROL_LG)).clicked() {
                    later = true;
                }
            });
        });
        if go || later {
            if go {
                let root = self.wl_root.trim().trim_end_matches(['\\', '/']).to_string();
                if !root.is_empty() && root != self.s.root {
                    self.s.doc_root = platform::join(&root, &["www"]);
                    self.s.root = root;
                }
                for sub in ["bin", "data", "www", "tmp", "etc"] {
                    let _ = std::fs::create_dir_all(Path::new(&self.s.root).join(sub));
                }
            }
            self.welcome = false;
            self.save();
            if go && !self.imp_found.is_empty() {
                // Now that the folder is chosen, offer the import against it.
                self.open_import();
            }
            if go {
                // One download after the other, shown on each module's row and in the activity list.
                let tools: Vec<&'static str> = self.wl_pick.iter().filter(|(_, on)| *on).map(|(t, _)| *t).collect();
                let (shared, s) = (self.shared.clone(), self.s.clone());
                std::thread::spawn(move || {
                    for t in tools {
                        install::install(shared.clone(), s.clone(), t.to_string(), t.to_string());
                    }
                });
            }
        }
    }

    /// Ask GitHub for a newer Ninho in the background. `say`: also report "up to date" or the error (menu check).
    fn check_update(&self, ctx: &egui::Context, say: bool) {
        let (slot, shared, ctx, lang, msg, open) = (self.update.clone(), self.shared.clone(), ctx.clone(), self.s.language, self.upd_msg.clone(), self.upd_open.clone());
        std::thread::spawn(move || {
            match crate::selfupdate::check() {
                Ok(Some(r)) => {
                    shared.say(format!("{} {}", tr(lang, "update_available"), r.version));
                    *slot.lock().unwrap() = Some(r);
                    if say {
                        // Asked from the menu: go straight to the update window.
                        *msg.lock().unwrap() = None;
                        open.store(true, Ordering::SeqCst);
                    }
                }
                Ok(None) => {
                    if say {
                        let text = format!("{} ({})", tr(lang, "up_to_date_ninho"), env!("CARGO_PKG_VERSION"));
                        shared.say(text.clone());
                        *msg.lock().unwrap() = Some(text);
                    }
                }
                Err(e) => {
                    if say {
                        let text = format!("{} {e}", tr(lang, "update_failed"));
                        shared.say(text.clone());
                        *msg.lock().unwrap() = Some(text);
                    }
                }
            }
            ctx.request_repaint();
        });
    }

    /// The new version's notes, then download (with progress), check, swap and restart.
    fn update_window(&mut self, ctx: &egui::Context) {
        let Some(r) = self.update.lock().unwrap().clone() else {
            self.update_win = false;
            return;
        };
        let state = self.update_state.lock().unwrap().clone();
        let mut open = self.update_win;
        let mut start = false;
        egui::Window::new(format!("Ninho {}", r.version)).open(&mut open).collapsible(false).resizable(true).default_size(vec2(560.0, 380.0)).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(RichText::new(format!("{} {} → {}", self.t("update_from_to"), env!("CARGO_PKG_VERSION"), r.version)).color(fg()));
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                ui.label(RichText::new(&r.notes).size(12.5).color(muted()));
            });
            if ui.link(RichText::new(&r.page).size(12.5)).clicked() {
                open_url(&r.page);
            }
            ui.add_space(6.0);
            if state.is_empty() {
                if ui.add(button(self.t("update_now"), Tone::Primary, CONTROL)).clicked() {
                    start = true;
                }
            } else {
                let mb = self.update_bytes.load(Ordering::Relaxed) as f64 / 1_048_576.0;
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(format!("{state} · {mb:.1} MB")).color(amber()));
                });
            }
        });
        self.update_win = open;
        if start {
            *self.update_state.lock().unwrap() = self.t("downloading").to_string();
            let (bytes, state, shared, ctx, lang, restart_to) = (self.update_bytes.clone(), self.update_state.clone(), self.shared.clone(), ctx.clone(), self.s.language, self.restart_to.clone());
            std::thread::spawn(move || {
                let progress = |n: u64| {
                    bytes.store(n, Ordering::Relaxed);
                    ctx.request_repaint();
                };
                match crate::selfupdate::download_and_swap(&r, &progress) {
                    Ok(exe) => {
                        *state.lock().unwrap() = tr(lang, "restarting").to_string();
                        // The UI thread stops the servers, starts the new copy and closes (see update()).
                        *restart_to.lock().unwrap() = Some(exe);
                    }
                    Err(e) => {
                        shared.say(format!("{} {e}", tr(lang, "update_failed")));
                        state.lock().unwrap().clear();
                    }
                }
                ctx.request_repaint();
            });
        }
    }

    /// Everything that opens (windows, menus, lists) grows in from 92 % over a fifth of a second; windows also fade in.
    fn pop_step(&mut self, ctx: &egui::Context) {
        let now = std::time::Instant::now();
        let shown: Vec<(egui::LayerId, egui::Rect)> = ctx.memory(|m| {
            m.areas()
                .visible_layer_ids()
                .into_iter()
                .filter(|l| matches!(l.order, egui::Order::Middle | egui::Order::Foreground))
                .filter_map(|l| m.area_rect(l.id).map(|r| (l, r)))
                .collect()
        });
        self.pops.retain(|l, _| shown.iter().any(|(s, _)| s == l));
        let mut moving = false;
        for (l, r) in shown {
            let ready = self.pops_ready;
            let start = *self.pops.entry(l).or_insert_with(|| if ready { now } else { now - std::time::Duration::from_secs(1) });
            let t: f32 = (now - start).as_secs_f32() / 0.2;
            if t < 1.0 {
                let e: f32 = 1.0 - (1.0 - t).powi(3);
                let s = 0.92 + 0.08 * e;
                let c = r.center().to_vec2();
                ctx.transform_layer_shapes(l, egui::emath::TSTransform::from_translation(c * (1.0 - s)) * egui::emath::TSTransform::from_scaling(s));
                moving = true;
            }
        }
        self.pops_ready = true;
        if moving {
            ctx.request_repaint();
        }
    }

    fn open_import(&mut self) {
        self.imp_found = crate::importer::find(Path::new(&self.s.root));
        self.imp_win = true;
    }

    /// Sites and databases from XAMPP, WampServer or Laragon. The old install is only read, never changed.
    fn import_window(&mut self, ctx: &egui::Context) {
        let mut open = self.imp_win;
        let busy = self.imp_busy.load(Ordering::Relaxed);
        let need_pass = self.imp_need_pass.load(Ordering::Relaxed);
        let mut run: Option<(usize, bool)> = None; // (which install, true = databases)
        let mut load_now: Option<PathBuf> = None;
        egui::Window::new(self.t("import_title")).open(&mut open).collapsible(false).resizable(true).default_width(620.0).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(RichText::new(self.t("import_intro")).color(fg()));
            ui.add_space(6.0);
            if self.imp_found.is_empty() {
                ui.label(RichText::new(self.t("import_none")).color(muted()));
                ui.add_space(4.0);
                self.folder_button_with(ui, "imp", "", "look_elsewhere");
            }
            for (i, f) in self.imp_found.iter().enumerate() {
                ui.separator();
                ui.label(RichText::new(format!("{}  ·  {}", f.kind.name(), f.root.display())).strong().color(fg()));
                let sites = if f.sites.is_empty() { "-".to_string() } else { f.sites.join(", ") };
                ui.label(RichText::new(format!("{} ({}): {sites}", self.t("import_sites"), f.sites.len())).size(12.5).color(muted()));
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        if !f.sites.is_empty() && ui.add(button(self.t("import_copy_sites"), Tone::Primary, CONTROL)).clicked() {
                            run = Some((i, false));
                        }
                        if f.mysql_bin.is_some() && f.data_dir.is_some() && ui.add(button(self.t("import_dbs"), Tone::Neutral, CONTROL)).clicked() {
                            run = Some((i, true));
                        }
                    });
                });
            }
            // The passwords are only asked when there is something to import.
            if !self.imp_found.is_empty() {
                if need_pass {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(self.t("import_password"));
                        ui.add(egui::TextEdit::singleline(&mut self.imp_pass).password(true).desired_width(200.0));
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(self.t("import_ninho_password"));
                    ui.add(egui::TextEdit::singleline(&mut self.db_pw).password(true).desired_width(200.0));
                });
            }
            let pending = self.imp_pending.lock().unwrap().clone();
            if let Some((dump, _, clash)) = pending {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{} {}", self.t("import_replace"), clash.join(", "))).color(amber()));
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, button(self.t("import_load_anyway"), Tone::Danger, CONTROL)).clicked() {
                        load_now = Some(dump.clone());
                    }
                    if ui.add_enabled(!busy, button(self.t("cancel"), Tone::Neutral, CONTROL)).clicked() {
                        *self.imp_pending.lock().unwrap() = None;
                    }
                });
            }
            ui.add_space(6.0);
            ui.label(RichText::new(self.t("import_hint")).size(12.0).color(muted()));
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(self.t("import_busy")).color(amber()));
                });
            }
        });
        self.imp_win = open;
        if let Some(dump) = load_now {
            let (shared, s, busy, pending, lang, ctx, npw) = (self.shared.clone(), self.s.clone(), self.imp_busy.clone(), self.imp_pending.clone(), self.s.language, ctx.clone(), self.db_pw.clone());
            busy.store(true, Ordering::Relaxed);
            std::thread::spawn(move || {
                let n = pending.lock().unwrap().as_ref().map(|p| p.1.len()).unwrap_or(0);
                match crate::importer::load_dump(&s, &dump, &npw) {
                    Ok(()) => {
                        shared.say(format!("{} {n}", tr(lang, "import_loaded")));
                        *pending.lock().unwrap() = None;
                    }
                    Err(e) => shared.say(format!("{} {e}", tr(lang, "import_failed"))),
                }
                busy.store(false, Ordering::Relaxed);
                ctx.request_repaint();
            });
            return;
        }
        let Some((i, dbs)) = run else { return };
        let Some(f) = self.imp_found.get(i).cloned() else { return };
        let (shared, s, busy, need, pass, lang, ctx, npw, pending) = (self.shared.clone(), self.s.clone(), self.imp_busy.clone(), self.imp_need_pass.clone(), self.imp_pass.clone(), self.s.language, ctx.clone(), self.db_pw.clone(), self.imp_pending.clone());
        busy.store(true, Ordering::Relaxed);
        std::thread::spawn(move || {
            let say = |m: &str| {
                shared.say(m.to_string());
                ctx.request_repaint();
            };
            if dbs {
                match crate::importer::dump_databases(&f, Path::new(&s.root), s.svc("mysql").port, &pass, &say) {
                    Ok((dump, names)) => {
                        need.store(false, Ordering::Relaxed);
                        say(&format!("{} {} -> {}", tr(lang, "import_dumped"), names.join(", "), dump.display()));
                        // Databases Ninho already has would be replaced: ask first.
                        let have = crate::dbtools::databases(&s, "mysql", &npw).unwrap_or_default();
                        let clash: Vec<String> = names.iter().filter(|n| have.iter().any(|h| h.eq_ignore_ascii_case(n))).cloned().collect();
                        if clash.is_empty() {
                            match crate::importer::load_dump(&s, &dump, &npw) {
                                Ok(()) => say(&format!("{} {}", tr(lang, "import_loaded"), names.len())),
                                Err(e) => say(&format!("{} {e}", tr(lang, "import_failed"))),
                            }
                        } else {
                            *pending.lock().unwrap() = Some((dump, names, clash));
                        }
                    }
                    Err(e) => {
                        if crate::importer::needs_password(&e) {
                            need.store(true, Ordering::Relaxed);
                        }
                        say(&format!("{} {e}", tr(lang, "import_failed")));
                    }
                }
            } else {
                match crate::importer::copy_sites(&f, Path::new(&s.doc_root), &[], &say) {
                    Ok((copied, skipped)) => {
                        say(&format!("{} {}", tr(lang, "import_copied"), if copied.is_empty() { "-".into() } else { copied.join(", ") }));
                        if !skipped.is_empty() {
                            say(&format!("{} {}", tr(lang, "import_skipped"), skipped.join(", ")));
                        }
                    }
                    Err(e) => say(&format!("{} {e}", tr(lang, "import_failed"))),
                }
            }
            busy.store(false, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }

    fn open_node_win(&mut self) {
        self.node_win = true;
        let slot = self.node_list.clone();
        *slot.lock().unwrap() = None;
        std::thread::spawn(move || {
            let r = nodever::available();
            *slot.lock().unwrap() = Some(r);
        });
    }

    /// Node.js versions from nodejs.org (newest of each LTS line and the Current), installed side by side.
    fn node_window(&mut self, ctx: &egui::Context) {
        let mut open = self.node_win;
        let list = self.node_list.lock().unwrap().clone();
        let have = nodever::installed(&self.s);
        egui::Window::new(self.t("node_more")).open(&mut open).collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(RichText::new(self.t("node_hint")).size(12.0).color(muted()));
            ui.label(RichText::new(self.t("lts_hint")).size(12.0).color(muted()));
            ui.add_space(6.0);
            match list {
                None => {
                    ui.label(RichText::new(self.t("checking")).color(muted()));
                }
                Some(Err(e)) => {
                    ui.colored_label(bad(), e);
                }
                Some(Ok(vs)) => {
                    egui::Grid::new("nodevers").num_columns(3).striped(true).spacing([14.0, 8.0]).min_row_height(CONTROL_SM).show(ui, |ui| {
                        for (v, lts, cur) in vs.into_iter().filter(|(v, _, _)| !nodever::is_prerelease(v)) {
                            ui.label(RichText::new(format!("Node.js {v}")).strong().color(fg()));
                            let tag = if !lts.is_empty() { format!("LTS {lts}") } else if cur { "Current".into() } else { String::new() };
                            ui.label(RichText::new(tag).size(12.0).color(muted()));
                            let busy = self.shared.installing.lock().unwrap().get(&format!("node-{v}")).cloned();
                            if have.contains(&v) {
                                ui.label(RichText::new(self.t("node_installed")).size(12.5).color(ok()));
                            } else if let Some(m) = busy {
                                ui.label(RichText::new(format!("{} {m}", self.t("installing"))).size(12.5).color(amber()));
                            } else if ui.add(button(self.t("install"), Tone::Primary, CONTROL_SM).min_size(vec2(92.0, CONTROL_SM))).clicked() {
                                let (s, shared, v2) = (self.s.clone(), self.shared.clone(), v.clone());
                                std::thread::spawn(move || {
                                    match nodever::install(&s, &shared, &v2) {
                                        Ok(p) => shared.say(format!("Node.js {v2}: {}", p.display())),
                                        Err(e) => shared.say(format!("Node.js {v2}: {e}")),
                                    }
                                    shared.refresh();
                                });
                            }
                            ui.end_row();
                        }
                    });
                }
            }
        });
        self.node_win = open;
    }

    /// Serve one site with its own PHP server (its PHP version, its port), like a per-site PHP in Herd.
    fn serve_site(&mut self, folder: &str, cfg: &settings::SiteCfg) {
        let php_dir = if cfg.php.is_empty() { services::version_dir(&self.s, "php") } else { Some(Path::new(&self.s.root).join("bin").join("php").join(&cfg.php)) };
        let Some(php_dir) = php_dir.filter(|d| d.join(platform::exe("php")).is_file()) else {
            self.shared.say(self.t("need_php_site"));
            return;
        };
        if services::port_open(cfg.port) {
            self.shared.say(format!("{folder}: {} {}", self.t("port"), cfg.port));
            return;
        }
        let root = sites::web_root(&Path::new(&self.s.doc_root).join(folder));
        let mut cmd = std::process::Command::new(php_dir.join(platform::exe("php")));
        cmd.args(["-S", &format!("127.0.0.1:{}", cfg.port), "-t"]).arg(&root).current_dir(&root);
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        services::hide_window(&mut cmd);
        match cmd.spawn() {
            Ok(ch) => {
                self.shared.children.lock().unwrap().insert(format!("site:{folder}"), ch);
                self.shared.say(format!("{folder}: {} http://127.0.0.1:{} (PHP {})", self.t("site_serving"), cfg.port, short_ver(&php_dir.file_name().unwrap_or_default().to_string_lossy())));
            }
            Err(e) => self.shared.say(format!("{folder}: {} {e}", self.t("log_php_start_failed"))),
        }
    }

    /// HTTPS for the .test sites: local authority trusted by Windows (user store, Windows asks), a certificate per
    /// site, Apache on 443. Turning it off keeps the certificates for next time.
    fn set_https(&mut self, on: bool) {
        self.s.https = on;
        self.save();
        let (shared, s) = (self.shared.clone(), self.s.clone());
        let lang = s.language;
        std::thread::spawn(move || {
            if on {
                let Some(openssl) = services::version_dir(&s, "apache").map(|v| v.join("bin").join(platform::exe("openssl"))).filter(|p| p.is_file()) else {
                    shared.say(format!("HTTPS: {}", tr(lang, "log_https_need_apache")));
                    return;
                };
                if let Err(e) = certs::ensure_ca(&s, &openssl) {
                    shared.say(format!("HTTPS: {e}"));
                    return;
                }
                if !certs::ca_trusted(&s) {
                    shared.say(format!("HTTPS: {}", tr(lang, "log_https_trust_ask")));
                    match certs::trust_ca(&s) {
                        Ok(()) => shared.say(format!("HTTPS: {}", tr(lang, "log_https_trusted"))),
                        Err(e) => shared.say(format!("HTTPS: {e}")),
                    }
                }
            }
            sites::refresh(&shared, &s, true);
            if shared.state("apache") == State::Running {
                services::stop(&shared, &s, "apache");
                services::start(&shared, &s, "apache");
            }
            shared.say(if on { tr(lang, "log_https_on") } else { tr(lang, "log_https_off") });
        });
    }

    /// The last 256 KB of the log, read again each second (only when the file changed).
    fn log_window(&mut self, ctx: &egui::Context) {
        let w = &mut self.log_win;
        if w.last_read.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(1)) {
            w.last_read = Some(std::time::Instant::now());
            let len = std::fs::metadata(&w.file).map(|m| m.len()).unwrap_or(0);
            if len != w.len {
                w.len = len;
                use std::io::{Read, Seek, SeekFrom};
                if let Ok(mut f) = std::fs::File::open(&w.file) {
                    let start = len.saturating_sub(256 * 1024);
                    let _ = f.seek(SeekFrom::Start(start));
                    let mut buf = Vec::new();
                    let _ = f.read_to_end(&mut buf);
                    let mut text = String::from_utf8_lossy(&buf).into_owned();
                    if start > 0 {
                        // Drop the first, cut line.
                        text = text.split_once('\n').map(|(_, r)| r.to_string()).unwrap_or(text);
                    }
                    w.text = text;
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        let mut open = w.open;
        let title = fill(tr(self.s.language, "log_title"), &[("name", &self.log_win.title)]);
        let (filter_hint, open_label, lines_label, copy_label) = (tr(self.s.language, "filter"), tr(self.s.language, "open_notepad"), tr(self.s.language, "lines"), tr(self.s.language, "copy"));
        let w = &mut self.log_win;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(vec2(760.0, 440.0))
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized([220.0, CONTROL_SM], egui::TextEdit::singleline(&mut w.filter).hint_text(filter_hint));
                    if ui.add(button(open_label, Tone::Neutral, CONTROL_SM)).clicked() {
                        open_text(&w.file);
                    }
                    ui.label(RichText::new(w.file.to_string_lossy()).size(12.0).color(muted()));
                });
                let f = w.filter.to_lowercase();
                let lines: Vec<&str> = w.text.lines().filter(|l| f.is_empty() || l.to_lowercase().contains(&f)).collect();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} {lines_label}", lines.len())).size(12.0).color(muted()));
                    if ui.add(button(copy_label, Tone::Neutral, CONTROL_SM)).clicked() {
                        ui.ctx().copy_text(lines.join("\n"));
                    }
                });
                egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    for l in lines.iter().rev().take(2000).rev() {
                        let low = l.to_lowercase();
                        let col = if low.contains("error") || low.contains("fatal") { bad() } else if low.contains("warn") { amber() } else { fg() };
                        ui.label(RichText::new(*l).font(FontId::monospace(12.0)).color(col));
                    }
                });
            });
        self.log_win.open = open;
    }

    fn open_db_tools(&mut self, id: &str) {
        self.db.open = true;
        self.db.target = id.to_string();
        self.db.confirm = None;
        self.db.search.clear();
        self.db.snap_shown = 10;
        self.load_dbs();
    }

    fn load_dbs(&self) {
        let (s, id, pw, slot) = (self.s.clone(), self.db.target.clone(), self.db.password.clone(), self.db.list.clone());
        *slot.lock().unwrap() = None;
        std::thread::spawn(move || {
            let r = dbtools::databases(&s, &id, &pw);
            *slot.lock().unwrap() = Some(r);
        });
    }

    /// Run a database job on a thread, showing it as busy and writing the outcome to the activity list.
    fn db_job(&self, what: String, job: impl FnOnce() -> Result<String, String> + Send + 'static) {
        let (shared, busy) = (self.shared.clone(), self.db.busy.clone());
        *busy.lock().unwrap() = what.clone();
        std::thread::spawn(move || {
            match job() {
                Ok(m) => shared.say(m),
                Err(e) => shared.say(format!("{what}: {e}")),
            }
            busy.lock().unwrap().clear();
        });
    }

    /// Databases of one MySQL server: backup (snapshot), restore and import .sql files.
    fn db_tools_window(&mut self, ctx: &egui::Context) {
        let mut shown_win = self.db.open;
        let id = self.db.target.clone();
        let running = self.shared.state(&id) == State::Running;
        let busy = self.db.busy.lock().unwrap().clone();
        // Lists are as tall as whole rows (a button row plus the gap), so no row is ever cut in half.
        let rows_h = |n: usize| n as f32 * CONTROL_SM + (n.saturating_sub(1)) as f32 * 8.0;
        egui::Window::new(self.t("db_tools"))
            .open(&mut shown_win)
            .collapsible(false)
            .resizable(true)
            .default_size(vec2(640.0, 520.0))
            .pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let mysqls: Vec<(String, String)> = self.s.services.iter().filter(|c| c.key == "mysql").map(|c| (c.id.clone(), format!("{} ({})", c.title(), c.port))).collect();
                    let shown = mysqls.iter().find(|(i, _)| *i == id).map(|(_, t)| t.clone()).unwrap_or_default();
                    let mut pick = None;
                    egui::ComboBox::from_id_salt("dbsrv").selected_text(shown).show_ui(ui, |ui| {
                        for (i, t) in &mysqls {
                            if ui.selectable_label(*i == id, t).clicked() {
                                pick = Some(i.clone());
                            }
                        }
                    });
                    ui.label(self.t("db_password"));
                    let hint = self.t("db_password_empty");
                    ui.add_sized([190.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.db.password).password(true).hint_text(hint));
                    if ui.add(button(self.t("refresh"), Tone::Neutral, CONTROL_SM)).clicked() {
                        self.load_dbs();
                    }
                    if let Some(i) = pick {
                        self.db.target = i;
                        self.load_dbs();
                    }
                });
                if !running {
                    ui.colored_label(bad(), format!("{} {}", self.t("start_first"), self.s.svc(&id).title()));
                }
                if !busy.is_empty() {
                    ui.label(RichText::new(format!("{} {busy}", self.t("working"))).color(amber()));
                }
                ui.separator();
                let list = self.db.list.lock().unwrap().clone();
                let total = match &list {
                    Some(Ok(d)) => d.len(),
                    _ => 0,
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.t("databases")).strong().color(fg()));
                    // More than 10 databases: a search field.
                    if total > 10 {
                        let hint = self.t("filter");
                        ui.add_sized([200.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.db.search).hint_text(hint));
                    }
                });
                let query = if total > 10 { self.db.search.trim().to_lowercase() } else { String::new() };
                let vis = if total > 4 { egui::scroll_area::ScrollBarVisibility::AlwaysVisible } else { egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded };
                egui::ScrollArea::vertical().id_salt("dblist").max_height(rows_h(4)).scroll_bar_visibility(vis).show(ui, |ui| match list {
                    None => {
                        ui.label(RichText::new(self.t("checking")).color(muted()));
                    }
                    Some(Err(e)) => {
                        ui.colored_label(bad(), e);
                    }
                    Some(Ok(dbs)) => {
                        if dbs.is_empty() {
                            ui.label(RichText::new(self.t("no_databases")).color(muted()));
                        }
                        for db in dbs.iter().filter(|d| query.is_empty() || d.to_lowercase().contains(&query)) {
                            ui.horizontal(|ui| {
                                ui.set_min_height(CONTROL_SM);
                                ui.label(RichText::new(db).monospace().color(fg()));
                                if ui.add_enabled(busy.is_empty() && running, button(self.t("backup"), Tone::Neutral, CONTROL_SM)).clicked() {
                                    let (s, id2, pw, db2) = (self.s.clone(), id.clone(), self.db.password.clone(), db.clone());
                                    let word = self.t("backup");
                                    self.db_job(format!("{word} {db}"), move || dbtools::backup(&s, &id2, &db2, &pw).map(|f| format!("{word} {db2}: {}", f.display())));
                                }
                            });
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.t("snapshots")).strong().color(fg()));
                    if ui.add(button(self.t("explore"), Tone::Neutral, CONTROL_SM)).clicked() {
                        let dir = Path::new(&self.s.root).join("backups").join(&id);
                        let _ = std::fs::create_dir_all(&dir);
                        open(&dir.to_string_lossy());
                    }
                });
                let snaps = dbtools::snapshots(&self.s, &id);
                let shown_n = self.db.snap_shown.max(10);
                let vis = if snaps.len() > 4 { egui::scroll_area::ScrollBarVisibility::AlwaysVisible } else { egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded };
                egui::ScrollArea::vertical().id_salt("snaps").max_height(rows_h(4)).scroll_bar_visibility(vis).show(ui, |ui| {
                    if snaps.is_empty() {
                        ui.label(RichText::new(self.t("no_snapshots")).color(muted()));
                    }
                    for sn in snaps.iter().take(shown_n) {
                        ui.horizontal(|ui| {
                            ui.set_min_height(CONTROL_SM);
                            ui.label(RichText::new(format!("{}  ·  {}  ·  {:.1} MB", sn.database, sn.when, sn.bytes as f64 / 1_048_576.0)).monospace().size(12.0).color(fg()));
                            if ui.add_enabled(busy.is_empty() && running, button(self.t("restore"), Tone::Neutral, CONTROL_SM)).clicked() {
                                self.db.confirm = Some((sn.file.clone(), sn.database.clone()));
                            }
                        });
                    }
                    if snaps.len() > shown_n && ui.add(button(self.t("show_more"), Tone::Neutral, CONTROL_SM)).clicked() {
                        self.db.snap_shown = shown_n + 10;
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(self.t("import_into"));
                    let hint = self.t("db_name");
                    ui.add_sized([160.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.db.import_db).hint_text(hint));
                    let name_good = name_ok(self.db.import_db.trim());
                    let can = busy.is_empty() && running && name_good;
                    let why = if !name_good { self.t("import_need_db").to_string() } else if !running { format!("{} {}", self.t("start_first"), self.s.svc(&id).title()) } else { String::new() };
                    let b = ui.add_enabled(can, button(self.t("import_sql"), Tone::Neutral, CONTROL_SM));
                    let b = if why.is_empty() { b } else { b.on_disabled_hover_text(why) };
                    if b.clicked() {
                        if let Some(file) = pick_sql_file() {
                            let (s, id2, pw, db2) = (self.s.clone(), id.clone(), self.db.password.clone(), self.db.import_db.trim().to_string());
                            let word = self.t("log_import");
                            self.db_job(format!("{word} {db2}"), move || dbtools::import(&s, &id2, &file, &db2, &pw).map(|_| format!("{word} {db2}: {}", file.display())));
                        }
                    }
                });
            });
        // Restoring replaces the tables of that database: ask first.
        if let Some((file, db)) = self.db.confirm.clone() {
            let mut keep = true;
            egui::Window::new(self.t("restore")).collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
                ui.label(format!("{} {db} {} {}?", self.t("restore_q1"), self.t("restore_q2"), file.file_name().unwrap_or_default().to_string_lossy()));
                ui.label(RichText::new(self.t("restore_warn")).size(12.0).color(muted()));
                ui.horizontal(|ui| {
                    if ui.add(button(self.t("restore"), Tone::Danger, CONTROL)).clicked() {
                        let (s, id2, pw) = (self.s.clone(), id.clone(), self.db.password.clone());
                        let (f2, db2) = (file.clone(), db.clone());
                        let (word, ok) = (self.t("restore"), self.t("log_done"));
                        self.db_job(format!("{word} {db}"), move || dbtools::restore(&s, &id2, &f2, &db2, &pw).map(|_| format!("{word} {db2}: {ok}")));
                        keep = false;
                    }
                    if ui.add(button(self.t("cancel"), Tone::Neutral, CONTROL)).clicked() {
                        keep = false;
                    }
                });
            });
            if !keep {
                self.db.confirm = None;
            }
        }
        self.db.open = shown_win;
    }

    /// Admin of a running module: its page, its database tool, or its command line.
    fn admin(&mut self, id: &str) {
        let c = self.s.svc(id).clone();
        match c.key.as_str() {
            "apache" | "nginx" | "php" => open(&if c.port == 80 { "http://localhost".to_string() } else { format!("http://localhost:{}", c.port) }),
            "mailpit" => open(&format!("http://localhost:{}", services::mailpit_web_port(&c))),
            "redis" => {
                if let Some(v) = services::version_dir(&self.s, id) {
                    let cli = v.join(platform::exe("redis-cli"));
                    platform::terminal_run("Redis", &format!("{} -p {}", platform::quote(&cli.display().to_string()), c.port), None, None);
                }
            }
            "mysql" | "mariadb" => self.mysql_admin(&c),
            "postgres" => {
                if let Some(v) = services::version_dir(&self.s, id) {
                    let psql = v.join("bin").join(platform::exe("psql"));
                    platform::terminal_run("PostgreSQL", &format!("{} -h 127.0.0.1 -p {} -U postgres", platform::quote(&psql.display().to_string()), c.port), None, None);
                }
            }
            _ => {}
        }
    }

    /// MySQL admin: HeidiSQL when present; else Adminer (one PHP file, from adminer.org) served by the installed PHP.
    fn mysql_admin(&mut self, c: &ServiceCfg) {
        let heidi = Path::new(&self.s.root).join("bin").join("heidisql").join("heidisql.exe");
        if heidi.is_file() {
            let _ = std::process::Command::new(heidi).spawn();
            return;
        }
        let Some(php) = services::version_dir(&self.s, "php") else {
            self.shared.say(self.t("need_php"));
            return;
        };
        let (shared, root, mysql_port, lang) = (self.shared.clone(), self.s.root.clone(), c.port, self.s.language);
        std::thread::spawn(move || {
            let dir = Path::new(&root).join("bin").join("adminer");
            let file = dir.join("index.php");
            if !file.is_file() {
                let _ = std::fs::create_dir_all(&dir);
                shared.say(format!("Adminer: {} https://www.adminer.org/latest-mysql-en.php", tr(lang, "log_downloading")));
                let curl = if cfg!(windows) { PathBuf::from(r"C:\Windows\System32\curl.exe") } else { platform::curl() };
                let ok = std::process::Command::new(curl)
                    .args(["-L", "-f", "-s", "-o"])
                    .arg(&file)
                    .arg("https://www.adminer.org/latest-mysql-en.php")
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if !ok {
                    shared.say(format!("Adminer: {}", tr(lang, "log_could_not_download")));
                    return;
                }
            }
            // One Adminer for all the MySQL servers, on its own port, started once.
            let port = 8099u16;
            if !services::port_open(port) {
                let mut cmd = std::process::Command::new(php.join(platform::exe("php")));
                cmd.args(["-S", &format!("127.0.0.1:{port}"), "-t"]).arg(&dir);
                cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
                services::hide_window(&mut cmd);
                match cmd.spawn() {
                    Ok(ch) => {
                        shared.children.lock().unwrap().insert("adminer".into(), ch);
                        std::thread::sleep(std::time::Duration::from_millis(600));
                    }
                    Err(e) => {
                        shared.say(format!("Adminer: {} {e}", tr(lang, "log_php_start_failed")));
                        return;
                    }
                }
            }
            open(&format!("http://127.0.0.1:{port}/?server=127.0.0.1:{mysql_port}&username=root"));
        });
    }


    /// A free port for a server whose port is taken: Mailpit's usual alternative 2525, else the next free one.
    fn suggest_port(&self, c: &ServiceCfg) -> u16 {
        let usual: &[u16] = match c.key.as_str() {
            "mailpit" => &[2525, 1026, 1027],
            "apache" => &[8080, 8081],
            "mysql" => &[3307, 3308],
            "redis" => &[6380],
            _ => &[],
        };
        usual
            .iter()
            .copied()
            .find(|p| !self.s.services.iter().any(|o| o.port == *p) && !services::port_open(*p))
            .unwrap_or_else(|| free_port(&self.s, c.port.saturating_add(1)))
    }

    fn set_port(&mut self, id: &str, port: u16) {
        if let Some(c) = self.s.services.iter_mut().find(|c| c.id == id) {
            c.port = port;
            let title = c.title();
            // The warning goes away now, not on the next check (or a second click would move the port again).
            self.shared.state.lock().unwrap().insert(id.into(), State::Stopped);
            self.shared.foreign.lock().unwrap().remove(id);
            self.save();
            self.shared.say(format!("{title}: {} {port}.", self.t("port_changed")));
        }
    }

    /// Every port something listens on in this PC, plus the ports of this app's servers that are free.
    fn ports_view(&mut self, ui: &mut egui::Ui) {
        let list = self.shared.listeners.lock().unwrap().clone();
        // Port → (server id, server name, its program), to say "Ninho: MySQL" only when that program is really the one listening.
        let mine: std::collections::HashMap<u16, (String, String, String)> = self.s.services.iter().map(|c| (c.port, (c.id.clone(), c.title(), services::image(&c.key)))).collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} {}", list.len(), self.t("ports_in_use"))).strong().color(fg()));
            ui.add_space(8.0);
            let label = self.t("only_local");
            ui.checkbox(&mut self.only_local, label);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(button(self.t("refresh"), Tone::Neutral, CONTROL_SM)).clicked() {
                    let shared = self.shared.clone();
                    std::thread::spawn(move || shared.refresh());
                }
            });
        });
        ui.horizontal(|ui| {
            let hint = self.t("ports_filter");
            ui.add_sized([260.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.port_filter).hint_text(hint));
            ui.add_space(12.0);
            let hint = self.t("test_port");
            let r = ui.add_sized([150.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.port_test).hint_text(hint));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.add(button(self.t("test_go"), Tone::Neutral, CONTROL_SM)).clicked() || enter {
                self.port_tested = Some(self.port_test.trim().parse::<u16>().ok());
            }
            match self.port_tested {
                Some(Some(p)) => {
                    let (txt, col) = match ports::owner(&list, p) {
                        Some(l) => (format!("{p}: {} {} (PID {})", self.t("taken_by"), l.program, l.pid), bad()),
                        None => (format!("{p}: {}", self.t("free")), ok()),
                    };
                    ui.label(RichText::new(txt).size(12.5).color(col));
                }
                Some(None) => {
                    ui.label(RichText::new(self.t("bad_port")).size(12.5).color(bad()));
                }
                None => {}
            }
        });
        ui.add_space(6.0);
        let query = self.port_filter.trim().to_lowercase();
        // One row per listener: what it is, and the server of Ninho that owns it (its stop button is the normal stop).
        let rows: Vec<(&ports::Listener, String, Option<String>)> = list
            .iter()
            .filter(|l| !self.only_local || l.addrs.iter().all(|a| a.starts_with("127.") || a == "[::1]"))
            .map(|l| match mine.get(&l.port) {
                Some((id, t, img)) if l.program.to_ascii_lowercase() == *img => (l, format!("Ninho: {t}"), Some(id.clone())),
                Some((_, t, _)) => (l, format!("{} {t}", self.t("blocks")), None),
                None if ports::is_system(&l.program, l.pid) => (l, platform::SYSTEM_WORD.to_string(), None),
                None => (l, ports::hint(l.port).to_string(), None),
            })
            .filter(|(l, who, _)| query.is_empty() || l.port.to_string().contains(&query) || l.program.to_lowercase().contains(&query) || who.to_lowercase().contains(&query))
            .collect();
        let mut stop_module: Option<String> = None;
        egui::ScrollArea::both().id_salt("ports").auto_shrink([false, false]).max_height((ui.available_height() - 26.0).max(100.0)).show(ui, |ui| {
            egui::Grid::new("portgrid").num_columns(6).striped(true).spacing([14.0, 4.0]).min_row_height(CONTROL_SM + 4.0).show(ui, |ui| {
                for h in ["port", "program", "pid", "who", "reach", ""] {
                    let r = ui.label(RichText::new(if h.is_empty() { "" } else { self.t(h) }).strong().size(12.5).color(muted()));
                    if h == "reach" {
                        r.on_hover_text(self.t("reach_hint"));
                    }
                }
                ui.end_row();
                for (l, who, ninho_id) in &rows {
                    ui.label(RichText::new(l.port.to_string()).strong().color(fg()));
                    ui.label(RichText::new(if l.program.is_empty() { "?" } else { &l.program }).color(fg()));
                    ui.label(RichText::new(l.pid.to_string()).color(muted()));
                    let col = if ninho_id.is_some() { ok() } else if who == "Windows" { muted() } else if who.starts_with(self.t("blocks")) { amber() } else { fg() };
                    ui.label(RichText::new(who).color(col));
                    let local = l.addrs.iter().all(|a| a.starts_with("127.") || a == "[::1]");
                    ui.label(RichText::new(if local { self.t("this_pc") } else { self.t("network") }).size(12.0).color(muted()))
                        .on_hover_text(l.addrs.join("  "));
                    ui.horizontal(|ui| {
                        if ports::hint(l.port) != "Windows" && named(ui.add(button(self.t("open"), Tone::Neutral, CONTROL_SM)), format!("{} {}", self.t("open"), l.port)).clicked() {
                            open(&format!("http://localhost:{}", l.port));
                        }
                        // Windows' own programs can not be stopped from here.
                        if !ports::is_system(&l.program, l.pid) {
                            if let Some(id) = ninho_id {
                                // A server of Ninho: the normal stop, not a kill.
                                if named(ui.add(button(self.t("stop"), Tone::Neutral, CONTROL_SM)), format!("{} {who}", self.t("stop"))).clicked() {
                                    stop_module = Some(id.clone());
                                }
                            } else if named(ui.add(button(self.t("stop_program"), Tone::Danger, CONTROL_SM)), format!("{} {} {}", self.t("stop_program"), l.program, l.port)).clicked() {
                                self.confirm_kill = Some((*l).clone());
                            }
                        }
                    });
                    ui.end_row();
                }
            });
        });
        if let Some(id) = stop_module {
            let (shared, s) = (self.shared.clone(), self.s.clone());
            std::thread::spawn(move || services::stop(&shared, &s, &id));
        }
        ui.add_space(6.0);
        let free: Vec<String> = self
            .s
            .services
            .iter()
            .filter(|c| c.enabled && ports::owner(&list, c.port).is_none())
            .map(|c| format!("{} ({})", c.port, c.title()))
            .collect();
        if !free.is_empty() {
            ui.label(RichText::new(format!("{}: {}", self.t("ninho_free"), free.join(" · "))).size(12.0).color(muted()));
        }
    }

    /// Every tool, the versions already here and the newest stable one on its official site, with a download button.
    fn updates_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_updates;
        egui::Window::new(self.t("updates"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.label(RichText::new(self.t("updates_hint")).size(12.0).color(muted()));
                // Linux and macOS (preview): where the servers come from, and why Apache is on 8080.
                if cfg!(unix) {
                    ui.label(RichText::new(self.t("unix_updates_note")).size(12.0).color(amber()));
                }
                ui.add_space(6.0);
                egui::Grid::new("upd").num_columns(4).striped(true).spacing([14.0, 8.0]).show(ui, |ui| {
                    for h in ["tool", "installed", "latest", ""] {
                        ui.label(RichText::new(if h.is_empty() { "" } else { self.t(h) }).strong().size(12.5).color(muted()));
                    }
                    ui.end_row();
                    for (tool, name, dir) in install::TOOLS {
                        let have = install::installed(&self.s.root, dir);
                        let latest = self.shared.latest.lock().unwrap().get(*tool).cloned();
                        let busy = self.shared.installing.lock().unwrap().get(&format!("upd-{tool}")).cloned();
                        ui.label(RichText::new(*name).strong().color(fg()));
                        ui.label(RichText::new(if have.is_empty() { self.t("not_installed").to_string() } else { have.join("\n") }).size(12.0).color(muted()));
                        match &latest {
                            None => ui.label(RichText::new(self.t("checking")).size(12.5).color(muted())),
                            Some(Ok(v)) => ui.label(RichText::new(short_ver(v)).strong().color(brand())),
                            // Linux and macOS: not installed on this computer; the hover has the command that installs it.
                            Some(Err(e)) if cfg!(unix) => ui.label(RichText::new(self.t("unix_missing")).size(12.5).color(amber())).on_hover_text(e),
                            Some(Err(e)) => ui.label(RichText::new(self.t("offline")).size(12.5).color(bad())).on_hover_text(e),
                        };
                        let up_to_date = matches!(&latest, Some(Ok(v)) if install::is_up_to_date(&have, v));
                        if let Some(m) = busy {
                            ui.label(RichText::new(format!("{} {m}", self.t("installing"))).size(12.5).color(amber()));
                        } else if up_to_date {
                            ui.label(RichText::new(self.t("up_to_date")).size(12.5).color(ok()));
                        } else {
                            let b = button(self.t("download_install"), Tone::Primary, CONTROL_SM).min_size(vec2(92.0, CONTROL_SM));
                            if ui.add_enabled(matches!(latest, Some(Ok(_))), b).clicked() {
                                self.install_tool(tool, &format!("upd-{tool}"));
                            }
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new(self.t("mysql_lts_hint")).size(12.0).color(muted()));
                ui.label(RichText::new(format!("{}: {}", if cfg!(windows) { "Nginx, Memcached" } else { "Nginx" }, self.t("by_hand"))).size(12.0).color(muted()));
                ui.add_space(4.0);
                if ui.add(button(self.t("check_new_versions"), Tone::Neutral, CONTROL_SM)).clicked() {
                    self.check_updates();
                }
            });
        self.show_updates = open;
    }

    fn kill_window(&mut self, ctx: &egui::Context) {
        let Some(l) = self.confirm_kill.clone() else { return };
        let mut keep = true;
        egui::Window::new(self.t("stop_program"))
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                let exe = if l.program.is_empty() { "?" } else { l.program.as_str() };
                ui.label(fill(self.t("end_text"), &[("exe", exe), ("pid", &l.pid.to_string()), ("port", &l.port.to_string())]));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let b = button(self.t("end_go"), Tone::Danger, CONTROL);
                    if ui.add(b).clicked() {
                        match ports::kill(l.pid) {
                            Ok(()) => self.shared.say(format!("{} {} (PID {})", self.t("stopped_program"), l.program, l.pid)),
                            Err(e) => self.shared.say(format!("{} {}: {e}", self.t("could_not_stop"), l.program)),
                        }
                        let shared = self.shared.clone();
                        std::thread::spawn(move || shared.refresh());
                        keep = false;
                    }
                    if ui.add(button(self.t("cancel"), Tone::Neutral, CONTROL)).clicked() {
                        keep = false;
                    }
                });
            });
        if !keep {
            self.confirm_kill = None;
        }
    }

    fn new_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_new;
        let mut go = false;
        let mut cancel = false;
        egui::Window::new(self.t("new_server"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.label(RichText::new(self.t("new_server_hint")).size(12.0).color(muted()));
                ui.add_space(4.0);
                let mut changed = false;
                egui::Grid::new("nn").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label(self.t("kind"));
                    let current = services::def(&self.nn_kind).name;
                    egui::ComboBox::from_id_salt("nnkind").selected_text(current).show_ui(ui, |ui| {
                        for d in SERVICES.iter().filter(|d| d.multi) {
                            if ui.selectable_value(&mut self.nn_kind, d.key.to_string(), d.name).changed() {
                                changed = true;
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(self.t("name"));
                    ui.add(egui::TextEdit::singleline(&mut self.nn_label).hint_text(format!("{} 2", current)).desired_width(220.0));
                    ui.end_row();
                    ui.label(self.t("port"));
                    ui.add(egui::TextEdit::singleline(&mut self.nn_port).desired_width(80.0));
                    ui.end_row();
                    if self.nn_kind == "mysql" || self.nn_kind == "php" {
                        ui.label(self.t(if self.nn_kind == "mysql" { "data_folder" } else { "site_folder" }));
                        ui.horizontal(|ui| {
                            ui.add_sized([240.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.nn_dir));
                            self.folder_button(ui, "nn_dir", &self.nn_dir.clone());
                        });
                        ui.end_row();
                    }
                });
                if changed {
                    self.prefill_new();
                }
                if self.nn_kind == "mysql" {
                    ui.label(RichText::new(self.t("mysql_new_hint")).size(12.0).color(muted()));
                }
                if let Some(e) = &self.nn_err {
                    ui.colored_label(bad(), e);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add(button(self.t("create_server_go"), Tone::Primary, CONTROL)).clicked() {
                        go = true;
                    }
                    if ui.add(button(self.t("cancel"), Tone::Neutral, CONTROL)).clicked() {
                        cancel = true;
                    }
                });
            });
        self.show_new = open && !cancel;
        if go {
            self.create_server();
        }
    }

    fn prefs_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_prefs;
        let lang = self.draft.language;
        let mut save = false;
        let mut cancel = false;
        let mut to_updates = false;
        // The tabs scroll when the window is short; Save and Cancel stay below, always visible.
        let scroll_h = (ctx.content_rect().height() - 220.0).max(160.0);
        egui::Window::new(tr(lang, "preferences"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, key) in [(0u8, "general"), (1u8, "services_ports")] {
                        let on = self.prefs_tab == i;
                        if toggle_named(ui.add(button(tr(lang, key), if on { Tone::Nav } else { Tone::Neutral }, CONTROL_SM)), tr(lang, key), on).clicked() {
                            self.prefs_tab = i;
                        }
                    }
                });
                ui.separator();
                if self.prefs_tab == 0 {
                    egui::ScrollArea::vertical().id_salt("prefs-general").max_height(scroll_h).auto_shrink([false, true]).show(ui, |ui| {
                        egui::Grid::new("general").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                            ui.label(tr(lang, "language"));
                            egui::ComboBox::from_id_salt("lang").selected_text(self.draft.language.label()).show_ui(ui, |ui| {
                                for l in Lang::all() {
                                    ui.selectable_value(&mut self.draft.language, l, l.label());
                                }
                            });
                            ui.end_row();
                            ui.label(tr(lang, "install_root"));
                            ui.horizontal(|ui| {
                                ui.add_sized([260.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.draft.root));
                                self.folder_button(ui, "root", &self.draft.root.clone());
                            });
                            ui.end_row();
                            ui.label(tr(lang, "doc_root"));
                            ui.horizontal(|ui| {
                                ui.add_sized([260.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.draft.doc_root));
                                self.folder_button(ui, "doc_root", &self.draft.doc_root.clone());
                            });
                            ui.end_row();
                            ui.label(tr(lang, "hostname"));
                            ui.add_sized([160.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.draft.hostname_format));
                            ui.end_row();
                            ui.label(tr(lang, "theme"));
                            ui.horizontal(|ui| {
                                for (value, key) in [("light", "theme_light"), ("dark", "theme_dark"), ("system", "theme_system")] {
                                    let on = self.draft.theme == value;
                                    if toggle_named(ui.add(button(tr(lang, key), if on { Tone::Nav } else { Tone::Neutral }, CONTROL_SM)), tr(lang, key), on).clicked() {
                                        self.draft.theme = value.into();
                                    }
                                }
                            });
                            ui.end_row();
                        });
                        ui.label(RichText::new(tr(lang, "hostname_hint")).size(12.0).color(muted()));
                        ui.add_space(6.0);
                        ui.checkbox(&mut self.draft.auto_vhosts, tr(lang, "auto_vhosts"));
                        ui.label(RichText::new(tr(lang, "hosts_note")).size(12.0).color(muted()));
                        ui.checkbox(&mut self.draft.start_all_on_launch, tr(lang, "start_all_auto"));
                        ui.checkbox(&mut self.draft.run_at_startup, tr(lang, "run_at_startup"));
                        ui.checkbox(&mut self.draft.close_to_tray, tr(lang, "close_to_tray"));
                        ui.checkbox(&mut self.draft.start_hidden, tr(lang, "start_hidden"));
                        ui.checkbox(&mut self.draft.check_updates, tr(lang, "check_updates"));
                        ui.separator();
                        // Portable or installed: one exe either way.
                        if !crate::selfinstall::AVAILABLE {
                            ui.label(RichText::new(tr(lang, "install_app_unavailable")).size(12.5).color(muted()));
                        } else if crate::selfinstall::is_installed_copy() {
                            ui.label(RichText::new(format!("{} {}", tr(lang, "installed_at"), crate::selfinstall::install_dir().display())).size(12.5).color(muted()));
                            ui.label(RichText::new(tr(lang, "uninstall_hint")).size(12.0).color(muted()));
                        } else {
                            ui.label(RichText::new(tr(lang, "portable_now")).size(12.5).color(muted()));
                            let label = if crate::selfinstall::installed() { tr(lang, "update_install") } else { tr(lang, "install_app") };
                            if ui.add(button(label, Tone::Neutral, CONTROL)).on_hover_text(tr(lang, "install_app_hint")).clicked() {
                                match crate::selfinstall::install(env!("CARGO_PKG_VERSION")) {
                                    Ok(exe) => self.shared.say(format!("{} {}", tr(lang, "install_app_done"), exe.display())),
                                    Err(e) => self.shared.say(format!("{} {e}", tr(lang, "install_app_failed"))),
                                }
                            }
                        }
                    });
                } else {
                    ui.label(RichText::new(tr(lang, "services_hint")).size(12.5).color(muted()));
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().id_salt("prefs-services").max_height(scroll_h).auto_shrink([false, true]).show(ui, |ui| {
                        egui::Grid::new("svc").num_columns(4).spacing([14.0, 8.0]).show(ui, |ui| {
                            ui.label(RichText::new(tr(lang, "in_modules")).strong()).on_hover_text(tr(lang, "in_modules_hint"));
                            ui.label(RichText::new(tr(lang, "server")).strong());
                            ui.label(RichText::new(tr(lang, "port")).strong());
                            ui.label(RichText::new(tr(lang, "version")).strong());
                            ui.end_row();
                            let root = self.draft.root.clone();
                            for i in 0..self.draft.services.len() {
                                let cfg = &mut self.draft.services[i];
                                let d = services::def(&cfg.key);
                                let title = cfg.title();
                                let tip = format!("{} {title}", tr(lang, "in_modules_hint"));
                                named(ui.checkbox(&mut cfg.enabled, ""), title.clone()).on_hover_text(tip);
                                ui.label(title.clone());
                                named(ui.add_sized([76.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.port_text[i]).font(egui::TextStyle::Monospace)), format!("{} {title}", tr(lang, "port")));
                                let vs = services::versions(&root, d);
                                if vs.is_empty() {
                                    // Nothing installed: a way to get it, not an empty list.
                                    if ui.add(button(tr(lang, "download"), Tone::Neutral, CONTROL_SM)).on_hover_text(tr(lang, "not_installed")).clicked() {
                                        to_updates = true;
                                    }
                                } else {
                                    let current = if cfg.version.is_empty() { vs.last().cloned().unwrap_or_default() } else { cfg.version.clone() };
                                    egui::ComboBox::from_id_salt(format!("v-{}", cfg.id)).width(200.0).selected_text(short_ver(&current)).show_ui(ui, |ui| {
                                        for v in vs.iter().rev() {
                                            ui.selectable_value(&mut cfg.version, v.clone(), short_ver(v)).on_hover_text(v);
                                        }
                                    });
                                }
                                ui.end_row();
                            }
                        });
                    });
                }
                if let Some(e) = &self.prefs_err {
                    ui.add_space(4.0);
                    ui.colored_label(bad(), e);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(button(tr(lang, "save"), Tone::Primary, CONTROL)).clicked() {
                        save = true;
                    }
                    if ui.add(button(tr(lang, "cancel"), Tone::Neutral, CONTROL)).clicked() {
                        cancel = true;
                    }
                });
            });
        self.show_prefs = open && !cancel;
        if to_updates {
            self.show_updates = true;
            self.check_updates();
        }
        if save {
            self.save_prefs();
        }
    }

    fn project_window(&mut self, ctx: &egui::Context) {
        if self.np_list.is_empty() {
            self.np_list = templates::all(&self.s);
            self.np_kind = 0;
            self.np_db = self.np_list.first().is_some_and(|t| t.create_db);
        }
        let mut open = self.show_project;
        let mut go = false;
        let mut picked = None;
        egui::Window::new(self.t("quick_app"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                egui::Grid::new("np").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                    ui.label(self.t("project_name"));
                    let r = ui.add(egui::TextEdit::singleline(&mut self.np_name).desired_width(260.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        go = true;
                    }
                    ui.end_row();
                    ui.label(self.t("project_type"));
                    let current = self.np_list.get(self.np_kind).map(|t| t.name.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("kind").width(260.0).selected_text(current).show_ui(ui, |ui| {
                        for (i, t) in self.np_list.iter().enumerate() {
                            if ui.selectable_label(i == self.np_kind, &t.name).clicked() {
                                picked = Some(i);
                            }
                        }
                    });
                    ui.end_row();
                    if self.np_list.get(self.np_kind).is_some_and(templates::needs_source) {
                        let hint = if self.np_list[self.np_kind].kind == "git" { "https://github.com/…/….git" } else { "https://…/projeto.zip" };
                        ui.label(self.t("project_source"));
                        ui.add(egui::TextEdit::singleline(&mut self.np_source).hint_text(hint).desired_width(260.0));
                        ui.end_row();
                    }
                });
                let label = self.t("auto_db");
                ui.checkbox(&mut self.np_db, label);
                let host = sites::hostname(&self.s.hostname_format, if self.np_name.is_empty() { "…" } else { self.np_name.trim() });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.t("your_site_at")).color(muted()));
                    ui.label(RichText::new(format!("http://{host}")).color(fg()));
                });
                ui.label(RichText::new(self.t("own_templates")).size(12.0).color(muted()));
                if let Some(e) = &self.np_err {
                    ui.colored_label(bad(), e);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add(button(self.t("create_project"), Tone::Primary, CONTROL)).clicked() {
                        go = true;
                    }
                    if ui.add(button(self.t("templates_file"), Tone::Neutral, CONTROL)).clicked() {
                        open_text(&Path::new(&self.s.root).join("etc").join("ninho-templates.json"));
                    }
                });
            });
        if let Some(i) = picked {
            self.np_kind = i;
            self.np_db = self.np_list[i].create_db;
            self.np_err = None;
        }
        self.show_project = open;
        if !open {
            // Read the templates again next time (the person may have edited the file).
            self.np_list.clear();
        }
        if go {
            self.create_project();
        }
    }

    fn db_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_db;
        let mut go = false;
        let mut cancel = false;
        let mysqls: Vec<(String, String)> = self.s.services.iter().filter(|c| c.key == "mysql").map(|c| (c.id.clone(), format!("{} ({} {})", c.title(), self.t("port"), c.port))).collect();
        egui::Window::new(self.t("create_db"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                egui::Grid::new("db").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    if mysqls.len() > 1 {
                        ui.label(self.t("which_mysql"));
                        let shown = mysqls.iter().find(|(id, _)| *id == self.db_target).map(|(_, t)| t.clone()).unwrap_or_default();
                        egui::ComboBox::from_id_salt("dbt").selected_text(shown).show_ui(ui, |ui| {
                            for (id, t) in &mysqls {
                                ui.selectable_value(&mut self.db_target, id.clone(), t);
                            }
                        });
                        ui.end_row();
                    }
                    ui.label(self.t("db_name"));
                    ui.add_sized([220.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.db_name));
                    ui.end_row();
                    ui.label(self.t("db_password"));
                    let hint = self.t("db_password_empty");
                    ui.add_sized([220.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.db_pw).password(true).hint_text(hint));
                    ui.end_row();
                });
                ui.label(RichText::new(self.t("db_password_hint")).size(12.0).color(muted()));
                if let Some(e) = &self.db_err {
                    ui.colored_label(bad(), e);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add(button(self.t("create_db_go"), Tone::Primary, CONTROL)).clicked() {
                        go = true;
                    }
                    if ui.add(button(self.t("cancel"), Tone::Neutral, CONTROL)).clicked() {
                        cancel = true;
                    }
                });
            });
        self.show_db = open && !cancel;
        if go {
            self.create_db();
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        self.frames += 1;
        ctx.request_repaint();
        if shot.done.load(Ordering::SeqCst) || self.frames > 900 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // The update window waits for the official sites to answer before the picture.
        let waiting = (self.show_updates && self.shared.latest.lock().unwrap().len() < install::TOOLS.len() || self.health_win && self.health.lock().unwrap().is_none()) && self.frames < 800;
        if self.frames >= 120 && !shot.requested && !waiting {
            shot.requested = true;
            let (path, done) = (shot.path.clone(), shot.done.clone());
            let repaint = ctx.clone();
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if let Err(e) = buf.save(&path) {
                        eprintln!("could not save screenshot: {e}");
                    }
                }
                done.store(true, Ordering::SeqCst);
                repaint.request_repaint();
            });
        }
    }
}

/// Keyboard use: the focus ring, Escape closes the top window, Ctrl+1/2/3 switch views.
impl NinhoApp {
    fn go_tab(&mut self, tab: u8) {
        self.tab = tab;
        let shared = self.shared.clone();
        std::thread::spawn(move || shared.refresh());
    }

    /// Open windows (not the welcome one) as small ids, 0..=12.
    fn open_windows(&self) -> [bool; 13] {
        [
            self.update_win, self.imp_win, self.show_prefs, self.show_project, self.show_db, self.show_new, self.show_updates,
            self.db.open, self.log_win.open, self.node_win, self.health_win, self.confirm_kill.is_some(), self.db.confirm.is_some(),
        ]
    }

    fn close_window(&mut self, id: u8) {
        match id {
            0 => self.update_win = false,
            1 => self.imp_win = false,
            2 => self.show_prefs = false,
            3 => self.show_project = false,
            4 => self.show_db = false,
            5 => self.show_new = false,
            6 => self.show_updates = false,
            7 => self.db.open = false,
            8 => self.log_win.open = false,
            9 => self.node_win = false,
            10 => self.health_win = false,
            11 => self.confirm_kill = None,
            _ => self.db.confirm = None,
        }
    }

    fn keyboard_step(&mut self, ctx: &egui::Context) {
        use egui::{Event, Key, Modifiers};
        // Show the ring after Tab / arrows, hide it after a click (mouse users never see it).
        let (nav, clicked, esc) = ctx.input(|i| {
            let nav = i.events.iter().any(|e| matches!(e, Event::Key { key: Key::Tab | Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight, pressed: true, .. }));
            (nav, i.pointer.any_pressed(), i.key_pressed(Key::Escape))
        });
        if clicked {
            self.kbd_nav = false;
        } else if nav {
            self.kbd_nav = true;
        }
        // Windows stack in the order they opened.
        let open = self.open_windows();
        self.win_stack.retain(|w| open[*w as usize]);
        for (i, on) in open.iter().enumerate() {
            if *on && !self.win_stack.contains(&(i as u8)) {
                self.win_stack.push(i as u8);
            }
        }
        // Escape belongs to a text field or an open menu first.
        if esc && !ctx.egui_wants_keyboard_input() && !egui::Popup::is_any_open(ctx) {
            if let Some(top) = self.win_stack.pop() {
                self.close_window(top);
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Num1)) {
            self.go_tab(0);
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Num2)) {
            self.go_tab(2);
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Num3)) {
            self.go_tab(1);
        }
        // The focus ring: 2 px, on top of everything, around the widget that has the keyboard focus.
        if self.kbd_nav {
            if let Some(r) = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)) {
                let col = if dark() { accent() } else { brand() };
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("ninho-focus-ring")));
                painter.rect_stroke(r.rect.expand(2.0), CornerRadius::same(RADIUS + 2), Stroke::new(2.0, col), egui::StrokeKind::Outside);
            }
        }
    }
}

impl eframe::App for NinhoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let t0 = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        // NINHO_THEME=dark|light forces a theme (used by the screenshot check).
        let forced = std::env::var("NINHO_THEME").ok();
        let want_dark = match forced.as_deref().unwrap_or(self.s.theme.as_str()) {
            "dark" => true,
            "light" => false,
            _ => ctx.system_theme() == Some(egui::Theme::Dark),
        };
        // A language with another writing system needs a font from Windows (CJK, Devanagari).
        if self.fonts_for != Some(self.s.language) {
            crate::i18n::install_fonts(&ctx, self.s.language);
            self.fonts_for = Some(self.s.language);
        }
        if want_dark != dark() || !self.styled {
            DARK.store(want_dark, Ordering::Relaxed);
            style(&ctx);
            self.styled = true;
        }
        self.screenshot_step(&ctx);
        self.apply_folder_pick();
        self.tray_step(&ctx);
        if self.show_req.swap(false, Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if self.welcome {
            self.welcome_window(&ctx);
        }
        if self.upd_open.swap(false, Ordering::SeqCst) {
            self.update_win = true;
        }
        if self.update_win {
            self.update_window(&ctx);
        }
        let note = self.upd_msg.lock().unwrap().clone();
        if let Some(text) = note {
            let mut open = true;
            let mut ok = false;
            egui::Window::new(self.t("update_note_title")).id(egui::Id::unique("upd-note")).open(&mut open).collapsible(false).resizable(false).default_width(380.0).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    if text == self.t("update_checking") {
                        ui.spinner();
                    }
                    ui.label(RichText::new(&text).color(fg()));
                });
                ui.add_space(6.0);
                if ui.add(button(self.t("got_it"), Tone::Primary, CONTROL)).clicked() {
                    ok = true;
                }
            });
            if !open || ok {
                *self.upd_msg.lock().unwrap() = None;
            }
        }
        let restart = self.restart_to.lock().unwrap().take();
        if let Some(exe) = restart {
            let running: Vec<String> = self.shared.children.lock().unwrap().keys().filter(|k| self.s.services.iter().any(|c| &c.id == *k)).cloned().collect();
            match crate::selfupdate::restart(&exe, &running) {
                Ok(()) => {
                    // A normal close: on_exit stops the servers; the new copy waits for this one and starts them again.
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(e) => {
                    self.shared.say(format!("{} {e} ({})", self.t("update_failed"), exe.display()));
                    self.update_state.lock().unwrap().clear();
                }
            }
        }
        if self.imp_win {
            self.import_window(&ctx);
        }
        self.header(ui);
        self.sidebar(ui);
        self.log_panel(ui);
        self.center(ui);
        if self.show_prefs {
            self.prefs_window(&ctx);
        }
        if self.show_project {
            self.project_window(&ctx);
        }
        if self.show_db {
            self.db_window(&ctx);
        }
        if self.show_new {
            self.new_window(&ctx);
        }
        if self.show_updates {
            self.updates_window(&ctx);
        }
        if self.db.open {
            self.db_tools_window(&ctx);
        }
        if self.log_win.open {
            self.log_window(&ctx);
        }
        if self.node_win {
            self.node_window(&ctx);
        }
        if self.health_win {
            self.health_window(&ctx);
        }
        self.kill_window(&ctx);
        self.keyboard_step(&ctx);
        self.pop_step(&ctx);
        // A server that was just downloaded shows up in Modules right away (it was easy to miss it unticked).
        let fresh: Vec<String> = std::mem::take(&mut *self.shared.just_installed.lock().unwrap());
        let mut turned_on = Vec::new();
        for key in fresh {
            if !self.s.services.iter().any(|c| c.key == key && c.enabled) {
                if let Some(c) = self.s.services.iter_mut().find(|c| c.key == key) {
                    c.enabled = true;
                    turned_on.push(c.title());
                }
            }
        }
        if !turned_on.is_empty() {
            self.save();
            self.shared.say(format!("{} {}", self.t("now_in_modules"), turned_on.join(", ")));
        }
        // NINHO_PROFILE=1: time of each frame goes to tmp\ninho-profile.log (ms), to find what makes clicks feel slow.
        if std::env::var_os("NINHO_PROFILE").is_some() {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            if let Ok(mut h) = std::fs::OpenOptions::new().create(true).append(true).open(Path::new(&self.s.root).join("tmp").join("ninho-profile.log")) {
                let _ = std::io::Write::write_all(&mut h, format!("{ms:.1} tab={} prefs={} welcome={} lang={}\n", self.tab, self.show_prefs, self.welcome, self.s.language.code()).as_bytes());
            }
        }
        // Keep the activity list and states fresh.
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Stop what this app started when Ninho closes.
        let started: Vec<String> = self.shared.children.lock().unwrap().keys().cloned().collect();
        for id in started {
            if self.s.services.iter().any(|c| c.id == id) {
                services::stop(&self.shared, &self.s, &id);
            }
        }
        // The Adminer page server and the sites' own PHP servers.
        let extra: Vec<String> = self.shared.children.lock().unwrap().keys().filter(|k| *k == "adminer" || k.starts_with("site:")).cloned().collect();
        for k in extra {
            if let Some(mut ch) = self.shared.children.lock().unwrap().remove(&k) {
                let _ = ch.kill();
            }
        }
    }
}

fn sync_ports(shared: &Shared, s: &Settings) {
    let mut ports = shared.ports.lock().unwrap();
    ports.clear();
    for c in &s.services {
        ports.insert(c.id.clone(), (c.port, services::image(&c.key)));
    }
}

fn open(target: &str) {
    platform::open(target);
}

/// egui's own colors and spacing from the tokens (called again when the theme changes).
fn style(ctx: &egui::Context) {
    let mut v = if dark() { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = surface();
    v.window_fill = surface();
    v.extreme_bg_color = surface2();
    v.faint_bg_color = surface2();
    v.selection.bg_fill = accent();
    v.selection.stroke = Stroke::new(1.0, on_accent());
    v.hyperlink_color = brand();
    v.window_stroke = Stroke::new(1.0, line());
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, line());
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, fg());
    v.widgets.inactive.weak_bg_fill = surface();
    v.widgets.inactive.bg_fill = surface2();
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, line());
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, fg());
    v.widgets.hovered.weak_bg_fill = surface2();
    v.widgets.hovered.bg_fill = surface2();
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, brand());
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, fg());
    v.widgets.active.weak_bg_fill = surface2();
    v.widgets.active.bg_stroke = Stroke::new(2.0, brand());
    v.widgets.open.weak_bg_fill = surface2();
    v.window_corner_radius = CornerRadius::same(12);
    // A disabled button stays readable: text on a faded light button needs more than half opacity to keep 3:1.
    v.disabled_alpha = if dark() { 0.5 } else { 0.7 };
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(RADIUS);
    }
    ctx.set_visuals(v);
    ctx.all_styles_mut(|st| {
        st.spacing.item_spacing = vec2(8.0, 8.0);
        st.spacing.button_padding = vec2(12.0, 6.0);
        st.spacing.interact_size.y = CONTROL_SM;
        // A click a few pixels outside a button still counts (egui's default is 5), and the window X is bigger.
        st.interaction.interact_radius = 10.0;
        st.spacing.icon_width = 18.0;
        st.spacing.icon_width_inner = 10.0;
        st.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        st.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        st.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
    });
}

/// The logo (assets/ninho-64.png, made from assets/ninho.svg) as a texture for the header.
fn load_logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(include_bytes!("../assets/ninho-64.png")).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("ninho-logo", color, egui::TextureOptions::LINEAR))
}

/// "mysql-8.4.3-winx64" → "8.4.3", "node-v26.13.1-win-x64" → "26.13.1", "php-8.5.12-1-Win32-vs17-x64" → "8.5.12".
fn short_ver(folder: &str) -> String {
    install::short_ver(folder)
}

/// A text with {placeholders} filled in ("{exe}", "{port}"...).
fn fill(text: &str, pairs: &[(&str, &str)]) -> String {
    let mut s = text.to_string();
    for (k, v) in pairs {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// The system's "choose a folder" dialog, on a thread so the window does not freeze. The folder lands in `slot` with
/// the `tag` of the field that asked for it; `busy` is true while the dialog is open.
fn pick_folder(slot: Arc<std::sync::Mutex<Option<(String, PathBuf)>>>, busy: Arc<AtomicBool>, tag: &str, start: &str, ctx: egui::Context) {
    busy.store(true, Ordering::SeqCst);
    let (tag, start) = (tag.to_string(), start.to_string());
    std::thread::spawn(move || {
        if let Some(p) = platform::pick_folder(&start) {
            *slot.lock().unwrap() = Some((tag, PathBuf::from(p)));
        }
        busy.store(false, Ordering::SeqCst);
        ctx.request_repaint();
    });
}

/// A text file (config, log) in the system's text editor (Notepad on Windows).
fn open_text(file: &Path) {
    platform::open_text(file);
}

/// The system's own "open file" dialog for a .sql file (no extra crate needed).
fn pick_sql_file() -> Option<PathBuf> {
    platform::pick_sql_file()
}

/// A web page in the default browser.
fn open_url(url: &str) {
    if url.starts_with("https://") {
        open(url);
    }
}
