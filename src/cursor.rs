//! Cursors from the user's installed xcursor theme (Breeze, Adwaita, ...),
//! same as every other desktop. The pixel arrow is only a fallback.

use std::collections::HashMap;

use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    input::pointer::CursorIcon,
    utils::{Logical, Point, Transform},
};

pub struct Cursors {
    theme: xcursor::CursorTheme,
    size: u32,
    cache: HashMap<CursorIcon, (MemoryRenderBuffer, Point<i32, Logical>)>,
}

impl Cursors {
    pub fn load() -> Self {
        let name = std::env::var("XCURSOR_THEME").ok().or_else(kde_cursor_theme).unwrap_or_else(|| "default".into());
        let size = std::env::var("XCURSOR_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(24);
        tracing::info!("cursor theme: {name} @ {size}px");
        Self { theme: xcursor::CursorTheme::load(&name), size, cache: HashMap::new() }
    }

    /// The picture and hotspot for `icon`, falling back to the default arrow.
    pub fn get(&mut self, icon: CursorIcon) -> &(MemoryRenderBuffer, Point<i32, Logical>) {
        if !self.cache.contains_key(&icon) {
            let loaded = std::iter::once(icon.name())
                .chain(icon.alt_names().iter().copied())
                .find_map(|n| self.load_named(n))
                .or_else(|| self.load_named("default"))
                .or_else(|| self.load_named("left_ptr"))
                .unwrap_or_else(|| (fallback_arrow(), Point::from((0, 0))));
            self.cache.insert(icon, loaded);
        }
        &self.cache[&icon]
    }

    fn load_named(&self, name: &str) -> Option<(MemoryRenderBuffer, Point<i32, Logical>)> {
        let path = self.theme.load_icon(name)?;
        let bytes = std::fs::read(path).ok()?;
        let images = xcursor::parser::parse_xcursor(&bytes)?;
        // Closest nominal size; first frame only (no animated cursors yet).
        let img = images.iter().min_by_key(|i| (i.size as i32 - self.size as i32).abs())?;
        let buf = MemoryRenderBuffer::from_slice(
            &img.pixels_rgba,
            Fourcc::Abgr8888,
            (img.width as i32, img.height as i32),
            1,
            Transform::Normal,
            None,
        );
        Some((buf, Point::from((img.xhot as i32, img.yhot as i32))))
    }
}

fn kde_cursor_theme() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let rc = std::fs::read_to_string(format!("{home}/.config/kcminputrc")).ok()?;
    rc.lines().find_map(|l| l.strip_prefix("cursorTheme=")).map(|s| s.trim().to_string())
}

// Last-resort arrow if no theme is installed at all.
const ARROW: &[&str] = &[
    "#         ",
    "##        ",
    "#.#       ",
    "#..#      ",
    "#...#     ",
    "#....#    ",
    "#.....#   ",
    "#......#  ",
    "#.......# ",
    "#....#####",
    "#.#..#    ",
    "## #..#   ",
    "#  #..#   ",
    "    ##    ",
];

fn fallback_arrow() -> MemoryRenderBuffer {
    let (w, h) = (ARROW[0].len(), ARROW.len());
    let mut px = vec![0u8; w * h * 4];
    for (y, row) in ARROW.iter().enumerate() {
        for (x, c) in row.bytes().enumerate() {
            let v = match c {
                b'#' => 0x00,
                b'.' => 0xff,
                _ => continue,
            };
            px[(y * w + x) * 4..][..4].copy_from_slice(&[v, v, v, 0xff]);
        }
    }
    MemoryRenderBuffer::from_slice(&px, Fourcc::Argb8888, (w as i32, h as i32), 1, Transform::Normal, None)
}
