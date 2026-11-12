//! Task View: one taskbar button that opens a panel of all virtual desktops,
//! each tile showing the apps on it. Click a tile to switch.

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    utils::{Logical, Point, Rectangle},
};

use crate::{
    canvas::{ellipsize, hex, Canvas, Rgba},
    state::State,
    ui::{self, SEARCH_END, TASKBAR_H},
    wm::WORKSPACE_COUNT,
};

const COLS: i32 = 5;
const TILE_W: i32 = 150;
const TILE_H: i32 = 96;
const GAP: i32 = 10;
const PAD: i32 = 14;
const HEADER: i32 = 26;
pub const PANEL_W: i32 = PAD * 2 + COLS * TILE_W + (COLS - 1) * GAP;
const ROWS: i32 = (WORKSPACE_COUNT as i32 + COLS - 1) / COLS;
pub const PANEL_H: i32 = PAD * 2 + HEADER + ROWS * TILE_H + (ROWS - 1) * GAP;
const ICON: i32 = 24;
const MAX_ICONS: usize = 4;

const BG: Rgba = hex(0x1f1f1f);
const BORDER: Rgba = hex(0x3c3c3c);
const TILE: Rgba = hex(0x2b2b2b);
const TILE_HOVER: Rgba = hex(0x3a3a3a);
const TILE_CURRENT: Rgba = hex(0x333333);
const ACCENT: Rgba = hex(0x76b9ed);
const WHITE: Rgba = hex(0xffffff);
const DIM: Rgba = hex(0x8a8a8a);

#[derive(Clone, PartialEq)]
struct Key {
    current: usize,
    /// Icon names of the apps on each desktop.
    apps: Vec<Vec<String>>,
    hover: Option<usize>,
}

#[derive(Default)]
pub struct Desktops {
    pub open: bool,
    pub hover: Option<usize>,
    picture: Option<(Key, MemoryRenderBuffer)>,
}

fn tile_rect(i: usize) -> Rectangle<i32, Logical> {
    let (col, row) = (i as i32 % COLS, i as i32 / COLS);
    Rectangle::new((PAD + col * (TILE_W + GAP), PAD + HEADER + row * (TILE_H + GAP)).into(), (TILE_W, TILE_H).into())
}

impl State {
    pub fn desktops_rect(&self) -> Rectangle<i32, Logical> {
        let x = (SEARCH_END - 8).clamp(0, (self.output_size.w - PANEL_W).max(0));
        Rectangle::new((x, self.output_size.h - TASKBAR_H - PANEL_H - 8).into(), (PANEL_W, PANEL_H).into())
    }

    /// Which desktop tile is under `p` (global coords), if the panel is open.
    pub fn desktop_at(&self, p: Point<f64, Logical>) -> Option<usize> {
        let local = (p - self.desktops_rect().loc.to_f64()).to_i32_round();
        (0..WORKSPACE_COUNT).find(|&i| tile_rect(i).contains(local))
    }

    pub fn toggle_desktops(&mut self) {
        self.desktops.open = !self.desktops.open;
        self.desktops.hover = None;
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

    pub fn desktops_picture(&mut self) -> Option<MemoryRenderBuffer> {
        if !self.desktops.open {
            return None;
        }
        let mut apps = vec![Vec::new(); WORKSPACE_COUNT];
        let mut windows: Vec<_> = self.wm.windows.iter().filter(|w| w.mapped).collect();
        windows.sort_by_key(|w| w.id);
        for w in windows {
            apps[w.workspace].push(crate::apps::icon_for_app_id(&self.apps, &w.app_id()));
        }
        let key = Key { current: self.wm.current_workspace, apps, hover: self.desktops.hover };
        if self.desktops.picture.as_ref().is_none_or(|(k, _)| *k != key) {
            let buf = draw(self.font.as_ref(), &mut self.icons, &key);
            self.desktops.picture = Some((key, buf));
        }
        self.desktops.picture.as_ref().map(|(_, b)| b.clone())
    }
}

fn draw(font: Option<&fontdue::Font>, icons: &mut crate::icons::Icons, key: &Key) -> MemoryRenderBuffer {
    let mut c = Canvas::new(PANEL_W, PANEL_H);
    c.fill(BG);
    c.outline(0, 0, PANEL_W, PANEL_H, BORDER);
    if let Some(f) = font {
        c.text(f, 13.0, PAD, PAD - 4, HEADER, WHITE, "Desktops");
    }
    for (i, apps) in key.apps.iter().enumerate() {
        let r = tile_rect(i);
        let (x, y) = (r.loc.x, r.loc.y);
        let current = i == key.current;
        let bg = if key.hover == Some(i) { TILE_HOVER } else if current { TILE_CURRENT } else { TILE };
        c.round_rect(x, y, TILE_W, TILE_H, 4, bg);
        if current {
            c.rect(x, y + TILE_H - 3, TILE_W, 3, ACCENT);
        }
        if let Some(f) = font {
            c.text(f, 12.0, x + 10, y + 4, 22, if current { WHITE } else { DIM }, &format!("Desktop {}", i + 1));
        }
        // Up to four app icons, then "+N".
        let iy = y + TILE_H - ICON - 14;
        for (j, name) in apps.iter().take(MAX_ICONS).enumerate() {
            let ix = x + 10 + j as i32 * (ICON + 6);
            match ui::app_icon(icons, name, ICON as u32) {
                Some(icon) => c.icon(&icon, ix, iy),
                None => c.round_rect(ix, iy, ICON, ICON, 4, ui::ACCENT),
            }
        }
        if let Some(f) = font {
            if apps.len() > MAX_ICONS {
                let more = format!("+{}", apps.len() - MAX_ICONS);
                c.text(f, 12.0, x + 10 + MAX_ICONS as i32 * (ICON + 6), iy, ICON, DIM, &more);
            } else if apps.is_empty() {
                let t = ellipsize(f, 12.0, "Empty", TILE_W - 20);
                c.text(f, 12.0, x + 10, iy, ICON, DIM, &t);
            }
        }
    }
    c.into_buffer()
}
