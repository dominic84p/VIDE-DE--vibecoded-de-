//! A tiny CPU 2D canvas: rects, rounded rects, circles, text. Everything in
//! our chrome (panel, titlebars) is painted here and uploaded as one texture.

use fontdue::Font;
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::Transform,
};

/// Straight (non-premultiplied) RGBA.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

pub const fn hex(c: u32) -> Rgba {
    Rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 0xff)
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    /// Premultiplied BGRA, i.e. Argb8888 in memory on little endian.
    px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        let (w, h) = (w.max(1), h.max(1));
        Self { w, h, px: vec![0; (w * h * 4) as usize] }
    }

    /// Source-over blend of `c` at `cov` coverage (0..=1).
    fn blend(&mut self, x: i32, y: i32, c: Rgba, cov: f32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || cov <= 0.0 {
            return;
        }
        let a = c.3 as f32 / 255.0 * cov.min(1.0);
        let i = ((y * self.w + x) * 4) as usize;
        let src = [c.2, c.1, c.0];
        for k in 0..3 {
            let d = self.px[i + k] as f32;
            self.px[i + k] = (src[k] as f32 * a + d * (1.0 - a)).round() as u8;
        }
        let da = self.px[i + 3] as f32;
        self.px[i + 3] = (255.0 * a + da * (1.0 - a)).round() as u8;
    }

    pub fn fill(&mut self, c: Rgba) {
        self.rect(0, 0, self.w, self.h, c);
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.blend(xx, yy, c, 1.0);
            }
        }
    }

    /// Rounded rectangle with 1px anti-aliased corners.
    pub fn round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: Rgba) {
        let r = r.min(w / 2).min(h / 2) as f32;
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                let px = xx as f32 + 0.5;
                let py = yy as f32 + 0.5;
                // Distance outside the rounded shape (negative = inside).
                let cx = px.clamp(x as f32 + r, (x + w) as f32 - r);
                let cy = py.clamp(y as f32 + r, (y + h) as f32 - r);
                let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r;
                self.blend(xx, yy, c, (0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for yy in y0..=y1 {
            for xx in x0..=x1 {
                let d = ((xx as f32 + 0.5 - cx).powi(2) + (yy as f32 + 0.5 - cy).powi(2)).sqrt() - r;
                self.blend(xx, yy, c, (0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    /// Anti-aliased line segment of thickness `width`.
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, c: Rgba) {
        let pad = width.ceil() as i32 + 1;
        let (minx, maxx) = (x0.min(x1) as i32 - pad, x0.max(x1) as i32 + pad);
        let (miny, maxy) = (y0.min(y1) as i32 - pad, y0.max(y1) as i32 + pad);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        for y in miny..=maxy {
            for x in minx..=maxx {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0);
                let d = ((px - (x0 + t * dx)).powi(2) + (py - (y0 + t * dy)).powi(2)).sqrt() - width / 2.0;
                self.blend(x, y, c, (0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    /// 1px rectangle outline.
    pub fn outline(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        self.rect(x, y, w, 1, c);
        self.rect(x, y + h - 1, w, 1, c);
        self.rect(x, y, 1, h, c);
        self.rect(x + w - 1, y, 1, h, c);
    }

    /// Blit a premultiplied RGBA icon with its top-left at (x, y).
    pub fn icon(&mut self, icon: &crate::icons::Icon, x: i32, y: i32) {
        for iy in 0..icon.h as i32 {
            for ix in 0..icon.w as i32 {
                let (dx, dy) = (x + ix, y + iy);
                if dx < 0 || dy < 0 || dx >= self.w || dy >= self.h {
                    continue;
                }
                let s = ((iy * icon.w as i32 + ix) * 4) as usize;
                let a = icon.rgba[s + 3] as f32 / 255.0;
                if a == 0.0 {
                    continue;
                }
                let d = ((dy * self.w + dx) * 4) as usize;
                // Premultiplied source over: dst = src + dst * (1 - a). Canvas is BGRA.
                let src = [icon.rgba[s + 2], icon.rgba[s + 1], icon.rgba[s], icon.rgba[s + 3]];
                for k in 0..4 {
                    self.px[d + k] = (src[k] as f32 + self.px[d + k] as f32 * (1.0 - a)).round().min(255.0) as u8;
                }
            }
        }
    }

    /// Fill with a diagonal two-colour gradient plus a soft glow at (gx, gy).
    pub fn wallpaper(&mut self, a: Rgba, b: Rgba, glow: Rgba, gx: f32, gy: f32, gr: f32) {
        let (w, h) = (self.w as f32, self.h as f32);
        for y in 0..self.h {
            for x in 0..self.w {
                let t = ((x as f32 / w) * 0.6 + (y as f32 / h) * 0.4).clamp(0.0, 1.0);
                let mix = |p: u8, q: u8| (p as f32 + (q as f32 - p as f32) * t) as u8;
                let mut c = [mix(a.0, b.0) as f32, mix(a.1, b.1) as f32, mix(a.2, b.2) as f32];
                let d = (((x as f32 - gx).powi(2) + (y as f32 - gy).powi(2)).sqrt() / gr).min(1.0);
                let g = (1.0 - d).powi(2) * glow.3 as f32 / 255.0;
                c[0] += (glow.0 as f32 - c[0]) * g;
                c[1] += (glow.1 as f32 - c[1]) * g;
                c[2] += (glow.2 as f32 - c[2]) * g;
                let i = ((y * self.w + x) * 4) as usize;
                self.px[i..i + 4].copy_from_slice(&[c[2] as u8, c[1] as u8, c[0] as u8, 255]);
            }
        }
    }

    /// Draw `text` with its left edge at `x`, vertically centred in the row [y, y+h).
    /// Returns the drawn width.
    pub fn text(&mut self, font: &Font, size: f32, x: i32, y: i32, h: i32, c: Rgba, text: &str) -> i32 {
        let lm = font.horizontal_line_metrics(size).unwrap();
        let baseline = y as f32 + (h as f32 + lm.ascent + lm.descent) / 2.0;
        let mut pen = x as f32;
        for ch in text.chars() {
            if vide_fonts::invisible(ch) {
                continue;
            }
            // Characters our font lacks (Japanese, emoji, Arabic...) come from a fallback font.
            let glyph_font = match vide_fonts::pick(font, ch) {
                vide_fonts::Pick::Font(f) => f,
                vide_fonts::Pick::Missing => font,
                vide_fonts::Pick::Emoji => {
                    let px = (size * 1.2).round() as u32;
                    if let Some(e) = vide_fonts::emoji(ch, px) {
                        let top = (baseline - lm.ascent + (lm.ascent - lm.descent - px as f32) / 2.0).round() as i32;
                        self.rgba_image(&e.rgba, e.size as i32, pen.round() as i32, top);
                    }
                    pen += px as f32;
                    continue;
                }
            };
            let (m, cov) = glyph_font.rasterize(ch, size);
            let gx = pen.round() as i32 + m.xmin;
            let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let a = cov[row * m.width + col] as f32 / 255.0;
                    self.blend(gx + col as i32, gy + row as i32, c, a);
                }
            }
            pen += m.advance_width;
        }
        (pen - x as f32).round() as i32
    }

    /// A square straight-alpha RGBA image (a colour emoji).
    fn rgba_image(&mut self, rgba: &[u8], size: i32, x: i32, y: i32) {
        for iy in 0..size {
            for ix in 0..size {
                let o = ((iy * size + ix) * 4) as usize;
                let p = Rgba(rgba[o], rgba[o + 1], rgba[o + 2], 255);
                self.blend(x + ix, y + iy, p, rgba[o + 3] as f32 / 255.0);
            }
        }
    }

    pub fn into_buffer(self) -> MemoryRenderBuffer {
        MemoryRenderBuffer::from_slice(&self.px, Fourcc::Argb8888, (self.w, self.h), 1, Transform::Normal, None)
    }

    /// Like `into_buffer`, but tells the renderer the whole thing is opaque.
    pub fn into_opaque_buffer(self) -> MemoryRenderBuffer {
        let full = smithay::utils::Rectangle::from_size((self.w, self.h).into());
        MemoryRenderBuffer::from_slice(&self.px, Fourcc::Argb8888, (self.w, self.h), 1, Transform::Normal, Some(vec![full]))
    }
}

pub fn measure(font: &Font, size: f32, text: &str) -> i32 {
    vide_fonts::width(font, text, size).round() as i32
}

/// Shorten `text` with an ellipsis so it fits in `max` pixels.
pub fn ellipsize(font: &Font, size: f32, text: &str, max: i32) -> String {
    if measure(font, size, text) <= max {
        return text.to_string();
    }
    let mut s: String = text.to_string();
    while !s.is_empty() && measure(font, size, &format!("{s}…")) > max {
        s.pop();
    }
    format!("{s}…")
}

/// Find a sans font on the system. We don't ship one (yet).
pub fn load_font() -> Option<Font> {
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/noto/NotoSans-Medium.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = Font::from_bytes(bytes, fontdue::FontSettings::default()) {
                tracing::info!("ui font: {path}");
                return Some(font);
            }
        }
    }
    tracing::warn!("no ui font found; text will be missing");
    None
}
