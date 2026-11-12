//! App icons from the installed icon themes (PNG and SVG), like any desktop.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

/// Premultiplied RGBA pixels.
pub struct Icon {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

pub struct Icons {
    /// Icon name -> best file for it, across the installed themes.
    index: HashMap<String, PathBuf>,
    cache: HashMap<(String, u32), Option<Rc<Icon>>>,
}

impl Icons {
    pub fn new() -> Self {
        let mut themes = Vec::new();
        if let Some(t) = kde_icon_theme() {
            themes.push(t);
        }
        for t in ["breeze", "breeze-dark", "Adwaita", "hicolor"] {
            if !themes.iter().any(|x| x == t) {
                themes.push(t.into());
            }
        }
        let mut bases = vec![PathBuf::from("/usr/share/icons")];
        if let Ok(home) = std::env::var("HOME") {
            bases.insert(0, PathBuf::from(format!("{home}/.local/share/icons")));
        }

        // Earlier themes win; within a theme, the best-scoring file wins.
        let mut index: HashMap<String, (usize, i32, PathBuf)> = HashMap::new();
        for (rank, theme) in themes.iter().enumerate() {
            for base in &bases {
                walk(&base.join(theme), 0, &mut |path| {
                    let Some(name) = path.file_stem().and_then(|s| s.to_str()) else { return };
                    let score = score(path);
                    let better = match index.get(name) {
                        None => true,
                        Some((r, sc, _)) => rank < *r || (rank == *r && score > *sc),
                    };
                    if better {
                        index.insert(name.to_string(), (rank, score, path.to_path_buf()));
                    }
                });
            }
        }
        let rank = themes.len();
        walk(Path::new("/usr/share/pixmaps"), 1, &mut |path| {
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                index.entry(name.to_string()).or_insert((rank, 0, path.to_path_buf()));
            }
        });
        tracing::info!("indexed {} icons from {:?}", index.len(), themes);
        Self { index: index.into_iter().map(|(k, (_, _, p))| (k, p)).collect(), cache: HashMap::new() }
    }

    pub fn get(&mut self, name: &str, size: u32) -> Option<Rc<Icon>> {
        let key = (name.to_string(), size);
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let icon = self.find(name).and_then(|p| load(&p, size)).map(Rc::new);
        self.cache.insert(key, icon.clone());
        icon
    }

    fn find(&self, name: &str) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }
        let p = Path::new(name);
        if p.is_absolute() {
            return p.exists().then(|| p.to_path_buf());
        }
        self.index.get(name).cloned()
    }
}

/// Collect .png/.svg files under `dir`, a few levels deep.
fn walk(dir: &Path, depth: u32, f: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if depth < 3 {
                walk(&path, depth + 1, f);
            }
        } else if matches!(path.extension().and_then(|x| x.to_str()), Some("png" | "svg")) {
            f(&path);
        }
    }
}

/// Prefer full-colour scalable icons, then rasters near 48px; avoid symbolic/monochrome ones.
fn score(path: &Path) -> i32 {
    let s = path.to_string_lossy();
    if s.contains("-symbolic") || s.contains("/symbolic/") {
        return -1000;
    }
    let mut score = if s.ends_with(".svg") { 100 } else { 0 };
    for comp in path.components().rev().skip(1).take(3) {
        let c = comp.as_os_str().to_string_lossy();
        let n: Option<i32> = c.split('x').next().and_then(|n| n.split('@').next()).and_then(|n| n.parse().ok());
        if let Some(n) = n {
            score += 50 - (n - 48).abs().min(50);
            break;
        }
    }
    if s.contains("/apps/") {
        score += 20;
    }
    score
}

fn kde_icon_theme() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let rc = std::fs::read_to_string(format!("{home}/.config/kdeglobals")).ok()?;
    let mut in_icons = false;
    for line in rc.lines() {
        if line.starts_with('[') {
            in_icons = line == "[Icons]";
        } else if in_icons {
            if let Some(t) = line.strip_prefix("Theme=") {
                return Some(t.trim().to_string());
            }
        }
    }
    None
}

fn load(path: &Path, size: u32) -> Option<Icon> {
    match path.extension()?.to_str()? {
        "svg" => load_svg(path, size),
        "png" => load_png(path).map(|i| scale(&i, size)),
        _ => None,
    }
}

fn load_svg(path: &Path, size: u32) -> Option<Icon> {
    use resvg::{tiny_skia, usvg};
    let data = std::fs::read(path).ok()?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let s = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, tiny_skia::Transform::from_scale(s, s), &mut pixmap.as_mut());
    Some(Icon { w: size, h: size, rgba: pixmap.take() })
}

fn load_png(path: &Path) -> Option<Icon> {
    decode_png(std::io::BufReader::new(std::fs::File::open(path).ok()?))
}

/// Our logo, baked into the binary so it shows no matter where we're run from.
pub fn logo(height: u32) -> Option<Icon> {
    let src = decode_png(std::io::Cursor::new(include_bytes!("../assets/logo-512.png")))?;
    let w = (src.w * height + src.h / 2) / src.h;
    Some(scale_to(&src, w, height))
}

fn decode_png(r: impl std::io::BufRead + std::io::Seek) -> Option<Icon> {
    let mut decoder = png::Decoder::new(r);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let px = &buf[..info.buffer_size()];
    let mut rgba = Vec::with_capacity((info.width * info.height * 4) as usize);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(px),
        png::ColorType::Rgb => px.chunks(3).for_each(|c| rgba.extend_from_slice(&[c[0], c[1], c[2], 255])),
        png::ColorType::GrayscaleAlpha => px.chunks(2).for_each(|c| rgba.extend_from_slice(&[c[0], c[0], c[0], c[1]])),
        png::ColorType::Grayscale => px.iter().for_each(|&g| rgba.extend_from_slice(&[g, g, g, 255])),
        png::ColorType::Indexed => return None,
    }
    // Premultiply.
    for p in rgba.chunks_mut(4) {
        let a = p[3] as u16;
        for c in &mut p[..3] {
            *c = ((*c as u16 * a + 127) / 255) as u8;
        }
    }
    Some(Icon { w: info.width, h: info.height, rgba })
}

fn scale(src: &Icon, size: u32) -> Icon {
    scale_to(src, size, size)
}

/// Box-filter (or nearest, when enlarging) scale to w×h.
fn scale_to(src: &Icon, w: u32, h: u32) -> Icon {
    if src.w == w && src.h == h {
        return Icon { w, h, rgba: src.rgba.clone() };
    }
    let mut out = vec![0u8; (w * h * 4) as usize];
    let (sx, sy) = (src.w as f32 / w as f32, src.h as f32 / h as f32);
    for y in 0..h {
        for x in 0..w {
            let (x0, y0) = ((x as f32 * sx) as u32, (y as f32 * sy) as u32);
            let x1 = (((x + 1) as f32 * sx) as u32).max(x0 + 1).min(src.w);
            let y1 = (((y + 1) as f32 * sy) as u32).max(y0 + 1).min(src.h);
            let mut acc = [0u32; 4];
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let i = ((yy * src.w + xx) * 4) as usize;
                    for k in 0..4 {
                        acc[k] += src.rgba[i + k] as u32;
                    }
                }
            }
            let n = ((x1 - x0) * (y1 - y0)).max(1);
            let o = ((y * w + x) * 4) as usize;
            for k in 0..4 {
                out[o + k] = (acc[k] / n) as u8;
            }
        }
    }
    Icon { w, h, rgba: out }
}
