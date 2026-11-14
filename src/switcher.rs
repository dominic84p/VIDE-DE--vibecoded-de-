//! Alt+Tab: a Windoze-style switcher with live thumbnails of the windows on the
//! current desktop. Tab/Shift+Tab move the highlight; releasing Alt switches.
//!
//! The same panel is the Win+Shift+P pin picker: click a window (or pick it with the
//! arrows and Enter) to keep it on top of everything; pick a pinned one to let it go.
//! Apps that draw their own titlebar have no window menu of ours, so this is the way
//! to pin those.

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
/// Pinned windows' badge and outline: Windoze's yellow accent, unlike anything else we draw.
pub const PIN_COLOR: Rgba = hex(0xffb900);
/// The pin picker's "what is this" line above the tiles.
const HEADER: i32 = 30;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Switch,
    Pin,
}

pub struct Switcher {
    /// Windoze in most-recently-used order (the stacking order, top first).
    pub order: Vec<WlSurface>,
    pub index: usize,
    pub mode: Mode,
    /// Painted for this highlight and these pinned flags.
    pub(crate) picture: Option<(usize, Vec<bool>, MemoryRenderBuffer)>,
}

impl Switcher {
    fn header(&self) -> i32 {
        if self.mode == Mode::Pin { HEADER } else { 0 }
    }

    fn cols(&self) -> i32 {
        (self.order.len() as i32).clamp(1, MAX_COLS)
    }

    fn rows(&self) -> i32 {
        (self.order.len() as i32 + MAX_COLS - 1) / MAX_COLS
    }

    pub fn size(&self) -> (i32, i32) {
        let (c, r) = (self.cols(), self.rows().max(1));
        (PAD * 2 + c * TILE_W + (c - 1) * GAP, PAD * 2 + self.header() + r * TILE_H + (r - 1) * GAP)
    }

    /// Tile rectangle relative to the panel.
    pub fn tile(&self, i: usize) -> Rectangle<i32, Logical> {
        let (col, row) = (i as i32 % MAX_COLS, i as i32 / MAX_COLS);
        Rectangle::new((PAD + col * (TILE_W + GAP), PAD + self.header() + row * (TILE_H + GAP)).into(), (TILE_W, TILE_H).into())
    }

    /// The tile at a point relative to the panel.
    pub fn tile_at(&self, p: Point<i32, Logical>) -> Option<usize> {
        (0..self.order.len()).find(|&i| self.tile(i).contains(p))
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
            // First Tab already moves to the previous window, like Windoze.
            self.switcher = Some(Switcher { order, index: 0, mode: Mode::Switch, picture: None });
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

    /// Win+Shift+P: every window on this desktop, top first; pressing it again closes it.
    pub fn pin_picker(&mut self) {
        if self.switcher.as_ref().is_some_and(|s| s.mode == Mode::Pin) {
            return self.switcher_cancel();
        }
        let ws = self.wm.current_workspace;
        let order: Vec<WlSurface> =
            self.wm.windows.iter().rev().filter(|w| w.mapped && !w.minimized && w.workspace == ws).map(|w| w.surface().clone()).collect();
        if order.is_empty() {
            return;
        }
        self.release_pointer();
        self.switcher = Some(Switcher { order, index: 0, mode: Mode::Pin, picture: None });
        self.needs_redraw = true;
    }

    /// Pin (or unpin) the picker's `i`th window and close the picker.
    pub fn pin_pick(&mut self, i: usize) {
        let Some(s) = self.switcher.take() else { return };
        if let Some(target) = s.order.get(i).cloned() {
            self.toggle_on_top(&target);
        }
        self.needs_redraw = true;
    }

    /// Keep a window above all others, or stop. A window just pinned gets focus too.
    pub fn toggle_on_top(&mut self, s: &WlSurface) {
        let Some(on) = self.wm.find(s).map(|w| !w.on_top) else { return };
        self.wm.set_on_top(s, on);
        tracing::info!("{} {}", if on { "pinned on top:" } else { "unpinned:" }, self.wm.find(s).map(|w| w.title()).unwrap_or_default());
        if on {
            self.focus(Some(s.clone()), SERIAL_COUNTER.next_serial());
        }
        self.needs_redraw = true;
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
        let (titles, pinned): (Vec<String>, Vec<bool>) = self
            .switcher
            .as_ref()?
            .order
            .iter()
            .map(|s| self.wm.find(s).map(|w| (w.title(), w.on_top)).unwrap_or_default())
            .unzip();
        let font = self.font.as_ref();
        let s = self.switcher.as_mut()?;
        if s.picture.as_ref().is_none_or(|(i, p, _)| *i != s.index || *p != pinned) {
            let (w, h) = s.size();
            let mut c = Canvas::new(w, h);
            c.round_rect(0, 0, w, h, 8, Rgba(0x1f, 0x1f, 0x1f, 0xf0));
            if s.mode == Mode::Pin {
                if let Some(f) = font {
                    let line = "Keep which window on top?";
                    c.text(f, 13.0, PAD, PAD - 4, HEADER, Rgba(0xff, 0xff, 0xff, 0xff), &ellipsize(f, 13.0, line, w - PAD * 2));
                }
            }
            for (i, title) in titles.iter().enumerate() {
                let r = s.tile(i);
                if i == s.index {
                    c.round_rect(r.loc.x - 3, r.loc.y - 3, r.size.w + 6, r.size.h + 6, 6, hex(0x76b9ed));
                    c.round_rect(r.loc.x, r.loc.y, r.size.w, r.size.h, 4, hex(0x2b2b2b));
                }
                let badge = if pinned[i] && s.mode == Mode::Pin { 64 } else { 0 };
                if let Some(f) = font {
                    let t = ellipsize(f, 12.0, title, r.size.w - 16 - badge);
                    c.text(f, 12.0, r.loc.x + 8, r.loc.y + 4, 22, Rgba(0xff, 0xff, 0xff, 0xff), &t);
                    if badge > 0 {
                        let x = r.loc.x + r.size.w - badge - 4;
                        c.round_rect(x, r.loc.y + 6, badge, 18, 9, PIN_COLOR);
                        c.text(f, 11.0, x + 9, r.loc.y + 6, 18, Rgba(0x1f, 0x1f, 0x1f, 0xff), "On top");
                    }
                }
            }
            s.picture = Some((s.index, pinned, c.into_buffer()));
        }
        Some(s.picture.as_ref().unwrap().2.clone())
    }
}
