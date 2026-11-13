//! Window Shopping: VIDE-DE's app store, in the spirit of the Microslop Store. Browse
//! Flathub's apps, read about them, install them with one click. Real apps only, and no
//! package names or terminal output anywhere in the window.

mod api;
mod flatpak;
mod images;

use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{self, Receiver, Sender},
};

use api::{App, Detail, FEATURED_ID};
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};
use flatpak::{Installed, Kind, TaskEvent};
use images::{Images, Img};

const HEADER_BG: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
const CANVAS: Color32 = Color32::WHITE;
const BORDER: Color32 = Color32::from_rgb(0xe1, 0xe1, 0xe1);
const BLUE: Color32 = Color32::from_rgb(0x00, 0x78, 0xd7);
const BLUE_DARK: Color32 = Color32::from_rgb(0x00, 0x5a, 0x9e);
const GREEN: Color32 = Color32::from_rgb(0x10, 0x7c, 0x10);
const RED: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x1c);
const TEXT: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
const MUTED: Color32 = Color32::from_rgb(0x60, 0x60, 0x60);
const HERO_BG: Color32 = Color32::from_rgb(0x14, 0x1b, 0x26);

const PAGE_SIZE: u32 = 48;
const TILE_GAP: f32 = 12.0;
const TILE_MIN_W: f32 = 168.0;
const TILE_H: f32 = 214.0;

#[derive(Clone, PartialEq, Debug)]
enum View {
    Home,
    Apps,
    Games,
    Library,
    Search(String),
    App(String),
}

enum Event {
    Section { key: String, page: u32, apps: Result<Vec<App>, ()> },
    Search { query: String, apps: Result<Vec<App>, ()> },
    Detail { id: String, detail: Result<Detail, ()> },
    Library { installed: Vec<Installed>, updates: HashSet<String> },
    Task(TaskEvent),
}

#[derive(Default)]
struct Section {
    apps: Vec<App>,
    page: u32,
    more: bool,
    loading: bool,
    failed: bool,
}

struct Task {
    kind: Kind,
    id: String,
    name: String,
    percent: Option<f32>,
    phase: &'static str,
}

struct Notice {
    ok: bool,
    text: String,
    /// An app the notice offers to open.
    open: Option<String>,
}

struct StoreApp {
    images: Images,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    ctx: egui::Context,

    view: View,
    history: Vec<View>,
    scroll_top: bool,

    sections: HashMap<String, Section>,
    apps_category: String,
    known: HashMap<String, App>,
    details: HashMap<String, Detail>,
    detail_loading: HashSet<String>,
    failed_details: HashSet<String>,

    search_text: String,
    search: Option<(String, Option<Result<Vec<App>, ()>>)>,

    installed: HashMap<String, Installed>,
    updates: HashSet<String>,
    icon_paths: HashMap<String, String>,
    library_loading: bool,

    task: Option<Task>,
    notice: Option<Notice>,
    styled: bool,
}

// ------------------------------------------------------------------ small helpers

fn lerp(a: [u8; 3], b: [u8; 3], t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]))
}

fn luminance(c: [u8; 3]) -> f32 {
    (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0
}

/// Accent darkened until white text reads on it.
fn deep(accent: [u8; 3]) -> Color32 {
    let t = ((luminance(accent) - 0.18) * 1.8).clamp(0.2, 0.8);
    lerp(accent, [10, 14, 20], t)
}

/// A steady colour for apps with no icon: derived from the name.
fn name_colour(name: &str) -> [u8; 3] {
    let h = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    [(60 + h % 120) as u8, (60 + (h >> 8) % 120) as u8, (90 + (h >> 16) % 120) as u8]
}

fn paint_text(p: &egui::Painter, pos: Pos2, text: &str, size: f32, colour: Color32, max_w: f32, rows: usize) -> Rect {
    let mut job = egui::text::LayoutJob::simple(text.to_string(), FontId::proportional(size), colour, max_w);
    job.wrap.max_rows = rows;
    job.wrap.overflow_character = Some('…');
    let galley = p.layout_job(job);
    let rect = Rect::from_min_size(pos, galley.size());
    p.galley(pos, galley, colour);
    rect
}

fn text_width(p: &egui::Painter, text: &str, size: f32) -> f32 {
    p.layout_no_wrap(text.to_string(), FontId::proportional(size), Color32::WHITE).size().x
}

fn cover_uv(img: [usize; 2], into: Rect) -> Rect {
    let (ia, ra) = (img[0] as f32 / img[1].max(1) as f32, into.width() / into.height().max(1.0));
    if ia > ra {
        let vis = ra / ia;
        let x0 = (1.0 - vis) / 2.0;
        Rect::from_min_max(pos2(x0, 0.0), pos2(x0 + vis, 1.0))
    } else {
        let vis = ia / ra;
        let y0 = (1.0 - vis) / 2.0;
        Rect::from_min_max(pos2(0.0, y0), pos2(1.0, y0 + vis))
    }
}

const FULL_UV: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(1.0, 1.0) };

fn gradient(p: &egui::Painter, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), left);
    mesh.colored_vertex(rect.right_top(), right);
    mesh.colored_vertex(rect.right_bottom(), right);
    mesh.colored_vertex(rect.left_bottom(), left);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(mesh));
}

fn spinner(ui: &egui::Ui, p: &egui::Painter, c: Pos2, r: f32, colour: Color32) {
    let t = ui.ctx().input(|i| i.time) as f32;
    let (start, sweep) = (t * 4.4, 1.6 + 0.9 * (t * 2.3).sin());
    let pts: Vec<Pos2> = (0..=20)
        .map(|i| {
            let a = start + sweep * (i as f32 / 20.0);
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new((r * 0.2).max(1.8), colour)));
    ui.ctx().request_repaint();
}

/// An app's icon, or its initials on a colour while there is none.
fn paint_icon(ui: &egui::Ui, p: &egui::Painter, img: Img, rect: Rect, name: &str) {
    match img {
        Img::Ready { id, size, .. } => {
            let (w, h) = (size[0] as f32, size[1] as f32);
            let s = (rect.width() / w).min(rect.height() / h);
            p.image(id, Rect::from_center_size(rect.center(), vec2(w * s, h * s)), FULL_UV, Color32::WHITE);
        }
        Img::Loading => {
            p.rect_filled(rect, CornerRadius::same(6), Color32::from_rgba_unmultiplied(0, 0, 0, 14));
            spinner(ui, p, rect.center(), rect.width() * 0.2, BLUE);
        }
        Img::Missing => {
            p.rect_filled(rect, CornerRadius::same(6), lerp(name_colour(name), [0, 0, 0], 0.1));
            let initials: String = name.split_whitespace().take(2).filter_map(|w| w.chars().next()).collect::<String>().to_uppercase();
            p.text(rect.center(), egui::Align2::CENTER_CENTER, initials, FontId::proportional(rect.width() * 0.38), Color32::WHITE);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Look {
    Primary,
    Secondary,
    OnDark,
}

fn button(ui: &mut egui::Ui, rect: Rect, label: &str, look: Look, enabled: bool) -> bool {
    let id = ui.make_persistent_id(("button", label, rect.min.x as i32, rect.min.y as i32));
    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let hot = enabled && resp.hovered();
    let (fill, stroke, text) = match look {
        Look::Primary => (if hot { BLUE_DARK } else { BLUE }, Stroke::NONE, Color32::WHITE),
        Look::Secondary => (if hot { Color32::from_rgb(0xe6, 0xe6, 0xe6) } else { Color32::from_rgb(0xf5, 0xf5, 0xf5) }, Stroke::new(1.0, Color32::from_rgb(0xc8, 0xc8, 0xc8)), TEXT),
        Look::OnDark => (Color32::from_white_alpha(if hot { 70 } else { 40 }), Stroke::new(1.0, Color32::from_white_alpha(110)), Color32::WHITE),
    };
    let p = ui.painter();
    let fill = if enabled { fill } else { Color32::from_rgb(0xd6, 0xd6, 0xd6) };
    p.rect_filled(rect, CornerRadius::same(2), fill);
    if stroke != Stroke::NONE {
        p.rect_stroke(rect, CornerRadius::same(2), stroke, StrokeKind::Inside);
    }
    let text = if enabled { text } else { MUTED };
    p.text(rect.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(14.0), text);
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    enabled && resp.clicked()
}

fn progress_bar(ui: &egui::Ui, rect: Rect, percent: Option<f32>, on_dark: bool) {
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(2), if on_dark { Color32::from_white_alpha(50) } else { Color32::from_rgb(0xdd, 0xdd, 0xdd) });
    let fill = if on_dark { Color32::WHITE } else { BLUE };
    match percent {
        Some(f) => {
            let w = rect.width() * f.clamp(0.0, 1.0);
            p.rect_filled(Rect::from_min_size(rect.min, vec2(w, rect.height())), CornerRadius::same(2), fill);
        }
        None => {
            // Indeterminate: a block sliding across.
            let t = ui.ctx().input(|i| i.time) as f32;
            let seg = rect.width() * 0.3;
            let x = rect.left() + ((t * 0.9).fract() * (rect.width() + seg)) - seg;
            let (x0, x1) = (x.max(rect.left()), (x + seg).min(rect.right()));
            if x1 > x0 {
                p.rect_filled(Rect::from_min_max(pos2(x0, rect.top()), pos2(x1, rect.bottom())), CornerRadius::same(2), fill);
            }
        }
    }
    ui.ctx().request_repaint();
}

// ------------------------------------------------------------------ the app

impl StoreApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            images: Images::new(&cc.egui_ctx),
            tx,
            rx,
            ctx: cc.egui_ctx.clone(),
            view: View::Home,
            history: Vec::new(),
            scroll_top: false,
            sections: HashMap::new(),
            apps_category: "popular".into(),
            known: HashMap::new(),
            details: HashMap::new(),
            detail_loading: HashSet::new(),
            failed_details: HashSet::new(),
            search_text: String::new(),
            search: None,
            installed: HashMap::new(),
            updates: HashSet::new(),
            icon_paths: HashMap::new(),
            library_loading: false,
            task: None,
            notice: None,
            styled: false,
        };
        for key in ["popular", "trending", "recently-updated", "cat:Game"] {
            app.load_section(key);
        }
        app.load_detail(FEATURED_ID);
        app.refresh_library();

        // Testing aid: `WINDOW_SHOPPING_INSTALL=<id>` installs that app on launch.
        if let Ok(id) = std::env::var("WINDOW_SHOPPING_INSTALL") {
            app.start_task(Kind::Install, &id, &id);
        }

        // Testing aid: `WINDOW_SHOPPING_VIEW=games|apps|library|search:<text>|app:<id>` opens that page.
        if let Ok(v) = std::env::var("WINDOW_SHOPPING_VIEW") {
            match v.split_once(':') {
                Some(("search", q)) => app.start_search(q),
                Some(("app", id)) => {
                    app.load_detail(id);
                    app.navigate(View::App(id.to_string()));
                }
                _ => match v.as_str() {
                    "games" => app.navigate(View::Games),
                    "apps" => app.navigate(View::Apps),
                    "library" => app.navigate(View::Library),
                    _ => {}
                },
            }
        }
        app
    }

    fn spawn(&self, f: impl FnOnce(&Sender<Event>) + Send + 'static) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            f(&tx);
            ctx.request_repaint();
        });
    }

    fn load_section(&mut self, key: &str) {
        let s = self.sections.entry(key.to_string()).or_default();
        if s.loading || (s.page > 0 && !s.failed) {
            return;
        }
        s.loading = true;
        s.failed = false;
        let (key, page) = (key.to_string(), s.page + 1);
        self.spawn(move |tx| {
            let apps = match key.strip_prefix("cat:") {
                Some(cat) => api::category(cat, page, PAGE_SIZE),
                None => api::collection(&key, page, PAGE_SIZE),
            };
            let _ = tx.send(Event::Section { key, page, apps: apps.map_err(|_| ()) });
        });
    }

    fn load_more(&mut self, key: &str) {
        if let Some(s) = self.sections.get_mut(key) {
            if s.loading || !s.more {
                return;
            }
            s.loading = true;
            let (key, page) = (key.to_string(), s.page + 1);
            self.spawn(move |tx| {
                let apps = match key.strip_prefix("cat:") {
                    Some(cat) => api::category(cat, page, PAGE_SIZE),
                    None => api::collection(&key, page, PAGE_SIZE),
                };
                let _ = tx.send(Event::Section { key, page, apps: apps.map_err(|_| ()) });
            });
        }
    }

    fn load_detail(&mut self, id: &str) {
        if self.details.contains_key(id) || self.detail_loading.contains(id) || self.failed_details.contains(id) || !flatpak::valid_id(id) {
            return;
        }
        self.detail_loading.insert(id.to_string());
        let id = id.to_string();
        self.spawn(move |tx| {
            let detail = api::detail(&id).map_err(|_| ());
            let _ = tx.send(Event::Detail { id, detail });
        });
    }

    fn refresh_library(&mut self) {
        if self.library_loading {
            return;
        }
        self.library_loading = true;
        self.spawn(|tx| {
            let installed = flatpak::list_installed();
            let _ = tx.send(Event::Library { installed, updates: HashSet::new() });
            let updates = flatpak::list_updates();
            let installed = flatpak::list_installed();
            let _ = tx.send(Event::Library { installed, updates });
        });
    }

    fn start_search(&mut self, query: &str) {
        let query = query.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.search = Some((query.clone(), None));
        self.navigate(View::Search(query.clone()));
        self.spawn(move |tx| {
            let apps = api::search(&query).map_err(|_| ());
            let _ = tx.send(Event::Search { query, apps });
        });
    }

    fn navigate(&mut self, view: View) {
        if view != self.view {
            let old = std::mem::replace(&mut self.view, view);
            self.history.push(old);
            self.scroll_top = true;
        }
    }

    fn open(&mut self, app: &App) {
        self.known.insert(app.id.clone(), app.clone());
        self.failed_details.remove(&app.id);
        self.load_detail(&app.id);
        self.navigate(View::App(app.id.clone()));
    }

    fn back(&mut self) {
        if let Some(v) = self.history.pop() {
            self.view = v;
            self.scroll_top = true;
        }
    }

    fn start_task(&mut self, kind: Kind, id: &str, name: &str) {
        if self.task.is_some() || (kind != Kind::UpdateAll && !flatpak::valid_id(id)) {
            return;
        }
        self.notice = None;
        self.task = Some(Task { kind, id: id.to_string(), name: name.to_string(), percent: None, phase: "Starting" });
        let id = id.to_string();
        self.spawn(move |tx| flatpak::run(kind, &id, &|e| drop(tx.send(Event::Task(e)))));
    }

    fn finish_task(&mut self, ok: bool) {
        let Some(task) = self.task.take() else { return };
        let (text, open) = match (task.kind, ok) {
            (Kind::Install, true) => (format!("{} is installed.", task.name), Some(task.id.clone())),
            (Kind::Install, false) => (format!("Couldn't install {}. Check your internet connection and try again.", task.name), None),
            (Kind::Uninstall, true) => (format!("{} was removed.", task.name), None),
            (Kind::Uninstall, false) => (format!("Couldn't remove {}. Try again in a moment.", task.name), None),
            (Kind::Update, true) => (format!("{} is up to date.", task.name), Some(task.id.clone())),
            (Kind::UpdateAll, true) => ("Your apps are up to date.".to_string(), None),
            (Kind::Update | Kind::UpdateAll, false) => ("Couldn't finish updating. Check your internet connection and try again.".to_string(), None),
        };
        self.notice = Some(Notice { ok, text, open });
        self.library_loading = false;
        self.refresh_library();
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if self.images.poll(ctx) {
            ctx.request_repaint();
        }
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Section { key, page, apps } => {
                    let s = self.sections.entry(key).or_default();
                    s.loading = false;
                    match apps {
                        Ok(apps) => {
                            s.page = page;
                            s.more = apps.len() as u32 >= PAGE_SIZE;
                            for a in &apps {
                                self.known.insert(a.id.clone(), a.clone());
                            }
                            s.apps.extend(apps);
                        }
                        Err(()) => s.failed = true,
                    }
                }
                Event::Search { query, apps } => {
                    if let Ok(list) = &apps {
                        for a in list {
                            self.known.insert(a.id.clone(), a.clone());
                        }
                    }
                    if self.search.as_ref().is_some_and(|(q, _)| *q == query) {
                        self.search = Some((query, Some(apps)));
                    }
                }
                Event::Detail { id, detail } => {
                    self.detail_loading.remove(&id);
                    match detail {
                        Ok(d) => {
                            self.known.entry(id.clone()).or_insert_with(|| d.app.clone());
                            self.details.insert(id, d);
                        }
                        // Remembered, so we don't hammer the API; opening the app again retries.
                        Err(()) => {
                            self.failed_details.insert(id);
                        }
                    }
                }
                Event::Library { installed, updates } => {
                    self.icon_paths = installed.iter().filter_map(|i| Some((i.id.clone(), flatpak::local_icon(&i.id)?))).collect();
                    self.installed = installed.into_iter().map(|i| (i.id.clone(), i)).collect();
                    if !updates.is_empty() || !self.library_loading {
                        self.updates = updates;
                    }
                    self.library_loading = false;
                }
                Event::Task(TaskEvent::Progress { percent, phase }) => {
                    if let Some(t) = self.task.as_mut() {
                        t.percent = percent;
                        t.phase = phase;
                    }
                }
                Event::Task(TaskEvent::Done { ok }) => self.finish_task(ok),
            }
        }
    }

    fn style(&mut self, ctx: &egui::Context) {
        if self.styled {
            return;
        }
        self.styled = true;
        let mut style = (*ctx.global_style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = CANVAS;
        style.visuals.selection.bg_fill = BLUE;
        style.spacing.item_spacing = vec2(8.0, 8.0);
        ctx.set_global_style(style);
    }

    // ------------------------------------------------------------ data helpers

    /// The featured app, from its live listing when we have it.
    fn featured(&self) -> App {
        self.details.get(FEATURED_ID).map(|d| d.app.clone()).unwrap_or_else(|| App {
            id: FEATURED_ID.into(),
            name: "Leather Client".into(),
            summary: "A performance-focused Minecraft launcher and client".into(),
            developer: "Leather Client".into(),
            icon: String::new(),
            category: "Game".into(),
        })
    }

    fn section_apps(&self, key: &str) -> Vec<App> {
        self.sections.get(key).map(|s| s.apps.clone()).unwrap_or_default()
    }

    /// `apps` with the featured app moved (or added) to the front.
    fn with_featured(&self, apps: Vec<App>) -> Vec<App> {
        let mut out = vec![self.featured()];
        out.extend(apps.into_iter().filter(|a| a.id != FEATURED_ID));
        out
    }

    fn icon_src(&self, app: &App) -> String {
        if !app.icon.is_empty() {
            app.icon.clone()
        } else if let Some(p) = self.icon_paths.get(&app.id) {
            p.clone()
        } else {
            format!("https://dl.flathub.org/repo/appstream/x86_64/icons/128x128/{}.png", app.id)
        }
    }

    fn icon(&mut self, app: &App, size: u32) -> Img {
        let src = self.icon_src(app);
        self.images.get(&src, size)
    }

    fn is_installed(&self, id: &str) -> bool {
        self.installed.contains_key(id)
    }

    // ------------------------------------------------------------ chrome

    fn header(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("header")
            .frame(egui::Frame::new().fill(HEADER_BG).stroke(Stroke::new(1.0, BORDER)))
            .show(ui, |ui| {
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
                let p = ui.painter().clone();
                let mut x = bar.left() + 8.0;

                if !self.history.is_empty() {
                    let r = Rect::from_min_size(pos2(x, bar.top()), vec2(40.0, 48.0));
                    let resp = ui.interact(r, ui.make_persistent_id("back"), Sense::click());
                    if resp.hovered() {
                        p.rect_filled(r, 0, Color32::from_rgb(0xe2, 0xe2, 0xe2));
                    }
                    p.text(r.center(), egui::Align2::CENTER_CENTER, "←", FontId::proportional(18.0), TEXT);
                    if resp.clicked() {
                        self.back();
                    }
                    x += 44.0;
                }

                p.text(pos2(x + 6.0, bar.center().y), egui::Align2::LEFT_CENTER, "Window Shopping", FontId::proportional(15.0), TEXT);
                x += text_width(&p, "Window Shopping", 15.0) + 30.0;

                let current = match &self.view {
                    View::Home => Some(0),
                    View::Apps => Some(1),
                    View::Games => Some(2),
                    _ => None,
                };
                for (i, (label, view)) in [("Home", View::Home), ("Apps", View::Apps), ("Games", View::Games)].into_iter().enumerate() {
                    let w = text_width(&p, label, 14.5) + 28.0;
                    let r = Rect::from_min_size(pos2(x, bar.top()), vec2(w, 48.0));
                    let resp = ui.interact(r, ui.make_persistent_id(("tab", i)), Sense::click());
                    if resp.hovered() {
                        p.rect_filled(r, 0, Color32::from_rgb(0xe8, 0xe8, 0xe8));
                    }
                    p.text(r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(14.5), if current == Some(i) { TEXT } else { MUTED });
                    if current == Some(i) {
                        p.rect_filled(Rect::from_min_size(pos2(r.left() + 12.0, r.bottom() - 3.0), vec2(w - 24.0, 3.0)), 0, BLUE);
                    }
                    if resp.clicked() {
                        self.history.clear();
                        self.view = view;
                        self.scroll_top = true;
                    }
                    x += w;
                }

                // Right side: Library (with a badge for updates) and search.
                let lib_label = "Library";
                let lw = text_width(&p, lib_label, 14.0) + 28.0;
                let lr = Rect::from_min_size(pos2(bar.right() - lw - 8.0, bar.top()), vec2(lw, 48.0));
                let resp = ui.interact(lr, ui.make_persistent_id("library"), Sense::click());
                if resp.hovered() || self.view == View::Library {
                    p.rect_filled(lr, 0, Color32::from_rgb(0xe8, 0xe8, 0xe8));
                }
                p.text(lr.center(), egui::Align2::CENTER_CENTER, lib_label, FontId::proportional(14.0), TEXT);
                if !self.updates.is_empty() {
                    let c = pos2(lr.right() - 8.0, lr.top() + 14.0);
                    p.circle_filled(c, 8.0, BLUE);
                    p.text(c, egui::Align2::CENTER_CENTER, self.updates.len().min(99).to_string(), FontId::proportional(10.0), Color32::WHITE);
                }
                if resp.clicked() {
                    self.navigate(View::Library);
                }

                let search_w = (bar.width() * 0.26).clamp(140.0, 280.0);
                let sr = Rect::from_min_size(pos2(lr.left() - search_w - 12.0, bar.top() + 9.0), vec2(search_w, 30.0));
                if sr.left() > x + 12.0 {
                    p.rect_filled(sr, 0, Color32::WHITE);
                    p.rect_stroke(sr, 0, Stroke::new(1.0, Color32::from_rgb(0xa0, 0xa0, 0xa0)), StrokeKind::Inside);
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(sr.shrink2(vec2(8.0, 0.0))));
                    let edit = egui::TextEdit::singleline(&mut self.search_text)
                        .hint_text("Search apps")
                        .frame(egui::Frame::NONE)
                        .align(egui::Align2::LEFT_CENTER)
                        .desired_width(search_w - 44.0)
                        .margin(vec2(0.0, 6.0));
                    let resp = child.add(edit);
                    p.text(pos2(sr.right() - 16.0, sr.center().y), egui::Align2::CENTER_CENTER, "🔍", FontId::proportional(13.0), MUTED);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let q = self.search_text.clone();
                        self.start_search(&q);
                    }
                }
            });
    }

    /// Progress of what's installing, or what just happened.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        if self.task.is_none() && self.notice.is_none() {
            return;
        }
        let mut dismiss = false;
        let mut open = None;
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(HEADER_BG).stroke(Stroke::new(1.0, BORDER)))
            .show(ui, |ui| {
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
                let p = ui.painter().clone();
                if let Some(t) = &self.task {
                    let verb = if t.kind == Kind::UpdateAll { "Updating your apps".to_string() } else { format!("{} {}", t.phase, t.name) };
                    p.text(pos2(bar.left() + 20.0, bar.top() + 14.0), egui::Align2::LEFT_CENTER, verb, FontId::proportional(13.5), TEXT);
                    progress_bar(ui, Rect::from_min_size(pos2(bar.left() + 20.0, bar.top() + 29.0), vec2(bar.width() - 40.0, 4.0)), t.percent, false);
                } else if let Some(n) = &self.notice {
                    p.text(pos2(bar.left() + 20.0, bar.center().y), egui::Align2::LEFT_CENTER, &n.text, FontId::proportional(13.5), if n.ok { GREEN } else { RED });
                    let mut right = bar.right() - 20.0;
                    let close = Rect::from_min_size(pos2(right - 80.0, bar.center().y - 15.0), vec2(80.0, 30.0));
                    if button(ui, close, "Dismiss", Look::Secondary, true) {
                        dismiss = true;
                    }
                    right -= 90.0;
                    if let Some(id) = &n.open {
                        let r = Rect::from_min_size(pos2(right - 80.0, bar.center().y - 15.0), vec2(80.0, 30.0));
                        if button(ui, r, "Open", Look::Primary, true) {
                            open = Some(id.clone());
                        }
                    }
                }
            });
        if let Some(id) = open {
            flatpak::launch(&id);
            dismiss = true;
        }
        if dismiss {
            self.notice = None;
        }
    }

    // ------------------------------------------------------------ tiles and rows

    fn cols_for(width: f32) -> usize {
        (((width + TILE_GAP) / (TILE_MIN_W + TILE_GAP)).floor() as usize).max(1)
    }

    fn app_tile(&mut self, ui: &mut egui::Ui, app: &App, w: f32) -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(w, TILE_H), Sense::click());
        let p = ui.painter().clone();
        let hot = resp.hovered();
        p.rect_filled(rect, 0, Color32::WHITE);

        let stage = Rect::from_min_size(rect.min + vec2(1.0, 1.0), vec2(w - 2.0, 118.0));
        let img = self.icon(app, 128);
        let accent = if let Img::Ready { accent, .. } = img { accent } else { name_colour(&app.name) };
        p.rect_filled(stage, 0, lerp(accent, [255, 255, 255], 0.8));
        paint_icon(ui, &p, img, Rect::from_center_size(stage.center(), Vec2::splat(64.0)), &app.name);

        if app.id == FEATURED_ID {
            let tag = Rect::from_min_size(stage.min, vec2(70.0, 20.0));
            p.rect_filled(tag, 0, BLUE);
            p.text(tag.center(), egui::Align2::CENTER_CENTER, "FEATURED", FontId::proportional(10.5), Color32::WHITE);
        }

        let x = rect.left() + 12.0;
        let tw = w - 24.0;
        paint_text(&p, pos2(x, stage.bottom() + 10.0), &app.name, 14.5, TEXT, tw, 1);
        let sub = if app.category.is_empty() { app.developer.clone() } else { api::category_label(&app.category) };
        paint_text(&p, pos2(x, stage.bottom() + 31.0), &sub, 12.0, MUTED, tw, 1);
        if self.is_installed(&app.id) {
            paint_text(&p, pos2(x, rect.bottom() - 28.0), "✓ Installed", 12.5, GREEN, tw, 1);
        } else {
            paint_text(&p, pos2(x, rect.bottom() - 28.0), "Free", 12.5, TEXT, tw, 1);
        }

        p.rect_stroke(rect, 0, Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { BLUE } else { BORDER }), StrokeKind::Inside);
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.clicked()
    }

    /// A grid of tiles filling the width. Returns the app that was clicked.
    fn grid(&mut self, ui: &mut egui::Ui, apps: &[App], max_rows: Option<usize>) -> Option<App> {
        let width = ui.available_width();
        let cols = Self::cols_for(width);
        let w = (width - TILE_GAP * (cols as f32 - 1.0)) / cols as f32;
        let mut clicked = None;
        ui.spacing_mut().item_spacing = vec2(TILE_GAP, TILE_GAP);
        for (row, chunk) in apps.chunks(cols).enumerate() {
            if max_rows.is_some_and(|m| row >= m) {
                break;
            }
            ui.horizontal(|ui| {
                for app in chunk {
                    if self.app_tile(ui, app, w) {
                        clicked = Some(app.clone());
                    }
                }
            });
        }
        clicked
    }

    fn section_title(&mut self, ui: &mut egui::Ui, title: &str, see_all: Option<View>) {
        ui.add_space(26.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        let p = ui.painter().clone();
        p.text(rect.left_center(), egui::Align2::LEFT_CENTER, title, FontId::proportional(20.0), TEXT);
        if let Some(view) = see_all {
            let label = "See all";
            let w = text_width(&p, label, 13.5) + 12.0;
            let r = Rect::from_min_size(pos2(rect.right() - w, rect.top()), vec2(w, rect.height()));
            let resp = ui.interact(r, ui.make_persistent_id(("see_all", title)), Sense::click());
            p.text(r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(13.5), if resp.hovered() { BLUE_DARK } else { BLUE });
            if resp.clicked() {
                self.history.clear();
                self.view = view;
                self.scroll_top = true;
            }
        }
        ui.add_space(6.0);
    }

    /// One row of tiles (as many as fit) with a title and "See all".
    fn row(&mut self, ui: &mut egui::Ui, title: &str, key: &str, apps: Vec<App>, see_all: View) -> Option<App> {
        self.section_title(ui, title, Some(see_all));
        let failed = self.sections.get(key).is_some_and(|s| s.failed);
        if apps.is_empty() {
            self.loading_or_error(ui, failed, 120.0, key);
            return None;
        }
        self.grid(ui, &apps, Some(1))
    }

    fn loading_or_error(&mut self, ui: &mut egui::Ui, failed: bool, h: f32, retry_key: &str) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        let p = ui.painter().clone();
        if failed {
            p.text(pos2(rect.center().x, rect.center().y - 14.0), egui::Align2::CENTER_CENTER, "Can't reach the store right now. Check your internet connection.", FontId::proportional(13.5), MUTED);
            let r = Rect::from_center_size(pos2(rect.center().x, rect.center().y + 20.0), vec2(100.0, 30.0));
            if button(ui, r, "Try again", Look::Secondary, true) {
                if let Some(s) = self.sections.get_mut(retry_key) {
                    s.failed = false;
                    s.page = 0;
                }
                let key = retry_key.to_string();
                self.load_section(&key);
            }
        } else {
            spinner(ui, &p, rect.center(), 14.0, BLUE);
        }
    }

    // ------------------------------------------------------------ featured banner

    /// The big Leather Client spotlight (Home), or a slimmer strip of it (Games, detail pages).
    fn featured_banner(&mut self, ui: &mut egui::Ui, compact: bool) {
        let app = self.featured();
        let detail = self.details.get(FEATURED_ID).cloned();
        let width = ui.available_width();
        let h = if compact { 112.0 } else if width > 760.0 { 340.0 } else { 300.0 };
        let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
        let p = ui.painter().clone();

        let icon = self.icon(&app, 256);
        let accent = if let Img::Ready { accent, .. } = icon { Some(accent) } else { None };
        let base = accent.map(deep).unwrap_or(HERO_BG);
        p.rect_filled(rect, 0, base);

        // Screenshot art on the right, fading into the base colour.
        if !compact {
            if let Some(url) = detail.as_ref().and_then(|d| d.screenshots.first()) {
                if let Img::Ready { id, size, .. } = self.images.get(url, 1100) {
                    let art = Rect::from_min_max(pos2(rect.left() + width * 0.36, rect.top()), rect.max);
                    p.image(id, art, cover_uv(size, art), Color32::WHITE);
                    gradient(&p, Rect::from_min_max(art.left_top(), pos2(art.left() + art.width() * 0.5, art.bottom())), base, Color32::TRANSPARENT);
                }
            }
        } else if let Some(a) = accent {
            gradient(&p, rect, base, lerp(a, [0, 0, 0], 0.55));
        }

        let pad = if compact { 20.0 } else { 40.0 };
        let icon_size = if compact { 72.0 } else { 0.0 };
        let mut tx = rect.left() + pad;
        if compact {
            let ir = Rect::from_min_size(pos2(tx, rect.center().y - icon_size / 2.0), Vec2::splat(icon_size));
            // A light plate, so dark icons (Leather Client's is black) don't vanish into the banner.
            p.rect_filled(ir.expand(8.0), CornerRadius::same(8), Color32::from_rgb(0xee, 0xee, 0xee));
            paint_icon(ui, &p, icon, ir, &app.name);
            tx += icon_size + 26.0;
        }
        let text_w = if compact { (width - tx - 190.0).max(120.0) } else { (width * 0.4).clamp(220.0, 460.0) };

        let mut y = rect.top() + if compact { 22.0 } else { 38.0 };
        let tag = Rect::from_min_size(pos2(tx, y), vec2(78.0, 22.0));
        p.rect_filled(tag, CornerRadius::same(2), BLUE);
        p.text(tag.center(), egui::Align2::CENTER_CENTER, "FEATURED", FontId::proportional(11.5), Color32::WHITE);
        y += 32.0;
        let name = if compact { format!("Play Minecraft with {}", app.name) } else { app.name.clone() };
        y = paint_text(&p, pos2(tx, y), &name, if compact { 18.0 } else { 38.0 }, Color32::WHITE, text_w, 2).bottom() + 6.0;
        if !compact {
            y = paint_text(&p, pos2(tx, y), &format!("{}  ·  Games  ·  Free", app.developer), 13.0, Color32::from_white_alpha(190), text_w, 1).bottom() + 10.0;
            y = paint_text(&p, pos2(tx, y), &app.summary, 16.0, Color32::WHITE, text_w, 3).bottom() + 18.0;
        } else {
            paint_text(&p, pos2(tx, y), &app.summary, 13.0, Color32::from_white_alpha(210), text_w, 1);
        }

        let installed = self.is_installed(&app.id);
        let busy = self.task.is_some();
        let (label, enabled) = if installed { ("Open", true) } else { ("Get", !busy) };
        let bw = if compact { 100.0 } else { 140.0 };
        let bh = if compact { 36.0 } else { 42.0 };
        let (bx, by) = if compact { (rect.right() - pad - bw, rect.center().y - bh / 2.0) } else { (tx, y) };
        let btn = Rect::from_min_size(pos2(bx, by), vec2(bw, bh));
        let mut acted = false;
        if button(ui, btn, label, Look::Primary, enabled) {
            acted = true;
            if installed {
                flatpak::launch(&app.id);
            } else {
                self.start_task(Kind::Install, &app.id, &app.name);
            }
        }
        if !compact && button(ui, Rect::from_min_size(pos2(bx + bw + 10.0, by), vec2(bw, bh)), "Learn more", Look::OnDark, true) {
            acted = true;
            self.open(&app);
        }
        if let Some(t) = self.task.as_ref().filter(|t| t.id == app.id) {
            let bar = Rect::from_min_size(pos2(tx, by + bh + 12.0), vec2(text_w.min(300.0), 4.0));
            if !compact {
                progress_bar(ui, bar, t.percent, true);
            }
        }
        if resp.clicked() && !acted {
            self.open(&app);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    // ------------------------------------------------------------ pages

    fn home(&mut self, ui: &mut egui::Ui) {
        self.featured_banner(ui, false);
        let mut clicked = None;

        let popular = self.with_featured(self.section_apps("popular"));
        clicked = clicked.or(self.row(ui, "Top free apps", "popular", popular, View::Apps));

        let games = self.with_featured(self.section_apps("cat:Game"));
        clicked = clicked.or(self.row(ui, "Top free games", "cat:Game", games, View::Games));

        let trending = self.section_apps("trending");
        clicked = clicked.or(self.row(ui, "Trending now", "trending", trending, View::Apps));

        let recent = self.section_apps("recently-updated");
        clicked = clicked.or(self.row(ui, "Recently updated", "recently-updated", recent, View::Apps));

        ui.add_space(30.0);
        if let Some(app) = clicked {
            self.open(&app);
        }
    }

    fn catalog(&mut self, ui: &mut egui::Ui, games: bool) {
        let key = if games { "cat:Game".to_string() } else { self.apps_category.clone() };
        if games {
            self.featured_banner(ui, true);
        }
        self.section_title(ui, if games { "Games" } else { "Apps" }, None);

        if !games {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                let chips = std::iter::once(("popular".to_string(), "Top free".to_string()))
                    .chain(api::CATEGORIES.iter().filter(|(k, _)| *k != "Game").map(|(k, v)| (format!("cat:{k}"), v.to_string())));
                for (k, label) in chips {
                    let selected = self.apps_category == k;
                    let w = text_width(ui.painter(), &label, 13.5) + 28.0;
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
                    let p = ui.painter();
                    p.rect_filled(r, CornerRadius::same(2), if selected { BLUE } else if resp.hovered() { Color32::from_rgb(0xe6, 0xe6, 0xe6) } else { Color32::from_rgb(0xf2, 0xf2, 0xf2) });
                    p.text(r.center(), egui::Align2::CENTER_CENTER, &label, FontId::proportional(13.5), if selected { Color32::WHITE } else { TEXT });
                    if resp.clicked() {
                        self.apps_category = k.clone();
                        self.load_section(&k);
                    }
                }
            });
            ui.add_space(10.0);
        }

        self.load_section(&key);
        let mut apps = self.section_apps(&key);
        if games {
            apps = self.with_featured(apps);
        }
        let (loading, failed, more) = self.sections.get(&key).map(|s| (s.loading, s.failed, s.more)).unwrap_or((true, false, false));
        let clicked = if apps.is_empty() {
            self.loading_or_error(ui, failed, 200.0, &key);
            None
        } else {
            self.grid(ui, &apps, None)
        };
        if more {
            ui.add_space(14.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
            if loading {
                spinner(ui, &ui.painter().clone(), r.center(), 11.0, BLUE);
            } else if button(ui, Rect::from_center_size(r.center(), vec2(140.0, 36.0)), "Show more", Look::Secondary, true) {
                self.load_more(&key);
            }
        }
        ui.add_space(30.0);
        if let Some(app) = clicked {
            self.open(&app);
        }
    }

    fn search_page(&mut self, ui: &mut egui::Ui, query: &str) {
        self.section_title(ui, &format!("Results for “{query}”"), None);
        let result = self.search.as_ref().filter(|(q, _)| q == query).and_then(|(_, r)| r.clone());
        match result {
            None => {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 200.0), Sense::hover());
                spinner(ui, &ui.painter().clone(), r.center(), 14.0, BLUE);
            }
            Some(Err(())) => {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "Can't reach the store right now. Check your internet connection.", FontId::proportional(13.5), MUTED);
            }
            Some(Ok(apps)) if apps.is_empty() => {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "No apps found. Try a different search.", FontId::proportional(14.0), MUTED);
            }
            Some(Ok(mut apps)) => {
                // People looking for a launcher or Minecraft get the featured one first.
                let q = query.to_lowercase();
                if ["minecraft", "launcher", "leather", "game"].iter().any(|k| q.contains(k)) {
                    apps = self.with_featured(apps);
                }
                if let Some(app) = self.grid(ui, &apps, None) {
                    self.open(&app);
                }
            }
        }
        ui.add_space(30.0);
    }

    fn detail_page(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(app) = self.details.get(id).map(|d| d.app.clone()).or_else(|| self.known.get(id).cloned()) else {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 300.0), Sense::hover());
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "This app isn't available.", FontId::proportional(14.0), MUTED);
            return;
        };
        let detail = self.details.get(id).cloned();
        let width = ui.available_width();
        let compact = width < 620.0;

        // Header.
        let (head, _) = ui.allocate_exact_size(vec2(width, if compact { 230.0 } else { 170.0 }), Sense::hover());
        let p = ui.painter().clone();
        let icon = self.icon(&app, 256);
        let accent = if let Img::Ready { accent, .. } = icon { accent } else { name_colour(&app.name) };
        let icon_rect = Rect::from_min_size(head.min + vec2(0.0, 8.0), Vec2::splat(if compact { 88.0 } else { 140.0 }));
        p.rect_filled(icon_rect, CornerRadius::same(4), lerp(accent, [255, 255, 255], 0.8));
        paint_icon(ui, &p, icon, icon_rect.shrink(icon_rect.width() * 0.14), &app.name);

        let tx = icon_rect.right() + 22.0;
        let tw = (head.right() - tx).max(120.0);
        let mut y = head.top() + 6.0;
        y = paint_text(&p, pos2(tx, y), &app.name, 28.0, TEXT, tw, 2).bottom() + 4.0;
        if !app.developer.is_empty() {
            y = paint_text(&p, pos2(tx, y), &app.developer, 14.0, BLUE, tw, 1).bottom() + 4.0;
        }
        let sub = format!("{}  ·  Free", api::category_label(&app.category));
        y = paint_text(&p, pos2(tx, y), &sub, 13.0, MUTED, tw, 1).bottom() + 14.0;

        let installed = self.is_installed(id);
        let has_update = self.updates.contains(id);
        let busy = self.task.is_some();
        let working = self.task.as_ref().is_some_and(|t| t.id == id);
        let mut bx = tx;
        let bh = 38.0;
        if working {
            let t = self.task.as_ref().unwrap();
            paint_text(&p, pos2(tx, y), &format!("{}…", t.phase), 13.5, MUTED, tw, 1);
            progress_bar(ui, Rect::from_min_size(pos2(tx, y + 24.0), vec2(tw.min(340.0), 4.0)), t.percent, false);
        } else if installed {
            if button(ui, Rect::from_min_size(pos2(bx, y), vec2(120.0, bh)), "Open", Look::Primary, true) {
                flatpak::launch(id);
            }
            bx += 130.0;
            if has_update && button(ui, Rect::from_min_size(pos2(bx, y), vec2(120.0, bh)), "Update", Look::Secondary, !busy) {
                self.start_task(Kind::Update, id, &app.name);
            }
            if has_update {
                bx += 130.0;
            }
            if button(ui, Rect::from_min_size(pos2(bx, y), vec2(120.0, bh)), "Uninstall", Look::Secondary, !busy) {
                self.start_task(Kind::Uninstall, id, &app.name);
            }
        } else if button(ui, Rect::from_min_size(pos2(bx, y), vec2(140.0, bh)), "Get", Look::Primary, !busy) {
            self.start_task(Kind::Install, id, &app.name);
        }

        // Summary and screenshots.
        ui.add_space(14.0);
        if !app.summary.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(&app.summary).size(16.0).color(TEXT)).wrap());
        }
        let Some(detail) = detail else {
            ui.add_space(30.0);
            let (r, _) = ui.allocate_exact_size(vec2(width, 80.0), Sense::hover());
            if self.failed_details.contains(id) {
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "Couldn't load the details. Check your internet connection.", FontId::proportional(13.5), MUTED);
            } else {
                spinner(ui, &ui.painter().clone(), r.center(), 12.0, BLUE);
            }
            return;
        };

        if !detail.screenshots.is_empty() {
            ui.add_space(18.0);
            egui::ScrollArea::horizontal().id_salt(("shots", id)).auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    for url in &detail.screenshots {
                        let height = 280.0;
                        match self.images.get(url, 1100) {
                            Img::Ready { id, size, .. } => {
                                let w = height * size[0] as f32 / size[1].max(1) as f32;
                                let (r, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
                                ui.painter().image(id, r, FULL_UV, Color32::WHITE);
                                ui.painter().rect_stroke(r, 0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
                            }
                            _ => {
                                let (r, _) = ui.allocate_exact_size(vec2(440.0, height), Sense::hover());
                                ui.painter().rect_filled(r, 0, Color32::from_rgb(0xf3, 0xf3, 0xf3));
                                spinner(ui, &ui.painter().clone(), r.center(), 14.0, BLUE);
                            }
                        }
                    }
                });
            });
        }

        let body_w = width.min(760.0);
        if !detail.description.is_empty() {
            ui.add_space(22.0);
            ui.label(egui::RichText::new("Description").size(19.0).color(TEXT));
            ui.add_space(4.0);
            ui.scope(|ui| {
                ui.set_max_width(body_w);
                ui.add(egui::Label::new(egui::RichText::new(&detail.description).size(14.0).color(Color32::from_rgb(0x33, 0x33, 0x33))).wrap());
            });
        }
        if !detail.notes.is_empty() {
            ui.add_space(22.0);
            ui.label(egui::RichText::new("What's new").size(19.0).color(TEXT));
            ui.add_space(4.0);
            ui.scope(|ui| {
                ui.set_max_width(body_w);
                ui.add(egui::Label::new(egui::RichText::new(&detail.notes).size(14.0).color(Color32::from_rgb(0x33, 0x33, 0x33))).wrap());
            });
        }

        ui.add_space(22.0);
        ui.label(egui::RichText::new("Additional information").size(19.0).color(TEXT));
        ui.add_space(6.0);
        let cats: Vec<String> = detail.categories.iter().filter_map(|c| api::CATEGORIES.iter().find(|(k, _)| k == c).map(|(_, v)| v.to_string())).collect();
        let mut rows = vec![("Published by", app.developer.clone()), ("Version", detail.version.clone()), ("Updated", detail.released.clone()), ("Category", cats.join(", ")), ("License", detail.license.clone()), ("Website", detail.homepage.clone())];
        rows.retain(|(_, v)| !v.is_empty());
        egui::Grid::new("info").num_columns(2).spacing([28.0, 8.0]).show(ui, |ui| {
            for (k, v) in rows {
                ui.label(egui::RichText::new(k).size(13.5).color(MUTED));
                ui.label(egui::RichText::new(v).size(13.5).color(TEXT));
                ui.end_row();
            }
        });

        // Games get the featured game underneath.
        if id != FEATURED_ID && detail.categories.iter().any(|c| c == "Game") {
            ui.add_space(26.0);
            self.featured_banner(ui, true);
        }
        ui.add_space(30.0);
    }

    fn library(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        ui.add_space(18.0);
        let (head, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
        let p = ui.painter().clone();
        p.text(head.left_center(), egui::Align2::LEFT_CENTER, "Library", FontId::proportional(26.0), TEXT);
        if !self.updates.is_empty() {
            let r = Rect::from_min_size(pos2(head.right() - 150.0, head.top()), vec2(150.0, 34.0));
            if button(ui, r, "Update all", Look::Primary, self.task.is_none()) {
                self.start_task(Kind::UpdateAll, "", "your apps");
            }
        } else if self.library_loading {
            spinner(ui, &p, pos2(head.right() - 16.0, head.center().y), 10.0, BLUE);
        } else {
            let r = Rect::from_min_size(pos2(head.right() - 150.0, head.top()), vec2(150.0, 34.0));
            if button(ui, r, "Check for updates", Look::Secondary, true) {
                self.refresh_library();
            }
        }
        ui.add_space(10.0);

        let mut apps: Vec<Installed> = self.installed.values().cloned().collect();
        apps.sort_by(|a, b| (!self.updates.contains(&a.id), a.name.to_lowercase()).cmp(&(!self.updates.contains(&b.id), b.name.to_lowercase())));
        if apps.is_empty() {
            let (r, _) = ui.allocate_exact_size(vec2(width, 200.0), Sense::hover());
            let msg = if self.library_loading { "Looking at your apps…" } else { "Apps you get from the store show up here." };
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, msg, FontId::proportional(14.0), MUTED);
            return;
        }

        let mut open_app = None;
        for item in apps {
            let (row, resp) = ui.allocate_exact_size(vec2(width, 68.0), Sense::click());
            let p = ui.painter().clone();
            if resp.hovered() {
                p.rect_filled(row, 0, Color32::from_rgb(0xf5, 0xf5, 0xf5));
            }
            p.line_segment([row.left_bottom(), row.right_bottom()], Stroke::new(1.0, Color32::from_rgb(0xee, 0xee, 0xee)));
            let app = self.known.get(&item.id).cloned().unwrap_or(App { id: item.id.clone(), name: item.name.clone(), ..Default::default() });
            let img = self.icon(&app, 128);
            paint_icon(ui, &p, img, Rect::from_min_size(row.min + vec2(10.0, 14.0), Vec2::splat(40.0)), &item.name);
            let update = self.updates.contains(&item.id);
            let sub = if update { "Update available".to_string() } else if item.version.is_empty() { "Installed".to_string() } else { format!("Version {}", item.version) };
            paint_text(&p, pos2(row.left() + 64.0, row.top() + 13.0), &item.name, 14.5, TEXT, width - 420.0, 1);
            paint_text(&p, pos2(row.left() + 64.0, row.top() + 35.0), &sub, 12.5, if update { BLUE } else { MUTED }, width - 420.0, 1);

            let busy = self.task.is_some();
            let mut x = row.right() - 10.0;
            x -= 110.0;
            let mut acted = false;
            if button(ui, Rect::from_min_size(pos2(x, row.top() + 17.0), vec2(110.0, 34.0)), "Uninstall", Look::Secondary, !busy) {
                self.start_task(Kind::Uninstall, &item.id, &item.name);
                acted = true;
            }
            if update {
                x -= 100.0;
                if button(ui, Rect::from_min_size(pos2(x, row.top() + 17.0), vec2(90.0, 34.0)), "Update", Look::Secondary, !busy) {
                    self.start_task(Kind::Update, &item.id, &item.name);
                    acted = true;
                }
            }
            x -= 100.0;
            if button(ui, Rect::from_min_size(pos2(x, row.top() + 17.0), vec2(90.0, 34.0)), "Open", Look::Primary, true) {
                flatpak::launch(&item.id);
                acted = true;
            }
            if resp.clicked() && !acted {
                open_app = Some(app);
            }
        }
        if let Some(app) = open_app {
            self.open(&app);
        }
        ui.add_space(30.0);
    }
}

impl eframe::App for StoreApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.style(&ctx);
        self.poll(&ctx);
        self.header(ui);
        self.status_bar(ui);

        egui::CentralPanel::default().frame(egui::Frame::new().fill(CANVAS).inner_margin(egui::Margin::symmetric(28, 0))).show(ui, |ui| {
            let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
            if std::mem::take(&mut self.scroll_top) {
                area = area.vertical_scroll_offset(0.0);
            }
            area.show(ui, |ui| {
                ui.add_space(20.0);
                match self.view.clone() {
                    View::Home => self.home(ui),
                    View::Apps => self.catalog(ui, false),
                    View::Games => self.catalog(ui, true),
                    View::Library => self.library(ui),
                    View::Search(q) => self.search_page(ui, &q),
                    View::App(id) => self.detail_page(ui, &id),
                }
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Window Shopping").with_app_id("window-shopping").with_inner_size([1100.0, 720.0]).with_min_inner_size([480.0, 380.0]),
        ..Default::default()
    };
    eframe::run_native("Window Shopping", options, Box::new(|cc| Ok(Box::new(StoreApp::new(cc)))))
}
