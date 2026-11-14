//! Right-click menus: taskbar buttons, titlebars (the "window menu") and the desktop.
//! One menu at a time, drawn above everything but the cursor; any click outside closes it.

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, Size, SERIAL_COUNTER},
};

use crate::{
    canvas::{measure, Canvas},
    state::State,
    theme::th,
    ui::TASKBAR_H,
};

const ROW: i32 = 32;
const PAD: i32 = 4;
const MIN_W: i32 = 200;
/// Windoze 10's taskbar jump list: flush on the taskbar, taller rows, an icon on each.
const JUMP_W: i32 = 256;
const JUMP_ROW: i32 = 40;
const JUMP_PAD: i32 = 6;

#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    Launch(usize),
    Minimize(WlSurface),
    Restore(WlSurface),
    Maximize(WlSurface),
    Fullscreen(WlSurface),
    Close(WlSurface),
    /// "Close all windows" on a taskbar button with several.
    CloseAll(Vec<WlSurface>),
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
    /// Keep a window above all others, or stop.
    OnTop(WlSurface),
}

/// A row's icon in a jump list: the app's own, or one of our glyphs (SVG drawn in white).
pub enum MenuIcon {
    Image(std::rc::Rc<crate::icons::Icon>),
    Glyph(&'static str),
}

pub struct Menu {
    pub pos: Point<i32, Logical>,
    pub items: Vec<(String, Action)>,
    pub hover: Option<usize>,
    pub size: Size<i32, Logical>,
    pub(crate) picture: Option<(Option<usize>, MemoryRenderBuffer)>,
    /// Set for a taskbar jump list: one icon per item.
    jump: Option<Vec<Option<MenuIcon>>>,
}

impl Menu {
    pub fn rect(&self) -> Rectangle<i32, Logical> {
        Rectangle::new(self.pos, self.size)
    }

    /// Row index at a point (global coords), skipping separators.
    pub fn item_at(&self, p: Point<f64, Logical>) -> Option<usize> {
        let (pad, row) = self.metrics();
        let local = p - self.pos.to_f64();
        if local.x < 0.0 || local.x >= self.size.w as f64 || local.y < pad as f64 {
            return None;
        }
        let i = ((local.y as i32 - pad) / row) as usize;
        self.items.get(i).filter(|(_, a)| !matches!(a, Action::Separator | Action::Inert)).map(|_| i)
    }

    /// Top padding and row height.
    fn metrics(&self) -> (i32, i32) {
        if self.jump.is_some() { (JUMP_PAD, JUMP_ROW) } else { (PAD, ROW) }
    }

    pub fn picture(&mut self, font: Option<&fontdue::Font>) -> &MemoryRenderBuffer {
        if self.jump.is_some() && self.picture.as_ref().is_none_or(|(h, _)| *h != self.hover) {
            let buf = self.draw_jump_list(font);
            self.picture = Some((self.hover, buf));
        }
        if self.picture.as_ref().is_none_or(|(h, _)| *h != self.hover) {
            let mut c = Canvas::new(self.size.w, self.size.h);
            c.fill(th().menu);
            c.outline(0, 0, self.size.w, self.size.h, th().border);
            for (i, (label, action)) in self.items.iter().enumerate() {
                let y = PAD + i as i32 * ROW;
                if *action == Action::Separator {
                    c.rect(12, y + ROW / 2, self.size.w - 24, 1, th().border);
                    continue;
                }
                if self.hover == Some(i) {
                    c.rect(1, y, self.size.w - 2, ROW, th().menu_hover);
                }
                if let Some(f) = font {
                    // "Label\tShortcut": shortcut right-aligned and dimmed, like Windoze menus.
                    let (text, shortcut) = label.split_once('\t').unwrap_or((label, ""));
                    let color = if *action == Action::Inert { th().disabled } else { th().text };
                    c.text(f, 13.0, 16, y, ROW, color, text);
                    if !shortcut.is_empty() {
                        let sw = measure(f, 13.0, shortcut);
                        c.text(f, 13.0, self.size.w - 16 - sw, y, ROW, th().disabled, shortcut);
                    }
                }
            }
            self.picture = Some((self.hover, c.into_buffer()));
        }
        &self.picture.as_ref().unwrap().1
    }
}

impl Menu {
    fn draw_jump_list(&self, font: Option<&fontdue::Font>) -> MemoryRenderBuffer {
        let mut c = Canvas::new(self.size.w, self.size.h);
        c.fill(th().menu);
        let icons = self.jump.as_deref().unwrap_or(&[]);
        for (i, (label, action)) in self.items.iter().enumerate() {
            let y = JUMP_PAD + i as i32 * JUMP_ROW;
            if *action == Action::Separator {
                c.rect(0, y + JUMP_ROW / 2, self.size.w, 1, th().border);
                continue;
            }
            if self.hover == Some(i) {
                c.rect(0, y, self.size.w, JUMP_ROW, th().menu_hover);
            }
            let mid = y + JUMP_ROW / 2;
            match icons.get(i).and_then(|i| i.as_ref()) {
                Some(MenuIcon::Image(icon)) => c.icon(icon, 16, mid - icon.h as i32 / 2),
                Some(MenuIcon::Glyph(svg)) => crate::tray::glyph(&mut c, (*svg).into(), 24.0, mid as f32, 16),
                None => {}
            }
            if let Some(f) = font {
                let color = if *action == Action::Inert { th().disabled } else { th().text };
                c.text(f, 14.0, 48, y, JUMP_ROW, color, &crate::canvas::ellipsize(f, 14.0, label, self.size.w - 60));
            }
        }
        c.into_buffer()
    }
}

// Jump list glyphs (20x20, white), after Windoze 10's.
const PIN: &str = r##"<path d="M11.8 2.6l5.6 5.6-1.6.6-3.2 3.2.3 3.2-1.4 1.4-3.6-3.6-4.8 4.8M8.3 9.5l-3-.3-1.4 1.4 7.6 7.6M11.8 2.6l-.6 1.6-3 3" fill="none" stroke="#fff" stroke-width="1.1" stroke-linejoin="round"/>"##;
const UNPIN: &str = r##"<path d="M11.8 2.6l5.6 5.6-1.6.6-3.2 3.2.3 3.2-1.4 1.4-3.6-3.6-4.8 4.8M8.3 9.5l-3-.3-1.4 1.4 7.6 7.6M11.8 2.6l-.6 1.6-3 3M2.5 2.5l15 15" fill="none" stroke="#fff" stroke-width="1.1" stroke-linejoin="round"/>"##;
const CLOSE: &str = r##"<path d="M4.5 4.5l11 11M15.5 4.5l-11 11" stroke="#fff" stroke-width="1.2"/>"##;
const END: &str = r##"<circle cx="10" cy="10" r="6.8" fill="none" stroke="#fff" stroke-width="1.2"/><path d="M5.2 14.8l9.6-9.6" stroke="#fff" stroke-width="1.2"/>"##;

impl State {
    /// A taskbar button's jump list, flush on the taskbar and centred on the button at `x`.
    pub fn open_jump_list(&mut self, x: i32, rows: Vec<(String, Action, Option<MenuIcon>)>) {
        let h = rows.len() as i32 * JUMP_ROW + JUMP_PAD * 2;
        let pos = Point::from(((x - JUMP_W / 2).clamp(0, (self.output_size.w - JUMP_W).max(0)), self.output_size.h - TASKBAR_H - h));
        let (items, icons): (Vec<_>, Vec<_>) = rows.into_iter().map(|(l, a, i)| ((l, a), i)).unzip();
        self.release_pointer();
        self.menu = Some(Menu { pos, items, hover: None, size: Size::from((JUMP_W, h)), picture: None, jump: Some(icons) });
        self.needs_redraw = true;
    }

    /// The jump list rows every app gets: launch it, pin or unpin it.
    fn jump_app_rows(&mut self, app: usize) -> Vec<(String, Action, Option<MenuIcon>)> {
        let a = self.apps[app].clone();
        let icon = crate::ui::app_icon(&mut self.icons, &a.icon, 16).map(MenuIcon::Image);
        let pinned = self.pins.taskbar.contains(&a.id);
        vec![
            (a.name.clone(), Action::Launch(app), icon),
            (
                if pinned { "Unpin from taskbar" } else { "Pin to taskbar" }.into(),
                Action::PinTaskbar(a.id.clone()),
                Some(MenuIcon::Glyph(if pinned { UNPIN } else { PIN })),
            ),
        ]
    }

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
        self.release_pointer();
        self.menu = Some(Menu { pos, items, hover: None, size, picture: None, jump: None });
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
        // Windoze 10's window menu: entries that don't apply are greyed, not hidden.
        let resized = w.maximized.is_some() || w.fullscreen.is_some();
        let mut items = vec![
            ("Restore".into(), if resized { Action::Restore(s.clone()) } else { Action::Inert }),
            ("Minimize".into(), Action::Minimize(s.clone())),
            ("Maximize".into(), if w.maximized.is_none() { Action::Maximize(s.clone()) } else { Action::Inert }),
            (String::new(), Action::Separator),
            (if w.on_top { "Unpin from top" } else { "Always on top\tWin+Shift+P" }.into(), Action::OnTop(s.clone())),
            (String::new(), Action::Separator),
            ("Close\tAlt+F4".into(), Action::Close(s.clone())),
        ];
        // A hung app won't answer Close; Windoze offers to end it then, so do we.
        if w.unresponsive {
            items.push(("End process".into(), Action::Kill(s)));
        }
        self.open_menu(at, items);
    }

    /// Right-click on a taskbar button: Windoze 10's jump list for the app, with its windows'
    /// Close (all) and End process.
    pub fn open_taskbar_menu(&mut self, windows: &[WlSurface], x: i32) {
        let Some(last) = windows.last().cloned() else { return };
        let Some(w) = self.wm.find(&last) else { return };
        let pinned = w.on_top;
        let app = crate::apps::match_app(&self.apps, &w.app_id()).and_then(|a| self.apps.iter().position(|b| b.id == a.id));
        let mut rows = match app {
            Some(i) => self.jump_app_rows(i),
            // No .desktop file: the window's own name and icon.
            None => {
                let icon = crate::ui::app_icon(&mut self.icons, &w.app_id(), 16).map(MenuIcon::Image);
                vec![(w.title(), Action::Inert, icon)]
            }
        };
        if windows.len() > 1 {
            rows.push(("Close all windows".into(), Action::CloseAll(windows.to_vec()), Some(MenuIcon::Glyph(CLOSE))));
        } else {
            rows.push(("Close window".into(), Action::Close(last.clone()), Some(MenuIcon::Glyph(CLOSE))));
        }
        let on_top = if pinned { "Unpin from top" } else { "Always on top" };
        rows.push((on_top.into(), Action::OnTop(last.clone()), None));
        rows.push(("End process".into(), Action::Kill(last), Some(MenuIcon::Glyph(END))));
        self.open_jump_list(x, rows);
    }

    /// A pinned app with no windows.
    pub fn open_pinned_jump_list(&mut self, app: usize, x: i32) {
        let rows = self.jump_app_rows(app);
        self.open_jump_list(x, rows);
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
            // _NET_WM_PID is only a hint, and inside a Flatpak it's the sandbox's own numbering;
            // `kill_window_process` asks the X server instead.
            Some(x) => x.pid(),
            None => {
                use smithay::reexports::wayland_server::Resource;
                let client = self.dh.get_client(surface.id()).ok()?;
                client.get_credentials(&self.dh).ok().map(|c| c.pid as u32)
            }
        }
    }

    /// End process: SIGKILL the app that owns the window, now. For X11 windows the X server
    /// knows the real PID (XRes), which is right even for Flatpak apps; asking it is a blocking
    /// round trip to Xwayland, which this thread serves, so that happens on a thread of its own.
    pub fn kill_window_process(&mut self, surface: &WlSurface) {
        let Some(w) = self.wm.find(surface) else { return };
        let ours = std::process::id();
        let xwayland = self.xwayland_pid;
        let safe = move |pid: u32| pid > 1 && pid != ours && Some(pid) != xwayland;
        match (w.x11(), self.xdisplay) {
            (Some(x), Some(display)) => {
                let (window, title) = (x.window_id(), w.title());
                std::thread::spawn(move || match x11_client_pid(display, window).filter(|&p| safe(p)) {
                    Some(pid) => sigkill(pid, &title),
                    None => tracing::warn!("end process: no PID for X11 window {window:#x} ({title})"),
                });
            }
            _ => match self.window_pid(surface).filter(|&p| safe(p)) {
                Some(pid) => sigkill(pid, &w.title()),
                None => {
                    tracing::warn!("end process: couldn't find a PID; asking the window to close instead");
                    w.close();
                }
            },
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
            Action::OnTop(s) => self.toggle_on_top(&s),
            Action::Maximize(s) => {
                let area = self.work_area_of(&s);
                self.wm.maximize(&s, area);
                self.focus(Some(s), serial);
            }
            Action::Fullscreen(s) => {
                let rect = self.screen_of(&s);
                self.wm.fullscreen(&s, rect);
                self.focus(Some(s), serial);
            }
            Action::CloseAll(list) => {
                for s in &list {
                    if let Some(w) = self.wm.find(s) {
                        w.close();
                    }
                }
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

fn sigkill(pid: u32, title: &str) {
    tracing::info!("end process: SIGKILL {pid} ({title})");
    if let Err(e) = std::process::Command::new("kill").args(["-KILL", &pid.to_string()]).status() {
        tracing::warn!("end process: {e}");
    }
}

/// The PID of the X client owning `window`, from the X server's own records (XRes).
fn x11_client_pid(display: u32, window: u32) -> Option<u32> {
    use x11rb::protocol::res::{query_client_ids, ClientIdMask, ClientIdSpec};
    let (conn, _) = x11rb::rust_connection::RustConnection::connect(Some(&format!(":{display}"))).ok()?;
    let spec = ClientIdSpec { client: window, mask: ClientIdMask::LOCAL_CLIENT_PID };
    let reply = query_client_ids(&conn, &[spec]).ok()?.reply().ok()?;
    reply.ids.iter().find(|id| id.spec.mask == ClientIdMask::LOCAL_CLIENT_PID).and_then(|id| id.value.first().copied())
}
