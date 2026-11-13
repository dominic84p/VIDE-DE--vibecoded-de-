//! Icons and screenshots: fetched on a small worker pool, cached on disk, uploaded to the
//! GPU as they arrive. Also works out each picture's accent colour for tile backgrounds.

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
};

use eframe::egui::{self, ColorImage, TextureHandle, TextureId, TextureOptions};

const WORKERS: usize = 6;

#[derive(Clone, Copy)]
pub enum Img {
    Ready { id: TextureId, accent: [u8; 3], size: [usize; 2] },
    Loading,
    Missing,
}

struct Request {
    key: String,
    source: String,
    max_side: u32,
}

struct Done {
    key: String,
    image: Option<(ColorImage, [u8; 3])>,
}

enum Entry {
    Loading,
    Ready { tex: TextureHandle, accent: [u8; 3], size: [usize; 2] },
    Missing,
}

pub struct Images {
    entries: HashMap<String, Entry>,
    requests: Sender<Request>,
    done: Receiver<Done>,
}

fn cache_path(source: &str) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".cache/window-shopping/v2").join(format!("{:016x}", h.finish()))
}

fn load_bytes(source: &str) -> Option<Vec<u8>> {
    if source.starts_with('/') {
        return std::fs::read(source).ok();
    }
    let path = cache_path(source);
    if let Ok(bytes) = std::fs::read(&path) {
        return Some(bytes);
    }
    let bytes = crate::api::fetch_bytes(source).ok()?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, &bytes);
    Some(bytes)
}

fn decode(bytes: &[u8], max_side: u32) -> Option<(ColorImage, [u8; 3])> {
    let mut img = image::load_from_memory(bytes).ok()?;
    if img.width().max(img.height()) > max_side {
        img = img.resize(max_side, max_side, image::imageops::FilterType::Triangle);
    }
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for p in rgba.pixels().filter(|p| p.0[3] > 128) {
        for k in 0..3 {
            sum[k] += p.0[k] as u64;
        }
        n += 1;
    }
    let accent = if n == 0 { [0x00, 0x78, 0xd7] } else { [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8] };
    Some((ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw()), accent))
}

impl Images {
    pub fn new(ctx: &egui::Context) -> Self {
        let (requests, request_rx) = mpsc::channel::<Request>();
        let (done_tx, done) = mpsc::channel::<Done>();
        let request_rx = Arc::new(Mutex::new(request_rx));
        for _ in 0..WORKERS {
            let (rx, tx, ctx) = (request_rx.clone(), done_tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                loop {
                    let Ok(req) = rx.lock().map(|r| r.recv()).unwrap_or_else(|_| Err(mpsc::RecvError)) else { return };
                    let image = load_bytes(&req.source).and_then(|b| decode(&b, req.max_side));
                    if tx.send(Done { key: req.key, image }).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        }
        Self { entries: HashMap::new(), requests, done }
    }

    /// Upload whatever finished loading. Returns true while pictures are still on the way.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        while let Ok(d) = self.done.try_recv() {
            let entry = match d.image {
                Some((img, accent)) => {
                    let size = img.size;
                    Entry::Ready { tex: ctx.load_texture(format!("img:{}", d.key), img, TextureOptions::LINEAR), accent, size }
                }
                None => Entry::Missing,
            };
            self.entries.insert(d.key, entry);
        }
        self.entries.values().any(|e| matches!(e, Entry::Loading))
    }

    /// A picture by URL (or an absolute file path), downscaled so its longest side is at most `max_side`.
    pub fn get(&mut self, source: &str, max_side: u32) -> Img {
        if source.is_empty() {
            return Img::Missing;
        }
        let key = format!("{source}@{max_side}");
        if !self.entries.contains_key(&key) {
            self.entries.insert(key.clone(), Entry::Loading);
            let _ = self.requests.send(Request { key: key.clone(), source: source.to_string(), max_side });
        }
        match &self.entries[&key] {
            Entry::Ready { tex, accent, size } => Img::Ready { id: tex.id(), accent: *accent, size: *size },
            Entry::Loading => Img::Loading,
            Entry::Missing => Img::Missing,
        }
    }
}
