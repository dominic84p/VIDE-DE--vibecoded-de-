use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

#[derive(Default)]
pub struct IconCache {
    textures: HashMap<String, Option<TextureHandle>>,
    search_roots: Vec<PathBuf>,
}

impl IconCache {
    pub fn new() -> Self {
        let mut roots = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            roots.push(PathBuf::from(&home).join(".local/share/flatpak/exports/share/icons"));
            roots.push(PathBuf::from(&home).join(".local/share/icons"));
        }
        roots.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
        roots.push(PathBuf::from("/usr/share/icons"));
        roots.push(PathBuf::from("/usr/share/pixmaps"));
        Self {
            textures: HashMap::new(),
            search_roots: roots,
        }
    }

    pub fn get(&mut self, ctx: &egui::Context, icon_name: &str, pkg_id: &str) -> Option<&TextureHandle> {
        let key = if !icon_name.is_empty() {
            icon_name.to_string()
        } else {
            pkg_id.to_string()
        };
        if !self.textures.contains_key(&key) {
            let tex = self.load_texture(ctx, &key, pkg_id);
            self.textures.insert(key.clone(), tex);
        }
        self.textures.get(&key).and_then(|o| o.as_ref())
    }

    fn load_texture(&self, ctx: &egui::Context, icon_name: &str, pkg_id: &str) -> Option<TextureHandle> {
        let path = self.find_icon_path(icon_name).or_else(|| {
            if icon_name != pkg_id {
                self.find_icon_path(pkg_id)
            } else {
                None
            }
        })?;

        let img = load_rgba(&path, 96)?;
        Some(ctx.load_texture(
            format!("store-icon:{icon_name}"),
            img,
            TextureOptions::LINEAR,
        ))
    }

    fn find_icon_path(&self, name: &str) -> Option<PathBuf> {
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

        for root in &self.search_roots {
            // Flat pixmaps check
            for ext in exts {
                let p = root.join(format!("{name}.{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
            for theme in themes {
                for size in sizes {
                    for ext in exts {
                        let p = root.join(theme).join(size).join("apps").join(format!("{name}.{ext}"));
                        if p.is_file() {
                            return Some(p);
                        }
                    }
                }
            }
        }
        None
    }
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
        let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(tx, ty);
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        return Some(ColorImage::from_rgba_unmultiplied(
            [target_px as usize, target_px as usize],
            pixmap.data(),
        ));
    }
    None
}
