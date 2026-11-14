//! One Ninho window at a time. The first copy owns a named mutex and waits on a named event; opening Ninho again
//! (Start menu, desktop, a second double click) just fires that event, and the first copy shows its window — even
//! when it was hidden in the tray. Two copies would fight over the same ports and settings.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
mod win {
    pub type Handle = *mut core::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreateMutexW(attrs: *mut core::ffi::c_void, initial_owner: i32, name: *const u16) -> Handle;
        pub fn CreateEventW(attrs: *mut core::ffi::c_void, manual_reset: i32, initial_state: i32, name: *const u16) -> Handle;
        pub fn OpenEventW(access: u32, inherit: i32, name: *const u16) -> Handle;
        pub fn SetEvent(event: Handle) -> i32;
        pub fn WaitForSingleObject(handle: Handle, ms: u32) -> u32;
        pub fn GetLastError() -> u32;
        pub fn CloseHandle(handle: Handle) -> i32;
    }
    pub const ERROR_ALREADY_EXISTS: u32 = 183;
    pub const EVENT_MODIFY_STATE: u32 = 0x0002;
    pub const INFINITE: u32 = 0xFFFF_FFFF;
    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

/// Per Windows user session, so two people on one PC each get their own Ninho.
const MUTEX: &str = "Local\\Ninho.SingleInstance";
const SHOW: &str = "Local\\Ninho.Show";

/// True when another Ninho is already open: it was asked to show its window, and this copy should quit.
/// `wait`: right after an update the old copy is still closing; wait up to 30 s for it instead of quitting.
pub fn already_running(wait: bool) -> bool {
    #[cfg(windows)]
    unsafe {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let m = win::CreateMutexW(std::ptr::null_mut(), 0, win::wide(MUTEX).as_ptr());
            if !m.is_null() && win::GetLastError() == win::ERROR_ALREADY_EXISTS {
                if wait && std::time::Instant::now() < deadline {
                    win::CloseHandle(m);
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    continue;
                }
                let e = win::OpenEventW(win::EVENT_MODIFY_STATE, 0, win::wide(SHOW).as_ptr());
                if e.is_null() {
                    // The other copy is not answering (it may run as administrator): say so instead of vanishing.
                    let s = crate::settings::Settings::load();
                    crate::selfinstall::message("Ninho", crate::i18n::tr(s.language, "already_running"));
                } else {
                    win::SetEvent(e);
                    win::CloseHandle(e);
                }
                return true;
            }
            break;
        }
        // The mutex handle stays open for the life of this copy (closing it would let a second copy start).
        // The event is made now too, so a copy opened while this window is still starting finds it.
        let _ = win::CreateEventW(std::ptr::null_mut(), 0, 0, win::wide(SHOW).as_ptr());
    }
    false
}

static GUI: AtomicBool = AtomicBool::new(false);

/// The window app is starting: from now on a crash also shows a message (never in mcp/cli modes, which have no one to click OK).
pub fn gui_mode() {
    GUI.store(true, Ordering::SeqCst);
}

/// In the first copy: a thread that waits for "show yourself" and raises `flag` (the UI then shows the window).
pub fn listen(flag: Arc<AtomicBool>, ctx: eframe::egui::Context) {
    #[cfg(windows)]
    std::thread::spawn(move || unsafe {
        let e = win::CreateEventW(std::ptr::null_mut(), 0, 0, win::wide(SHOW).as_ptr());
        if e.is_null() {
            return;
        }
        loop {
            if win::WaitForSingleObject(e, win::INFINITE) == 0 {
                flag.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            }
        }
    });
    #[cfg(not(windows))]
    let _ = (flag, ctx);
}

/// Crash log: the reason, where it happened and the call stack go to <root>\tmp\ninho-crash.log, and Windows shows
/// a message with that path (the window is gone by then).
pub fn crash_log() {
    std::panic::set_hook(Box::new(|info| {
        let s = crate::settings::Settings::load();
        let dir = std::path::Path::new(&s.root).join("tmp");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("ninho-crash.log");
        let text = format!(
            "Ninho {} ({})\n{}\n\n{}\n\n{}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::ARCH,
            crate::services::clock(),
            info,
            std::backtrace::Backtrace::force_capture()
        );
        // Appended, so a second panic (often a poisoned lock after the first) does not hide the real cause.
        let _ = std::io::Write::write_all(&mut std::fs::OpenOptions::new().create(true).append(true).open(&file).unwrap_or_else(|_| std::fs::File::create(std::env::temp_dir().join("ninho-crash.log")).expect("crash log")), format!("{text}\n").as_bytes());
        eprintln!("{text}");
        if !GUI.load(Ordering::SeqCst) {
            return;
        }
        let msg = format!("{}\n\n{}", crate::i18n::tr(s.language, "crash_text"), file.display());
        crate::selfinstall::message("Ninho", &msg);
    }));
}
