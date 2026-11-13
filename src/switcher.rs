//! Alt+Tab: a Windows-style switcher with live thumbnails of the windows on the
//! current desktop. Tab/Shift+Tab move the highlight; releasing Alt switches.

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
};

use crate::{
    canvas::{ellipsize, hex, Canvas, Rgba},
    state::State,
};

pub const TILE_W: i32 = 220;
pub const TILE_H: i32 = 170;
pub const THUMB_H: i32 = 130;
const PAD: i32 = 16;
const GAP: i32 = 8;
const MAX_COLS: i32 = 6;

pub struct Switcher {
    /// Windows in most-recently-used order (the stacking order, top first).
    pub order: Vec<WlSurface>,
    pub index: usize,
    pub(crate) picture: Option<(usize, MemoryRenderBuffer)>,
}

impl Switcher {
    fn cols(&self) -> i32 {
        (self.order.len() as i32).clamp(1, MAX_COLS)
    }

    fn rows(&self) -> i32 {
        (self.order.len() as i32 + MAX_COLS - 1) / MAX_COLS
    }

    pub fn size(&self) -> (i32, i32) {
        let (c, r) = (self.cols(), self.rows().max(1));
        (PAD * 2 + c * TILE_W + (c - 1) * GAP, PAD * 2 + r * TILE_H + (r - 1) * GAP)
    }

    /// Tile rectangle relative to the panel.
    pub fn tile(&self, i: usize) -> Rectangle<i32, Logical> {
        let (col, row) = (i as i32 % MAX_COLS, i as i32 / MAX_COLS);
        Rectangle::new((PAD + col * (TILE_W + GAP), PAD + row * (TILE_H + GAP)).into(), (TILE_W, TILE_H).into())
    }
}

impl State {
    pub fn switcher_step(&mut self, back: bool) {
        if self.switcher.is_none() {
            let ws = self.wm.current_workspace;
            let order: Vec<WlSurface> =
                self.wm.windows.iter().rev().filter(|w| w.mapped && w.workspace == ws).map(|w| w.surface().clone()).collect();
            if order.len() < 2 {
                return;
            }
            self.release_pointer();
            // First Tab already moves to the previous window, like Windows.
            self.switcher = Some(Switcher { order, index: 0, picture: None });
        }
        let s = self.switcher.as_mut().unwrap();
        let n = s.order.len();
        s.index = if back { (s.index + n - 1) % n } else { (s.index + 1) % n };
        self.needs_redraw = true;
    }

    pub fn switcher_commit(&mut self) {
        if let Some(s) = self.switcher.take() {
            if let Some(target) = s.order.get(s.index).cloned() {
                self.focus(Some(target), SERIAL_COUNTER.next_serial());
            }
            self.needs_redraw = true;
        }
    }

    pub fn switcher_cancel(&mut self) {
        if self.switcher.take().is_some() {
            self.needs_redraw = true;
        }
    }

    /// Panel origin, centred on screen.
    pub fn switcher_origin(&self) -> Option<Point<i32, Logical>> {
        let s = self.switcher.as_ref()?;
        let (w, h) = s.size();
        Some(Point::from(((self.output_size.w - w) / 2, (self.output_size.h - h) / 2)))
    }

    /// The panel background with titles and highlight (thumbnails are drawn on top).
    pub fn switcher_picture(&mut self) -> Option<MemoryRenderBuffer> {
        let titles: Vec<String> = self
            .switcher
            .as_ref()?
            .order
            .iter()
            .map(|s| self.wm.find(s).map(|w| w.title()).unwrap_or_default())
            .collect();
        let font = self.font.as_ref();
        let s = self.switcher.as_mut()?;
        if s.picture.as_ref().is_none_or(|(i, _)| *i != s.index) {
            let (w, h) = s.size();
            let mut c = Canvas::new(w, h);
            c.round_rect(0, 0, w, h, 8, Rgba(0x1f, 0x1f, 0x1f, 0xf0));
            for (i, title) in titles.iter().enumerate() {
                let r = s.tile(i);
                if i == s.index {
                    c.round_rect(r.loc.x - 3, r.loc.y - 3, r.size.w + 6, r.size.h + 6, 6, hex(0x76b9ed));
                    c.round_rect(r.loc.x, r.loc.y, r.size.w, r.size.h, 4, hex(0x2b2b2b));
                }
                if let Some(f) = font {
                    let t = ellipsize(f, 12.0, title, r.size.w - 16);
                    c.text(f, 12.0, r.loc.x + 8, r.loc.y + 4, 22, Rgba(0xff, 0xff, 0xff, 0xff), &t);
                }
            }
            s.picture = Some((s.index, c.into_buffer()));
        }
        Some(s.picture.as_ref().unwrap().1.clone())
    }
}
