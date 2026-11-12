//! Right-click menus: taskbar buttons, titlebars (the "window menu") and the desktop.
//! One menu at a time, drawn above everything but the cursor; any click outside closes it.

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, Size, SERIAL_COUNTER},
};

use crate::{
    canvas::{hex, measure, Canvas, Rgba},
    state::State,
    ui::TASKBAR_H,
};

const ROW: i32 = 32;
const PAD: i32 = 4;
const MIN_W: i32 = 200;
const BG: Rgba = hex(0x2b2b2b);
const BORDER: Rgba = hex(0x4a4a4a);
const HOVER: Rgba = Rgba(0xff, 0xff, 0xff, 0x1f);
const TEXT: Rgba = hex(0xffffff);
const TEXT_DIM: Rgba = hex(0x7a7a7a);

#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    Launch(usize),
    Minimize(WlSurface),
    Restore(WlSurface),
    Maximize(WlSurface),
    Fullscreen(WlSurface),
    Close(WlSurface),
    /// SIGKILL the window's process: for apps that won't close.
    Kill(WlSurface),
    Spawn(String),
    Screenshot,
    About,
    /// A tray app's own menu entry: (tray item id, dbusmenu entry id).
    TrayMenu(String, i32),
    /// Shown but not clickable (disabled entries in app menus).
    Inert,
    /// A separator line.
    Separator,
    /// Pin or unpin an app (by desktop-file id).
    PinStart(String),
    PinTaskbar(String),
    /// Start's account button: Sign out.
    SignOut,
}

pub struct Menu {
    pub pos: Point<i32, Logical>,
    pub items: Vec<(String, Action)>,
    pub hover: Option<usize>,
    pub size: Size<i32, Logical>,
    picture: Option<(Option<usize>, MemoryRenderBuffer)>,
}

impl Menu {
    pub fn rect(&self) -> Rectangle<i32, Logical> {
        Rectangle::new(self.pos, self.size)
    }

    /// Row index at a point (global coords), skipping separators.
    pub fn item_at(&self, p: Point<f64, Logical>) -> Option<usize> {
        let local = p - self.pos.to_f64();
        if local.x < 0.0 || local.x >= self.size.w as f64 || local.y < PAD as f64 {
            return None;
        }
        let i = ((local.y as i32 - PAD) / ROW) as usize;
        self.items.get(i).filter(|(_, a)| !matches!(a, Action::Separator | Action::Inert)).map(|_| i)
    }

    pub fn picture(&mut self, font: Option<&fontdue::Font>) -> &MemoryRenderBuffer {
        if self.picture.as_ref().is_none_or(|(h, _)| *h != self.hover) {
            let mut c = Canvas::new(self.size.w, self.size.h);
            c.fill(BG);
            c.outline(0, 0, self.size.w, self.size.h, BORDER);
            for (i, (label, action)) in self.items.iter().enumerate() {
                let y = PAD + i as i32 * ROW;
                if *action == Action::Separator {
                    c.rect(12, y + ROW / 2, self.size.w - 24, 1, BORDER);
                    continue;
                }
                if self.hover == Some(i) {
                    c.rect(1, y, self.size.w - 2, ROW, HOVER);
                }
                if let Some(f) = font {
                    // "Label\tShortcut": shortcut right-aligned and dimmed, like Windows menus.
                    let (text, shortcut) = label.split_once('\t').unwrap_or((label, ""));
                    let color = if *action == Action::Inert { TEXT_DIM } else { TEXT };
                    c.text(f, 13.0, 16, y, ROW, color, text);
                    if !shortcut.is_empty() {
                        let sw = measure(f, 13.0, shortcut);
                        c.text(f, 13.0, self.size.w - 16 - sw, y, ROW, TEXT_DIM, shortcut);
                    }
                }
            }
            self.picture = Some((self.hover, c.into_buffer()));
        }
        &self.picture.as_ref().unwrap().1
    }
}

impl State {
    /// Open a menu with its top-left near `at`, kept on screen.
    pub fn open_menu(&mut self, at: Point<i32, Logical>, items: Vec<(String, Action)>) {
        let text_w = self
            .font
            .as_ref()
            .map(|f| items.iter().map(|(l, _)| measure(f, 13.0, &l.replace('\t', "      "))).max().unwrap_or(0))
            .unwrap_or(120);
        let size = Size::from(((text_w + 48).max(MIN_W), items.len() as i32 * ROW + PAD * 2));
        let screen = self.output_size;
        let pos = Point::from((
            at.x.clamp(0, (screen.w - size.w).max(0)),
            at.y.clamp(0, (screen.h - size.h).max(0)),
        ));
        self.menu = Some(Menu { pos, items, hover: None, size, picture: None });
        self.needs_redraw = true;
    }

    pub fn close_menu(&mut self) {
        if self.menu.take().is_some() {
            self.needs_redraw = true;
        }
    }

    /// The window menu: titlebar right-click, Alt+Space, or an app asking for it.
    pub fn open_window_menu(&mut self, surface: &WlSurface, at: Point<i32, Logical>) {
        let Some(w) = self.wm.find(surface) else { return };
        let s = surface.clone();
        let mut items = Vec::new();
        if w.maximized.is_some() || w.fullscreen.is_some() {
            items.push(("Restore".into(), Action::Restore(s.clone())));
        }
        items.push(("Minimize".into(), Action::Minimize(s.clone())));
        if w.maximized.is_none() {
            items.push(("Maximize".into(), Action::Maximize(s.clone())));
        }
        if w.fullscreen.is_none() {
            items.push(("Fullscreen".into(), Action::Fullscreen(s.clone())));
        }
        items.push((String::new(), Action::Separator));
        items.push(("Close\tAlt+F4".into(), Action::Close(s.clone())));
        items.push(("End process".into(), Action::Kill(s)));
        self.open_menu(at, items);
    }

    /// Right-click on a taskbar button: Windows-style jump list, anchored above the taskbar.
    pub fn open_taskbar_menu(&mut self, surface: &WlSurface, x: i32) {
        let Some(w) = self.wm.find(surface) else { return };
        let s = surface.clone();
        let app = crate::apps::match_app(&self.apps, &w.app_id()).and_then(|a| self.apps.iter().position(|b| b.id == a.id));
        let mut items = Vec::new();
        if let Some(i) = app {
            let id = self.apps[i].id.clone();
            let pin = if self.pins.taskbar.contains(&id) { "Unpin from taskbar" } else { "Pin to taskbar" };
            items.push((self.apps[i].name.clone(), Action::Launch(i)));
            items.push((pin.into(), Action::PinTaskbar(id)));
            items.push((String::new(), Action::Separator));
        }
        if w.minimized {
            items.push(("Restore".into(), Action::Restore(s.clone())));
        } else {
            items.push(("Minimize".into(), Action::Minimize(s.clone())));
        }
        if w.maximized.is_some() {
            items.push(("Restore size".into(), Action::Restore(s.clone())));
        } else {
            items.push(("Maximize".into(), Action::Maximize(s.clone())));
        }
        items.push(("Close window".into(), Action::Close(s.clone())));
        items.push(("End process".into(), Action::Kill(s)));
        self.open_menu_above_taskbar(x, items);
    }

    pub fn open_menu_above_taskbar(&mut self, x: i32, items: Vec<(String, Action)>) {
        let h = items.len() as i32 * ROW + PAD * 2;
        self.open_menu((x - MIN_W / 2, self.output_size.h - TASKBAR_H - h).into(), items);
    }

    /// A tray app sent us its menu: show it above the taskbar, under its icon.
    pub fn open_tray_menu(&mut self, id: String, entries: Vec<crate::sni::MenuEntry>) {
        let items: Vec<(String, Action)> = entries
            .into_iter()
            .map(|e| {
                if e.separator {
                    (String::new(), Action::Separator)
                } else {
                    let label = format!("{}{}", "    ".repeat(e.depth), e.label);
                    (label, if e.enabled { Action::TrayMenu(id.clone(), e.id) } else { Action::Inert })
                }
            })
            .collect();
        if items.is_empty() {
            return;
        }
        let h = items.len() as i32 * ROW + PAD * 2;
        let x = self.tray_menu_x;
        self.open_menu((x - MIN_W / 2, self.output_size.h - TASKBAR_H - h).into(), items);
    }

    /// The process behind a window: X11 windows report their own PID (their Wayland
    /// connection is Xwayland's, which must never be killed); Wayland apps by socket.
    pub fn window_pid(&self, surface: &WlSurface) -> Option<u32> {
        let w = self.wm.find(surface)?;
        match w.x11() {
            Some(x) => x.pid(),
            None => {
                use smithay::reexports::wayland_server::Resource;
                let client = self.dh.get_client(surface.id()).ok()?;
                client.get_credentials(&self.dh).ok().map(|c| c.pid as u32)
            }
        }
    }

    pub fn kill_window_process(&mut self, surface: &WlSurface) {
        let Some(pid) = self.window_pid(surface).filter(|&p| p > 1 && p != std::process::id()) else {
            tracing::warn!("end process: couldn't find a PID; asking the window to close instead");
            if let Some(w) = self.wm.find(surface) {
                w.close();
            }
            return;
        };
        tracing::info!("end process: SIGKILL {pid}");
        if let Err(e) = std::process::Command::new("kill").args(["-9", &pid.to_string()]).spawn() {
            tracing::warn!("end process: {e}");
        }
    }

    pub fn open_desktop_menu(&mut self, at: Point<i32, Logical>) {
        let items = vec![
            ("Open terminal".into(), Action::Spawn(crate::input::terminal())),
            ("Take screenshot".into(), Action::Screenshot),
            (String::new(), Action::Separator),
            ("About VIDE-DE".into(), Action::About),
        ];
        self.open_menu(at, items);
    }

    pub fn run_menu_action(&mut self, action: Action) {
        self.close_menu();
        let serial = SERIAL_COUNTER.next_serial();
        match action {
            Action::Launch(i) => {
                let app = self.apps[i].clone();
                self.launch(&app);
            }
            Action::PinStart(id) => {
                self.pins.toggle_start(&id);
                self.refresh_start();
            }
            Action::PinTaskbar(id) => {
                self.pins.toggle_taskbar(&id);
                self.taskbar = None;
            }
            Action::Minimize(s) => self.minimize(&s),
            Action::Restore(s) => {
                self.wm.unfullscreen(&s);
                self.wm.unmaximize(&s);
                self.focus(Some(s), serial);
            }
            Action::Maximize(s) => {
                let area = self.work_area();
                self.wm.maximize(&s, area);
                self.focus(Some(s), serial);
            }
            Action::Fullscreen(s) => {
                let rect = self.output_rect();
                self.wm.fullscreen(&s, rect);
                self.focus(Some(s), serial);
            }
            Action::Close(s) => {
                if let Some(w) = self.wm.find(&s) {
                    w.close();
                }
            }
            Action::Kill(s) => self.kill_window_process(&s),
            Action::Spawn(cmd) => self.spawn(&cmd),
            Action::Screenshot => self.screenshot_requested = Some(None),
            Action::SignOut => self.power(crate::ui::PowerAction::SignOut),
            Action::About => {
                // The About app ships next to our own binary.
                let about = std::env::current_exe()
                    .ok()
                    .and_then(|p| Some(p.parent()?.join("vide-about")))
                    .filter(|p| p.exists())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "vide-about".into());
                self.spawn(&format!("'{about}'"));
            }
            Action::TrayMenu(id, entry) => {
                if let Some(tx) = &self.sni_tx {
                    let _ = tx.send(crate::sni::Cmd::MenuClick { id, entry });
                }
            }
            Action::Separator | Action::Inert => {}
        }
        self.needs_redraw = true;
    }
}
