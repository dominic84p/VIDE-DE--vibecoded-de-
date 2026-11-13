//! Task View (Win+Tab or the taskbar button), like Windoze 10's: the whole screen above the
//! taskbar shows this desktop's windows as big live thumbnails to pick or close, with the
//! desktops in a strip along the top and "New desktop" after them. The live thumbnails are
//! drawn by `render` over the picture painted here.

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, Size},
};

use crate::{
    canvas::{ellipsize, hex, measure, Canvas, Rgba},
    state::State,
    theme::th,
    ui::{self, TASKBAR_H},
    wm::WORKSPACE_COUNT,
};

const STRIP_H: i32 = 168;
const DESK_W: i32 = 176;
const DESK_H: i32 = 99;
const DESK_GAP: i32 = 24;
const DESK_TOP: i32 = 36;
const ICON: i32 = 24;
const MAX_ICONS: usize = 4;
/// Window cells: the title row above each thumbnail, and the room around them.
const TITLE_ROW: i32 = 34;
const CELL_GAP: i32 = 28;
const MAX_CELL_W: i32 = 440;
const CLOSE_W: i32 = 32;

#[derive(Clone, PartialEq, Debug, Default)]
pub enum TaskViewHit {
    Desktop(usize),
    NewDesktop,
    Window(WlSurface),
    Close(WlSurface),
    #[default]
    Nothing,
}

#[derive(Clone, PartialEq)]
struct Key {
    size: Size<i32, Logical>,
    current: usize,
    /// The desktops in the strip, with their apps' icon names.
    shown: Vec<(usize, Vec<String>)>,
    can_add: bool,
    /// This desktop's windows: (surface, title, icon name).
    windows: Vec<(WlSurface, String, String)>,
    hover: TaskViewHit,
}

#[derive(Default)]
pub struct Desktops {
    pub open: bool,
    pub hover: TaskViewHit,
    /// Empty desktops added with "New desktop", kept in the strip until something goes there.
    added: Vec<usize>,
    picture: Option<(Key, MemoryRenderBuffer)>,
}

impl Desktops {
    /// Repaint on the next frame (after a light/dark switch).
    pub fn repaint(&mut self) {
        self.picture = None;
    }
}

/// Where the strip's tiles go: one per shown desktop, then "New desktop".
fn desk_rects(width: i32, count: usize) -> Vec<Rectangle<i32, Logical>> {
    let total = count as i32 * DESK_W + (count as i32 - 1).max(0) * DESK_GAP;
    let x0 = ((width - total) / 2).max(16);
    (0..count).map(|i| Rectangle::new((x0 + i as i32 * (DESK_W + DESK_GAP), DESK_TOP).into(), (DESK_W, DESK_H).into())).collect()
}

/// Window cells in a centred grid below the strip: the whole cell (title row + thumbnail area).
fn window_cells(size: Size<i32, Logical>, n: usize) -> Vec<Rectangle<i32, Logical>> {
    if n == 0 {
        return Vec::new();
    }
    let (area_w, area_h) = (size.w - 2 * CELL_GAP, size.h - TASKBAR_H - STRIP_H - 2 * CELL_GAP);
    // The column count that gives the biggest thumbnails.
    let (mut best, mut best_w) = (1, 0);
    for cols in 1..=n as i32 {
        let rows = (n as i32 + cols - 1) / cols;
        let by_w = (area_w - (cols - 1) * CELL_GAP) / cols;
        let by_h = ((area_h - (rows - 1) * CELL_GAP) / rows - TITLE_ROW) * 16 / 10;
        let w = by_w.min(by_h).min(MAX_CELL_W);
        if w > best_w {
            (best, best_w) = (cols, w);
        }
    }
    let cols = best;
    let rows = (n as i32 + cols - 1) / cols;
    let (cw, ch) = (best_w.max(80), best_w.max(80) * 10 / 16 + TITLE_ROW);
    let top = STRIP_H + CELL_GAP + ((area_h - rows * ch - (rows - 1) * CELL_GAP) / 2).max(0);
    (0..n as i32)
        .map(|i| {
            let (col, row) = (i % cols, i / cols);
            let in_row = (n as i32 - row * cols).min(cols);
            let left = (size.w - in_row * cw - (in_row - 1) * CELL_GAP) / 2;
            Rectangle::new((left + col * (cw + CELL_GAP), top + row * (ch + CELL_GAP)).into(), (cw, ch).into())
        })
        .collect()
}

fn thumb_area(cell: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    Rectangle::new((cell.loc.x, cell.loc.y + TITLE_ROW).into(), (cell.size.w, cell.size.h - TITLE_ROW).into())
}

fn close_rect(cell: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    Rectangle::new((cell.loc.x + cell.size.w - CLOSE_W, cell.loc.y).into(), (CLOSE_W, TITLE_ROW - 4).into())
}

impl State {
    /// Everything above the taskbar.
    pub fn desktops_rect(&self) -> Rectangle<i32, Logical> {
        Rectangle::from_size((self.output_size.w, self.output_size.h - TASKBAR_H).into())
    }

    /// The desktops in the strip: any with windows, the current one, and ones just added.
    fn shown_desktops(&self) -> Vec<usize> {
        (0..WORKSPACE_COUNT)
            .filter(|&ws| {
                ws == self.wm.current_workspace
                    || self.desktops.added.contains(&ws)
                    || self.wm.windows.iter().any(|w| w.mapped && w.workspace == ws)
            })
            .collect()
    }

    /// This desktop's windows, most recently used first.
    pub fn task_view_windows(&self) -> Vec<WlSurface> {
        let ws = self.wm.current_workspace;
        self.wm.windows.iter().rev().filter(|w| w.mapped && w.workspace == ws).map(|w| w.surface().clone()).collect()
    }

    /// Live thumbnails to draw: each window and the area its thumbnail fits in (global coords).
    pub fn task_view_thumbs(&self) -> Vec<(WlSurface, Rectangle<i32, Logical>)> {
        let wins = self.task_view_windows();
        window_cells(self.output_size, wins.len()).into_iter().zip(wins).map(|(cell, s)| (s, thumb_area(cell))).collect()
    }

    pub fn task_view_at(&self, p: Point<f64, Logical>) -> TaskViewHit {
        let p = p.to_i32_round();
        let shown = self.shown_desktops();
        let can_add = shown.len() < WORKSPACE_COUNT;
        let rects = desk_rects(self.output_size.w, shown.len() + can_add as usize);
        for (i, r) in rects.iter().enumerate() {
            if r.contains(p) {
                return shown.get(i).map_or(TaskViewHit::NewDesktop, |&ws| TaskViewHit::Desktop(ws));
            }
        }
        let wins = self.task_view_windows();
        for (cell, s) in window_cells(self.output_size, wins.len()).into_iter().zip(wins) {
            if close_rect(cell).contains(p) {
                return TaskViewHit::Close(s);
            }
            if cell.contains(p) {
                return TaskViewHit::Window(s);
            }
        }
        TaskViewHit::Nothing
    }

    pub fn toggle_desktops(&mut self) {
        self.desktops.open = !self.desktops.open;
        self.desktops.hover = TaskViewHit::Nothing;
        self.desktops.added.clear();
        if self.desktops.open {
            self.release_pointer();
        }
        self.needs_redraw = true;
    }

    pub fn close_desktops(&mut self) {
        if self.desktops.open {
            self.desktops.open = false;
            self.needs_redraw = true;
        }
    }

    pub fn switch_desktop(&mut self, ws: usize) {
        self.wm.set_workspace(ws);
        self.focus_top();
        self.close_desktops();
        self.needs_redraw = true;
    }

    pub fn task_view_click(&mut self, hit: TaskViewHit) {
        match hit {
            TaskViewHit::Desktop(ws) => self.switch_desktop(ws),
            // Like Windoze: the new desktop joins the strip; click it to go there.
            TaskViewHit::NewDesktop => {
                let shown = self.shown_desktops();
                if let Some(ws) = (0..WORKSPACE_COUNT).find(|ws| !shown.contains(ws)) {
                    self.desktops.added.push(ws);
                }
            }
            TaskViewHit::Window(s) => {
                self.close_desktops();
                self.focus(Some(s), smithay::utils::SERIAL_COUNTER.next_serial());
            }
            TaskViewHit::Close(s) => {
                if let Some(w) = self.wm.find(&s) {
                    w.close();
                }
            }
            TaskViewHit::Nothing => self.close_desktops(),
        }
        self.needs_redraw = true;
    }

    pub fn desktops_picture(&mut self) -> Option<MemoryRenderBuffer> {
        if !self.desktops.open {
            return None;
        }
        let mut windows: Vec<_> = self.wm.windows.iter().filter(|w| w.mapped).collect();
        windows.sort_by_key(|w| w.id);
        let shown = self
            .shown_desktops()
            .into_iter()
            .map(|ws| {
                let icons = windows.iter().filter(|w| w.workspace == ws).map(|w| crate::apps::icon_for_app_id(&self.apps, &w.app_id())).collect();
                (ws, icons)
            })
            .collect::<Vec<_>>();
        let can_add = shown.len() < WORKSPACE_COUNT;
        let windows = self
            .task_view_windows()
            .into_iter()
            .filter_map(|s| self.wm.find(&s).map(|w| (s.clone(), w.title(), crate::apps::icon_for_app_id(&self.apps, &w.app_id()))))
            .collect();
        let key = Key { size: self.output_size, current: self.wm.current_workspace, shown, can_add, windows, hover: self.desktops.hover.clone() };
        if self.desktops.picture.as_ref().is_none_or(|(k, _)| *k != key) {
            let buf = draw(self.font.as_ref(), &mut self.icons, &key);
            self.desktops.picture = Some((key, buf));
        }
        self.desktops.picture.as_ref().map(|(_, b)| b.clone())
    }
}

fn draw(font: Option<&fontdue::Font>, icons: &mut crate::icons::Icons, key: &Key) -> MemoryRenderBuffer {
    let (w, h) = (key.size.w, key.size.h - TASKBAR_H);
    let mut c = Canvas::new(w, h);
    // Windoze 10 blurs the desktop behind Task View; we tint it.
    let veil = if crate::theme::light() { Rgba(0xe6, 0xe6, 0xe6, 0xf2) } else { Rgba(0x1a, 0x1a, 0x1a, 0xf2) };
    c.fill(veil);

    // The desktops strip.
    let rects = desk_rects(w, key.shown.len() + key.can_add as usize);
    for (r, (ws, apps)) in rects.iter().zip(&key.shown) {
        let (x, y) = (r.loc.x, r.loc.y);
        let current = *ws == key.current;
        let hover = key.hover == TaskViewHit::Desktop(*ws);
        if current || hover {
            c.outline(x - 3, y - 3, DESK_W + 6, DESK_H + 6, if hover { th().text_dim } else { th().accent_light });
            c.outline(x - 2, y - 2, DESK_W + 4, DESK_H + 4, if hover { th().text_dim } else { th().accent_light });
        }
        // A miniature desktop: the wallpaper blue with the apps on it.
        c.rect(x, y, DESK_W, DESK_H, hex(0x0b4b8f));
        let iy = y + (DESK_H - ICON) / 2;
        let n = apps.len().min(MAX_ICONS) as i32;
        let ix0 = x + (DESK_W - n * ICON - (n - 1).max(0) * 6) / 2;
        for (j, name) in apps.iter().take(MAX_ICONS).enumerate() {
            let ix = ix0 + j as i32 * (ICON + 6);
            match ui::app_icon(icons, name, ICON as u32) {
                Some(icon) => c.icon(&icon, ix, iy),
                None => c.round_rect(ix, iy, ICON, ICON, 4, ui::ACCENT),
            }
        }
        if let Some(f) = font {
            if apps.len() > MAX_ICONS {
                c.text(f, 12.0, x + DESK_W - 30, y + DESK_H - 22, 20, hex(0xffffff), &format!("+{}", apps.len() - MAX_ICONS));
            }
            let label = format!("Desktop {}", ws + 1);
            c.text(f, 13.0, x, y + DESK_H + 6, 22, th().text, &label);
        }
    }
    if key.can_add {
        let r = rects[key.shown.len()];
        let (x, y) = (r.loc.x, r.loc.y);
        let hover = key.hover == TaskViewHit::NewDesktop;
        c.rect(x, y, DESK_W, DESK_H, if hover { th().card_hover } else { th().card });
        let (cx, cy) = (x + DESK_W / 2, y + DESK_H / 2 - 10);
        c.rect(cx - 11, cy, 22, 2, th().text);
        c.rect(cx - 1, cy - 10, 2, 22, th().text);
        if let Some(f) = font {
            let t = "New desktop";
            c.text(f, 13.0, x + (DESK_W - measure(f, 13.0, t)) / 2, cy + 18, 22, th().text, t);
        }
    }

    // This desktop's windows: icon and title above where `render` draws each thumbnail.
    let cells = window_cells(key.size, key.windows.len());
    for (cell, (s, title, icon_name)) in cells.iter().zip(&key.windows) {
        let (x, y) = (cell.loc.x, cell.loc.y);
        let hover = matches!(&key.hover, TaskViewHit::Window(h) | TaskViewHit::Close(h) if h == s);
        if hover {
            c.rect(x - 4, y - 4, cell.size.w + 8, cell.size.h + 8, th().highlight);
        }
        match ui::app_icon(icons, icon_name, 20) {
            Some(icon) => c.icon(&icon, x + 2, y + (TITLE_ROW - 20) / 2 - 2),
            None => c.round_rect(x + 2, y + (TITLE_ROW - 20) / 2 - 2, 20, 20, 3, ui::ACCENT),
        }
        if let Some(f) = font {
            let room = cell.size.w - 30 - if hover { CLOSE_W } else { 0 };
            c.text(f, 13.0, x + 30, y - 2, TITLE_ROW, th().text, &ellipsize(f, 13.0, title, room));
        }
        if hover {
            let r = close_rect(*cell);
            if key.hover == TaskViewHit::Close(s.clone()) {
                c.rect(r.loc.x, r.loc.y, r.size.w, r.size.h, hex(0xe81123));
            }
            let (cx, cy) = ((r.loc.x + r.size.w / 2) as f32, (r.loc.y + r.size.h / 2) as f32);
            let col = if key.hover == TaskViewHit::Close(s.clone()) { hex(0xffffff) } else { th().text };
            c.line(cx - 5.0, cy - 5.0, cx + 5.0, cy + 5.0, 1.2, col);
            c.line(cx + 5.0, cy - 5.0, cx - 5.0, cy + 5.0, 1.2, col);
        }
    }
    if key.windows.is_empty() {
        if let Some(f) = font {
            let t = "No windows open on this desktop";
            c.text(f, 15.0, (w - measure(f, 15.0, t)) / 2, STRIP_H + 80, 26, th().text_dim, t);
        }
    }
    c.into_buffer()
}
