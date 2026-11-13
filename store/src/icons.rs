use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

pub enum IconLookup<'a> {
    Ready(&'a TextureHandle),
    Loading,
    Missing,
}

enum IconEntry {
    Loading,
    Ready(TextureHandle),
    Missing,
}

struct LoadedIcon {
    key: String,
    image: Option<ColorImage>,
}

pub struct IconCache {
    entries: HashMap<String, IconEntry>,
    search_roots: Arc<Vec<PathBuf>>,
    appstream_icon_dirs: Arc<Vec<PathBuf>>,
    cache_dir: Arc<PathBuf>,
    tx: Sender<LoadedIcon>,
    rx: Receiver<LoadedIcon>,
}

impl Default for IconCache {
    fn default() -> Self {
        Self::new()
    }
}

impl IconCache {
    pub fn new() -> Self {
        let mut roots = Vec::new();
        let mut appstream_dirs = Vec::new();
        let home = std::env::var("HOME").ok();
        if let Some(ref h) = home {
            roots.push(PathBuf::from(h).join(".local/share/flatpak/exports/share/icons"));
            roots.push(PathBuf::from(h).join(".local/share/icons"));
            collect_appstream_icon_dirs(
                &PathBuf::from(h).join(".local/share/flatpak/appstream"),
                &mut appstream_dirs,
            );
        }
        roots.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
        roots.push(PathBuf::from("/usr/share/icons"));
        roots.push(PathBuf::from("/usr/share/pixmaps"));
        collect_appstream_icon_dirs(Path::new("/var/lib/flatpak/appstream"), &mut appstream_dirs);

        let cache_dir = home
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".cache/window-shopping/icons");

        let (tx, rx) = mpsc::channel();
        Self {
            entries: HashMap::new(),
            search_roots: Arc::new(roots),
            appstream_icon_dirs: Arc::new(appstream_dirs),
            cache_dir: Arc::new(cache_dir),
            tx,
            rx,
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let mut any_still_loading = false;
        while let Ok(loaded) = self.rx.try_recv() {
            if let Some(img) = loaded.image {
                let tex = ctx.load_texture(
                    format!("store-icon:{}", loaded.key),
                    img,
                    TextureOptions::LINEAR,
                );
                self.entries.insert(loaded.key, IconEntry::Ready(tex));
            } else {
                self.entries.insert(loaded.key, IconEntry::Missing);
            }
        }
        for entry in self.entries.values() {
            if matches!(entry, IconEntry::Loading) {
                any_still_loading = true;
                break;
            }
        }
        any_still_loading
    }

    pub fn get(&mut self, ctx: &egui::Context, icon_name: &str, pkg_id: &str) -> IconLookup<'_> {
        let key = if !icon_name.is_empty() {
            icon_name.to_string()
        } else {
            pkg_id.to_string()
        };

        if !self.entries.contains_key(&key) {
            self.entries.insert(key.clone(), IconEntry::Loading);
            let roots = Arc::clone(&self.search_roots);
            let appstream = Arc::clone(&self.appstream_icon_dirs);
            let cache_dir = Arc::clone(&self.cache_dir);
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            let icon_owned = key.clone();
            let pkg_owned = pkg_id.to_string();
            std::thread::spawn(move || {
                let image = find_and_decode(&roots, &appstream, &cache_dir, &icon_owned, &pkg_owned);
                let _ = tx.send(LoadedIcon {
                    key: icon_owned,
                    image,
                });
                ctx.request_repaint();
            });
        }

        match self.entries.get(&key) {
            Some(IconEntry::Ready(tex)) => IconLookup::Ready(tex),
            Some(IconEntry::Loading) => IconLookup::Loading,
            _ => IconLookup::Missing,
        }
    }
}

fn collect_appstream_icon_dirs(base: &Path, out: &mut Vec<PathBuf>) {
    let Ok(remotes) = std::fs::read_dir(base) else { return };
    for remote in remotes.flatten() {
        let Ok(arches) = std::fs::read_dir(remote.path()) else { continue };
        for arch in arches.flatten() {
            let active = arch.path().join("active/icons");
            if active.is_dir() {
                out.push(active);
                continue;
            }
            let Ok(revs) = std::fs::read_dir(arch.path()) else { continue };
            for rev in revs.flatten() {
                let icons = rev.path().join("icons");
                if icons.is_dir() {
                    out.push(icons);
                }
            }
        }
    }
}

fn find_and_decode(
    roots: &[PathBuf],
    appstream: &[PathBuf],
    cache_dir: &Path,
    icon_name: &str,
    pkg_id: &str,
) -> Option<ColorImage> {
    let mut candidates = vec![icon_name.to_string()];
    if pkg_id != icon_name && !pkg_id.is_empty() {
        candidates.push(pkg_id.to_string());
    }
    if let Some(last) = pkg_id.rsplit('.').next()
        && !last.is_empty()
    {
        candidates.push(last.to_string());
        candidates.push(last.to_lowercase());
    }

    // 1. Local Flatpak AppStream icon cache (has uninstalled Flathub apps!)
    for cand in &candidates {
        for dir in appstream {
            for size in ["128x128", "64x64"] {
                let p = dir.join(size).join(format!("{cand}.png"));
                if p.is_file()
                    && let Some(img) = load_rgba(&p, 96)
                {
                    return Some(img);
                }
            }
        }
    }

    // 2. Installed icon themes & pixmaps
    for cand in &candidates {
        if let Some(path) = find_icon_path(roots, cand)
            && let Some(img) = load_rgba(&path, 96)
        {
            return Some(img);
        }
    }

    // 3. On-disk cache or Flathub CDN fetch for reverse-DNS app IDs
    let flatpak_id = candidates.iter().find(|c| c.matches('.').count() >= 2)?;
    let cached = cache_dir.join(format!("{flatpak_id}.png"));
    if cached.is_file()
        && let Some(img) = load_rgba(&cached, 96)
    {
        return Some(img);
    }

    let _ = std::fs::create_dir_all(cache_dir);
    let url = format!("https://dl.flathub.org/repo/appstream/x86_64/icons/128x128/{flatpak_id}.png");
    let status = Command::new("curl")
        .args(["-fsSL", "--max-time", "5", "-o"])
        .arg(&cached)
        .arg(&url)
        .status()
        .ok()?;
    if status.success() && cached.is_file() {
        return load_rgba(&cached, 96);
    }
    let _ = std::fs::remove_file(&cached);
    None
}

fn find_icon_path(roots: &[PathBuf], name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let direct = Path::new(name);
    if direct.is_absolute() && direct.is_file() {
        return Some(direct.to_path_buf());
    }

    let exts = ["png", "svg"];
    let sizes = ["128x128", "256x256", "96x96", "64x64", "48x48", "scalable"];
    let themes = ["hicolor", "breeze", "Adwaita", "Papirus"];

    for root in roots {
        for ext in exts {
            let p = root.join(format!("{name}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
        for theme in themes {
            for size in sizes {
                for ext in exts {
                    let p = root
                        .join(theme)
                        .join(size)
                        .join("apps")
                        .join(format!("{name}.{ext}"));
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

fn load_rgba(path: &Path, target_px: u32) -> Option<ColorImage> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == "png" {
        let bytes = std::fs::read(path).ok()?;
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?;
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        return Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()));
    }
    if ext == "svg" {
        let bytes = std::fs::read(path).ok()?;
        let tree = resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default()).ok()?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(target_px, target_px)?;
        let tree_size = tree.size();
        let sx = target_px as f32 / tree_size.width().max(1.0);
        let sy = target_px as f32 / tree_size.height().max(1.0);
        let scale = sx.min(sy);
        let tx = (target_px as f32 - tree_size.width() * scale) * 0.5;
        let ty = (target_px as f32 - tree_size.height() * scale) * 0.5;
        let transform =
            resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(tx, ty);
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        return Some(ColorImage::from_rgba_unmultiplied(
            [target_px as usize, target_px as usize],
            pixmap.data(),
        ));
    }
    None
}
