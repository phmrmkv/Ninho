//! Tray icon (next to the clock) with the WampServer-style status dot and a menu: show, start all, stop all, quit.
//! Events reach the app through channels; each one also asks egui to repaint, so they arrive even while the window
//! is hidden in the tray.
use std::sync::{OnceLock, mpsc};

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

static TRAY_TX: OnceLock<mpsc::Sender<TrayIconEvent>> = OnceLock::new();
static MENU_TX: OnceLock<mpsc::Sender<MenuEvent>> = OnceLock::new();

/// What the person asked for from the tray.
pub enum Ask {
    Show,
    StartAll,
    StopAll,
    Quit,
}

/// Status shown by the dot on the icon.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    AllOn,
    SomeOn,
    AllOff,
}

pub struct Tray {
    icon: TrayIcon,
    tray_rx: mpsc::Receiver<TrayIconEvent>,
    menu_rx: mpsc::Receiver<MenuEvent>,
    show: MenuId,
    start: MenuId,
    stop: MenuId,
    quit: MenuId,
    dot: Option<Dot>,
    tip: String,
}

/// The 64 px logo with a status dot in the bottom-right corner.
fn icon(dot: Dot) -> Option<Icon> {
    let mut img = image::load_from_memory(include_bytes!("../assets/ninho-64.png")).ok()?.to_rgba8();
    let (w, h) = (img.width() as i32, img.height() as i32);
    let color: [u8; 3] = match dot {
        Dot::AllOn => [0x2e, 0xc4, 0x6a],
        Dot::SomeOn => [0xf2, 0xa1, 0x2c],
        Dot::AllOff => [0x9a, 0xa6, 0xad],
    };
    let (cx, cy, r) = (w - 15, h - 15, 13);
    for y in (cy - r - 2).max(0)..(cy + r + 2).min(h) {
        for x in (cx - r - 2).max(0)..(cx + r + 2).min(w) {
            let d = (((x - cx) * (x - cx) + (y - cy) * (y - cy)) as f32).sqrt();
            let px = img.get_pixel_mut(x as u32, y as u32);
            if d <= r as f32 {
                *px = image::Rgba([color[0], color[1], color[2], 255]);
            } else if d <= r as f32 + 2.5 {
                // White ring so the dot reads on any taskbar color.
                *px = image::Rgba([255, 255, 255, 255]);
            }
        }
    }
    Icon::from_rgba(img.into_raw(), w as u32, h as u32).ok()
}

impl Tray {
    /// Must run on the main thread after the event loop started (the app's `new`).
    pub fn new(ctx: &egui::Context, texts: [&str; 4]) -> Option<Tray> {
        let (ttx, tray_rx) = mpsc::channel();
        let (mtx, menu_rx) = mpsc::channel();
        let _ = TRAY_TX.set(ttx);
        let _ = MENU_TX.set(mtx);
        let c1 = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |e| {
            if let Some(t) = TRAY_TX.get() {
                let _ = t.send(e);
            }
            c1.request_repaint();
        }));
        let c2 = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e| {
            if let Some(t) = MENU_TX.get() {
                let _ = t.send(e);
            }
            c2.request_repaint();
        }));
        let show = MenuItem::new(texts[0], true, None);
        let start = MenuItem::new(texts[1], true, None);
        let stop = MenuItem::new(texts[2], true, None);
        let quit = MenuItem::new(texts[3], true, None);
        let menu = Menu::new();
        menu.append_items(&[&show, &PredefinedMenuItem::separator(), &start, &stop, &PredefinedMenuItem::separator(), &quit]).ok()?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("Ninho")
            .with_icon(icon(Dot::AllOff)?)
            .build()
            .ok()?;
        Some(Tray {
            icon,
            tray_rx,
            menu_rx,
            show: show.id().clone(),
            start: start.id().clone(),
            stop: stop.id().clone(),
            quit: quit.id().clone(),
            dot: Some(Dot::AllOff),
            tip: String::new(),
        })
    }

    /// Everything clicked since the last frame.
    pub fn asks(&self) -> Vec<Ask> {
        let mut out = Vec::new();
        while let Ok(e) = self.tray_rx.try_recv() {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                out.push(Ask::Show);
            }
        }
        while let Ok(e) = self.menu_rx.try_recv() {
            if e.id == self.show {
                out.push(Ask::Show);
            } else if e.id == self.start {
                out.push(Ask::StartAll);
            } else if e.id == self.stop {
                out.push(Ask::StopAll);
            } else if e.id == self.quit {
                out.push(Ask::Quit);
            }
        }
        out
    }

    /// Update the dot and the tooltip, only when they change.
    pub fn status(&mut self, dot: Dot, tip: String) {
        if self.dot != Some(dot) {
            if let Some(i) = icon(dot) {
                let _ = self.icon.set_icon(Some(i));
            }
            self.dot = Some(dot);
        }
        if self.tip != tip {
            let _ = self.icon.set_tooltip(Some(&tip));
            self.tip = tip;
        }
    }
}
