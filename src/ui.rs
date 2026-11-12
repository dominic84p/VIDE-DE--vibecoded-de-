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
const TASK_W: i32 = 48;
const CLOCK_W: i32 = 88;
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
    Window(WlSurface),
    ShowDesktop,
    Nothing,
}

#[derive(Clone, PartialEq)]
pub struct TaskItem {
    pub surface: WlSurface,
    pub title: String,
    pub app_id: String,
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
}

pub struct Taskbar {
    pub key: TaskbarKey,
    pub buffer: MemoryRenderBuffer,
    /// Hit rects relative to the taskbar's top-left.
    pub hits: Vec<(Rectangle<i32, Logical>, TaskbarHit)>,
}

pub fn draw_taskbar(font: Option<&Font>, icons: &mut Icons, logo: Option<&crate::icons::Icon>, key: TaskbarKey) -> Taskbar {
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

    // Window buttons.
    let right_edge = w - CLOCK_W - SHOW_DESKTOP_W;
    let mut x = sx + SEARCH_W + 4;
    for item in &key.items {
        if x + TASK_W > right_edge {
            break;
        }
        let hit = TaskbarHit::Window(item.surface.clone());
        let bg = if item.focused { TASK_ACTIVE } else if hovered(&hit) { TASK_HOVER } else { TASKBAR_BG };
        c.rect(x, 0, TASK_W, TASKBAR_H, bg);
        match app_icon(icons, &item.app_id, 24) {
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
        // Open-window underline; full width when focused.
        if item.focused {
            c.rect(x, TASKBAR_H - 2, TASK_W, 2, TASK_LINE);
        } else {
            c.rect(x + 6, TASKBAR_H - 2, TASK_W - 12, 2, TASK_LINE);
        }
        hits.push((rect(x, 0, TASK_W, TASKBAR_H), hit));
        x += TASK_W;
    }

    // Clock.
    if let Some(f) = font {
        let cx = right_edge + CLOCK_W / 2;
        let tw = measure(f, 12.0, &key.time);
        c.text(f, 12.0, cx - tw / 2, 3, TASKBAR_H / 2, WHITE, &key.time);
        let dw = measure(f, 12.0, &key.date);
        c.text(f, 12.0, cx - dw / 2, TASKBAR_H / 2 - 3, TASKBAR_H / 2, WHITE, &key.date);
    }

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
    Nothing,
}

#[derive(Clone, PartialEq)]
pub struct StartKey {
    pub height: i32,
    pub query: String,
    /// Filtered (name, icon) pairs.
    pub items: Vec<(String, String)>,
    pub selected: usize,
    pub scroll: usize,
    pub hover: Option<StartHit>,
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

    // Power (quits the session) at the bottom of the rail.
    let (px, py) = (RAIL_W / 2, h - 26);
    if key.hover == Some(StartHit::Power) {
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
        let (t, col) = if key.query.is_empty() { ("Type to search apps".to_string(), TEXT_DIM) } else { (format!("{}|", key.query), WHITE) };
        c.text(f, 14.0, lx + 22, 8, 36, col, &t);
    }
    c.rect(lx, 46, MENU_W - lx - 16, 1, hex(0x505050));

    // App list.
    let rows = start_menu_rows(h);
    for (row, (i, (name, icon))) in key.items.iter().enumerate().skip(key.scroll).take(rows).enumerate() {
        let y = MENU_TOP + row as i32 * MENU_ROW;
        let hit = StartHit::Item(i);
        if i == key.selected {
            c.rect(RAIL_W, y, MENU_W - RAIL_W, MENU_ROW, TASK_ACTIVE);
        } else if key.hover.as_ref() == Some(&hit) {
            c.rect(RAIL_W, y, MENU_W - RAIL_W, MENU_ROW, TASK_HOVER);
        }
        match icons.get(icon, 24) {
            Some(ic) => c.icon(&ic, RAIL_W + 12, y + (MENU_ROW - 24) / 2),
            None => c.round_rect(RAIL_W + 12, y + 6, 24, 24, 4, ACCENT),
        }
        if let Some(f) = font {
            let t = ellipsize(f, 13.0, name, MENU_W - RAIL_W - 60);
            c.text(f, 13.0, RAIL_W + 48, y, MENU_ROW, WHITE, &t);
        }
        hits.push((rect(RAIL_W, y, MENU_W - RAIL_W, MENU_ROW), hit));
    }
    if key.items.is_empty() {
        if let Some(f) = font {
            c.text(f, 13.0, RAIL_W + 16, MENU_TOP, MENU_ROW, TEXT_DIM, "No matches");
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
