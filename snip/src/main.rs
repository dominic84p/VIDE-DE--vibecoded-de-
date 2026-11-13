//! Snip & Sketchy: VIDE-DE's screenshot markup app, modelled on Windoze 10's
//! Snip & Sketch. The compositor captures (only it can see the screen) and opens
//! the snip here; drawing, cropping, saving and copying happen in this app.
//!
//! Toolbar like the original: New on the left; Ballpoint pen, Pencil, Highlighter,
//! Eraser, Crop in the middle; Zoom, Save, Copy on the right. Ctrl+Z/Y, Ctrl+S, Ctrl+C.

use std::{io::Write, path::PathBuf};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Vec2};

const BAR: Color32 = Color32::from_rgb(0xf3, 0xf3, 0xf3);
const CANVAS_BG: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
const ACTIVE: Color32 = Color32::from_rgb(0xd6, 0xd6, 0xd6);
const TEXT: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
const DIM: Color32 = Color32::from_rgb(0x6d, 0x6d, 0x6d);
const ACCENT: Color32 = Color32::from_rgb(0x00, 0x78, 0xd7);

/// Pen colours offered, like the original's flyout.
const PALETTE: [Color32; 8] = [
    Color32::from_rgb(0x00, 0x00, 0x00),
    Color32::from_rgb(0xff, 0xff, 0xff),
    Color32::from_rgb(0xe8, 0x11, 0x23),
    Color32::from_rgb(0x00, 0x78, 0xd7),
    Color32::from_rgb(0x10, 0x7c, 0x10),
    Color32::from_rgb(0xff, 0xb9, 0x00),
    Color32::from_rgb(0x88, 0x17, 0x98),
    Color32::from_rgb(0xf7, 0x63, 0x0c),
];

fn main() -> eframe::Result {
    let file = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Snip & Sketchy")
            .with_app_id("snip-and-sketchy")
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([560.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Snip & Sketchy",
        options,
        Box::new(move |cc| {
            vide_fonts::install_egui!(eframe::egui, cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(App::new(&cc.egui_ctx, file)))
        }),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Ballpoint,
    Pencil,
    Highlighter,
    Eraser,
    Crop,
}

#[derive(Clone)]
struct StrokeData {
    /// Points in image pixel coordinates.
    points: Vec<Pos2>,
    color: Color32,
    width: f32,
}

/// Straight RGBA image plus everything drawn on it.
#[derive(Clone)]
struct Doc {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
    strokes: Vec<StrokeData>,
}

struct App {
    doc: Option<Doc>,
    texture: Option<TextureHandle>,
    undo: Vec<Doc>,
    redo: Vec<Doc>,
    tool: Tool,
    colors: [Color32; 3],
    widths: [f32; 3],
    drawing: Option<StrokeData>,
    crop: Option<(Pos2, Pos2)>,
    zoom: Option<f32>,
    flyout: Option<Tool>,
    status: Option<String>,
}

fn load_png(path: &std::path::Path) -> Option<Doc> {
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
    Some(Doc { w: info.width as usize, h: info.height as usize, rgba, strokes: Vec::new() })
}

fn ipc(cmd: &str) -> bool {
    let Some(path) = std::env::var_os("VIDE_IPC") else { return false };
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(mut s) => s.write_all(format!("{cmd}\n").as_bytes()).is_ok(),
        Err(_) => false,
    }
}

impl App {
    fn new(ctx: &egui::Context, file: Option<PathBuf>) -> Self {
        let mut app = Self {
            doc: None,
            texture: None,
            undo: Vec::new(),
            redo: Vec::new(),
            tool: Tool::Ballpoint,
            colors: [PALETTE[0], Color32::from_rgb(0x55, 0x55, 0x55), Color32::from_rgba_unmultiplied(0xff, 0xf1, 0x00, 0x80)],
            widths: [4.0, 2.0, 18.0],
            drawing: None,
            crop: None,
            zoom: None,
            flyout: None,
            status: None,
        };
        if let Some(f) = file {
            match load_png(&f) {
                Some(doc) => app.set_doc(ctx, doc),
                None => app.status = Some(format!("Couldn't open {}", f.display())),
            }
        }
        app
    }

    fn set_doc(&mut self, ctx: &egui::Context, doc: Doc) {
        let img = egui::ColorImage::from_rgba_unmultiplied([doc.w, doc.h], &doc.rgba);
        self.texture = Some(ctx.load_texture("snip", img, egui::TextureOptions::LINEAR));
        self.doc = Some(doc);
    }

    fn checkpoint(&mut self) {
        if let Some(d) = &self.doc {
            self.undo.push(d.clone());
            self.redo.clear();
        }
    }

    fn undo(&mut self, ctx: &egui::Context) {
        if let (Some(prev), Some(cur)) = (self.undo.pop(), self.doc.take()) {
            self.redo.push(cur);
            self.set_doc(ctx, prev);
        }
    }

    fn redo(&mut self, ctx: &egui::Context) {
        if let (Some(next), Some(cur)) = (self.redo.pop(), self.doc.take()) {
            self.undo.push(cur);
            self.set_doc(ctx, next);
        }
    }

    fn tool_index(&self) -> usize {
        match self.tool {
            Tool::Pencil => 1,
            Tool::Highlighter => 2,
            _ => 0,
        }
    }

    /// The image with its markup burned in, as straight RGBA.
    fn flatten(&self) -> Option<(usize, usize, Vec<u8>)> {
        let d = self.doc.as_ref()?;
        let mut px = d.rgba.clone();
        for s in &d.strokes {
            raster_stroke(&mut px, d.w, d.h, s);
        }
        Some((d.w, d.h, px))
    }

    fn encode(&self) -> Option<Vec<u8>> {
        let (w, h, px) = self.flatten()?;
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().ok()?.write_image_data(&px).ok()?;
        Some(out)
    }

    fn save(&mut self) {
        let Some(bytes) = self.encode() else { return };
        let dir = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Pictures/Screenshots");
        let path = dir.join(chrono::Local::now().format("Annotation_%Y-%m-%d_%H-%M-%S.png").to_string());
        self.status = Some(match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, bytes)) {
            Ok(()) => format!("Saved to {}", path.display()),
            Err(e) => format!("Couldn't save: {e}"),
        });
    }

    fn copy(&mut self) {
        let Some(bytes) = self.encode() else { return };
        let tmp = std::env::temp_dir().join(format!("snip-and-sketchy-{}.png", std::process::id()));
        if std::fs::write(&tmp, bytes).is_err() {
            return;
        }
        // The compositor owns the clipboard; ask it, else fall back to wl-copy.
        let ok = ipc(&format!("copy-png {}", tmp.display()))
            || std::process::Command::new("sh").arg("-c").arg(format!("wl-copy -t image/png < '{}'", tmp.display())).status().is_ok_and(|s| s.success());
        self.status = Some(if ok { "Copied to clipboard".into() } else { "Couldn't copy".into() });
    }

    fn new_snip(&mut self, ctx: &egui::Context) {
        if ipc("snip") {
            // The compositor opens a fresh window with the new snip.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.status = Some("New snips need VIDE-DE (Win+Shift+S)".into());
        }
    }

    fn apply_crop(&mut self, ctx: &egui::Context, a: Pos2, b: Pos2) {
        let Some((w, h, px)) = self.flatten() else { return };
        let (x0, y0) = (a.x.min(b.x).max(0.0) as usize, a.y.min(b.y).max(0.0) as usize);
        let (x1, y1) = ((a.x.max(b.x) as usize).min(w), (a.y.max(b.y) as usize).min(h));
        if x1 <= x0 + 2 || y1 <= y0 + 2 {
            return;
        }
        self.checkpoint();
        let (nw, nh) = (x1 - x0, y1 - y0);
        let mut out = Vec::with_capacity(nw * nh * 4);
        for y in y0..y1 {
            out.extend_from_slice(&px[(y * w + x0) * 4..(y * w + x1) * 4]);
        }
        self.set_doc(ctx, Doc { w: nw, h: nh, rgba: out, strokes: Vec::new() });
    }
}

/// Burn one stroke into straight RGBA: a disc stamped along each segment.
fn raster_stroke(px: &mut [u8], w: usize, h: usize, s: &StrokeData) {
    let r = s.width / 2.0;
    let [cr, cg, cb, ca] = s.color.to_srgba_unmultiplied();
    let alpha = ca as f32 / 255.0;
    // Coverage per pixel for this stroke, so overlapping stamps don't stack up
    // (a highlighter stays evenly translucent).
    let mut cov = vec![0f32; w * h];
    let pts = if s.points.len() == 1 { vec![s.points[0], s.points[0]] } else { s.points.clone() };
    for seg in pts.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let (minx, maxx) = ((a.x.min(b.x) - r - 1.0).max(0.0) as usize, ((a.x.max(b.x) + r + 1.0) as usize).min(w.saturating_sub(1)));
        let (miny, maxy) = ((a.y.min(b.y) - r - 1.0).max(0.0) as usize, ((a.y.max(b.y) + r + 1.0) as usize).min(h.saturating_sub(1)));
        let d = b - a;
        let len2 = d.length_sq().max(1e-6);
        for y in miny..=maxy {
            for x in minx..=maxx {
                let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                let t = ((p - a).dot(d) / len2).clamp(0.0, 1.0);
                let dist = (p - (a + d * t)).length();
                let c = (r + 0.5 - dist).clamp(0.0, 1.0);
                let i = y * w + x;
                cov[i] = cov[i].max(c);
            }
        }
    }
    for (i, &c) in cov.iter().enumerate() {
        if c <= 0.0 {
            continue;
        }
        let a = c * alpha;
        let o = i * 4;
        px[o] = (cr as f32 * a + px[o] as f32 * (1.0 - a)) as u8;
        px[o + 1] = (cg as f32 * a + px[o + 1] as f32 * (1.0 - a)) as u8;
        px[o + 2] = (cb as f32 * a + px[o + 2] as f32 * (1.0 - a)) as u8;
        px[o + 3] = 255;
    }
}

fn icon(name: &str) -> egui::Image<'static> {
    egui::Image::new(format!("file:///usr/share/icons/breeze/actions/22/{name}.svg")).fit_to_exact_size(Vec2::splat(22.0))
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut v = egui::Visuals::light();
        v.panel_fill = BAR;
        ctx.set_visuals(v);

        // Shortcuts.
        let pressed = |m: egui::Modifiers, k: egui::Key| ctx.input_mut(|i| i.consume_key(m, k));
        if pressed(egui::Modifiers::CTRL, egui::Key::Z) {
            self.undo(&ctx);
        }
        if pressed(egui::Modifiers::CTRL, egui::Key::Y) {
            self.redo(&ctx);
        }
        if pressed(egui::Modifiers::CTRL, egui::Key::S) {
            self.save();
        }
        if pressed(egui::Modifiers::CTRL, egui::Key::N) {
            self.new_snip(&ctx);
        }
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
            self.copy();
        }

        // Toolbar.
        egui::Panel::top("toolbar").frame(egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(8, 6))).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.add(egui::Button::image_and_text(icon("document-new"), RichText::new("New").color(TEXT)).fill(ACCENT.gamma_multiply(0.0))).on_hover_text("New snip (Ctrl+N)").clicked() {
                    self.new_snip(&ctx);
                }
                ui.add_space(24.0);
                // Drawing tools, centred like the original.
                let tools = [
                    (Tool::Ballpoint, "draw-calligraphic", "Ballpoint pen"),
                    (Tool::Pencil, "draw-freehand", "Pencil"),
                    (Tool::Highlighter, "draw-highlight", "Highlighter"),
                    (Tool::Eraser, "draw-eraser", "Eraser"),
                    (Tool::Crop, "transform-crop", "Image crop"),
                ];
                for (t, ic, tip) in tools {
                    let active = self.tool == t;
                    let mut btn = egui::Button::image(icon(ic)).min_size(Vec2::new(40.0, 34.0));
                    btn = btn.fill(if active { ACTIVE } else { BAR });
                    let r = ui.add(btn).on_hover_text(tip);
                    if active && matches!(t, Tool::Ballpoint | Tool::Pencil | Tool::Highlighter) {
                        // Colour bar under the active pen, like Windoze.
                        let c = self.colors[self.tool_index()];
                        ui.painter().rect_filled(Rect::from_min_size(r.rect.left_bottom() - Vec2::new(-6.0, 4.0), Vec2::new(r.rect.width() - 12.0, 3.0)), 1.0, c.to_opaque());
                    }
                    if r.clicked() {
                        // A second click on the active pen opens its colour/size flyout.
                        self.flyout = if active && matches!(t, Tool::Ballpoint | Tool::Pencil | Tool::Highlighter) && self.flyout.is_none() { Some(t) } else { None };
                        self.tool = t;
                        self.crop = None;
                    }
                }
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::image(icon("edit-undo"))).on_hover_text("Undo (Ctrl+Z)").clicked() {
                    self.undo(&ctx);
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::image(icon("edit-redo"))).on_hover_text("Redo (Ctrl+Y)").clicked() {
                    self.redo(&ctx);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(self.doc.is_some(), egui::Button::image(icon("edit-copy"))).on_hover_text("Copy (Ctrl+C)").clicked() {
                        self.copy();
                    }
                    if ui.add_enabled(self.doc.is_some(), egui::Button::image(icon("document-save-as"))).on_hover_text("Save as (Ctrl+S)").clicked() {
                        self.save();
                    }
                    let zoom_tip = if self.zoom.is_some() { "Fit to window" } else { "Actual size" };
                    if ui.add_enabled(self.doc.is_some(), egui::Button::image(icon(if self.zoom.is_some() { "zoom-fit-best" } else { "zoom-in" }))).on_hover_text(zoom_tip).clicked() {
                        self.zoom = if self.zoom.is_some() { None } else { Some(1.0) };
                    }
                });
            });
        });

        // Colour/size flyout for the active pen.
        if let Some(t) = self.flyout {
            let i = match t {
                Tool::Pencil => 1,
                Tool::Highlighter => 2,
                _ => 0,
            };
            egui::Window::new("pen").title_bar(false).resizable(false).fixed_pos(Pos2::new(140.0, 48.0)).show(&ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(200.0);
                    for c in PALETTE {
                        let c = if t == Tool::Highlighter { Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 0x80) } else { c };
                        let (rect, r) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                        ui.painter().circle_filled(rect.center(), 11.0, c.to_opaque());
                        if self.colors[i].to_opaque() == c.to_opaque() {
                            ui.painter().circle_stroke(rect.center(), 13.0, Stroke::new(2.0, ACCENT));
                        }
                        if r.clicked() {
                            self.colors[i] = c;
                        }
                    }
                });
                ui.label(RichText::new("Size").color(DIM));
                ui.add(egui::Slider::new(&mut self.widths[i], 1.0..=40.0).show_value(false));
            });
        }

        egui::Panel::bottom("status").frame(egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(10, 3))).show(ui, |ui| {
            ui.label(RichText::new(self.status.clone().unwrap_or_default()).color(DIM).size(12.0));
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(CANVAS_BG)).show(ui, |ui| {
            let Some(tex) = self.texture.clone() else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Snip & Sketchy\n\nPress New (or Win+Shift+S) to snip any part of your screen.\nMark it up, then save or copy it.").color(DIM).size(16.0));
                });
                return;
            };
            let doc_size = Vec2::new(self.doc.as_ref().unwrap().w as f32, self.doc.as_ref().unwrap().h as f32);
            let avail = ui.available_rect_before_wrap().shrink(16.0);
            let scale = self.zoom.unwrap_or_else(|| (avail.width() / doc_size.x).min(avail.height() / doc_size.y).min(1.0));
            egui::ScrollArea::both().show(ui, |ui| {
                let size = doc_size * scale;
                let pad = ((ui.available_size() - size) / 2.0).max(Vec2::splat(16.0));
                ui.add_space(pad.y);
                ui.horizontal(|ui| {
                    ui.add_space(pad.x);
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
                    let p = ui.painter_at(rect);
                    p.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                    let to_img = |pos: Pos2| Pos2::new((pos.x - rect.left()) / scale, (pos.y - rect.top()) / scale);
                    let to_scr = |pt: Pos2| Pos2::new(rect.left() + pt.x * scale, rect.top() + pt.y * scale);

                    // Existing strokes, then the one in progress.
                    let doc = self.doc.as_ref().unwrap();
                    for s in doc.strokes.iter().chain(self.drawing.iter()) {
                        let pts: Vec<Pos2> = s.points.iter().map(|&q| to_scr(q)).collect();
                        if pts.len() == 1 {
                            p.circle_filled(pts[0], s.width * scale / 2.0, s.color);
                        } else {
                            p.add(egui::Shape::line(pts, Stroke::new(s.width * scale, s.color)));
                        }
                    }

                    let pointer = resp.interact_pointer_pos();
                    match self.tool {
                        Tool::Ballpoint | Tool::Pencil | Tool::Highlighter => {
                            if resp.drag_started() || (resp.clicked() && self.drawing.is_none()) {
                                if let Some(pos) = pointer {
                                    let i = self.tool_index();
                                    self.drawing = Some(StrokeData { points: vec![to_img(pos)], color: self.colors[i], width: self.widths[i] });
                                }
                            }
                            if let (Some(s), Some(pos)) = (self.drawing.as_mut(), pointer) {
                                let q = to_img(pos);
                                if s.points.last().is_none_or(|l| l.distance(q) > 0.75) {
                                    s.points.push(q);
                                }
                            }
                            if resp.drag_stopped() || resp.clicked() {
                                if let Some(s) = self.drawing.take() {
                                    self.checkpoint();
                                    self.doc.as_mut().unwrap().strokes.push(s);
                                }
                            }
                        }
                        Tool::Eraser => {
                            // Stroke eraser: touching a stroke removes the whole stroke.
                            if let Some(pos) = pointer.filter(|_| resp.dragged() || resp.clicked()) {
                                let q = to_img(pos);
                                let hit = self.doc.as_ref().unwrap().strokes.iter().position(|s| s.points.iter().any(|pt| pt.distance(q) < s.width / 2.0 + 6.0 / scale));
                                if let Some(i) = hit {
                                    self.checkpoint();
                                    self.doc.as_mut().unwrap().strokes.remove(i);
                                }
                            }
                        }
                        Tool::Crop => {
                            if resp.drag_started() {
                                if let Some(pos) = pointer {
                                    self.crop = Some((to_img(pos), to_img(pos)));
                                }
                            }
                            if let (Some(c), Some(pos)) = (self.crop.as_mut(), pointer) {
                                c.1 = to_img(pos);
                            }
                            if let Some((a, b)) = self.crop {
                                let r = Rect::from_two_pos(to_scr(a), to_scr(b));
                                p.rect_filled(rect, 0.0, Color32::from_black_alpha(90));
                                // Show the kept area undimmed: redraw that part of the image on top.
                                let uv = |q: Pos2| Pos2::new((q.x - rect.left()) / size.x, (q.y - rect.top()) / size.y);
                                p.image(tex.id(), r, Rect::from_min_max(uv(r.min), uv(r.max)), Color32::WHITE);
                                p.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
                            }
                            if resp.drag_stopped() {
                                if let Some((a, b)) = self.crop.take() {
                                    self.apply_crop(&ctx, a, b);
                                }
                            }
                        }
                    }
                });
            });
        });
    }
}
