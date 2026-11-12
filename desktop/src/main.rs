//! vide-desktop: the VIDE-DE desktop as its own program. One full-screen surface on
//! the wlr-layer-shell *background* layer: the wallpaper, and the ~/Desktop items on
//! a Windows-style grid.
//!
//! Icons sit in grid cells you can drag them to (positions are remembered), or are
//! auto-arranged. Rubber-band and Ctrl/Shift-click selection; drop on a folder to move
//! files into it, or on the Recycle Bin to delete. Keyboard: Delete, F2 rename,
//! Enter, arrows, Ctrl+A, Esc. Right-click for View / Sort by and more.

mod gfx;

use std::{
    collections::HashMap,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant, SystemTime},
};

use calloop_wayland_source::WaylandSource;
use gfx::{desktop_dir, home, load_icon, mime_icon, own_app, parse_launcher, text_width, trash_dir, wrap, Canvas, Icon};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::calloop::{
        timer::{TimeoutAction, Timer},
        EventLoop,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT, BTN_RIGHT},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

const MARGIN: i32 = 8;
const MENU_W: i32 = 240;
const MENU_ROW: i32 = 30;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const DRAG_START: f64 = 5.0;
/// Room left for our taskbar at the bottom of the screen.
const TASKBAR_H: i32 = 40;

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
    // Keyboard only after you click the desktop (Delete, F2, arrows...).
    layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
    layer.commit();

    let prefs = Prefs::load();
    let mut desktop = Desktop {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        pool: SlotPool::new(1920 * 1080 * 4, &shm).expect("shm pool"),
        shm,
        layer,
        pointer: None,
        keyboard: None,
        mods: Modifiers::default(),
        size: (0, 0),
        configured: false,
        dirty: true,
        exit: false,
        config: vide_config::Config::load(),
        config_mtime: vide_config::Config::mtime(),
        wallpaper: None,
        font: gfx::load_font(),
        items: Vec::new(),
        desktop_mtime: None,
        prefs,
        hover: None,
        selected: Vec::new(),
        anchor: None,
        press: None,
        rubber: None,
        renaming: None,
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

// ---------------------------------------------------------------- prefs

#[derive(Clone, Copy, PartialEq)]
enum SortBy {
    Name,
    Modified,
    Type,
}

/// ~/.config/vide-de/desktop.conf: view options and where each icon was put.
struct Prefs {
    icon: u32,
    sort: SortBy,
    auto_arrange: bool,
    positions: HashMap<String, (i32, i32)>,
}

impl Prefs {
    fn path() -> PathBuf {
        home().join(".config/vide-de/desktop.conf")
    }

    fn load() -> Self {
        let mut p = Prefs { icon: 48, sort: SortBy::Name, auto_arrange: false, positions: HashMap::new() };
        let text = std::fs::read_to_string(Self::path()).unwrap_or_default();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("pos=") {
                let f: Vec<&str> = rest.split('\t').collect();
                if let [name, c, r] = f[..] {
                    if let (Ok(c), Ok(r)) = (c.parse(), r.parse()) {
                        p.positions.insert(name.to_string(), (c, r));
                    }
                }
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            match k {
                "icon_size" => p.icon = v.parse().unwrap_or(48).clamp(24, 96),
                "auto_arrange" => p.auto_arrange = v == "true",
                "sort" => {
                    p.sort = match v {
                        "modified" => SortBy::Modified,
                        "type" => SortBy::Type,
                        _ => SortBy::Name,
                    }
                }
                _ => {}
            }
        }
        p
    }

    fn save(&self) {
        let sort = match self.sort {
            SortBy::Name => "name",
            SortBy::Modified => "modified",
            SortBy::Type => "type",
        };
        let mut out = format!("icon_size={}\nauto_arrange={}\nsort={sort}\n", self.icon, self.auto_arrange);
        for (name, (c, r)) in &self.positions {
            out += &format!("pos={name}\t{c}\t{r}\n");
        }
        let path = Self::path();
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(path, out);
    }

    fn cell_w(&self) -> i32 {
        self.icon as i32 + 36
    }

    fn cell_h(&self) -> i32 {
        self.icon as i32 + 50
    }
}

// ---------------------------------------------------------------- items

struct Item {
    name: String,
    /// The file on disk (Recycle Bin: the trash folder).
    path: PathBuf,
    open: Open,
    icon: Option<Icon>,
    modified: Option<SystemTime>,
    kind: String,
    /// Grid cell (column, row).
    cell: (i32, i32),
    /// Can be renamed, deleted or moved into folders (not the Recycle Bin).
    real: bool,
}

enum Open {
    Folder(PathBuf),
    File(PathBuf),
    Launcher(String),
}

#[derive(Clone, PartialEq)]
enum MenuAction {
    Open,
    Rename,
    Delete,
    Refresh,
    NewFolder,
    Personalize,
    Terminal,
    About,
    IconSize(u32),
    AutoArrange,
    Sort(SortBy),
}

struct Menu {
    x: i32,
    y: i32,
    items: Vec<(String, Option<MenuAction>)>,
    hover: Option<usize>,
}

/// Left button held on an item: maybe a click, maybe the start of a drag.
struct Press {
    start: (f64, f64),
    item: usize,
    dragging: bool,
}

struct Desktop {
    registry: RegistryState,
    seat: SeatState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
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
    prefs: Prefs,
    hover: Option<usize>,
    selected: Vec<usize>,
    /// Where Shift-click ranges start.
    anchor: Option<usize>,
    press: Option<Press>,
    /// Rubber-band selection: (start, current).
    rubber: Option<((f64, f64), (f64, f64))>,
    renaming: Option<(usize, String)>,
    menu: Option<Menu>,
    mouse: (f64, f64),
    last_click: Option<(usize, Instant)>,
}

impl Desktop {
    fn rows(&self) -> i32 {
        ((self.size.1 as i32 - MARGIN * 2 - TASKBAR_H) / self.prefs.cell_h()).max(1)
    }

    fn cols(&self) -> i32 {
        ((self.size.0 as i32 - MARGIN * 2) / self.prefs.cell_w()).max(1)
    }

    fn rescan(&mut self) {
        let dir = desktop_dir();
        self.desktop_mtime = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
        let size = self.prefs.icon;
        let mut items = vec![Item {
            name: "Recycle Bin".into(),
            path: trash_dir(),
            open: Open::Folder(trash_dir()),
            icon: load_icon("user-trash", size),
            modified: None,
            kind: String::new(),
            cell: (0, 0),
            real: false,
        }];
        let entries: Vec<_> = std::fs::read_dir(&dir).map(|rd| rd.flatten().collect()).unwrap_or_default();
        for e in entries {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let modified = e.metadata().and_then(|m| m.modified()).ok();
            let kind = if path.is_dir() { " folder".into() } else { path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default() };
            let (display, open, icon) = if path.is_dir() {
                (name.clone(), Open::Folder(path.clone()), load_icon("folder", size))
            } else if name.ends_with(".desktop") {
                let (dname, exec, icon) = parse_launcher(&path);
                let icon = icon.and_then(|i| load_icon(&i, size)).or_else(|| load_icon("application-x-executable", size));
                (dname.unwrap_or_else(|| name.trim_end_matches(".desktop").to_string()), Open::Launcher(exec.unwrap_or_default()), icon)
            } else {
                let icon = load_icon(&mime_icon(&path), size).or_else(|| load_icon("text-x-generic", size));
                (name.clone(), Open::File(path.clone()), icon)
            };
            items.push(Item { name: display, path, open, icon, modified, kind, cell: (0, 0), real: true });
        }
        // Keep the Recycle Bin first; sort the rest.
        let sort = self.prefs.sort;
        items[1..].sort_by(|a, b| match sort {
            SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortBy::Modified => b.modified.cmp(&a.modified),
            SortBy::Type => a.kind.cmp(&b.kind).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        });
        self.items = items;
        self.selected.clear();
        self.hover = None;
        self.renaming = None;
        self.arrange();
        self.dirty = true;
    }

    /// The key we remember an item's position under.
    fn pos_key(item: &Item) -> String {
        if item.real { item.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { "::recycle-bin".into() }
    }

    /// Put every item in a cell: remembered positions first (unless auto-arranging),
    /// then the rest fill free cells top-to-bottom, left-to-right.
    fn arrange(&mut self) {
        let (cols, rows) = (self.cols(), self.rows());
        let mut taken = std::collections::HashSet::new();
        let mut unplaced = Vec::new();
        for i in 0..self.items.len() {
            let saved = if self.prefs.auto_arrange { None } else { self.prefs.positions.get(&Self::pos_key(&self.items[i])).copied() };
            match saved.filter(|&(c, r)| c < cols && r < rows && !taken.contains(&(c, r))) {
                Some(cell) => {
                    self.items[i].cell = cell;
                    taken.insert(cell);
                }
                None => unplaced.push(i),
            }
        }
        let mut next = (0..cols).flat_map(|c| (0..rows).map(move |r| (c, r))).filter(|c| !taken.contains(c));
        for i in unplaced {
            self.items[i].cell = next.next().unwrap_or((cols - 1, rows - 1));
        }
    }

    fn remember_positions(&mut self) {
        self.prefs.positions = self.items.iter().map(|it| (Self::pos_key(it), it.cell)).collect();
        self.prefs.save();
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
        if dm != self.desktop_mtime && self.renaming.is_none() {
            self.rescan();
        }
    }

    fn cell_xy(&self, cell: (i32, i32)) -> (i32, i32) {
        (MARGIN + cell.0 * self.prefs.cell_w(), MARGIN + cell.1 * self.prefs.cell_h())
    }

    fn cell_at(&self, x: f64, y: f64) -> (i32, i32) {
        let c = ((x as i32 - MARGIN) / self.prefs.cell_w()).clamp(0, self.cols() - 1);
        let r = ((y as i32 - MARGIN) / self.prefs.cell_h()).clamp(0, self.rows() - 1);
        (c, r)
    }

    fn item_rect(&self, i: usize) -> (i32, i32, i32, i32) {
        let (x, y) = self.cell_xy(self.items[i].cell);
        (x, y, self.prefs.cell_w() - 4, self.prefs.cell_h() - 4)
    }

    fn item_at(&self, x: f64, y: f64) -> Option<usize> {
        (0..self.items.len()).find(|&i| {
            let (ix, iy, w, h) = self.item_rect(i);
            x >= ix as f64 && x < (ix + w) as f64 && y >= iy as f64 && y < (iy + h) as f64
        })
    }

    fn menu_item_at(&self, x: f64, y: f64) -> Option<usize> {
        let m = self.menu.as_ref()?;
        let (lx, ly) = (x as i32 - m.x, y as i32 - m.y - 4);
        (lx >= 0 && lx < MENU_W && ly >= 0)
            .then(|| (ly / MENU_ROW) as usize)
            .filter(|&i| m.items.get(i).is_some_and(|(_, a)| a.is_some()))
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

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected.iter().filter_map(|&i| self.items.get(i)).filter(|it| it.real).map(|it| it.path.clone()).collect()
    }

    fn delete_selected(&mut self) {
        let paths = self.selected_paths();
        if !paths.is_empty() {
            let _ = Command::new("gio").arg("trash").args(&paths).status();
            self.rescan();
        }
    }

    fn start_rename(&mut self) {
        if let Some(&i) = self.selected.first().filter(|&&i| self.items[i].real) {
            let name = self.items[i].path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            self.renaming = Some((i, name));
            self.dirty = true;
        }
    }

    fn commit_rename(&mut self) {
        let Some((i, new)) = self.renaming.take() else { return };
        let new = new.trim().to_string();
        let Some(item) = self.items.get(i) else { return };
        if !new.is_empty() && !new.contains('/') {
            let target = desktop_dir().join(&new);
            if target != item.path && !target.exists() {
                let old_key = Self::pos_key(item);
                if std::fs::rename(&item.path, &target).is_ok() {
                    // Keep its place on the grid under the new name.
                    if let Some(pos) = self.prefs.positions.remove(&old_key) {
                        self.prefs.positions.insert(new, pos);
                        self.prefs.save();
                    }
                }
            }
        }
        self.rescan();
    }

    /// Move dragged items: onto a folder (or the bin) moves the files; elsewhere
    /// moves the icons by the same number of cells.
    fn drop_items(&mut self, from: usize, at: (f64, f64)) {
        if let Some(target) = self.item_at(at.0, at.1).filter(|t| !self.selected.contains(t)) {
            let paths = self.selected_paths();
            if paths.is_empty() {
                return;
            }
            if !self.items[target].real {
                let _ = Command::new("gio").arg("trash").args(&paths).status();
            } else if let Open::Folder(dir) = &self.items[target].open {
                for p in &paths {
                    if let Some(name) = p.file_name() {
                        let dst = dir.join(name);
                        if !dst.exists() {
                            let _ = std::fs::rename(p, &dst);
                        }
                    }
                }
            } else {
                return;
            }
            self.rescan();
            return;
        }
        let dest = self.cell_at(at.0, at.1);
        let (fc, fr) = self.items[from].cell;
        let (dc, dr) = (dest.0 - fc, dest.1 - fr);
        if (dc, dr) == (0, 0) {
            return;
        }
        let (cols, rows) = (self.cols(), self.rows());
        let moving: Vec<usize> = self.selected.clone();
        let mut taken: std::collections::HashSet<(i32, i32)> =
            (0..self.items.len()).filter(|i| !moving.contains(i)).map(|i| self.items[i].cell).collect();
        for &i in &moving {
            let (c, r) = self.items[i].cell;
            let mut cell = ((c + dc).clamp(0, cols - 1), (r + dr).clamp(0, rows - 1));
            // Occupied: take the next free cell down the column.
            let mut tries = 0;
            while taken.contains(&cell) && tries < cols * rows {
                cell.1 += 1;
                if cell.1 >= rows {
                    cell = ((cell.0 + 1) % cols, 0);
                }
                tries += 1;
            }
            taken.insert(cell);
            self.items[i].cell = cell;
        }
        // Placing icons by hand turns auto-arrange off, like Windows.
        self.prefs.auto_arrange = false;
        self.remember_positions();
    }

    fn select_click(&mut self, i: usize) {
        if self.mods.ctrl {
            if let Some(p) = self.selected.iter().position(|&s| s == i) {
                self.selected.remove(p);
            } else {
                self.selected.push(i);
            }
            self.anchor = Some(i);
        } else if self.mods.shift && self.anchor.is_some() {
            // Range in grid order (column by column), like Explorer.
            let a = self.anchor.unwrap();
            let order = |k: usize| self.items[k].cell;
            let (lo, hi) = if order(a) <= order(i) { (order(a), order(i)) } else { (order(i), order(a)) };
            self.selected = (0..self.items.len()).filter(|&k| order(k) >= lo && order(k) <= hi).collect();
        } else if !self.selected.contains(&i) {
            self.selected = vec![i];
            self.anchor = Some(i);
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
                for &i in &self.selected.clone() {
                    self.open(i);
                }
            }
            MenuAction::Rename => self.start_rename(),
            MenuAction::Delete => self.delete_selected(),
            MenuAction::Refresh => self.rescan(),
            MenuAction::NewFolder => {
                let dir = desktop_dir();
                let mut name = "New folder".to_string();
                let mut n = 2;
                while dir.join(&name).exists() {
                    name = format!("New folder ({n})");
                    n += 1;
                }
                let _ = std::fs::create_dir_all(dir.join(&name));
                self.rescan();
                // Straight into renaming it, like Windows.
                if let Some(i) = self.items.iter().position(|it| it.real && it.path.file_name().is_some_and(|f| f == name.as_str())) {
                    self.selected = vec![i];
                    self.start_rename();
                }
            }
            MenuAction::Personalize => spawn("vide-settings", &["--page", "personalization"]),
            MenuAction::Terminal => spawn("powerslop", &[]),
            MenuAction::About => spawn("vide-about", &[]),
            MenuAction::IconSize(s) => {
                self.prefs.icon = s;
                self.prefs.save();
                self.rescan();
            }
            MenuAction::AutoArrange => {
                self.prefs.auto_arrange = !self.prefs.auto_arrange;
                self.prefs.save();
                self.rescan();
            }
            MenuAction::Sort(s) => {
                // Sorting lays icons out afresh in the new order.
                self.prefs.sort = s;
                self.prefs.positions.clear();
                self.prefs.save();
                self.rescan();
            }
        }
        self.dirty = true;
    }

    fn pointer(&mut self, ev: &PointerEvent) {
        let (x, y) = ev.position;
        self.mouse = (x, y);
        match ev.kind {
            PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                if let Some(p) = self.press.as_mut() {
                    if !p.dragging && ((x - p.start.0).powi(2) + (y - p.start.1).powi(2)).sqrt() > DRAG_START {
                        p.dragging = true;
                    }
                    self.dirty = true;
                    return;
                }
                if let Some((start, _)) = self.rubber {
                    self.rubber = Some((start, (x, y)));
                    let (x0, x1) = (start.0.min(x), start.0.max(x));
                    let (y0, y1) = (start.1.min(y), start.1.max(y));
                    let base: Vec<usize> = if self.mods.ctrl { self.selected.clone() } else { Vec::new() };
                    let mut sel = base;
                    for i in 0..self.items.len() {
                        let (ix, iy, w, h) = self.item_rect(i);
                        let hit = (ix as f64) < x1 && ((ix + w) as f64) > x0 && (iy as f64) < y1 && ((iy + h) as f64) > y0;
                        if hit && !sel.contains(&i) {
                            sel.push(i);
                        }
                    }
                    self.selected = sel;
                    self.dirty = true;
                    return;
                }
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
                self.dirty = true;
                if self.renaming.is_some() {
                    self.commit_rename();
                }
                // An open menu takes the next click.
                if self.menu.is_some() {
                    let picked = self.menu_item_at(x, y).and_then(|i| self.menu.as_ref().and_then(|m| m.items[i].1.clone()));
                    self.menu = None;
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
                        self.select_click(i);
                        if double {
                            self.last_click = None;
                            self.open(i);
                        } else {
                            self.last_click = Some((i, now));
                            if self.selected.contains(&i) {
                                self.press = Some(Press { start: (x, y), item: i, dragging: false });
                            }
                        }
                    }
                    (BTN_LEFT, None) => {
                        if !self.mods.ctrl {
                            self.selected.clear();
                        }
                        self.rubber = Some(((x, y), (x, y)));
                    }
                    (BTN_RIGHT, Some(i)) => {
                        if !self.selected.contains(&i) {
                            self.selected = vec![i];
                            self.anchor = Some(i);
                        }
                        let mut items = vec![("Open".to_string(), Some(MenuAction::Open))];
                        if self.items[i].real {
                            items.push((String::new(), None));
                            items.push(("Rename\tF2".into(), Some(MenuAction::Rename)));
                            items.push(("Delete\tDel".into(), Some(MenuAction::Delete)));
                        }
                        self.open_menu(x, y, items);
                    }
                    (BTN_RIGHT, None) => {
                        self.selected.clear();
                        let check = |on: bool, s: &str| format!("{}{s}", if on { "✓  " } else { "     " });
                        let p = &self.prefs;
                        let items = vec![
                            (check(p.icon >= 64, "Large icons"), Some(MenuAction::IconSize(64))),
                            (check(p.icon == 48, "Medium icons"), Some(MenuAction::IconSize(48))),
                            (check(p.icon <= 32, "Small icons"), Some(MenuAction::IconSize(32))),
                            (check(p.auto_arrange, "Auto arrange icons"), Some(MenuAction::AutoArrange)),
                            (String::new(), None),
                            (check(p.sort == SortBy::Name, "Sort by name"), Some(MenuAction::Sort(SortBy::Name))),
                            (check(p.sort == SortBy::Modified, "Sort by date modified"), Some(MenuAction::Sort(SortBy::Modified))),
                            (check(p.sort == SortBy::Type, "Sort by type"), Some(MenuAction::Sort(SortBy::Type))),
                            (String::new(), None),
                            ("Refresh".into(), Some(MenuAction::Refresh)),
                            ("New folder".into(), Some(MenuAction::NewFolder)),
                            ("Open in PowerSlop".into(), Some(MenuAction::Terminal)),
                            (String::new(), None),
                            ("Personalize".into(), Some(MenuAction::Personalize)),
                            ("About VIDE-DE".into(), Some(MenuAction::About)),
                        ];
                        self.open_menu(x, y, items);
                    }
                    _ => {}
                }
            }
            PointerEventKind::Release { button: BTN_LEFT, .. } => {
                if let Some(p) = self.press.take() {
                    if p.dragging {
                        self.drop_items(p.item, (x, y));
                    }
                }
                self.rubber = None;
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn key(&mut self, ev: &KeyEvent) {
        self.dirty = true;
        if let Some((_, text)) = self.renaming.as_mut() {
            match ev.keysym {
                Keysym::Return | Keysym::KP_Enter => self.commit_rename(),
                Keysym::Escape => self.renaming = None,
                Keysym::BackSpace => {
                    text.pop();
                }
                _ => {
                    if let Some(s) = ev.utf8.as_ref().filter(|s| s.chars().all(|c| !c.is_control())) {
                        text.push_str(s);
                    }
                }
            }
            return;
        }
        match ev.keysym {
            Keysym::Delete => self.delete_selected(),
            Keysym::F2 => self.start_rename(),
            Keysym::F5 => self.rescan(),
            Keysym::Return | Keysym::KP_Enter => {
                for i in self.selected.clone() {
                    self.open(i);
                }
            }
            Keysym::Escape => {
                self.selected.clear();
                self.menu = None;
            }
            Keysym::a | Keysym::A if self.mods.ctrl => self.selected = (0..self.items.len()).collect(),
            Keysym::Up | Keysym::Down | Keysym::Left | Keysym::Right => {
                // Move the selection to the nearest icon in that direction.
                let cur = self.selected.last().copied().unwrap_or(0);
                let (c, r) = self.items.get(cur).map(|it| it.cell).unwrap_or((0, 0));
                let (dc, dr) = match ev.keysym {
                    Keysym::Up => (0, -1),
                    Keysym::Down => (0, 1),
                    Keysym::Left => (-1, 0),
                    _ => (1, 0),
                };
                let best = (0..self.items.len())
                    .filter(|&i| {
                        let (ic, ir) = self.items[i].cell;
                        (dc != 0 && (ic - c).signum() == dc && ir == r) || (dr != 0 && (ir - r).signum() == dr && ic == c)
                    })
                    .min_by_key(|&i| {
                        let (ic, ir) = self.items[i].cell;
                        (ic - c).abs() + (ir - r).abs()
                    });
                if let Some(b) = best {
                    self.selected = vec![b];
                    self.anchor = Some(b);
                }
            }
            _ => {}
        }
    }

    fn open_menu(&mut self, x: f64, y: f64, items: Vec<(String, Option<MenuAction>)>) {
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
            self.wallpaper = Some(((w, h), gfx::load_wallpaper(&self.config.wallpaper, w, h)));
        }
        let rects: Vec<(i32, i32, i32, i32)> = (0..self.items.len()).map(|i| self.item_rect(i)).collect();
        let icon_px = self.prefs.icon as i32;
        let cell_w = self.prefs.cell_w();
        // While dragging: ghost offset, and where the drop would land.
        let drag = self.press.as_ref().filter(|p| p.dragging).map(|p| ((self.mouse.0 - p.start.0) as i32, (self.mouse.1 - p.start.1) as i32));
        let drop_target = drag.and_then(|_| self.item_at(self.mouse.0, self.mouse.1)).filter(|t| !self.selected.contains(t));

        let stride = w as i32 * 4;
        let Ok((buffer, canvas)) = self.pool.create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888) else { return };
        let mut c = Canvas { w: w as i32, h: h as i32, px: canvas };
        c.px.copy_from_slice(&self.wallpaper.as_ref().unwrap().1);

        for (i, &(x, y, iw, ih)) in rects.iter().enumerate() {
            let selected = self.selected.contains(&i);
            let item = &self.items[i];
            if selected {
                c.rect(x, y, iw, ih, [0xd7, 0x78, 0x00, 0x66]);
                c.outline(x, y, iw, ih, [0xff, 0xc0, 0x80, 0xaa]);
            } else if self.hover == Some(i) || drop_target == Some(i) {
                c.rect(x, y, iw, ih, [0xff, 0xff, 0xff, 0x33]);
            }
            // Dragged icons are drawn at the pointer instead (below).
            if selected && drag.is_some() {
                continue;
            }
            draw_item(&mut c, self.font.as_ref(), item, x, y, icon_px, cell_w, self.renaming.as_ref().filter(|(ri, _)| *ri == i).map(|(_, t)| t.as_str()));
        }

        if let Some((dx, dy)) = drag {
            for &i in &self.selected {
                let (x, y, _, _) = rects[i];
                draw_item(&mut c, self.font.as_ref(), &self.items[i], x + dx, y + dy, icon_px, cell_w, None);
            }
        }

        if let Some((a, b)) = self.rubber {
            let (x0, y0) = (a.0.min(b.0) as i32, a.1.min(b.1) as i32);
            let (x1, y1) = (a.0.max(b.0) as i32, a.1.max(b.1) as i32);
            c.rect(x0, y0, x1 - x0, y1 - y0, [0xd7, 0x78, 0x00, 0x40]);
            c.outline(x0, y0, x1 - x0, y1 - y0, [0xd7, 0x78, 0x00, 0xcc]);
        }

        if let Some(m) = &self.menu {
            let mh = m.items.len() as i32 * MENU_ROW + 8;
            c.rect(m.x, m.y, MENU_W, mh, [0x2b, 0x2b, 0x2b, 0xf8]);
            c.outline(m.x, m.y, MENU_W, mh, [0x4a, 0x4a, 0x4a, 0xff]);
            for (i, (label, action)) in m.items.iter().enumerate() {
                let ry = m.y + 4 + i as i32 * MENU_ROW;
                if action.is_none() {
                    c.rect(m.x + 12, ry + MENU_ROW / 2, MENU_W - 24, 1, [0x4a, 0x4a, 0x4a, 0xff]);
                    continue;
                }
                if m.hover == Some(i) {
                    c.rect(m.x + 1, ry, MENU_W - 2, MENU_ROW, [0xff, 0xff, 0xff, 0x1f]);
                }
                if let Some(f) = &self.font {
                    let (text, shortcut) = label.split_once('\t').unwrap_or((label, ""));
                    c.text(f, m.x + 14, ry + 8, text, [0xff, 0xff, 0xff, 0xff]);
                    if !shortcut.is_empty() {
                        let sw = text_width(f, shortcut);
                        c.text(f, m.x + MENU_W - 14 - sw, ry + 8, shortcut, [0xa0, 0xa0, 0xa0, 0xff]);
                    }
                }
            }
        }

        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, w as i32, h as i32);
        let _ = buffer.attach_to(surface);
        self.layer.commit();
    }
}

/// One desktop icon: picture plus a centred two-line label (or the rename box).
#[allow(clippy::too_many_arguments)]
fn draw_item(c: &mut Canvas, font: Option<&fontdue::Font>, item: &Item, x: i32, y: i32, icon_px: i32, cell_w: i32, renaming: Option<&str>) {
    if let Some(icon) = &item.icon {
        c.image(icon, x + (cell_w - 4 - icon.w as i32) / 2, y + 6);
    }
    let Some(f) = font else { return };
    let ty = y + 6 + icon_px + 4;
    if let Some(text) = renaming {
        // Inline rename box, like Explorer's.
        let shown = format!("{text}|");
        let tw = text_width(f, &shown).max(40);
        let bx = x + (cell_w - 4 - tw) / 2 - 4;
        c.rect(bx, ty - 2, tw + 8, 20, [0xff, 0xff, 0xff, 0xff]);
        c.outline(bx, ty - 2, tw + 8, 20, [0xd7, 0x78, 0x00, 0xff]);
        c.text(f, bx + 4, ty, &shown, [0, 0, 0, 0xff]);
        return;
    }
    for (li, line) in wrap(f, &item.name, (cell_w - 10) as f32, 2).iter().enumerate() {
        let lw = text_width(f, line);
        let tx = x + (cell_w - 4 - lw) / 2;
        let ly = ty + li as i32 * 16;
        c.text(f, tx + 1, ly + 1, line, [0, 0, 0, 0xc0]);
        c.text(f, tx, ly, line, [0xff, 0xff, 0xff, 0xff]);
    }
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
            self.arrange();
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
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
        if capability == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
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

impl KeyboardHandler for Desktop {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {
        if self.renaming.is_some() {
            self.commit_rename();
        }
    }
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key(&event);
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key(&event);
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: RawModifiers, _: u32) {
        self.mods = modifiers;
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
