//! `ninho.project.json` on the Sites tab (logic in `crate::project`): the "project" tag, notes under the site,
//! the site's PHP / port from the file, the servers started with the site, "Create database", "Save project settings".

use super::*;
use crate::project::{self, DbState, PhpStatus, Project};

/// What one site row shows about its project file.
pub(super) struct ProjView {
    pub project: Option<Result<Project, String>>,
    pub php: PhpStatus,
    /// (server id, database name, state)
    pub db: Option<(String, String, DbState)>,
    pub unknown: Vec<String>,
}

/// A long message on one line, the whole text on hover.
fn short(text: &str) -> String {
    let one: String = text.lines().next().unwrap_or("").chars().take(110).collect();
    if one.len() < text.len() { format!("{one}…") } else { one }
}

impl NinhoApp {
    pub(super) fn project_views(&mut self, list: &[sites::Site]) -> Vec<ProjView> {
        let installed = services::versions(&self.s.root, services::def("php"));
        let s = self.s.clone();
        let shared = self.shared.clone();
        list.iter()
            .map(|site| {
                let dir = Path::new(&s.doc_root).join(&site.folder);
                let project = self.proj.get(&site.folder, &dir);
                match &project {
                    Some(Ok(p)) => {
                        let db = self.proj.db_state(&s, &shared, &site.folder, p);
                        ProjView { php: project::php_status(p, &installed), db, unknown: project::unknown_servers(p), project }
                    }
                    _ => ProjView { php: PhpStatus::Free, db: None, unknown: Vec::new(), project },
                }
            })
            .collect()
    }

    /// The site's settings as it runs: its own, with the project's PHP and port on top.
    pub(super) fn site_cfg_eff(&mut self, folder: &str) -> settings::SiteCfg {
        let cfg = self.site_cfg(folder);
        let dir = Path::new(&self.s.doc_root).join(folder);
        match self.proj.get(folder, &dir) {
            Some(Ok(p)) => project::apply(&cfg, &p, &services::versions(&self.s.root, services::def("php"))),
            _ => cfg,
        }
    }

    /// The small "project" tag next to the site's kind.
    pub(super) fn project_tag(&self, ui: &mut egui::Ui, view: &ProjView) {
        let Some(r) = &view.project else { return };
        let (col, bg) = if r.is_ok() { (on_accent(), accent()) } else { (on_bad(), bad_fill()) };
        ui.label(RichText::new(format!(" {} ", self.t("proj_tag"))).size(12.0).color(col).background_color(bg)).on_hover_text(self.t("proj_tag_hint"));
    }

    /// Notes under the site's address: the file's error, what is missing, with the button that fixes it.
    pub(super) fn project_notes(&self, ui: &mut egui::Ui, folder: &str, view: &ProjView, act: &mut Option<(String, &'static str)>) {
        match &view.project {
            Some(Err(e)) => {
                ui.label(RichText::new(short(e)).size(12.0).color(bad())).on_hover_text(e);
            }
            Some(Ok(_)) => {
                if !view.unknown.is_empty() {
                    ui.label(RichText::new(fill(self.t("proj_unknown_server"), &[("name", &view.unknown.join(", "))])).size(12.0).color(amber()));
                }
                if let PhpStatus::Missing(v) = &view.php {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(fill(self.t("proj_php_missing"), &[("version", v)])).size(12.0).color(amber()));
                        let label = fill(self.t("proj_php_get"), &[("version", v)]);
                        if named(ui.add(button(label.clone(), Tone::Neutral, 26.0)), format!("{label} {folder}")).clicked() {
                            *act = Some((folder.to_string(), "phpget"));
                        }
                    });
                }
                if let Some((id, name, st)) = &view.db {
                    let server = self.s.svc(id).title();
                    match st {
                        DbState::Missing => {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(fill(self.t("proj_db_missing"), &[("name", name), ("server", &server)])).size(12.0).color(amber()));
                                let label = fill(self.t("proj_db_create"), &[("name", name)]);
                                if named(ui.add(button(label.clone(), Tone::Primary, 26.0)), format!("{label} {folder}")).clicked() {
                                    *act = Some((folder.to_string(), "projdb"));
                                }
                            });
                        }
                        DbState::Unknown => {
                            ui.label(RichText::new(fill(self.t("proj_db_wait"), &[("name", name), ("server", &server)])).size(12.0).color(muted()));
                        }
                        DbState::Failed(e) => {
                            ui.label(RichText::new(short(&format!("{}: {e}", fill(self.t("proj_db_failed"), &[("name", name)])))).size(12.0).color(amber())).on_hover_text(e);
                        }
                        DbState::Checking | DbState::Exists => {}
                    }
                }
            }
            None => {}
        }
    }

    /// With "Serve": the servers the project lists (only the ones not running yet), in the background.
    pub(super) fn start_project_servers(&mut self, folder: &str) {
        let dir = Path::new(&self.s.doc_root).join(folder);
        let Some(Ok(p)) = self.proj.get(folder, &dir) else { return };
        if let PhpStatus::Missing(v) = project::php_status(&p, &services::versions(&self.s.root, services::def("php"))) {
            self.shared.say(format!("{folder}: {}", fill(self.t("proj_php_fallback"), &[("version", &v)])));
        }
        for k in project::unknown_servers(&p) {
            self.shared.say(format!("{folder}: {}", fill(self.t("proj_unknown_server"), &[("name", &k)])));
        }
        let ids: Vec<String> = project::server_ids(&self.s, &p).into_iter().filter(|id| self.shared.state(id) != State::Running).collect();
        let (mut todo, mut missing) = (Vec::new(), Vec::new());
        for id in ids {
            if services::version_dir(&self.s, &id).is_some() { todo.push(id) } else { missing.push(self.s.svc(&id).title()) }
        }
        for m in missing {
            self.shared.say(format!("{folder}: {}", fill(self.t("proj_server_missing"), &[("server", &m)])));
        }
        if todo.is_empty() {
            return;
        }
        let names: Vec<String> = todo.iter().map(|id| self.s.svc(id).title()).collect();
        self.shared.say(format!("{folder}: {} {}", self.t("proj_starting"), names.join(", ")));
        let (shared, s) = (self.shared.clone(), self.s.clone());
        std::thread::spawn(move || {
            for id in todo {
                services::start(&shared, &s, &id);
            }
        });
    }

    /// "Create database shop": the person pressed the button, so it is made now.
    pub(super) fn create_project_db(&mut self, folder: &str) {
        let dir = Path::new(&self.s.doc_root).join(folder);
        let Some(Ok(p)) = self.proj.get(folder, &dir) else { return };
        let (Some(id), Some(name)) = (project::db_server(&self.s, &p), p.database.clone()) else { return };
        if self.shared.state(&id) != State::Running {
            self.shared.say(format!("{} {}", self.t("start_first"), self.s.svc(&id).title()));
            return;
        }
        match project::create_database(&self.s, &id, &name) {
            Ok(()) => {
                self.proj.mark_created(folder);
                self.shared.say(format!("{name}: {} ({})", self.t("log_db_created"), self.s.svc(&id).title()));
            }
            Err(e) => self.shared.say(format!("{name}: {} {e}", self.t("log_db_create_failed"))),
        }
    }

    /// "Save project settings": write ninho.project.json from what the site runs with now.
    pub(super) fn save_project_file(&mut self, folder: &str) {
        let dir = Path::new(&self.s.doc_root).join(folder);
        let cfg = self.site_cfg_eff(folder);
        let existing = match self.proj.get(folder, &dir) {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                self.shared.say(format!("{folder}: {} {e}", self.t("proj_save_failed")));
                return;
            }
            None => Project::default(),
        };
        let installed = services::versions(&self.s.root, services::def("php"));
        let php = if sites::kind(&dir) == "Node.js" {
            None
        } else if matches!(project::php_status(&existing, &installed), PhpStatus::Missing(_)) {
            existing.php.clone()
        } else if cfg.php.is_empty() {
            services::version_dir(&self.s, "php").map(|d| project::major_minor(&d.file_name().unwrap_or_default().to_string_lossy()))
        } else {
            Some(project::major_minor(&cfg.php))
        };
        let node = nodever::wanted(&dir).filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit() || c == '.')).or(existing.node.clone());
        let servers = if existing.servers.is_empty() {
            let mut kinds: Vec<String> = Vec::new();
            for c in self.s.services.iter().filter(|c| matches!(c.key.as_str(), "mysql" | "mariadb" | "postgres" | "redis" | "memcached")) {
                if self.shared.state(&c.id) == State::Running && !kinds.contains(&c.key) {
                    kinds.push(c.key.clone());
                }
            }
            kinds
        } else {
            existing.servers.clone()
        };
        let p = Project { php: php.filter(|v| !v.is_empty()), node, servers, port: Some(cfg.port), database: existing.database.clone() };
        match project::save(&dir, &p) {
            Ok(()) => self.shared.say(format!("{folder}: {}", self.t("proj_saved"))),
            Err(e) => self.shared.say(format!("{folder}: {} {e}", self.t("proj_save_failed"))),
        }
        // Read it again at once.
        self.proj = Default::default();
    }
}
