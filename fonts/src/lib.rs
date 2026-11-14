//! Font fallback for everything we draw ourselves. Our UI font has Latin and a bit more; a
//! Japanese file name, an emoji in a window title or Arabic in a notification used to come out
//! as boxes. For a character our font lacks we ask fontconfig which installed font has it
//! (`fc-match sans-serif:charset=...`), the same choice the rest of the desktop makes, and cache
//! the answer. Colour emoji fonts have no outlines for fontdue to draw, so their bitmaps are
//! read directly.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use fontdue::{Font, FontSettings};

enum Loaded {
    Outline(Font),
    /// A bitmap (CBDT/sbix) font: the raw file and face index, parsed again to draw a glyph.
    Color(Arc<Vec<u8>>, u32),
}

/// Fonts loaded so far (leaked: a handful at most, kept for the life of the program), and
/// which one each character came from.
#[derive(Default)]
struct Registry {
    fonts: HashMap<(PathBuf, u32), Option<&'static Loaded>>,
    chars: HashMap<char, Option<&'static Loaded>>,
}

fn registry() -> &'static Mutex<Registry> {
    static R: OnceLock<Mutex<Registry>> = OnceLock::new();
    R.get_or_init(Default::default)
}

fn load(path: PathBuf, index: u32) -> Option<&'static Loaded> {
    let bytes = std::fs::read(&path).ok()?;
    let face = ttf_parser::Face::parse(&bytes, index).ok()?;
    let tables = face.tables();
    let loaded = if tables.glyf.is_none() && tables.cff.is_none() && (tables.cbdt.is_some() || tables.sbix.is_some()) {
        Loaded::Color(Arc::new(bytes), index)
    } else {
        Loaded::Outline(Font::from_bytes(bytes, FontSettings { collection_index: index, ..Default::default() }).ok()?)
    };
    Some(Box::leak(Box::new(loaded)))
}

fn covers(loaded: &Loaded, ch: char) -> bool {
    match loaded {
        Loaded::Outline(f) => f.lookup_glyph_index(ch) != 0,
        Loaded::Color(bytes, index) => ttf_parser::Face::parse(bytes, *index).ok().and_then(|f| f.glyph_index(ch)).is_some(),
    }
}

/// Every installed font, best first for sans-serif text: fontconfig's own preference order.
fn preference() -> &'static [(PathBuf, u32)] {
    static ORDER: OnceLock<Vec<(PathBuf, u32)>> = OnceLock::new();
    ORDER.get_or_init(|| fc_lines(&["-s", "sans-serif", "--format=%{file}\t%{index}\n"], "fc-match"))
}

fn fc_lines(args: &[&str], cmd: &str) -> Vec<(PathBuf, u32)> {
    let out = std::process::Command::new(cmd).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    out.lines()
        .filter_map(|l| {
            let (file, index) = l.split_once('\t')?;
            Some((PathBuf::from(file), index.trim().parse().unwrap_or(0)))
        })
        .collect()
}

/// The installed font for `ch`: of the fonts that have it (`fc-list :charset=`), the one
/// fontconfig would prefer for sans-serif text.
fn fallback_for(ch: char) -> Option<&'static Loaded> {
    if let Some(hit) = registry().lock().ok()?.chars.get(&ch) {
        return *hit;
    }
    let having: std::collections::HashSet<(PathBuf, u32)> =
        fc_lines(&["--format=%{file}\t%{index}\n", &format!(":charset={:x}", ch as u32)], "fc-list").into_iter().collect();
    // Colour emoji first, as on Windoze: plain fonts like DejaVu have black-and-white ones too.
    let emoji_fonts = having.iter().filter(|(p, _)| p.to_string_lossy().to_lowercase().contains("emoji"));
    let in_order = preference().iter().filter(|k| having.contains(*k));
    let found = emoji_fonts.chain(in_order).take(4).find_map(|key| {
        let mut reg = registry().lock().ok()?;
        let font = *reg.fonts.entry(key.clone()).or_insert_with(|| load(key.0.clone(), key.1));
        font.filter(|f| covers(f, ch))
    });
    if let Ok(mut reg) = registry().lock() {
        reg.chars.insert(ch, found);
    }
    found
}

/// How to draw one character.
pub enum Pick<'a> {
    Font(&'a Font),
    /// A colour emoji: draw `emoji(ch, px)`.
    Emoji,
    /// Nothing has it; draw it with the main font anyway (a box).
    Missing,
}

/// Joiners and variation selectors: they shape emoji sequences, which we draw piece by piece.
pub fn invisible(ch: char) -> bool {
    matches!(ch, '\u{200d}' | '\u{fe0e}' | '\u{fe0f}' | '\u{20e3}') || ('\u{1f3fb}'..='\u{1f3ff}').contains(&ch)
}

/// The font for `ch`: `primary` when it has it, otherwise whichever installed font does.
pub fn pick(primary: &Font, ch: char) -> Pick<'_> {
    if ch.is_ascii() || primary.lookup_glyph_index(ch) != 0 {
        return Pick::Font(primary);
    }
    match fallback_for(ch) {
        Some(Loaded::Outline(f)) => Pick::Font(f),
        Some(Loaded::Color(..)) => Pick::Emoji,
        None => Pick::Missing,
    }
}

/// How far the pen moves for `ch` at `size` px, wherever it's drawn from.
pub fn advance(primary: &Font, ch: char, size: f32) -> f32 {
    if invisible(ch) {
        return 0.0;
    }
    match pick(primary, ch) {
        Pick::Font(f) => f.metrics(ch, size).advance_width,
        Pick::Emoji => (size * 1.2).round(),
        Pick::Missing => primary.metrics(ch, size).advance_width,
    }
}

pub fn width(primary: &Font, text: &str, size: f32) -> f32 {
    text.chars().map(|c| advance(primary, c, size)).sum()
}

/// A colour emoji as straight (not premultiplied) RGBA, `px` pixels square.
pub struct Emoji {
    pub size: u32,
    pub rgba: Vec<u8>,
}

pub fn emoji(ch: char, px: u32) -> Option<Arc<Emoji>> {
    static CACHE: OnceLock<Mutex<HashMap<(char, u32), Option<Arc<Emoji>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok()?.get(&(ch, px)) {
        return hit.clone();
    }
    let drawn = match fallback_for(ch) {
        Some(Loaded::Color(bytes, index)) => draw_emoji(bytes, *index, ch, px),
        _ => None,
    };
    let drawn = drawn.map(Arc::new);
    cache.lock().ok()?.insert((ch, px), drawn.clone());
    drawn
}

fn draw_emoji(bytes: &[u8], index: u32, ch: char, px: u32) -> Option<Emoji> {
    let face = ttf_parser::Face::parse(bytes, index).ok()?;
    let gid = face.glyph_index(ch)?;
    let img = face.glyph_raster_image(gid, u16::MAX)?;
    if img.format != ttf_parser::RasterImageFormat::PNG {
        return None;
    }
    let mut dec = png::Decoder::new(std::io::Cursor::new(img.data));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (sw, sh) = (info.width as usize, info.height as usize);
    let channels = info.line_size / sw;
    // Box-filter down to `px` square (the bitmaps are 136px; we draw ~16-24px).
    let px = px.max(1) as usize;
    let mut rgba = vec![0u8; px * px * 4];
    let scale = sw.max(sh) as f32 / px as f32;
    for y in 0..px {
        for x in 0..px {
            let (x0, y0) = ((x as f32 * scale) as usize, (y as f32 * scale) as usize);
            let (x1, y1) = ((((x + 1) as f32 * scale) as usize).min(sw).max(x0 + 1), (((y + 1) as f32 * scale) as usize).min(sh).max(y0 + 1));
            let (mut acc, mut n) = ([0f32; 4], 0f32);
            for sy in y0..y1.min(sh) {
                for sx in x0..x1.min(sw) {
                    let p = &buf[sy * info.line_size + sx * channels..];
                    let a = if channels == 4 { p[3] as f32 } else { 255.0 };
                    acc[0] += p[0] as f32 * a;
                    acc[1] += p[1] as f32 * a;
                    acc[2] += p[2] as f32 * a;
                    acc[3] += a;
                    n += 1.0;
                }
            }
            if acc[3] > 0.0 {
                let o = (y * px + x) * 4;
                rgba[o] = (acc[0] / acc[3]) as u8;
                rgba[o + 1] = (acc[1] / acc[3]) as u8;
                rgba[o + 2] = (acc[2] / acc[3]) as u8;
                rgba[o + 3] = (acc[3] / n) as u8;
            }
        }
    }
    Some(Emoji { size: px as u32, rgba })
}

/// Fonts for an egui app's fallback list: one per script our UI font lacks (CJK, Arabic,
/// Hebrew, Devanagari, Thai, symbols), as (name, file bytes, collection index). egui can't
/// draw colour emoji, so those are left out; its own font has black-and-white ones.
pub fn egui_fallbacks() -> Vec<(String, Vec<u8>, u32)> {
    let probes = ['あ', '中', '한', 'ب', 'ש', 'क', 'ก', '★', '→'];
    let mut out: Vec<(String, Vec<u8>, u32)> = Vec::new();
    let mut used = std::collections::HashSet::new();
    for ch in probes {
        let Some(Loaded::Outline(_)) = fallback_for(ch) else { continue };
        let Ok(reg) = registry().lock() else { continue };
        let key = reg.fonts.iter().find(|(_, f)| f.is_some_and(|f| std::ptr::eq(f, fallback_for_cached(&reg, ch)))).map(|(k, _)| k.clone());
        drop(reg);
        if let Some((path, index)) = key.filter(|k| used.insert(k.clone())) {
            if let Ok(bytes) = std::fs::read(&path) {
                out.push((format!("vide-fallback-{}", out.len()), bytes, index));
            }
        }
    }
    out
}

fn fallback_for_cached(reg: &Registry, ch: char) -> *const Loaded {
    reg.chars.get(&ch).copied().flatten().map_or(std::ptr::null(), |f| f as *const Loaded)
}

/// Give an egui app the fallback fonts: `vide_fonts::install_egui!(eframe::egui, cc.egui_ctx)`.
/// A macro because our apps use two egui versions (PowerSlop is on egui_term's).
#[macro_export]
macro_rules! install_egui {
    ($egui:path, $ctx:expr) => {{
        use $egui as egui_;
        let mut fonts = egui_::FontDefinitions::default();
        for (name, bytes, index) in $crate::egui_fallbacks() {
            let mut data = egui_::FontData::from_owned(bytes);
            data.index = index;
            fonts.font_data.insert(name.clone(), std::sync::Arc::new(data));
            for family in [egui_::FontFamily::Proportional, egui_::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push(name.clone());
            }
        }
        $ctx.set_fonts(fonts);
    }};
}

/// Where distros keep fonts: Arch's folders, then Debian, Ubuntu and Mint's `truetype/` ones.
const FONT_DIRS: [&str; 9] = [
    "/usr/share/fonts/noto",
    "/usr/share/fonts/truetype/noto",
    "/usr/share/fonts/opentype/noto",
    "/usr/share/fonts/TTF",
    "/usr/share/fonts/dejavu",
    "/usr/share/fonts/truetype/dejavu",
    "/usr/share/fonts/liberation",
    "/usr/share/fonts/truetype/liberation",
    "/usr/share/fonts/truetype/liberation2",
];

/// The first of `files` (e.g. `NotoSans-Regular.ttf`) found in any distro's font folders.
pub fn read_system_font(files: &[&str]) -> Option<Vec<u8>> {
    files.iter().find_map(|f| FONT_DIRS.iter().find_map(|d| std::fs::read(std::path::Path::new(d).join(f)).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui_font() -> Option<Font> {
        let bytes = std::fs::read("/usr/share/fonts/noto/NotoSans-Regular.ttf").ok()?;
        Font::from_bytes(bytes, FontSettings::default()).ok()
    }

    // These need the Noto fonts a VIDE-DE machine has; without them they check nothing.
    #[test]
    fn falls_back_for_other_scripts() {
        let Some(ui) = ui_font() else { return };
        assert!(matches!(pick(&ui, 'a'), Pick::Font(f) if std::ptr::eq(f, &ui)));
        if std::path::Path::new("/usr/share/fonts/noto/NotoNaskhArabic-Regular.ttf").exists() {
            assert!(matches!(pick(&ui, 'ب'), Pick::Font(f) if !std::ptr::eq(f, &ui)), "Arabic should come from a fallback");
        }
    }

    #[test]
    fn colour_emoji() {
        let Some(ui) = ui_font() else { return };
        if !std::path::Path::new("/usr/share/fonts/noto/NotoColorEmoji.ttf").exists() {
            return;
        }
        assert!(matches!(pick(&ui, '😀'), Pick::Emoji));
        let e = emoji('😀', 20).expect("emoji bitmap");
        assert_eq!(e.size, 20);
        assert!(e.rgba.chunks(4).any(|p| p[3] > 200 && p[0] > 200 && p[1] > 150), "should be mostly yellow");
        assert!(invisible('\u{fe0f}'));
        assert_eq!(advance(&ui, '\u{200d}', 14.0), 0.0);
    }
}
