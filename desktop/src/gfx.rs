//! Pixels and files for the desktop: a tiny BGRA canvas, fonts, icons, wallpaper,
//! and .desktop launcher parsing.

use std::path::{Path, PathBuf};

pub const LABEL: f32 = 12.5;

/// Premultiplied BGRA (matches wl_shm Argb8888 in memory).
pub struct Icon {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}


pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// XDG desktop dir (user-dirs.dirs), defaulting to ~/Desktop.
pub fn desktop_dir() -> PathBuf {
    let h = home();
    let dirs = std::fs::read_to_string(h.join(".config/user-dirs.dirs")).unwrap_or_default();
    for line in dirs.lines() {
        if let Some(v) = line.strip_prefix("XDG_DESKTOP_DIR=") {
            return PathBuf::from(v.trim_matches('"').replace("$HOME", &h.to_string_lossy()));
        }
    }
    h.join("Desktop")
}

/// Our own apps live next to us.
pub fn own_app(name: &str) -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join(name);
    p.exists().then_some(p)
}

pub fn trash_dir() -> PathBuf {
    home().join(".local/share/Trash/files")
}


pub struct Canvas<'a> {
    pub w: i32,
    pub h: i32,
    /// BGRA, premultiplied.
    pub px: &'a mut [u8],
}

impl Canvas<'_> {
    /// Blend a straight-alpha BGRA colour at `cov` coverage.
    pub fn blend(&mut self, x: i32, y: i32, c: [u8; 4], cov: f32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let a = c[3] as f32 / 255.0 * cov;
        if a <= 0.0 {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        for k in 0..3 {
            self.px[i + k] = (c[k] as f32 * a + self.px[i + k] as f32 * (1.0 - a)) as u8;
        }
        self.px[i + 3] = 255;
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.blend(xx, yy, c, 1.0);
            }
        }
    }

    pub fn outline(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        self.rect(x, y, w, 1, c);
        self.rect(x, y + h - 1, w, 1, c);
        self.rect(x, y, 1, h, c);
        self.rect(x + w - 1, y, 1, h, c);
    }

    pub fn image(&mut self, img: &Icon, x: i32, y: i32) {
        for iy in 0..img.h as i32 {
            for ix in 0..img.w as i32 {
                let (dx, dy) = (x + ix, y + iy);
                if dx < 0 || dy < 0 || dx >= self.w || dy >= self.h {
                    continue;
                }
                let s = ((iy * img.w as i32 + ix) * 4) as usize;
                let a = img.px[s + 3] as f32 / 255.0;
                if a == 0.0 {
                    continue;
                }
                let d = ((dy * self.w + dx) * 4) as usize;
                for k in 0..3 {
                    self.px[d + k] = (img.px[s + k] as f32 + self.px[d + k] as f32 * (1.0 - a)).min(255.0) as u8;
                }
            }
        }
    }

    /// `y` is the top of the line.
    pub fn text(&mut self, font: &fontdue::Font, x: i32, y: i32, s: &str, c: [u8; 4]) {
        let lm = font.horizontal_line_metrics(LABEL).unwrap();
        let baseline = y as f32 + lm.ascent;
        let mut pen = x as f32;
        for ch in s.chars() {
            let (m, cov) = font.rasterize(ch, LABEL);
            let gx = pen.round() as i32 + m.xmin;
            let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    self.blend(gx + col as i32, gy + row as i32, c, cov[row * m.width + col] as f32 / 255.0);
                }
            }
            pen += m.advance_width;
        }
    }
}

pub fn text_width(font: &fontdue::Font, s: &str) -> i32 {
    s.chars().map(|c| font.metrics(c, LABEL).advance_width).sum::<f32>().round() as i32
}

/// Word-wrap into at most `lines` lines, ellipsizing the last.
pub fn wrap(font: &fontdue::Font, s: &str, max: f32, lines: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in s.split_inclusive([' ', '-', '_', '.']) {
        let trial = format!("{cur}{word}");
        if text_width(font, &trial) as f32 <= max || cur.is_empty() {
            cur = trial;
        } else {
            out.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    if out.len() > lines {
        out.truncate(lines);
        let last = out.last_mut().unwrap();
        while !last.is_empty() && text_width(font, &format!("{last}…")) as f32 > max {
            last.pop();
        }
        last.push('…');
    }
    // A single overlong word: cut it down too.
    for l in &mut out {
        while text_width(font, l) as f32 > max && l.chars().count() > 1 {
            l.pop();
            l.pop();
            l.push('…');
        }
    }
    out
}

pub fn load_font() -> Option<fontdue::Font> {
    ["/usr/share/fonts/noto/NotoSans-Regular.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/dejavu/DejaVuSans.ttf"]
        .iter()
        .find_map(|p| std::fs::read(p).ok())
        .and_then(|b| fontdue::Font::from_bytes(b, fontdue::FontSettings::default()).ok())
}

/// Decode a PNG to straight RGBA.
pub fn decode_png(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let mut d = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).ok()?));
    d.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    let px = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((info.width, info.height, rgba))
}

/// The wallpaper scaled to cover w×h (cropping the overflow), as BGRA.
pub fn load_wallpaper(path: &Path, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    let Some((sw, sh, src)) = decode_png(path) else {
        // No image: VIDE blue.
        for p in out.chunks_mut(4) {
            p.copy_from_slice(&[0x8f, 0x4b, 0x0b, 0xff]);
        }
        return out;
    };
    let scale = (w as f32 / sw as f32).max(h as f32 / sh as f32);
    let (ox, oy) = ((sw as f32 * scale - w as f32) / 2.0, (sh as f32 * scale - h as f32) / 2.0);
    for y in 0..h {
        let sy = (((y as f32 + oy) / scale) as u32).min(sh - 1);
        for x in 0..w {
            let sx = (((x as f32 + ox) / scale) as u32).min(sw - 1);
            let s = ((sy * sw + sx) * 4) as usize;
            let d = ((y * w + x) * 4) as usize;
            out[d..d + 4].copy_from_slice(&[src[s + 2], src[s + 1], src[s], 255]);
        }
    }
    out
}

/// A theme icon (SVG or PNG) as premultiplied BGRA, size×size.
pub fn load_icon(name: &str, size: u32) -> Option<Icon> {
    let path = if Path::new(name).is_absolute() { Some(PathBuf::from(name)) } else { find_icon(name) }?;
    if path.extension().is_some_and(|e| e == "svg") {
        use resvg::{tiny_skia, usvg};
        let tree = usvg::Tree::from_data(&std::fs::read(&path).ok()?, &usvg::Options::default()).ok()?;
        let mut pm = tiny_skia::Pixmap::new(size, size)?;
        let s = size as f32 / tree.size().width().max(tree.size().height());
        resvg::render(&tree, tiny_skia::Transform::from_scale(s, s), &mut pm.as_mut());
        let px = pm.take().chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
        Some(Icon { w: size, h: size, px })
    } else {
        let (sw, sh, rgba) = decode_png(&path)?;
        let mut px = vec![0u8; (size * size * 4) as usize];
        for y in 0..size {
            for x in 0..size {
                let s = (((y * sh / size) * sw + x * sw / size) * 4) as usize;
                let a = rgba[s + 3] as u16;
                let d = ((y * size + x) * 4) as usize;
                px[d..d + 4].copy_from_slice(&[
                    (rgba[s + 2] as u16 * a / 255) as u8,
                    (rgba[s + 1] as u16 * a / 255) as u8,
                    (rgba[s] as u16 * a / 255) as u8,
                    a as u8,
                ]);
            }
        }
        Some(Icon { w: size, h: size, px })
    }
}

pub fn find_icon(name: &str) -> Option<PathBuf> {
    let mut bases = vec![home().join(".local/share/icons")];
    for d in std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/share".into()).split(':') {
        bases.push(PathBuf::from(d).join("icons"));
    }
    bases.push("/usr/share/icons".into());
    let subdirs = ["places/48", "apps/48", "mimetypes/48", "places/64", "apps/64", "mimetypes/64", "48x48/apps", "scalable/apps", "256x256/apps", "128x128/apps", "places/32", "mimetypes/32"];
    for base in &bases {
        for theme in ["breeze", "hicolor", "Adwaita"] {
            for sub in subdirs {
                for ext in ["svg", "png"] {
                    let p = base.join(theme).join(sub).join(format!("{name}.{ext}"));
                    if p.exists() {
                        return Some(p);
                    }
                }
            }
        }
    }
    ["svg", "png"].iter().map(|e| PathBuf::from(format!("/usr/share/pixmaps/{name}.{e}"))).find(|p| p.exists())
}

/// Icon name for a file, from its extension.
pub fn mime_icon(path: &Path) -> String {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => "image-x-generic",
        "mp4" | "mkv" | "webm" | "mov" | "avi" => "video-x-generic",
        "mp3" | "flac" | "ogg" | "wav" | "opus" => "audio-x-generic",
        "pdf" => "application-pdf",
        "zip" | "tar" | "gz" | "xz" | "7z" | "rar" | "zst" => "package-x-generic",
        "sh" => "application-x-shellscript",
        "appimage" => "application-x-executable",
        "txt" | "md" | "log" => "text-x-generic",
        _ => "text-x-generic",
    }
    .into()
}

/// Name, Exec (field codes stripped) and Icon from a .desktop launcher.
pub fn parse_launcher(path: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let (mut name, mut exec, mut icon, mut in_entry) = (None, None, None, false);
    for line in text.lines() {
        if line.starts_with('[') {
            in_entry = line.trim() == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(v) = line.strip_prefix("Name=") {
            name.get_or_insert(v.to_string());
        } else if let Some(v) = line.strip_prefix("Exec=") {
            let cleaned: Vec<&str> = v.split_whitespace().filter(|w| !w.starts_with('%') && !w.starts_with("@@")).collect();
            exec.get_or_insert(cleaned.join(" "));
        } else if let Some(v) = line.strip_prefix("Icon=") {
            icon.get_or_insert(v.to_string());
        }
    }
    (name, exec, icon)
}

// ---------------------------------------------------------------- wayland plumbing

