//! "Change root password" window of MySQL / MariaDB (logic in `crate::mysqlpw`).

use super::*;
use crate::mysqlpw::{self, Fail};

impl NinhoApp {
    pub(super) fn open_pw(&mut self, id: &str) {
        self.pw.open_for(id);
    }

    /// Forget the remembered password of a server (the server itself keeps its password).
    pub(super) fn forget_pw(&mut self, id: &str) {
        mysqlpw::remember(&mut self.s, id, None);
        self.save();
        self.shared.say(format!("{}: {}", self.s.svc(id).title(), self.t("pw_forgotten")));
    }

    fn pw_fail_text(&self, f: &Fail, port: u16) -> String {
        match f {
            Fail::WrongCurrent => self.t("pw_wrong_current").to_string(),
            Fail::NoConnect => fill(self.t("pw_no_connect"), &[("port", &port.to_string())]),
            Fail::BadChar => self.t("pw_bad_char").to_string(),
            Fail::NotInstalled => self.t("not_installed").to_string(),
            Fail::Other(e) => format!("{} {e}", self.t("pw_failed")),
        }
    }

    fn pw_start(&mut self, ctx: &egui::Context) {
        let id = self.pw.id.clone();
        let title = self.s.svc(&id).title();
        if self.pw.new != self.pw.repeat {
            self.pw.err = Some(self.t("pw_mismatch").into());
            return;
        }
        if !mysqlpw::check(&self.pw.new) {
            self.pw.err = Some(self.t("pw_bad_char").into());
            return;
        }
        if self.shared.state(&id) != State::Running {
            self.pw.err = Some(fill(self.t("pw_not_running"), &[("server", &title)]));
            return;
        }
        self.pw.err = None;
        self.pw.busy = true;
        self.pw.sent_new = self.pw.new.clone();
        self.pw.sent_remember = self.pw.remember;
        let (s, cur, new, slot) = (self.s.clone(), self.pw.current.clone(), self.pw.new.clone(), self.pw.slot.clone());
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let r = mysqlpw::change(&s, &id, &cur, &new);
            *slot.lock().unwrap() = Some(r);
            repaint.request_repaint();
        });
    }

    pub(super) fn pw_window(&mut self, ctx: &egui::Context) {
        // The answer of the server, when the thread is done.
        let answer = self.pw.slot.lock().unwrap().take();
        if let Some(r) = answer {
            self.pw.busy = false;
            let id = self.pw.id.clone();
            let title = self.s.svc(&id).title();
            match r {
                Ok(()) => {
                    let keep = self.pw.sent_remember && !self.pw.sent_new.is_empty();
                    let sealed = mysqlpw::remember(&mut self.s, &id, if keep { Some(self.pw.sent_new.as_str()) } else { None });
                    self.save();
                    let first = if self.pw.sent_new.is_empty() { "pw_done_empty" } else { "pw_done" };
                    let tail = if keep && sealed { self.t("pw_remembered") } else if self.pw.sent_new.is_empty() { "" } else { self.t("pw_not_remembered") };
                    self.shared.say(format!("{title}: {} {tail}", self.t(first)).trim_end().to_string());
                    self.pw = Default::default();
                    return;
                }
                Err(f) => {
                    let port = self.s.svc(&id).port;
                    self.pw.err = Some(self.pw_fail_text(&f, port));
                }
            }
        }
        let id = self.pw.id.clone();
        let title = self.s.svc(&id).title();
        let have = mysqlpw::has_stored(&self.s, &id);
        let mut open = self.pw.open;
        let (mut go, mut cancel) = (false, false);
        egui::Window::new(self.t("pw_title")).open(&mut open).collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(RichText::new(format!("{title} ({} {})", self.t("port"), self.s.svc(&id).port)).strong().color(fg()));
            ui.add_space(4.0);
            let busy = self.pw.busy;
            ui.add_enabled_ui(!busy, |ui| {
                egui::Grid::new("pwgrid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label(self.t("pw_current"));
                    let hint = self.t(if have { "pw_current_remembered" } else { "db_password_empty" });
                    ui.add_sized([230.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.pw.current).password(true).hint_text(hint));
                    ui.end_row();
                    ui.label(self.t("pw_new"));
                    ui.add_sized([230.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.pw.new).password(true));
                    ui.end_row();
                    ui.label(self.t("pw_repeat"));
                    ui.add_sized([230.0, CONTROL_SM], egui::TextEdit::singleline(&mut self.pw.repeat).password(true));
                    ui.end_row();
                });
                let (label, hover) = (self.t("pw_remember"), self.t("pw_remember_hint"));
                ui.checkbox(&mut self.pw.remember, label).on_hover_text(hover);
            });
            ui.label(RichText::new(self.t("pw_hint")).size(12.0).color(muted()));
            if let Some(e) = &self.pw.err {
                ui.add_space(2.0);
                ui.colored_label(bad(), e);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.pw.busy, button(self.t("pw_change"), Tone::Primary, CONTROL)).clicked() {
                    go = true;
                }
                if ui.add(button(self.t("cancel"), Tone::Neutral, CONTROL)).clicked() {
                    cancel = true;
                }
                if self.pw.busy {
                    ui.spinner();
                }
            });
        });
        self.pw.open = open && !cancel;
        if go {
            self.pw_start(ctx);
        }
    }
}
