//! vide-desktop: the VIDE-DE desktop as its own program. One full-screen surface on
//! the wlr-layer-shell *background* layer: the wallpaper, and the ~/Desktop items in
//! a Windows-style grid (top to bottom, then left to right).
//!
//! Double-click opens (folders in File Exploder, launchers run, files via xdg-open);
//! right-click gives a desktop menu. Polls the wallpaper setting and ~/Desktop.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime},
};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    reexports::calloop::{
        timer::{TimeoutAction, Timer},
        EventLoop,
    },
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT, BTN_RIGHT},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use calloop_wayland_source::WaylandSource;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

const CELL_W: i32 = 84;
const CELL_H: i32 = 96;
const ICON: u32 = 48;
const MARGIN: i32 = 8;
const LABEL: f32 = 12.5;
const MENU_W: i32 = 220;
const MENU_ROW: i32 = 30;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

fn main() {
    let conn = Connection::connect_to_env().expect("vide-desktop needs a Wayland session");
    let (globals, queue) = registry_queue_init(&conn).expect("registry");
    let qh = queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("this compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm");

    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Background, Some("vide-desktop"), None);
    // Fill the whole output, and don't push windows around.
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    let mut desktop = Desktop {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        pool: SlotPool::new(1920 * 1080 * 4, &shm).expect("shm pool"),
        shm,
        layer,
        pointer: None,
        size: (0, 0),
        configured: false,
        dirty: true,
        exit: false,
        config: vide_config::Config::load(),
        config_mtime: vide_config::Config::mtime(),
        wallpaper: None,
        font: load_font(),
        items: Vec::new(),
        desktop_mtime: None,
        hover: None,
        selected: Vec::new(),
        menu: None,
        mouse: (0.0, 0.0),
        last_click: None,
    };
    desktop.rescan();

    let mut event_loop: EventLoop<Desktop> = EventLoop::try_new().expect("event loop");
    WaylandSource::new(conn, queue).insert(event_loop.handle()).expect("wayland source");
    // Pick up wallpaper changes from Settings and new files on the Desktop.
    event_loop
        .handle()
        .insert_source(Timer::from_duration(Duration::from_secs(2)), |_, _, d| {
            d.poll();
            TimeoutAction::ToDuration(Duration::from_secs(2))
        })
        .expect("timer");

    while !desktop.exit {
        event_loop.dispatch(Duration::from_millis(500), &mut desktop).expect("dispatch");
        if desktop.dirty && desktop.configured {
            desktop.draw(&qh);
        }
    }
}

struct Item {
    name: String,
    path: PathBuf,
    /// What double-click does.
    open: Open,
    icon: Option<Icon>,
}

enum Open {
    Folder(PathBuf),
    File(PathBuf),
    Launcher(String),
}

/// Premultiplied BGRA (matches wl_shm Argb8888 in memory).
struct Icon {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq)]
enum MenuAction {
    Open,
    Delete,
    Refresh,
    NewFolder,
    Personalize,
    Terminal,
    About,
}

struct Menu {
    x: i32,
    y: i32,
    items: Vec<(&'static str, MenuAction)>,
    hover: Option<usize>,
}

struct Desktop {
    registry: RegistryState,
    seat: SeatState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    size: (u32, u32),
    configured: bool,
    dirty: bool,
    exit: bool,
    config: vide_config::Config,
    config_mtime: Option<SystemTime>,
    /// Wallpaper scaled to the output: (size, BGRA pixels).
    wallpaper: Option<((u32, u32), Vec<u8>)>,
    font: Option<fontdue::Font>,
    items: Vec<Item>,
    desktop_mtime: Option<SystemTime>,
    hover: Option<usize>,
    selected: Vec<usize>,
    menu: Option<Menu>,
    mouse: (f64, f64),
    last_click: Option<(usize, Instant)>,
}

// ---------------------------------------------------------------- items

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// XDG desktop dir (user-dirs.dirs), defaulting to ~/Desktop.
fn desktop_dir() -> PathBuf {
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
fn own_app(name: &str) -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join(name);
    p.exists().then_some(p)
}

fn trash_dir() -> PathBuf {
    home().join(".local/share/Trash/files")
}

impl Desktop {
    fn rescan(&mut self) {
        let dir = desktop_dir();
        self.desktop_mtime = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
        let mut items = vec![Item {
            name: "Recycle Bin".into(),
            path: trash_dir(),
            open: Open::Folder(trash_dir()),
            icon: load_icon("user-trash", ICON),
        }];
        let mut entries: Vec<_> = std::fs::read_dir(&dir).map(|rd| rd.flatten().collect()).unwrap_or_default();
        entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
        for e in entries {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let item = if path.is_dir() {
                Item { name, open: Open::Folder(path.clone()), path, icon: load_icon("folder", ICON) }
            } else if name.ends_with(".desktop") {
                let (dname, exec, icon) = parse_launcher(&path);
                Item {
                    name: dname.unwrap_or_else(|| name.trim_end_matches(".desktop").to_string()),
                    open: Open::Launcher(exec.unwrap_or_default()),
                    path,
                    icon: icon.and_then(|i| load_icon(&i, ICON)).or_else(|| load_icon("application-x-executable", ICON)),
                }
            } else {
                let icon = mime_icon(&path);
                Item { name, open: Open::File(path.clone()), path, icon: load_icon(&icon, ICON).or_else(|| load_icon("text-x-generic", ICON)) }
            };
            items.push(item);
        }
        self.items = items;
        self.selected.clear();
        self.hover = None;
        self.dirty = true;
    }

    fn poll(&mut self) {
        let mtime = vide_config::Config::mtime();
        if mtime != self.config_mtime {
            self.config_mtime = mtime;
            let new = vide_config::Config::load();
            if new.wallpaper != self.config.wallpaper {
                self.wallpaper = None;
                self.dirty = true;
            }
            self.config = new;
        }
        let dm = std::fs::metadata(desktop_dir()).and_then(|m| m.modified()).ok();
        if dm != self.desktop_mtime {
            self.rescan();
        }
    }

    /// Item cell rectangles, Windows-style: down the first column, then the next.
    fn cell(&self, i: usize) -> (i32, i32) {
        let rows = ((self.size.1 as i32 - MARGIN * 2 - 48) / CELL_H).max(1);
        let (col, row) = (i as i32 / rows, i as i32 % rows);
        (MARGIN + col * CELL_W, MARGIN + row * CELL_H)
    }

    fn item_at(&self, x: f64, y: f64) -> Option<usize> {
        (0..self.items.len()).find(|&i| {
            let (cx, cy) = self.cell(i);
            x >= cx as f64 && x < (cx + CELL_W - 4) as f64 && y >= cy as f64 && y < (cy + CELL_H - 4) as f64
        })
    }

    fn menu_item_at(&self, x: f64, y: f64) -> Option<usize> {
        let m = self.menu.as_ref()?;
        let (lx, ly) = (x as i32 - m.x, y as i32 - m.y - 4);
        (lx >= 0 && lx < MENU_W && ly >= 0).then(|| (ly / MENU_ROW) as usize).filter(|&i| i < m.items.len())
    }

    fn open(&self, i: usize) {
        let Some(item) = self.items.get(i) else { return };
        let res = match &item.open {
            Open::Folder(p) => match own_app("file-exploder") {
                Some(fe) => Command::new(fe).arg(p).spawn(),
                None => Command::new("xdg-open").arg(p).spawn(),
            },
            Open::File(p) => Command::new("xdg-open").arg(p).spawn(),
            Open::Launcher(exec) if !exec.is_empty() => Command::new("sh").arg("-c").arg(exec).spawn(),
            Open::Launcher(_) => return,
        };
        if let Err(e) = res {
            eprintln!("vide-desktop: couldn't open {}: {e}", item.name);
        }
    }

    fn run(&mut self, action: MenuAction) {
        let spawn = |name: &str, args: &[&str]| {
            if let Some(p) = own_app(name) {
                let _ = Command::new(p).args(args).current_dir(desktop_dir()).spawn();
            }
        };
        match action {
            MenuAction::Open => {
                for &i in &self.selected {
                    self.open(i);
                }
            }
            MenuAction::Delete => {
                // Never the Recycle Bin itself (index 0), and only to the trash.
                let paths: Vec<_> = self.selected.iter().filter(|&&i| i > 0).filter_map(|&i| self.items.get(i)).map(|it| it.path.clone()).collect();
                if !paths.is_empty() {
                    let _ = Command::new("gio").arg("trash").args(&paths).status();
                }
                self.rescan();
            }
            MenuAction::Refresh => self.rescan(),
            MenuAction::NewFolder => {
                let dir = desktop_dir();
                let mut name = "New folder".to_string();
                let mut n = 2;
                while dir.join(&name).exists() {
                    name = format!("New folder ({n})");
                    n += 1;
                }
                let _ = std::fs::create_dir_all(dir.join(name));
                self.rescan();
            }
            MenuAction::Personalize => spawn("vide-settings", &["--page", "personalization"]),
            MenuAction::Terminal => spawn("powerslop", &[]),
            MenuAction::About => spawn("vide-about", &[]),
        }
    }

    fn pointer(&mut self, ev: &PointerEvent) {
        let (x, y) = ev.position;
        self.mouse = (x, y);
        match ev.kind {
            PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                let hover = self.item_at(x, y);
                let mh = self.menu_item_at(x, y);
                if hover != self.hover || self.menu.as_ref().is_some_and(|m| m.hover != mh) {
                    self.hover = hover;
                    if let Some(m) = self.menu.as_mut() {
                        m.hover = mh;
                    }
                    self.dirty = true;
                }
            }
            PointerEventKind::Leave { .. } => {
                if self.hover.take().is_some() {
                    self.dirty = true;
                }
            }
            PointerEventKind::Press { button, .. } => {
                // An open menu takes the next click.
                if self.menu.is_some() {
                    let picked = self.menu_item_at(x, y).and_then(|i| self.menu.as_ref().map(|m| m.items[i].1));
                    self.menu = None;
                    self.dirty = true;
                    if let (Some(a), BTN_LEFT) = (picked, button) {
                        self.run(a);
                    }
                    return;
                }
                let hit = self.item_at(x, y);
                match (button, hit) {
                    (BTN_LEFT, Some(i)) => {
                        let now = Instant::now();
                        let double = matches!(self.last_click, Some((li, t)) if li == i && now - t < DOUBLE_CLICK);
                        self.selected = vec![i];
                        if double {
                            self.last_click = None;
                            self.open(i);
                        } else {
                            self.last_click = Some((i, now));
                        }
                    }
                    (BTN_LEFT, None) => self.selected.clear(),
                    (BTN_RIGHT, Some(i)) => {
                        if !self.selected.contains(&i) {
                            self.selected = vec![i];
                        }
                        let mut items = vec![("Open", MenuAction::Open)];
                        if i > 0 {
                            items.push(("Delete", MenuAction::Delete));
                        }
                        self.open_menu(x, y, items);
                    }
                    (BTN_RIGHT, None) => {
                        self.selected.clear();
                        self.open_menu(
                            x,
                            y,
                            vec![
                                ("Refresh", MenuAction::Refresh),
                                ("New folder", MenuAction::NewFolder),
                                ("Open in PowerSlop", MenuAction::Terminal),
                                ("Personalize", MenuAction::Personalize),
                                ("About VIDE-DE", MenuAction::About),
                            ],
                        );
                    }
                    _ => {}
                }
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn open_menu(&mut self, x: f64, y: f64, items: Vec<(&'static str, MenuAction)>) {
        let h = items.len() as i32 * MENU_ROW + 8;
        let mx = (x as i32).min(self.size.0 as i32 - MENU_W - 2).max(0);
        let my = (y as i32).min(self.size.1 as i32 - h - 2).max(0);
        self.menu = Some(Menu { x: mx, y: my, items, hover: None });
    }

    // ------------------------------------------------------------ drawing

    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        let (w, h) = self.size;
        if w == 0 || h == 0 {
            return;
        }
        self.dirty = false;
        if self.wallpaper.as_ref().is_none_or(|(s, _)| *s != (w, h)) {
            self.wallpaper = Some(((w, h), load_wallpaper(&self.config.wallpaper, w, h)));
        }
        let cells: Vec<(i32, i32)> = (0..self.items.len()).map(|i| self.cell(i)).collect();
        let stride = w as i32 * 4;
        let Ok((buffer, canvas)) = self.pool.create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888) else { return };
        let mut c = Canvas { w: w as i32, h: h as i32, px: canvas };
        c.px.copy_from_slice(&self.wallpaper.as_ref().unwrap().1);

        for (i, &(x, y)) in cells.iter().enumerate() {
            let selected = self.selected.contains(&i);
            if selected {
                c.rect(x, y, CELL_W - 4, CELL_H - 4, [0xd7, 0x78, 0x00, 0x66]);
                c.outline(x, y, CELL_W - 4, CELL_H - 4, [0xff, 0xc0, 0x80, 0xaa]);
            } else if self.hover == Some(i) {
                c.rect(x, y, CELL_W - 4, CELL_H - 4, [0xff, 0xff, 0xff, 0x33]);
            }
            let item = &self.items[i];
            if let Some(icon) = &item.icon {
                c.image(icon, x + (CELL_W - 4 - icon.w as i32) / 2, y + 6);
            }
            if let Some(f) = &self.font {
                // Up to two lines, centred, white with a soft shadow (Windows desktop labels).
                for (li, line) in wrap(f, &item.name, (CELL_W - 10) as f32, 2).iter().enumerate() {
                    let lw = text_width(f, line);
                    let tx = x + (CELL_W - 4 - lw) / 2;
                    let ty = y + 6 + ICON as i32 + 4 + li as i32 * 16;
                    c.text(f, tx + 1, ty + 1, line, [0, 0, 0, 0xc0]);
                    c.text(f, tx, ty, line, [0xff, 0xff, 0xff, 0xff]);
                }
            }
        }

        if let Some(m) = &self.menu {
            let mh = m.items.len() as i32 * MENU_ROW + 8;
            c.rect(m.x, m.y, MENU_W, mh, [0x2b, 0x2b, 0x2b, 0xf8]);
            c.outline(m.x, m.y, MENU_W, mh, [0x4a, 0x4a, 0x4a, 0xff]);
            for (i, (label, _)) in m.items.iter().enumerate() {
                let ry = m.y + 4 + i as i32 * MENU_ROW;
                if m.hover == Some(i) {
                    c.rect(m.x + 1, ry, MENU_W - 2, MENU_ROW, [0xff, 0xff, 0xff, 0x1f]);
                }
                if let Some(f) = &self.font {
                    c.text(f, m.x + 16, ry + 8, label, [0xff, 0xff, 0xff, 0xff]);
                }
            }
        }

        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, w as i32, h as i32);
        let _ = buffer.attach_to(surface);
        self.layer.commit();
    }
}

// ---------------------------------------------------------------- pixels

struct Canvas<'a> {
    w: i32,
    h: i32,
    /// BGRA, premultiplied.
    px: &'a mut [u8],
}

impl Canvas<'_> {
    /// Blend a straight-alpha BGRA colour at `cov` coverage.
    fn blend(&mut self, x: i32, y: i32, c: [u8; 4], cov: f32) {
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

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.blend(xx, yy, c, 1.0);
            }
        }
    }

    fn outline(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        self.rect(x, y, w, 1, c);
        self.rect(x, y + h - 1, w, 1, c);
        self.rect(x, y, 1, h, c);
        self.rect(x + w - 1, y, 1, h, c);
    }

    fn image(&mut self, img: &Icon, x: i32, y: i32) {
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
    fn text(&mut self, font: &fontdue::Font, x: i32, y: i32, s: &str, c: [u8; 4]) {
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

fn text_width(font: &fontdue::Font, s: &str) -> i32 {
    s.chars().map(|c| font.metrics(c, LABEL).advance_width).sum::<f32>().round() as i32
}

/// Word-wrap into at most `lines` lines, ellipsizing the last.
fn wrap(font: &fontdue::Font, s: &str, max: f32, lines: usize) -> Vec<String> {
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

fn load_font() -> Option<fontdue::Font> {
    ["/usr/share/fonts/noto/NotoSans-Regular.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/dejavu/DejaVuSans.ttf"]
        .iter()
        .find_map(|p| std::fs::read(p).ok())
        .and_then(|b| fontdue::Font::from_bytes(b, fontdue::FontSettings::default()).ok())
}

/// Decode a PNG to straight RGBA.
fn decode_png(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
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
fn load_wallpaper(path: &Path, w: u32, h: u32) -> Vec<u8> {
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
fn load_icon(name: &str, size: u32) -> Option<Icon> {
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

fn find_icon(name: &str) -> Option<PathBuf> {
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
fn mime_icon(path: &Path) -> String {
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
fn parse_launcher(path: &Path) -> (Option<String>, Option<String>, Option<String>) {
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

impl CompositorHandler for Desktop {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Desktop {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Desktop {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let (w, h) = configure.new_size;
        if (w, h) != self.size && w > 0 && h > 0 {
            self.size = (w, h);
            self.dirty = true;
        }
        self.configured = true;
    }
}

impl SeatHandler for Desktop {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Desktop {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for ev in events {
            if &ev.surface == self.layer.wl_surface() {
                self.pointer(ev);
            }
        }
    }
}

impl ShmHandler for Desktop {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Desktop {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(Desktop);
smithay_client_toolkit::delegate_dispatch2!(Desktop);
