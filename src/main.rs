#![windows_subsystem = "windows"]

mod app;
mod caddy;
mod certs;
mod cli;
mod dbtools;
mod docker;
mod health;
mod home;
mod i18n;
mod icons;
mod importer;
mod install;
mod mcp;
mod mongo;
mod mysqlpw;
mod nodever;
mod phpini;
mod platform;
mod ports;
mod project;
mod qr;
mod selfinstall;
mod selfupdate;
mod services;
mod share;
mod settings;
mod single;
mod sites;
mod templates;
mod tray;
mod xdebug;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    // If Ninho ever breaks, the reason goes to tmp\ninho-crash.log.
    single::crash_log();
    // Windows "Installed apps" → Uninstall runs this.
    if args.get(1).map(String::as_str) == Some("--uninstall-app") {
        let s = settings::Settings::load();
        let (title, text) = (i18n::tr(s.language, "uninstall_title"), i18n::tr(s.language, "uninstall_done"));
        match selfinstall::uninstall() {
            Ok(()) => selfinstall::message(title, text),
            Err(e) => selfinstall::message(title, &e),
        }
        return Ok(());
    }
    // Ninho.exe mcp: Model Context Protocol server on stdin/stdout, for AI coding agents.
    if args.get(1).map(String::as_str) == Some("mcp") {
        mcp::serve();
        return Ok(());
    }
    if cli::run(&args) {
        return Ok(());
    }
    // `Ninho.exe --install php nodejs mailpit`: the same download as the "Download and update" window, with no window;
    // the activity goes to <root>\tmp\ninho-install.log.
    if let Some(i) = args.iter().position(|a| a == "--install") {
        let s = settings::Settings::load();
        let shared = services::Shared::new();
        for tool in &args[i + 1..] {
            install::install(shared.clone(), s.clone(), tool.clone(), tool.clone());
        }
        let log = std::path::Path::new(&s.root).join("tmp").join("ninho-install.log");
        let _ = std::fs::write(log, shared.log.lock().unwrap().join("\r\n") + "\r\n");
        return Ok(());
    }
    let shot = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);

    // One Ninho at a time: opening it again shows the window that is already open (or hidden in the tray).
    if shot.is_none() && single::already_running(args.iter().any(|a| a == "--after-update")) {
        return Ok(());
    }
    single::gui_mode();
    // The copy replaced by the last update (Ninho.exe.old) can go now.
    selfupdate::cleanup_old();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Ninho")
        .with_inner_size([1000.0, 660.0])
        .with_min_inner_size([1000.0, 640.0]);
    if let Ok(img) = image::load_from_memory(include_bytes!("../assets/ninho-256.png")) {
        let img = img.to_rgba8();
        viewport = viewport.with_icon(std::sync::Arc::new(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }));
    }
    if shot.is_some() {
        // A check must never steal focus or cover what the person is doing.
        viewport = viewport.with_active(false);
        #[cfg(windows)]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Ninho", options, Box::new(move |cc| Ok(Box::new(app::NinhoApp::new(cc, shot)))))
}
