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
    /// Icons windows hand us directly (X11 `_NET_WM_ICON`), by app_id.
    custom: HashMap<String, Icon>,
    /// Icon name -> best file for it, across the installed themes.
    index: HashMap<String, PathBuf>,
    /// Normalized (alphanumeric lowercase) icon name -> best file, for case/punctuation mismatches.
    norm_index: HashMap<String, PathBuf>,
    cache: HashMap<(String, u32), Option<Rc<Icon>>>,
}

fn norm_key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

impl Icons {
    pub fn new() -> Self {
        Self::with_first(None)
    }

    /// For things drawn on a dark background (the tray): prefer the dark theme
    /// variant, whose monochrome status icons are light.
    pub fn dark() -> Self {
        let dark = kde_icon_theme().map(|t| format!("{}-dark", t.trim_end_matches("-dark"))).filter(|t| {
            std::path::Path::new("/usr/share/icons").join(t).exists()
        });
        Self::with_first(Some(dark.unwrap_or_else(|| "breeze-dark".into())))
    }

    fn with_first(first: Option<String>) -> Self {
        let mut themes = Vec::new();
        themes.extend(first);
        if let Some(t) = kde_icon_theme() {
            if !themes.contains(&t) {
                themes.push(t);
            }
        }
        for t in ["breeze", "breeze-dark", "Adwaita", "hicolor"] {
            if !themes.iter().any(|x| x == t) {
                themes.push(t.into());
            }
        }
        let mut bases: Vec<PathBuf> = data_dirs().into_iter().map(|d| d.join("icons")).filter(|p| p.is_dir()).collect();
        if let Ok(home) = std::env::var("HOME") {
            let legacy = PathBuf::from(format!("{home}/.icons"));
            if legacy.is_dir() && !bases.contains(&legacy) {
                bases.push(legacy);
            }
        }
        // Include any other installed themes after our preferred list so apps using
        // custom or fallback themes still resolve.
        for base in &bases {
            if let Ok(entries) = std::fs::read_dir(base) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                            if !themes.iter().any(|x| x == name) {
                                themes.push(name.to_string());
                            }
                        }
                    }
                }
            }
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
        for pix in data_dirs().into_iter().map(|d| d.join("pixmaps")).chain(std::iter::once(PathBuf::from("/usr/share/pixmaps"))) {
            walk(&pix, 1, &mut |path| {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    index.entry(name.to_string()).or_insert((rank, 0, path.to_path_buf()));
                }
            });
        }
        tracing::info!("indexed {} icons from {:?}", index.len(), themes);
        let mut norm_index: HashMap<String, (usize, i32, PathBuf)> = HashMap::new();
        for (name, (r, sc, p)) in &index {
            let k = norm_key(name);
            if k.is_empty() {
                continue;
            }
            let better = match norm_index.get(&k) {
                None => true,
                Some((nr, nsc, _)) => *r < *nr || (*r == *nr && *sc > *nsc),
            };
            if better {
                norm_index.insert(k, (*r, *sc, p.clone()));
            }
        }
        Self {
            custom: HashMap::new(),
            index: index.into_iter().map(|(k, (_, _, p))| (k, p)).collect(),
            norm_index: norm_index.into_iter().map(|(k, (_, _, p))| (k, p)).collect(),
            cache: HashMap::new(),
        }
    }

    /// Remember an icon a window supplied itself; used when nothing installed matches.
    pub fn set_custom(&mut self, name: &str, icon: Icon) {
        self.cache.retain(|(n, _), _| n != name);
        self.custom.insert(name.to_string(), icon);
    }

    pub fn get(&mut self, name: &str, size: u32) -> Option<Rc<Icon>> {
        let key = (name.to_string(), size);
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let icon = self
            .find(name)
            .and_then(|p| load(&p, size))
            .or_else(|| self.find_custom(name).map(|i| scale(i, size)))
            .map(Rc::new);
        self.cache.insert(key, icon.clone());
        icon
    }

    fn find_custom(&self, name: &str) -> Option<&Icon> {
        if let Some(i) = self.custom.get(name) {
            return Some(i);
        }
        let want = norm_key(name);
        (!want.is_empty()).then(|| self.custom.iter().find(|(k, _)| norm_key(k) == want).map(|(_, v)| v)).flatten()
    }

    fn find(&self, name: &str) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }
        let p = Path::new(name);
        if p.is_absolute() {
            return p.exists().then(|| p.to_path_buf());
        }
        let lookup = |candidate: &str| -> Option<PathBuf> {
            self.index.get(candidate).cloned().or_else(|| {
                let k = norm_key(candidate);
                (!k.is_empty()).then(|| self.norm_index.get(&k).cloned()).flatten()
            })
        };
        if let Some(hit) = lookup(name) {
            return Some(hit);
        }
        // Reverse-DNS app IDs (e.g. com.valvesoftware.Steam, org.gimp.GIMP) -> last segment.
        if let Some(last) = name.rsplit('.').next().filter(|s| !s.is_empty() && *s != name) {
            if let Some(hit) = lookup(last) {
                return Some(hit);
            }
        }
        // Steam app IDs (steam_app_12345 -> steam_icon_12345) and Wine/Electron suffixes.
        if let Some(id) = name.strip_prefix("steam_app_") {
            if let Some(hit) = lookup(&format!("steam_icon_{id}")) {
                return Some(hit);
            }
        }
        for suffix in [".exe", "-bin", "-desktop", ".desktop"] {
            if let Some(stripped) = name.strip_suffix(suffix) {
                if let Some(hit) = lookup(stripped) {
                    return Some(hit);
                }
            }
        }
        // Hyphenated fallback (e.g. foo-bar-baz -> foo-bar -> foo).
        let mut cur = name;
        while let Some((prefix, _)) = cur.rsplit_once('-') {
            if let Some(hit) = lookup(prefix) {
                return Some(hit);
            }
            cur = prefix;
        }
        None
    }
}

/// Collect .png/.svg files under `dir`, a few levels deep.
fn walk(dir: &Path, depth: u32, f: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if depth < 4 {
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

/// Where icons and apps live: the user's dir, then `XDG_DATA_DIRS` (which is how
/// Flatpak and Snap apps get found), then the usual defaults.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(format!("{home}/.local/share")));
        dirs.push(PathBuf::from(format!("{home}/.local/share/flatpak/exports/share")));
    }
    let xdg = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    dirs.extend(xdg.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    for d in ["/var/lib/flatpak/exports/share", "/usr/local/share", "/usr/share"] {
        dirs.push(PathBuf::from(d));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
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
    render_svg(&std::fs::read(path).ok()?, size)
}

/// Rasterize SVG source to a size×size icon.
pub fn render_svg(data: &[u8], size: u32) -> Option<Icon> {
    use resvg::{tiny_skia, usvg};
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

/// Wrap raw premultiplied RGBA (e.g. a tray app's pixmap) as an icon of `size`×`size`.
pub fn from_rgba(w: u32, h: u32, rgba: Vec<u8>, size: u32) -> Icon {
    scale(&Icon { w, h, rgba }, size)
}

/// Fit into size×size keeping the aspect ratio, centred on transparent padding.
fn scale(src: &Icon, size: u32) -> Icon {
    if src.w == src.h {
        return scale_to(src, size, size);
    }
    let (w, h) = if src.w > src.h {
        (size, (src.h * size / src.w).max(1))
    } else {
        ((src.w * size / src.h).max(1), size)
    };
    let fitted = scale_to(src, w, h);
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let (ox, oy) = ((size - w) / 2, (size - h) / 2);
    for y in 0..h {
        let src_row = &fitted.rgba[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
        let at = (((y + oy) * size + ox) * 4) as usize;
        rgba[at..at + src_row.len()].copy_from_slice(src_row);
    }
    Icon { w: size, h: size, rgba }
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
