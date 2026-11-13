//! Screenshots: PrtSc grabs the whole screen, Win+Shift+S (or Shift+PrtSc) lets you
//! drag out a region first, like the Windoze snipping tool. Saved to
//! ~/Pictures/Screenshots and put on the clipboard as a PNG.

use std::{io::Write, path::PathBuf};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            damage::OutputDamageTracker,
            element::solid::SolidColorBuffer,
            gles::{GlesRenderer, GlesTexture},
            Bind, Color32F, ExportMem, Offscreen,
        },
    },
    input::pointer::{CursorIcon, CursorImageStatus},
    utils::{Logical, Point, Rectangle, Transform},
};

use crate::{render, state::State};

const DIM: Color32F = Color32F::new(0.0, 0.0, 0.0, 0.45);
const EDGE: Color32F = Color32F::new(1.0, 1.0, 1.0, 1.0);

/// Region selection in progress.
#[derive(Default)]
pub struct Snip {
    pub start: Option<Point<f64, Logical>>,
    /// Dimming around the selection (4) and its outline (4). Kept for stable damage ids.
    pub pieces: [SolidColorBuffer; 8],
}

impl Snip {
    pub fn selection(&self, pointer: Point<f64, Logical>) -> Option<Rectangle<i32, Logical>> {
        let a = self.start?;
        let (x0, y0) = (a.x.min(pointer.x) as i32, a.y.min(pointer.y) as i32);
        let (x1, y1) = (a.x.max(pointer.x) as i32, a.y.max(pointer.y) as i32);
        Some(Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into()))
    }

    /// The overlay: screen dimmed everywhere except the selection, which gets a white edge.
    pub fn overlay(&mut self, screen: Rectangle<i32, Logical>, sel: Option<Rectangle<i32, Logical>>) -> Vec<(&SolidColorBuffer, Point<i32, Logical>)> {
        let (sw, sh) = (screen.size.w, screen.size.h);
        let Some(r) = sel.filter(|r| r.size.w > 0 && r.size.h > 0) else {
            self.pieces[0].update((sw, sh), DIM);
            return vec![(&self.pieces[0], Point::from((0, 0)))];
        };
        let (x, y, w, h) = (r.loc.x, r.loc.y, r.size.w, r.size.h);
        let rects = [
            // dim: above, below, left, right
            ((0, 0), (sw, y), DIM),
            ((0, y + h), (sw, sh - y - h), DIM),
            ((0, y), (x, h), DIM),
            ((x + w, y), (sw - x - w, h), DIM),
            // outline
            ((x - 1, y - 1), (w + 2, 1), EDGE),
            ((x - 1, y + h), (w + 2, 1), EDGE),
            ((x - 1, y), (1, h), EDGE),
            ((x + w, y), (1, h), EDGE),
        ];
        // Outline first: element lists are front to back.
        let order = [4, 5, 6, 7, 0, 1, 2, 3];
        for (i, (_, size, color)) in rects.iter().enumerate() {
            self.pieces[i].update(*size, *color);
        }
        order.iter().map(|&i| (&self.pieces[i], Point::from(rects[i].0))).collect()
    }
}

impl State {
    /// Start, the search pane, flyouts and menus stay open, as on Windoze, so they
    /// can be captured.
    pub fn start_screenshot(&mut self, region: bool) {
        if region {
            self.snip = Some(Snip::default());
            self.cursor_status = CursorImageStatus::Named(CursorIcon::Crosshair);
        } else {
            self.screenshot_requested = Some(None);
        }
        self.needs_redraw = true;
    }

    pub fn cancel_snip(&mut self) {
        self.snip = None;
        self.cursor_status = CursorImageStatus::default_named();
        self.needs_redraw = true;
    }
}

/// Render the current scene (minus cursor) offscreen, crop, save, copy to clipboard.
pub fn take(renderer: &mut GlesRenderer, state: &mut State, region: Option<Rectangle<i32, Logical>>) {
    match capture(renderer, state, region) {
        Ok((w, h, rgba)) => {
            let path = save(state, w, h, rgba);
            // A region snip opens in Snip & Sketchy for marking up, like Windoze does.
            if let (true, Some(path), Some(app)) = (region.is_some(), path, crate::input::own_app("snip-and-sketchy")) {
                state.spawn(&format!("{app} '{}'", path.display()));
            }
        }
        Err(e) => tracing::warn!("screenshot failed: {e}"),
    }
}

pub fn screencast_tick(renderer: &mut GlesRenderer, state: &mut State) {
    if state.screencast_clients == 0 {
        return;
    }
    if state.last_screencast.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(33)) {
        return;
    }
    state.last_screencast = Some(std::time::Instant::now());
    let Ok((w, h, rgba)) = capture_with_cursor(renderer, state, None, true) else { return };
    let dir = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let tmp = dir.join("vide-screencast.tmp");
    let dst = dir.join("vide-screencast.rgba");
    if let Ok(mut f) = std::fs::File::create(&tmp) {
        let _ = f.write_all(&w.to_le_bytes());
        let _ = f.write_all(&h.to_le_bytes());
        if f.write_all(&rgba).is_ok() {
            let _ = std::fs::rename(&tmp, &dst);
        }
    }
}

fn capture(
    renderer: &mut GlesRenderer,
    state: &mut State,
    region: Option<Rectangle<i32, Logical>>,
) -> Result<(u32, u32, Vec<u8>), Box<dyn std::error::Error>> {
    capture_with_cursor(renderer, state, region, false)
}

fn capture_with_cursor(
    renderer: &mut GlesRenderer,
    state: &mut State,
    region: Option<Rectangle<i32, Logical>>,
    cursor: bool,
) -> Result<(u32, u32, Vec<u8>), Box<dyn std::error::Error>> {
    let size = state.output_size;
    let full = Rectangle::from_size(size);
    let rect = region.and_then(|r| r.intersection(full)).unwrap_or(full);

    let elements = render::elements_with(renderer, state, cursor);
    let mut tex: GlesTexture = renderer.create_buffer(Fourcc::Abgr8888, size.to_buffer(1, Transform::Normal))?;
    let mut fb = renderer.bind(&mut tex)?;
    let mut tracker = OutputDamageTracker::new(size.to_physical(1), 1.0, Transform::Normal);
    tracker.render_output(renderer, &mut fb, 0, &elements, render::CLEAR)?;

    let buf_rect = Rectangle::new((rect.loc.x, rect.loc.y).into(), (rect.size.w, rect.size.h).into());
    let mapping = renderer.copy_framebuffer(&fb, buf_rect, Fourcc::Abgr8888)?;
    let mut rgba = renderer.map_texture(&mapping)?.to_vec();
    // Screens are opaque; drop any stray alpha so viewers don't show holes.
    for px in rgba.chunks_mut(4) {
        px[3] = 255;
    }
    Ok((rect.size.w as u32, rect.size.h as u32, rgba))
}

/// Save to ~/Pictures/Screenshots and put it on the clipboard; returns where it went.
fn save(state: &mut State, w: u32, h: u32, rgba: Vec<u8>) -> Option<PathBuf> {
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        match enc.write_header().and_then(|mut wr| wr.write_image_data(&rgba)) {
            Ok(()) => {}
            Err(e) => {
                tracing::warn!("screenshot encode failed: {e}");
                return None;
            }
        }
    }

    // A test asked for a specific file: write only that, and leave the clipboard alone.
    if let Some(path) = state.screenshot_path.take() {
        return match std::fs::write(&path, &png_bytes) {
            Ok(()) => {
                tracing::info!("screenshot saved to {}", path.display());
                Some(path)
            }
            Err(e) => {
                tracing::warn!("couldn't save screenshot to {}: {e}", path.display());
                None
            }
        };
    }

    let dir = std::env::var("HOME").map(PathBuf::from).unwrap_or_default().join("Pictures/Screenshots");
    let path = dir.join(chrono::Local::now().format("Screenshot_%Y-%m-%d_%H-%M-%S.png").to_string());
    let res = std::fs::create_dir_all(&dir).and_then(|_| std::fs::File::create(&path)?.write_all(&png_bytes));
    let saved = match res {
        Ok(()) => {
            tracing::info!("screenshot saved to {}", path.display());
            Some(path)
        }
        Err(e) => {
            tracing::warn!("couldn't save screenshot to {}: {e}", path.display());
            None
        }
    };

    // Also on the clipboard, so it can be pasted straight into a chat.
    state.set_clipboard_bytes(vec!["image/png".into()], png_bytes);
    saved
}
