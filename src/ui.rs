//! Our chrome, styled after Windows 10: bottom taskbar, start menu, light
//! titlebars with min/max/close. Layout and look live here; painting goes
//! through `canvas`. Every picture is keyed on what it shows and only
//! repainted when that key changes.

use fontdue::Font;
use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Rectangle},
};

use crate::{
    canvas::{ellipsize, hex, measure, Canvas, Rgba},
    icons::Icons,
};

const fn rgba(c: u32, a: u8) -> Rgba {
    let h = hex(c);
    Rgba(h.0, h.1, h.2, a)
}

// Palette
pub const ACCENT: Rgba = hex(0x0078d7);
const TASKBAR_BG: Rgba = hex(0x101010);
const TASK_HOVER: Rgba = rgba(0xffffff, 0x1a);
const TASK_ACTIVE: Rgba = rgba(0xffffff, 0x26);
const TASK_LINE: Rgba = hex(0x76b9ed);
const SEARCH_BG: Rgba = hex(0x3a3a3a);
const WHITE: Rgba = hex(0xffffff);
const TEXT_DIM: Rgba = hex(0xa0a0a0);
const TITLE_BG: Rgba = hex(0xffffff);
const TITLE_TEXT: Rgba = hex(0x000000);
const TITLE_TEXT_IDLE: Rgba = hex(0x999999);
const BTN_HOVER: Rgba = hex(0xe5e5e5);
const CLOSE_HOVER: Rgba = hex(0xe81123);
const MENU_BG: Rgba = rgba(0x2b2b2b, 0xf5);
const MENU_RAIL: Rgba = rgba(0x1f1f1f, 0xf5);

pub const TASKBAR_H: i32 = 40;
pub const TITLE_H: i32 = 30;
pub const TITLE_BTN_W: i32 = 46;
pub const START_W: i32 = 48;
const SEARCH_W: i32 = 300;
/// Right edge of the search box: where the Task View button sits.
pub const SEARCH_END: i32 = START_W + SEARCH_W;
pub const TASK_W: i32 = 48;
/// Where the first taskbar button starts (after Start, search and Task View).
pub const TASK_BUTTONS_X: i32 = SEARCH_END + TASK_W + 4;
const CLOCK_W: i32 = 88;
const TRAY_W: i32 = 34;
const SHOW_DESKTOP_W: i32 = 6;
pub const MENU_W: i32 = 360;
const MENU_ROW: i32 = 36;
const MENU_TOP: i32 = 52;
const RAIL_W: i32 = 48;

// ---------------------------------------------------------------- taskbar

#[derive(Clone, PartialEq, Debug)]
pub enum TaskbarHit {
    Start,
    Search,
    Volume,
    Brightness,
    Network,
    Bluetooth,
    /// The clock: opens the Action Center (notifications).
    Clock,
    /// An app's tray icon, by its tray id.
    TrayApp(String),
    /// A taskbar button, by index into `TaskbarKey::items`.
    Button(usize),
    TaskView,
    ShowDesktop,
    Nothing,
}

#[derive(Clone, PartialEq)]
pub struct TaskItem {
    /// Windows behind this button, oldest first. None: a pinned app that isn't running.
    pub windows: Vec<WlSurface>,
    pub title: String,
    /// Icon name (an app_id we look up variations of).
    pub icon: String,
    /// Desktop-file id, when pinned.
    pub pinned: Option<String>,
    pub focused: bool,
}

#[derive(Clone, PartialEq)]
pub struct TaskbarKey {
    pub width: i32,
    pub items: Vec<TaskItem>,
    pub time: String,
    pub date: String,
    pub hover: Option<TaskbarHit>,
    pub start_open: bool,
    /// Volume percent and mute, for the tray icon.
    pub volume: (u32, bool),
    /// Radio state, so the network and Bluetooth glyphs show on/off.
    pub wifi_on: bool,
    pub bt_on: bool,
    pub flyout: Option<crate::tray::Flyout>,
    pub current_workspace: usize,
    pub taskview_open: bool,
    /// App tray icons: (tray id, something that changes when the icon does).
    pub tray_apps: Vec<(String, String)>,
}

pub struct Taskbar {
    pub key: TaskbarKey,
    pub buffer: MemoryRenderBuffer,
    /// Hit rects relative to the taskbar's top-left.
    pub hits: Vec<(Rectangle<i32, Logical>, TaskbarHit)>,
}

pub fn draw_taskbar(
    font: Option<&Font>,
    icons: &mut Icons,
    logo: Option<&crate::icons::Icon>,
    tray_icons: &[Option<std::rc::Rc<crate::icons::Icon>>],
    key: TaskbarKey,
) -> Taskbar {
    let w = key.width;
    let mut c = Canvas::new(w, TASKBAR_H);
    let mut hits = Vec::new();
    let hovered = |h: &TaskbarHit| key.hover.as_ref() == Some(h);
    c.fill(TASKBAR_BG);

    // Start button: our logo (a chevron if it somehow failed to load).
    let start_bg = if key.start_open { TASK_ACTIVE } else if hovered(&TaskbarHit::Start) { TASK_HOVER } else { TASKBAR_BG };
    c.rect(0, 0, START_W, TASKBAR_H, start_bg);
    match logo {
        Some(l) => c.icon(l, (START_W - l.w as i32) / 2, (TASKBAR_H - l.h as i32) / 2),
        None => {
            let mark = if key.start_open || hovered(&TaskbarHit::Start) { TASK_LINE } else { WHITE };
            c.line(16.0, 14.0, 24.0, 26.0, 2.5, mark);
            c.line(24.0, 26.0, 32.0, 14.0, 2.5, mark);
        }
    }
    hits.push((rect(0, 0, START_W, TASKBAR_H), TaskbarHit::Start));

    // Search box (opens the start menu, which has the real search).
    let sx = START_W;
    c.rect(sx, 0, SEARCH_W, TASKBAR_H, if hovered(&TaskbarHit::Search) { hex(0x474747) } else { SEARCH_BG });
    magnifier(&mut c, sx as f32 + 22.0, TASKBAR_H as f32 / 2.0, WHITE, SEARCH_BG);
    if let Some(f) = font {
        c.text(f, 13.0, sx + 42, 0, TASKBAR_H, TEXT_DIM, "Type here to search");
    }
    hits.push((rect(sx, 0, SEARCH_W, TASKBAR_H), TaskbarHit::Search));

    // Task View: one button for virtual desktops (two stacked windows, current desktop number).
    let tv_x = SEARCH_END;
    let tv_bg = if key.taskview_open { TASK_ACTIVE } else if hovered(&TaskbarHit::TaskView) { TASK_HOVER } else { TASKBAR_BG };
    c.rect(tv_x, 0, TASK_W, TASKBAR_H, tv_bg);
    let (cx, cy) = (tv_x + TASK_W / 2, TASKBAR_H / 2);
    c.outline(cx - 9, cy - 5, 14, 11, WHITE);
    c.rect(cx - 5, cy - 9, 14, 1, WHITE);
    c.rect(cx + 8, cy - 9, 1, 11, WHITE);
    if let Some(f) = font {
        let n = (key.current_workspace + 1).to_string();
        let tw = measure(f, 10.0, &n);
        c.round_rect(cx + 5, cy + 3, tw + 6, 12, 3, ACCENT);
        c.text(f, 10.0, cx + 8, cy + 3, 12, WHITE, &n);
    }
    if key.taskview_open {
        c.rect(tv_x, TASKBAR_H - 2, TASK_W, 2, TASK_LINE);
    }
    hits.push((rect(tv_x, 0, TASK_W, TASKBAR_H), TaskbarHit::TaskView));

    // Window buttons.
    let right_edge = w - CLOCK_W - SHOW_DESKTOP_W;
    const TRAY_ITEMS: i32 = 4;
    let apps_x = right_edge - TRAY_W * TRAY_ITEMS - TRAY_W * key.tray_apps.len() as i32;
    let mut x = TASK_BUTTONS_X;
    for (i, item) in key.items.iter().enumerate() {
        if x + TASK_W > apps_x {
            break;
        }
        let hit = TaskbarHit::Button(i);
        let bg = if item.focused { TASK_ACTIVE } else if hovered(&hit) { TASK_HOVER } else { TASKBAR_BG };
        c.rect(x, 0, TASK_W, TASKBAR_H, bg);
        match app_icon(icons, &item.icon, 24) {
            Some(icon) => c.icon(&icon, x + (TASK_W - 24) / 2, (TASKBAR_H - 24) / 2),
            None => {
                // No icon: first letter of the title in a tile.
                c.round_rect(x + 12, 8, 24, 24, 4, ACCENT);
                if let (Some(f), Some(ch)) = (font, item.title.chars().next()) {
                    let s = ch.to_uppercase().to_string();
                    let tw = measure(f, 14.0, &s);
                    c.text(f, 14.0, x + 24 - tw / 2, 8, 24, WHITE, &s);
                }
            }
        }
        // Open-window underline; full width when focused, none for a pinned app that isn't running.
        if item.focused {
            c.rect(x, TASKBAR_H - 2, TASK_W, 2, TASK_LINE);
        } else if !item.windows.is_empty() {
            c.rect(x + 6, TASKBAR_H - 2, TASK_W - 12, 2, TASK_LINE);
        }
        // Several windows: a second, offset line, like Windows' stacked-button edge.
        if item.windows.len() > 1 {
            c.rect(x + TASK_W - 3, 4, 1, TASKBAR_H - 8, hex(0x3a3a3a));
        }
        hits.push((rect(x, 0, TASK_W, TASKBAR_H), hit));
        x += TASK_W;
    }

    // App tray icons (Discord, Steam, ...), left of our own.
    for (i, (id, _)) in key.tray_apps.iter().enumerate() {
        let bx = apps_x + i as i32 * TRAY_W;
        let hit = TaskbarHit::TrayApp(id.clone());
        if hovered(&hit) {
            c.rect(bx, 0, TRAY_W, TASKBAR_H, TASK_HOVER);
        }
        match tray_icons.get(i).and_then(|o| o.as_ref()) {
            Some(icon) => c.icon(icon, bx + (TRAY_W - icon.w as i32) / 2, (TASKBAR_H - icon.h as i32) / 2),
            None => c.round_rect(bx + 9, 12, 16, 16, 3, ACCENT),
        }
        hits.push((rect(bx, 0, TRAY_W, TASKBAR_H), hit));
    }
    let tray_x = apps_x + TRAY_W * key.tray_apps.len() as i32;

    // Tray: network, Bluetooth, brightness, volume, just left of the clock.
    use crate::tray::{bluetooth, speaker, sun, wifi, Flyout};
    for (i, (hit, open)) in [
        (TaskbarHit::Network, key.flyout == Some(Flyout::Network)),
        (TaskbarHit::Bluetooth, key.flyout == Some(Flyout::Bluetooth)),
        (TaskbarHit::Brightness, key.flyout == Some(Flyout::Brightness)),
        (TaskbarHit::Volume, key.flyout == Some(Flyout::Volume)),
    ]
    .into_iter()
    .enumerate()
    {
        let bx = tray_x + i as i32 * TRAY_W;
        if open {
            c.rect(bx, 0, TRAY_W, TASKBAR_H, TASK_ACTIVE);
        } else if hovered(&hit) {
            c.rect(bx, 0, TRAY_W, TASKBAR_H, TASK_HOVER);
        }
        let (cx, cy) = (bx as f32 + TRAY_W as f32 / 2.0, TASKBAR_H as f32 / 2.0);
        match hit {
            TaskbarHit::Volume => speaker(&mut c, cx - 5.0, cy, key.volume.1, key.volume.0 as f32 / 100.0, WHITE),
            TaskbarHit::Brightness => sun(&mut c, cx, cy, WHITE),
            TaskbarHit::Network => wifi(&mut c, cx, cy, key.wifi_on),
            _ => bluetooth(&mut c, cx, cy, key.bt_on),
        }
        hits.push((rect(bx, 0, TRAY_W, TASKBAR_H), hit));
    }

    // Clock: clicking it opens the Action Center.
    if hovered(&TaskbarHit::Clock) {
        c.rect(right_edge, 0, CLOCK_W, TASKBAR_H, TASK_HOVER);
    }
    if let Some(f) = font {
        let cx = right_edge + CLOCK_W / 2;
        let tw = measure(f, 12.0, &key.time);
        c.text(f, 12.0, cx - tw / 2, 3, TASKBAR_H / 2, WHITE, &key.time);
        let dw = measure(f, 12.0, &key.date);
        c.text(f, 12.0, cx - dw / 2, TASKBAR_H / 2 - 3, TASKBAR_H / 2, WHITE, &key.date);
    }
    hits.push((rect(right_edge, 0, CLOCK_W, TASKBAR_H), TaskbarHit::Clock));

    // Show-desktop sliver at the far right.
    let dx = w - SHOW_DESKTOP_W;
    c.rect(dx, 0, 1, TASKBAR_H, hex(0x5a5a5a));
    if hovered(&TaskbarHit::ShowDesktop) {
        c.rect(dx + 1, 0, SHOW_DESKTOP_W - 1, TASKBAR_H, TASK_HOVER);
    }
    hits.push((rect(dx, 0, SHOW_DESKTOP_W, TASKBAR_H), TaskbarHit::ShowDesktop));

    Taskbar { key, buffer: c.into_buffer(), hits }
}

/// Look an app's icon up by its app_id, trying the usual name variations.
pub fn app_icon(icons: &mut Icons, app_id: &str, size: u32) -> Option<std::rc::Rc<crate::icons::Icon>> {
    let last = app_id.rsplit('.').next().unwrap_or(app_id);
    [app_id.to_string(), app_id.to_lowercase(), last.to_lowercase()]
        .iter()
        .find_map(|n| icons.get(n, size))
}

fn magnifier(c: &mut Canvas, x: f32, y: f32, fg: Rgba, bg: Rgba) {
    c.circle(x - 1.5, y - 1.5, 6.0, fg);
    c.circle(x - 1.5, y - 1.5, 4.6, bg);
    c.line(x + 2.5, y + 2.5, x + 7.0, y + 7.0, 1.6, fg);
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
    Rectangle::new((x, y).into(), (w, h).into())
}

// ---------------------------------------------------------------- titlebar

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TitleButton {
    Minimize,
    Maximize,
    Close,
}

#[derive(Clone, PartialEq)]
pub struct TitleKey {
    pub title: String,
    pub app_id: String,
    pub width: i32,
    pub active: bool,
    pub maximized: bool,
    pub hover: Option<TitleButton>,
}

/// Which titlebar button is at `x` (relative to the titlebar's left), if any.
pub fn title_button_at(width: i32, x: i32) -> Option<TitleButton> {
    let from_right = width - x;
    match from_right {
        r if r <= 0 => None,
        r if r <= TITLE_BTN_W => Some(TitleButton::Close),
        r if r <= TITLE_BTN_W * 2 => Some(TitleButton::Maximize),
        r if r <= TITLE_BTN_W * 3 => Some(TitleButton::Minimize),
        _ => None,
    }
}

pub fn draw_titlebar(font: Option<&Font>, icons: &mut Icons, key: &TitleKey) -> MemoryRenderBuffer {
    let w = key.width;
    let mut c = Canvas::new(w, TITLE_H);
    c.fill(TITLE_BG);

    let mut tx = 10;
    if let Some(icon) = app_icon(icons, &key.app_id, 16) {
        c.icon(&icon, 8, (TITLE_H - 16) / 2);
        tx = 32;
    }
    if let Some(f) = font {
        let t = ellipsize(f, 12.0, &key.title, w - tx - TITLE_BTN_W * 3 - 8);
        c.text(f, 12.0, tx, 0, TITLE_H, if key.active { TITLE_TEXT } else { TITLE_TEXT_IDLE }, &t);
    }

    let glyph = if key.active { TITLE_TEXT } else { TITLE_TEXT_IDLE };
    for (i, btn) in [TitleButton::Minimize, TitleButton::Maximize, TitleButton::Close].into_iter().enumerate() {
        let bx = w - TITLE_BTN_W * (3 - i as i32);
        let hover = key.hover == Some(btn);
        let mut fg = glyph;
        if hover {
            let bg = if btn == TitleButton::Close { CLOSE_HOVER } else { BTN_HOVER };
            c.rect(bx, 0, TITLE_BTN_W, TITLE_H, bg);
            if btn == TitleButton::Close {
                fg = WHITE;
            }
        }
        let (cx, cy) = (bx + TITLE_BTN_W / 2, TITLE_H / 2);
        match btn {
            TitleButton::Minimize => c.rect(cx - 5, cy, 10, 1, fg),
            TitleButton::Maximize if key.maximized => {
                // "Restore": two overlapping squares.
                c.outline(cx - 5, cy - 3, 8, 8, fg);
                c.rect(cx - 3, cy - 5, 8, 1, fg);
                c.rect(cx + 4, cy - 5, 1, 8, fg);
            }
            TitleButton::Maximize => c.outline(cx - 5, cy - 5, 10, 10, fg),
            TitleButton::Close => {
                let (x, y) = (cx as f32, cy as f32);
                c.line(x - 5.0, y - 5.0, x + 5.0, y + 5.0, 1.1, fg);
                c.line(x + 5.0, y - 5.0, x - 5.0, y + 5.0, 1.1, fg);
            }
        }
    }
    c.into_opaque_buffer()
}

// ---------------------------------------------------------------- start menu

#[derive(Clone, PartialEq, Debug)]
pub enum StartHit {
    /// Index into the filtered list.
    Item(usize),
    Power,
    PowerAction(PowerAction),
    Nothing,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PowerAction {
    SignOut,
    Restart,
    ShutDown,
}

impl PowerAction {
    const ALL: [PowerAction; 3] = [PowerAction::SignOut, PowerAction::Restart, PowerAction::ShutDown];

    fn label(self) -> &'static str {
        match self {
            PowerAction::SignOut => "Sign out",
            PowerAction::Restart => "Restart",
            PowerAction::ShutDown => "Shut down",
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum StartRow {
    Header(String),
    /// (label, icon name, dim detail text on the right)
    Item(String, String, String),
}

#[derive(Clone, PartialEq)]
pub struct StartKey {
    pub height: i32,
    pub query: String,
    pub items: Vec<StartRow>,
    pub selected: usize,
    pub scroll: usize,
    pub hover: Option<StartHit>,
    /// The power button's Sign out / Restart / Shut down list is showing.
    pub power_open: bool,
}

pub struct StartMenu {
    pub key: StartKey,
    pub buffer: MemoryRenderBuffer,
    pub hits: Vec<(Rectangle<i32, Logical>, StartHit)>,
}

pub fn start_menu_rows(height: i32) -> usize {
    ((height - MENU_TOP - 8) / MENU_ROW).max(1) as usize
}

pub fn draw_start_menu(font: Option<&Font>, icons: &mut Icons, key: StartKey) -> StartMenu {
    let h = key.height;
    let mut c = Canvas::new(MENU_W, h);
    let mut hits = vec![(rect(0, 0, MENU_W, h), StartHit::Nothing)];
    c.rect(0, 0, RAIL_W, h, MENU_RAIL);
    c.rect(RAIL_W, 0, MENU_W - RAIL_W, h, MENU_BG);

    // Power at the bottom of the rail; it opens a list rather than acting straight away.
    let (px, py) = (RAIL_W / 2, h - 26);
    if key.power_open {
        c.rect(0, py - 18, RAIL_W, 36, TASK_ACTIVE);
    } else if key.hover == Some(StartHit::Power) {
        c.rect(0, py - 18, RAIL_W, 36, TASK_HOVER);
    }
    c.circle(px as f32, py as f32 + 1.0, 7.5, WHITE);
    c.circle(px as f32, py as f32 + 1.0, 6.2, MENU_RAIL);
    c.rect(px - 3, py - 9, 6, 6, MENU_RAIL);
    c.rect(px - 1, py - 9, 2, 9, WHITE);
    hits.push((rect(0, py - 18, RAIL_W, 36), StartHit::Power));

    // Search line.
    let lx = RAIL_W + 16;
    magnifier(&mut c, lx as f32 + 6.0, 26.0, WHITE, MENU_BG);
    if let Some(f) = font {
        let (t, col) = if key.query.is_empty() { ("Type to search apps, settings and files".to_string(), TEXT_DIM) } else { (format!("{}|", key.query), WHITE) };
        c.text(f, 14.0, lx + 22, 8, 36, col, &t);
    }
    c.rect(lx, 46, MENU_W - lx - 16, 1, hex(0x505050));

    // App list.
    let rows = start_menu_rows(h);
    for (row, (i, item)) in key.items.iter().enumerate().skip(key.scroll).take(rows).enumerate() {
        let y = MENU_TOP + row as i32 * MENU_ROW;
        let (name, icon, detail) = match item {
            StartRow::Header(h) => {
                if let Some(f) = font {
                    c.text(f, 12.0, RAIL_W + 16, y + 6, MENU_ROW - 6, TEXT_DIM, h);
                }
                continue;
            }
            StartRow::Item(n, i, d) => (n, i, d),
        };
        let hit = StartHit::Item(i);
        if i == key.selected {
            c.rect(RAIL_W, y, MENU_W - RAIL_W, MENU_ROW, TASK_ACTIVE);
        } else if key.hover.as_ref() == Some(&hit) {
            c.rect(RAIL_W, y, MENU_W - RAIL_W, MENU_ROW, TASK_HOVER);
        }
        match icons.get(icon, 24).or_else(|| app_icon(icons, icon, 24)) {
            Some(ic) => c.icon(&ic, RAIL_W + 12, y + (MENU_ROW - 24) / 2),
            None => c.round_rect(RAIL_W + 12, y + 6, 24, 24, 4, ACCENT),
        }
        if let Some(f) = font {
            let dw = if detail.is_empty() { 0 } else { measure(f, 11.0, detail).min(120) + 12 };
            let t = ellipsize(f, 13.0, name, MENU_W - RAIL_W - 60 - dw);
            c.text(f, 13.0, RAIL_W + 48, y, MENU_ROW, WHITE, &t);
            if !detail.is_empty() {
                let d = ellipsize(f, 11.0, detail, 120);
                c.text(f, 11.0, MENU_W - 12 - measure(f, 11.0, &d), y, MENU_ROW, TEXT_DIM, &d);
            }
        }
        hits.push((rect(RAIL_W, y, MENU_W - RAIL_W, MENU_ROW), hit));
    }
    if key.items.is_empty() {
        if let Some(f) = font {
            c.text(f, 13.0, RAIL_W + 16, MENU_TOP, MENU_ROW, TEXT_DIM, "No matches");
        }
    }

    if key.power_open {
        let (lw, row) = (170, 36);
        let ly = py + 18 - row * PowerAction::ALL.len() as i32 - 4;
        c.round_rect(RAIL_W - 4, ly - 4, lw + 8, row * PowerAction::ALL.len() as i32 + 8, 4, hex(0x1b1b1b));
        c.outline(RAIL_W - 4, ly - 4, lw + 8, row * PowerAction::ALL.len() as i32 + 8, hex(0x3c3c3c));
        for (i, a) in PowerAction::ALL.into_iter().enumerate() {
            let y = ly + i as i32 * row;
            let hit = StartHit::PowerAction(a);
            if key.hover.as_ref() == Some(&hit) {
                c.rect(RAIL_W, y, lw, row, TASK_HOVER);
            }
            if let Some(f) = font {
                c.text(f, 13.0, RAIL_W + 14, y, row, WHITE, a.label());
            }
            hits.push((rect(RAIL_W, y, lw, row), hit));
        }
    }

    // Later entries win in hit tests, so reverse: specific rects before the background.
    hits.reverse();
    StartMenu { key, buffer: c.into_buffer(), hits }
}

// ---------------------------------------------------------------- wallpaper

pub fn draw_wallpaper(w: i32, h: i32) -> MemoryRenderBuffer {
    let mut c = Canvas::new(w, h);
    c.wallpaper(hex(0x001a3d), hex(0x0b4b8f), rgba(0x49b4ff, 0x90), w as f32 * 0.68, h as f32 * 0.42, w as f32 * 0.55);
    c.into_opaque_buffer()
}
