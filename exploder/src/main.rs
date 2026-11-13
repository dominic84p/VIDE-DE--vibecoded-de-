//! File Exploder: VIDE-DE's file manager, a copy of Windoze 10 File Explorer.
//! Ribbon (File / Home / Share / View, plus Manage in the Recycle Bin), back/forward/up
//! and a breadcrumb address bar, a navigation tree (Quick access, This PC, Recycle Bin),
//! icon / list / details / tiles views with image thumbnails, a This PC page with drive
//! usage bars, Properties, and copies that run in the background with a progress dialog.
//!
//! Double-click opens (folders here, files with xdg-open). Programs and scripts run,
//! like .exe and .bat files do in Explorer: scripts in a PowerSlop window so their
//! output can be read, programs and AppImages directly, .desktop launchers via gio.
//! Keys: F2 rename, Delete to Recycle Bin, Shift+Delete permanently, Ctrl+C/X/V,
//! Ctrl+Shift+N new folder, Alt+Left/Right/Up, Backspace, F5, Alt+Enter properties,
//! Ctrl+F / Ctrl+E / F3 search, Ctrl+L / Alt+D address, Ctrl+F1 ribbon,
//! Ctrl+Shift+2..7 views, Ctrl+N new window, Ctrl+W close, arrows to move.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime},
};

use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Key, Modifiers, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

const WHITE: Color32 = Color32::WHITE;
const BORDER: Color32 = Color32::from_rgb(0xe5, 0xe5, 0xe5);
const RIBBON_LINE: Color32 = Color32::from_rgb(0xda, 0xdb, 0xdc);
const HOVER: Color32 = Color32::from_rgb(0xe5, 0xf3, 0xff);
const HOVER_LINE: Color32 = Color32::from_rgb(0xcc, 0xe8, 0xff);
const SELECTED: Color32 = Color32::from_rgb(0xcc, 0xe8, 0xff);
const SELECTED_LINE: Color32 = Color32::from_rgb(0x99, 0xd1, 0xff);
const PRESSED: Color32 = Color32::from_rgb(0xb8, 0xdc, 0xfa);
const TEXT: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
const DIM: Color32 = Color32::from_rgb(0x6d, 0x6d, 0x6d);
const FAINT: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);
const HEADER_TEXT: Color32 = Color32::from_rgb(0x4c, 0x60, 0x7a);
const FILE_BLUE: Color32 = Color32::from_rgb(0x19, 0x79, 0xca);
const FILE_BLUE_HOT: Color32 = Color32::from_rgb(0x29, 0x8a, 0xdb);
const GROUP_BLUE: Color32 = Color32::from_rgb(0x1e, 0x32, 0x87);
const STAR_BLUE: Color32 = Color32::from_rgb(0x2b, 0x8c, 0xe8);
const BAR_BLUE: Color32 = Color32::from_rgb(0x26, 0xa0, 0xda);
const BAR_RED: Color32 = Color32::from_rgb(0xda, 0x26, 0x26);
const BAR_BG: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
const BAR_LINE: Color32 = Color32::from_rgb(0xbc, 0xbc, 0xbc);
const FREE_GREY: Color32 = Color32::from_rgb(0xd6, 0xd6, 0xd6);
const COPY_GREEN: Color32 = Color32::from_rgb(0x06, 0xb0, 0x25);
const ERROR: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x1c);
const ROW_H: f32 = 22.0;
const THUMB: u32 = 96;

fn main() -> eframe::Result {
    let start = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(home);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("File Exploder")
            .with_app_id("file-exploder")
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([600.0, 380.0]),
        ..Default::default()
    };
    eframe::run_native(
        "File Exploder",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            // A file manager, not a document: dragging over names shouldn't highlight text.
            cc.egui_ctx.all_styles_mut(|s| s.interaction.selectable_labels = false);
            // Noto Sans is what the shell uses, and the nearest installed thing to Seg-whoa UI.
            if let Ok(data) = std::fs::read("/usr/share/fonts/noto/NotoSans-Regular.ttf") {
                cc.egui_ctx.add_font(egui::FontInsert::new(
                    "noto-sans",
                    egui::FontData::from_owned(data),
                    vec![egui::InsertFontFamily { family: egui::FontFamily::Proportional, priority: egui::FontPriority::Highest }],
                ));
            }
            Ok(Box::new(Exploder::new(cc.egui_ctx.clone(), start)))
        }),
    )
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// Where the window is. Explorer's special places aren't folders, so history holds these.
#[derive(Clone, PartialEq)]
enum Loc {
    ThisPc,
    Trash,
    Dir(PathBuf),
    /// Search results under a folder, for a query.
    Search(PathBuf, String),
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    is_link: bool,
    size: u64,
    /// Date modified, or date deleted in the Recycle Bin.
    modified: Option<SystemTime>,
    kind: String,
    icon: &'static str,
    /// Recycle Bin: the folder it was deleted from. Search results: the folder it's in.
    location: String,
}

#[derive(Clone, Copy, PartialEq)]
enum SortBy {
    Name,
    Modified,
    Kind,
    Size,
    Location,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    LargeIcons,
    MediumIcons,
    SmallIcons,
    List,
    Details,
    Tiles,
}

const VIEWS: [(View, &str, &str); 6] = [
    (View::LargeIcons, "Large icons", "view-list-icons"),
    (View::MediumIcons, "Medium icons", "view-list-icons"),
    (View::SmallIcons, "Small icons", "view-list-text"),
    (View::List, "List", "view-list-text"),
    (View::Details, "Details", "view-list-details"),
    (View::Tiles, "Tiles", "view-list-tree"),
];

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Home,
    Share,
    View,
    Manage,
}

#[derive(Clone)]
enum Clip {
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
}

enum Confirm {
    /// Shift+Delete, or Delete inside the Recycle Bin.
    Delete(Vec<PathBuf>),
    EmptyTrash(usize),
    /// A paste would overwrite things; the plan waits for Replace / Skip / Keep both.
    Conflict { plan: Vec<(PathBuf, PathBuf)>, cut: bool, clashes: usize },
}

#[derive(Clone, Copy)]
enum Answer {
    Yes,
    No,
    Replace,
    Skip,
    KeepBoth,
}

struct Props {
    paths: Vec<PathBuf>,
    title: String,
    icon: &'static str,
    kind: String,
    location: String,
    md: Option<std::fs::Metadata>,
    drive: Option<Drive>,
    /// Bytes, files, folders, finished. Filled in by a thread for folders.
    totals: Arc<Mutex<(u64, u64, u64, bool)>>,
    readonly: bool,
    readonly_was: bool,
}

/// A copy or move running on a thread, shown as Explorer's progress dialog.
#[derive(Default)]
struct Progress {
    total: u64,
    done: u64,
    items: usize,
    current: String,
    from: String,
    to: String,
    cut: bool,
    cancel: bool,
    finished: bool,
    error: Option<String>,
}

#[derive(Clone)]
struct Drive {
    label: String,
    path: PathBuf,
    fs: String,
    removable: bool,
    /// Free and total bytes.
    usage: Option<(u64, u64)>,
}

enum Act {
    Go(Loc),
    Back,
    BackTo(usize),
    Forward,
    Up,
    Refresh,
    Open(usize),
    OpenDefault(usize),
    RunInPowerSlop(usize),
    OpenPath(PathBuf),
    Terminal(PathBuf),
    NewWindow(PathBuf),
    Cut,
    Copy,
    Paste,
    PasteShortcut,
    CopyPath,
    Delete(bool),
    Rename,
    CommitRename,
    CancelRename,
    NewFolder,
    NewFile,
    Shortcut,
    SelectAll,
    SelectNone,
    Invert,
    Properties,
    PinSelected,
    Pin(PathBuf),
    Unpin(PathBuf),
    Restore(bool),
    EmptyTrash,
    Zip,
    Email,
    MoveTo(PathBuf),
    CopyTo(PathBuf),
    SetView(View),
    Sort(SortBy),
    SortAscending(bool),
    ToggleHidden,
    ToggleExt,
    ToggleNav,
    Search,
    Close,
}

struct Exploder {
    loc: Loc,
    entries: Vec<Entry>,
    /// Indices into `entries` in display order, after the search filter.
    shown: Vec<usize>,
    back: Vec<Loc>,
    forward: Vec<Loc>,
    selected: Vec<usize>,
    anchor: Option<usize>,
    scroll_to: Option<usize>,
    sort: (SortBy, bool),
    view: View,
    filter: String,
    address: Option<String>,
    renaming: Option<(usize, String)>,
    rename_focus: bool,
    clipboard: Option<Clip>,
    status: Option<String>,
    show_hidden: bool,
    show_ext: bool,
    nav_pane: bool,
    ribbon_open: bool,
    tab: Tab,
    pins: Vec<PathBuf>,
    qa_open: bool,
    pc_open: bool,
    pc_sel: Option<PathBuf>,
    drives: Vec<Drive>,
    confirm: Option<Confirm>,
    props: Option<Props>,
    job: Option<(Arc<Mutex<Progress>>, f64)>,
    /// Set false to stop the running search thread; it sets it false itself when done.
    searching: Arc<AtomicBool>,
    search_rx: Option<mpsc::Receiver<Entry>>,
    /// Background work finished and the folder should be re-read.
    dirty: Arc<AtomicBool>,
    notice: Arc<Mutex<Option<String>>>,
    dir_mtime: Option<SystemTime>,
    last_poll: f64,
    /// Items per row (grid views) or per column (list view), for arrow keys.
    stride: usize,
    col_w: [f32; 4],
    title: String,
    focus_search: bool,
    thumbs: HashMap<PathBuf, Option<egui::TextureHandle>>,
    thumbs_for: Option<Loc>,
    thumb_gen: Arc<AtomicUsize>,
    thumb_tx: mpsc::Sender<(usize, PathBuf)>,
    thumb_rx: mpsc::Receiver<(PathBuf, Option<egui::ColorImage>)>,
}

impl Exploder {
    fn new(ctx: egui::Context, start: PathBuf) -> Self {
        let thumb_gen = Arc::new(AtomicUsize::new(0));
        let (thumb_tx, thumb_rx) = thumb_worker(ctx, thumb_gen.clone());
        let mut s = Self {
            loc: Loc::Dir(start),
            entries: Vec::new(),
            shown: Vec::new(),
            back: Vec::new(),
            forward: Vec::new(),
            selected: Vec::new(),
            anchor: None,
            scroll_to: None,
            sort: (SortBy::Name, true),
            view: View::Details,
            filter: String::new(),
            address: None,
            renaming: None,
            rename_focus: false,
            clipboard: None,
            status: None,
            show_hidden: false,
            show_ext: true,
            nav_pane: true,
            ribbon_open: true,
            tab: Tab::Home,
            pins: load_pins(),
            qa_open: true,
            pc_open: false,
            pc_sel: None,
            drives: drives(),
            confirm: None,
            props: None,
            job: None,
            searching: Arc::new(AtomicBool::new(false)),
            search_rx: None,
            dirty: Arc::new(AtomicBool::new(false)),
            notice: Arc::new(Mutex::new(None)),
            dir_mtime: None,
            last_poll: 0.0,
            stride: 1,
            col_w: [280.0, 160.0, 140.0, 90.0],
            title: String::new(),
            focus_search: false,
            thumbs: HashMap::new(),
            thumbs_for: None,
            thumb_gen,
            thumb_tx,
            thumb_rx,
        };
        s.reload();
        s
    }

    fn reload(&mut self) {
        // Stop any search still feeding results for the old place.
        self.searching.store(false, Ordering::Relaxed);
        self.search_rx = None;
        self.entries.clear();
        self.shown.clear();
        self.selected.clear();
        self.anchor = None;
        self.renaming = None;
        self.status = None;
        self.pc_sel = None;
        if self.thumbs_for.as_ref() != Some(&self.loc) {
            // Thumbnails are textures; only keep the ones for the folder on screen.
            self.thumbs.clear();
            self.thumb_gen.fetch_add(1, Ordering::Relaxed);
            self.thumbs_for = Some(self.loc.clone());
        }
        match self.loc.clone() {
            Loc::ThisPc => self.drives = drives(),
            Loc::Trash => {
                self.dir_mtime = mtime(&trash_dir().join("files"));
                self.entries = trash_entries();
            }
            Loc::Dir(dir) => {
                self.dir_mtime = mtime(&dir);
                match std::fs::read_dir(&dir) {
                    Ok(rd) => {
                        for e in rd.flatten() {
                            if e.file_name().to_string_lossy().starts_with('.') && !self.show_hidden {
                                continue;
                            }
                            if let Some(en) = entry(e.path(), String::new()) {
                                self.entries.push(en);
                            }
                        }
                    }
                    Err(e) => self.status = Some(format!("Can't open {}: {e}", dir.display())),
                }
            }
            Loc::Search(root, q) => {
                self.filter = q.clone();
                self.start_search(root, q);
            }
        }
        self.sort_entries();
    }

    /// Re-read the folder but keep what was selected.
    fn refresh(&mut self) {
        let keep = self.selected_paths();
        self.reload();
        self.select_paths(&keep);
    }

    fn select_paths(&mut self, paths: &[PathBuf]) {
        self.selected = self.entries.iter().enumerate().filter(|(_, e)| paths.contains(&e.path)).map(|(i, _)| i).collect();
        self.anchor = self.selected.first().copied();
    }

    fn start_search(&mut self, root: PathBuf, query: String) {
        let running = Arc::new(AtomicBool::new(true));
        self.searching = running.clone();
        let (tx, rx) = mpsc::channel();
        self.search_rx = Some(rx);
        let q = query.to_lowercase();
        let hidden = self.show_hidden;
        std::thread::spawn(move || {
            let mut stack = vec![root];
            let mut found = 0;
            'walk: while let Some(dir) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&dir) else { continue };
                for e in rd.flatten() {
                    if !running.load(Ordering::Relaxed) {
                        return;
                    }
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') && !hidden {
                        continue;
                    }
                    // file_type doesn't follow symlinks, so linked folders can't loop us.
                    if e.file_type().is_ok_and(|t| t.is_dir()) {
                        stack.push(e.path());
                    }
                    if name.to_lowercase().contains(&q)
                        && let Some(en) = entry(e.path(), dir.display().to_string())
                    {
                        if tx.send(en).is_err() {
                            return;
                        }
                        found += 1;
                        if found >= 5000 {
                            break 'walk;
                        }
                    }
                }
            }
            running.store(false, Ordering::Relaxed);
        });
    }

    fn sort_entries(&mut self) {
        let keep = self.selected_paths();
        let renaming = self.renaming.as_ref().and_then(|(i, t)| Some((self.entries.get(*i)?.path.clone(), t.clone())));
        let (by, asc) = self.sort;
        self.entries.sort_by(|a, b| {
            // Folders first, like Explorer.
            let ord = b.is_dir.cmp(&a.is_dir).then_with(|| match by {
                SortBy::Name => natural_cmp(&a.name, &b.name),
                SortBy::Modified => a.modified.cmp(&b.modified),
                SortBy::Kind => a.kind.cmp(&b.kind),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Location => a.location.cmp(&b.location),
            });
            if asc || a.is_dir != b.is_dir { ord } else { ord.reverse() }
        });
        self.select_paths(&keep);
        self.renaming = renaming.and_then(|(p, t)| Some((self.entries.iter().position(|e| e.path == p)?, t)));
    }

    fn go(&mut self, loc: Loc) {
        if loc == self.loc {
            return;
        }
        if !matches!(loc, Loc::Search(..)) {
            self.filter.clear();
        }
        self.back.push(std::mem::replace(&mut self.loc, loc));
        self.forward.clear();
        self.reload();
    }

    fn go_back(&mut self, steps: usize) {
        for _ in 0..steps {
            let Some(l) = self.back.pop() else { break };
            self.forward.push(std::mem::replace(&mut self.loc, l));
        }
        self.filter.clear();
        self.reload();
    }

    fn go_forward(&mut self) {
        if let Some(l) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.loc, l));
            self.filter.clear();
            self.reload();
        }
    }

    fn parent_loc(&self) -> Option<Loc> {
        match &self.loc {
            Loc::ThisPc => None,
            Loc::Trash => Some(Loc::ThisPc),
            Loc::Search(root, _) => Some(Loc::Dir(root.clone())),
            Loc::Dir(p) => Some(p.parent().map(|p| Loc::Dir(p.to_path_buf())).unwrap_or(Loc::ThisPc)),
        }
    }

    /// The folder new things go in, if this is a real folder.
    fn here(&self) -> Option<PathBuf> {
        match &self.loc {
            Loc::Dir(p) => Some(p.clone()),
            _ => None,
        }
    }

    fn open(&mut self, i: usize) {
        let Some(e) = self.entries.get(i).cloned() else { return };
        if self.loc == Loc::Trash {
            // Explorer won't open things in the Recycle Bin, it shows their properties.
            self.selected = vec![i];
            self.show_props();
            return;
        }
        if e.is_dir {
            self.go(Loc::Dir(e.path));
            return;
        }
        let res = match runnable(&e.path) {
            Some(Run::Script(interp)) => run_in_terminal(&e.path, interp),
            Some(Run::Program) => run_program(&e.path),
            Some(Run::Launcher) => spawn(std::process::Command::new("gio").arg("launch").arg(&e.path)),
            None => spawn(std::process::Command::new("xdg-open").arg(&e.path)),
        };
        if let Err(err) = res {
            self.status = Some(format!("Couldn't open {}: {err}", e.name));
        }
    }

    /// Open with the default app even if it's runnable, e.g. to edit a script.
    fn open_default(&mut self, path: &Path) {
        if let Err(err) = spawn(std::process::Command::new("xdg-open").arg(path)) {
            self.status = Some(format!("Couldn't open {}: {err}", path.display()));
        }
    }

    fn run_in_powerslop(&mut self, i: usize) {
        let Some(e) = self.entries.get(i).cloned() else { return };
        let interp = match runnable(&e.path) {
            Some(Run::Script(interp)) => interp,
            _ => None,
        };
        if let Err(err) = run_in_terminal(&e.path, interp) {
            self.status = Some(format!("Couldn't run {}: {err}", e.name));
        }
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected.iter().filter_map(|&i| self.entries.get(i)).map(|e| e.path.clone()).collect()
    }

    /// What the ribbon acts on: the selection, or the highlighted tile on This PC.
    fn targets(&self) -> Vec<PathBuf> {
        match &self.loc {
            Loc::ThisPc => self.pc_sel.iter().cloned().collect(),
            _ => self.selected_paths(),
        }
    }

    fn delete(&mut self, permanent: bool) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        if permanent || self.loc == Loc::Trash {
            self.confirm = Some(Confirm::Delete(paths));
            return;
        }
        // To the trash; only Shift+Delete skips it, and that asks first.
        let res = std::process::Command::new("gio").arg("trash").arg("--").args(&paths).status();
        if !res.is_ok_and(|s| s.success()) {
            self.status = Some("Couldn't move to the Recycle Bin (is gio installed?)".into());
        }
        self.refresh();
    }

    fn delete_now(&mut self, paths: &[PathBuf]) {
        let files = trash_dir().join("files");
        for p in paths {
            if let Err(e) = remove_rec(p) {
                self.status = Some(format!("Couldn't delete {}: {e}", p.display()));
                continue;
            }
            if p.parent() == Some(files.as_path())
                && let Some(n) = p.file_name()
            {
                let _ = std::fs::remove_file(trash_info(&n.to_string_lossy()));
            }
        }
        self.refresh();
    }

    fn restore(&mut self, all: bool) {
        let idx: Vec<usize> = if all { (0..self.entries.len()).collect() } else { self.selected.clone() };
        for i in idx {
            let Some(e) = self.entries.get(i) else { continue };
            let Some(tname) = e.path.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
            let info = trash_info(&tname);
            let Some(orig) = std::fs::read_to_string(&info).ok().and_then(|s| s.lines().find_map(|l| l.strip_prefix("Path=").map(percent_decode))) else {
                self.status = Some(format!("Don't know where {} came from", e.name));
                continue;
            };
            let orig = PathBuf::from(orig);
            if orig.exists() {
                self.status = Some(format!("Couldn't restore {}: {} already exists", e.name, orig.display()));
                continue;
            }
            if let Some(parent) = orig.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let res = std::fs::rename(&e.path, &orig).or_else(|_| {
                let p = Mutex::new(Progress::default());
                copy_tree(&e.path, &orig, &p).and_then(|_| remove_rec(&e.path))
            });
            match res {
                Ok(()) => {
                    let _ = std::fs::remove_file(&info);
                }
                Err(err) => self.status = Some(format!("Couldn't restore {}: {err}", e.name)),
            }
        }
        self.refresh();
    }

    fn empty_trash(&mut self) {
        for sub in ["files", "info"] {
            if let Ok(rd) = std::fs::read_dir(trash_dir().join(sub)) {
                for e in rd.flatten() {
                    let _ = remove_rec(&e.path());
                }
            }
        }
        self.refresh();
    }

    fn new_folder(&mut self) {
        let Some(dir) = self.here() else { return };
        let path = unique(&dir.join("New folder"), |stem, ext, n| format!("{stem} ({n}){ext}"));
        if let Err(e) = std::fs::create_dir(&path) {
            self.status = Some(format!("Couldn't create folder: {e}"));
            return;
        }
        self.reload();
        self.start_rename_path(&path);
    }

    fn new_file(&mut self) {
        let Some(dir) = self.here() else { return };
        let path = unique(&dir.join("New Text Document.txt"), |stem, ext, n| format!("{stem} ({n}){ext}"));
        if let Err(e) = std::fs::File::create_new(&path) {
            self.status = Some(format!("Couldn't create file: {e}"));
            return;
        }
        self.reload();
        self.start_rename_path(&path);
    }

    fn start_rename_path(&mut self, path: &Path) {
        if let Some(i) = self.entries.iter().position(|e| e.path == path) {
            self.selected = vec![i];
            self.anchor = Some(i);
            self.scroll_to = Some(i);
            self.start_rename(i);
        }
    }

    fn start_rename(&mut self, i: usize) {
        if self.loc == Loc::Trash {
            return;
        }
        if let Some(e) = self.entries.get(i) {
            self.renaming = Some((i, self.display_name(e)));
            self.rename_focus = true;
        }
    }

    fn commit_rename(&mut self) {
        let Some((i, new)) = self.renaming.take() else { return };
        let Some(e) = self.entries.get(i) else { return };
        let mut new = new.trim().to_string();
        // With extensions hidden the box didn't show it, so keep the old one.
        if !self.show_ext && !e.is_dir && self.display_name(e) != e.name
            && let Some(ext) = e.path.extension()
        {
            new = format!("{new}.{}", ext.to_string_lossy());
        }
        if new.is_empty() || new == e.name {
            return;
        }
        if new.contains('/') {
            self.status = Some("A file name can't contain /".into());
            return;
        }
        let dst = e.path.with_file_name(&new);
        if dst.exists() {
            self.status = Some(format!("There is already a file named {new} in this location"));
            return;
        }
        if let Err(err) = std::fs::rename(&e.path, &dst) {
            self.status = Some(format!("Couldn't rename: {err}"));
            return;
        }
        self.reload();
        self.select_paths(&[dst]);
    }

    /// Copy or move `srcs` into `dir`, asking first if anything would be overwritten.
    fn transfer(&mut self, srcs: Vec<PathBuf>, dir: PathBuf, cut: bool) {
        let mut plan = Vec::new();
        let mut clashes = 0;
        for src in srcs {
            let Some(name) = src.file_name() else { continue };
            let mut dst = dir.join(name);
            if dst == src {
                if cut {
                    continue;
                }
                // Explorer's "name - Copy" when copying into the same folder.
                dst = unique(&dst, |stem, ext, n| if n == 2 { format!("{stem} - Copy{ext}") } else { format!("{stem} - Copy ({}){ext}", n - 1) });
            } else if dir.starts_with(&src) {
                self.status = Some(format!("The destination folder is inside {}", src.display()));
                continue;
            } else if std::fs::symlink_metadata(&dst).is_ok() {
                clashes += 1;
            }
            plan.push((src, dst));
        }
        if clashes > 0 {
            self.confirm = Some(Confirm::Conflict { plan, cut, clashes });
        } else {
            self.start_job(plan, cut);
        }
    }

    fn paste(&mut self) {
        let (Some(dir), Some(clip)) = (self.here(), self.clipboard.clone()) else { return };
        match clip {
            Clip::Copy(p) => self.transfer(p, dir, false),
            Clip::Cut(p) => {
                self.clipboard = None;
                self.transfer(p, dir, true);
            }
        }
    }

    fn paste_shortcut(&mut self) {
        let (Some(dir), Some(Clip::Copy(paths) | Clip::Cut(paths))) = (self.here(), self.clipboard.clone()) else { return };
        for src in paths {
            let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dst = unique(&dir.join(format!("{name} - Shortcut")), |stem, ext, n| format!("{stem} ({n}){ext}"));
            if let Err(e) = std::os::unix::fs::symlink(&src, &dst) {
                self.status = Some(format!("Couldn't create shortcut: {e}"));
            }
        }
        self.refresh();
    }

    fn create_shortcut(&mut self) {
        for src in self.selected_paths() {
            let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dst = unique(&src.with_file_name(format!("{name} - Shortcut")), |stem, ext, n| format!("{stem} ({n}){ext}"));
            if let Err(e) = std::os::unix::fs::symlink(&src, &dst) {
                self.status = Some(format!("Couldn't create shortcut: {e}"));
            }
        }
        self.refresh();
    }

    fn resolve_conflict(&mut self, plan: Vec<(PathBuf, PathBuf)>, cut: bool, answer: Answer) {
        let plan = plan
            .into_iter()
            .filter_map(|(s, d)| {
                if std::fs::symlink_metadata(&d).is_err() {
                    return Some((s, d));
                }
                match answer {
                    Answer::Replace => Some((s, d)),
                    Answer::KeepBoth => Some((s, unique(&d, |stem, ext, n| format!("{stem} ({n}){ext}")))),
                    _ => None,
                }
            })
            .collect();
        self.start_job(plan, cut);
    }

    fn start_job(&mut self, plan: Vec<(PathBuf, PathBuf)>, cut: bool) {
        if plan.is_empty() {
            return;
        }
        let folder = |p: Option<&Path>| p.and_then(Path::parent).and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into());
        let prog = Arc::new(Mutex::new(Progress {
            items: plan.len(),
            cut,
            from: folder(plan.first().map(|(s, _)| s.as_path())),
            to: folder(plan.first().map(|(_, d)| d.as_path())),
            ..Default::default()
        }));
        self.job = Some((prog.clone(), f64::NAN));
        let dirty = self.dirty.clone();
        std::thread::spawn(move || {
            transfer_job(&plan, cut, &prog);
            prog.lock().unwrap().finished = true;
            dirty.store(true, Ordering::Relaxed);
        });
    }

    fn zip(&mut self) {
        let Some(dir) = self.here() else { return };
        let names: Vec<String> = self.selected.iter().filter_map(|&i| self.entries.get(i)).map(|e| e.name.clone()).collect();
        let Some(first) = names.first() else { return };
        let stem = Path::new(first).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| first.clone());
        let dst = unique(&dir.join(format!("{stem}.zip")), |stem, ext, n| format!("{stem} ({n}){ext}"));
        let (dirty, notice) = (self.dirty.clone(), self.notice.clone());
        std::thread::spawn(move || {
            let ok = std::process::Command::new("bsdtar").arg("-a").arg("-cf").arg(&dst).arg("-C").arg(&dir).arg("--").args(&names).status().is_ok_and(|s| s.success());
            if !ok {
                *notice.lock().unwrap() = Some("Couldn't create the zip (is bsdtar installed?)".into());
            }
            dirty.store(true, Ordering::Relaxed);
        });
    }

    fn show_props(&mut self) {
        let paths = self.targets();
        if paths.is_empty() {
            if let Some(dir) = self.here() {
                self.props = Some(props_for(vec![dir], None));
            }
            return;
        }
        let drive = if self.loc == Loc::ThisPc { self.drives.iter().find(|d| Some(&d.path) == self.pc_sel.as_ref()).cloned() } else { None };
        self.props = Some(props_for(paths, drive));
    }

    fn display_name(&self, e: &Entry) -> String {
        if self.show_ext || e.is_dir {
            return e.name.clone();
        }
        match Path::new(&e.name).file_stem() {
            Some(s) if !e.name.starts_with('.') => s.to_string_lossy().into_owned(),
            _ => e.name.clone(),
        }
    }

    fn loc_name(&self, loc: &Loc) -> String {
        match loc {
            Loc::ThisPc => "This PC".into(),
            Loc::Trash => "Recycle Bin".into(),
            Loc::Dir(p) => dir_name(p),
            Loc::Search(root, q) => format!("{q} - Search Results in {}", dir_name(root)),
        }
    }

    fn click(&mut self, i: usize, m: Modifiers) {
        if m.shift
            && let Some(a) = self.anchor
        {
            let (pa, pi) = (self.shown.iter().position(|&s| s == a), self.shown.iter().position(|&s| s == i));
            if let (Some(pa), Some(pi)) = (pa, pi) {
                self.selected = self.shown[pa.min(pi)..=pa.max(pi)].to_vec();
                return;
            }
        }
        if m.ctrl {
            if let Some(p) = self.selected.iter().position(|&s| s == i) {
                self.selected.remove(p);
            } else {
                self.selected.push(i);
            }
        } else {
            self.selected = vec![i];
        }
        self.anchor = Some(i);
    }

    fn step(&mut self, delta: isize, extend: bool) {
        let n = self.shown.len();
        if n == 0 {
            return;
        }
        let cur = self.selected.last().and_then(|i| self.shown.iter().position(|s| s == i));
        let next = match cur {
            None => 0,
            Some(c) => (c as isize + delta).clamp(0, n as isize - 1) as usize,
        };
        let i = self.shown[next];
        if extend {
            self.click(i, Modifiers::SHIFT);
        } else {
            self.selected = vec![i];
            self.anchor = Some(i);
        }
        self.scroll_to = Some(i);
    }

    fn keys(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        // Typing in the address, search or rename box: those keys are theirs.
        if ctx.egui_wants_keyboard_input() || self.confirm.is_some() {
            return;
        }
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        // Shift-less patterns also match with Shift held, so the Shift ones go first.
        let cs = Modifiers::CTRL | Modifiers::SHIFT;
        for (k, v) in [(Key::Num2, View::LargeIcons), (Key::Num3, View::MediumIcons), (Key::Num4, View::SmallIcons), (Key::Num5, View::List), (Key::Num6, View::Details), (Key::Num7, View::Tiles)] {
            if pressed(cs, k) {
                acts.push(Act::SetView(v));
            }
        }
        if pressed(cs, Key::N) {
            acts.push(Act::NewFolder);
        }
        if pressed(Modifiers::SHIFT, Key::Delete) {
            acts.push(Act::Delete(true));
        }
        if pressed(Modifiers::NONE, Key::Delete) {
            acts.push(Act::Delete(false));
        }
        if pressed(Modifiers::ALT, Key::ArrowLeft) || pressed(Modifiers::NONE, Key::Backspace) {
            acts.push(Act::Back);
        }
        if pressed(Modifiers::ALT, Key::ArrowRight) {
            acts.push(Act::Forward);
        }
        if pressed(Modifiers::ALT, Key::ArrowUp) {
            acts.push(Act::Up);
        }
        if pressed(Modifiers::ALT, Key::Enter) {
            acts.push(Act::Properties);
        }
        if pressed(Modifiers::ALT, Key::D) || pressed(Modifiers::CTRL, Key::L) {
            self.address = Some(self.here().map(|p| p.display().to_string()).unwrap_or_else(|| self.loc_name(&self.loc)));
        }
        if pressed(Modifiers::CTRL, Key::F) || pressed(Modifiers::CTRL, Key::E) || pressed(Modifiers::NONE, Key::F3) {
            self.focus_search = true;
        }
        if pressed(Modifiers::CTRL, Key::F1) {
            self.ribbon_open = !self.ribbon_open;
        }
        if pressed(Modifiers::NONE, Key::F5) || pressed(Modifiers::CTRL, Key::R) {
            acts.push(Act::Refresh);
        }
        if pressed(Modifiers::NONE, Key::F2) {
            acts.push(Act::Rename);
        }
        if pressed(Modifiers::NONE, Key::Enter) {
            if let Some(&i) = self.selected.first() {
                acts.push(Act::Open(i));
            } else if let Some(p) = &self.pc_sel {
                acts.push(Act::Go(Loc::Dir(p.clone())));
            }
        }
        if pressed(Modifiers::CTRL, Key::A) {
            acts.push(Act::SelectAll);
        }
        if pressed(Modifiers::CTRL, Key::H) {
            acts.push(Act::ToggleHidden);
        }
        if pressed(Modifiers::CTRL, Key::N) {
            acts.push(Act::NewWindow(self.here().unwrap_or_else(home)));
        }
        if pressed(Modifiers::CTRL, Key::W) {
            acts.push(Act::Close);
        }
        if pressed(Modifiers::NONE, Key::Escape) {
            self.selected.clear();
        }
        let (across, down) = match self.view {
            View::Details => (0, 1),
            View::List => (self.stride as isize, 1),
            _ => (1, self.stride as isize),
        };
        let shift = ctx.input(|i| i.modifiers.shift);
        for (k, d) in [(Key::ArrowUp, -down), (Key::ArrowDown, down), (Key::ArrowLeft, -across), (Key::ArrowRight, across), (Key::Home, -100_000), (Key::End, 100_000)] {
            if d != 0 && pressed(Modifiers::NONE, k) {
                self.step(d, shift);
            }
        }
        // egui turns Ctrl+C/X/V into events rather than key presses.
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Copy => acts.push(Act::Copy),
                egui::Event::Cut => acts.push(Act::Cut),
                egui::Event::Paste(_) => acts.push(Act::Paste),
                _ => {}
            }
        }
    }

    fn apply(&mut self, ctx: &egui::Context, act: Act) {
        match act {
            Act::Go(l) => self.go(l),
            Act::Back => self.go_back(1),
            Act::BackTo(n) => self.go_back(n),
            Act::Forward => self.go_forward(),
            Act::Up => {
                if let Some(l) = self.parent_loc() {
                    let from = self.here();
                    self.go(l);
                    // Like Explorer, land on the folder we came out of.
                    if let Some(from) = from {
                        self.select_paths(&[from]);
                        self.scroll_to = self.selected.first().copied();
                    }
                }
            }
            Act::Refresh => {
                self.drives = drives();
                self.refresh();
            }
            Act::Open(i) => self.open(i),
            Act::OpenDefault(i) => {
                if let Some(p) = self.entries.get(i).map(|e| e.path.clone()) {
                    self.open_default(&p);
                }
            }
            Act::RunInPowerSlop(i) => self.run_in_powerslop(i),
            Act::OpenPath(p) => {
                if p.is_dir() {
                    self.go(Loc::Dir(p));
                } else {
                    self.open_default(&p);
                }
            }
            Act::Terminal(dir) => spawn_terminal(&dir),
            Act::NewWindow(dir) => {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = spawn(std::process::Command::new(exe).arg(dir));
                }
            }
            Act::Cut if self.loc != Loc::Trash && !self.selected.is_empty() => self.clipboard = Some(Clip::Cut(self.selected_paths())),
            Act::Copy if !self.targets().is_empty() => self.clipboard = Some(Clip::Copy(self.targets())),
            Act::Cut | Act::Copy => {}
            Act::Paste => self.paste(),
            Act::PasteShortcut => self.paste_shortcut(),
            Act::CopyPath => {
                let paths = self.targets();
                let paths = if paths.is_empty() { self.here().into_iter().collect() } else { paths };
                ctx.copy_text(paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n"));
            }
            Act::Delete(permanent) => self.delete(permanent),
            Act::Rename => {
                if let Some(&i) = self.selected.first() {
                    self.start_rename(i);
                }
            }
            Act::CommitRename => self.commit_rename(),
            Act::CancelRename => self.renaming = None,
            Act::NewFolder => self.new_folder(),
            Act::NewFile => self.new_file(),
            Act::Shortcut => self.create_shortcut(),
            Act::SelectAll => self.selected = self.shown.clone(),
            Act::SelectNone => self.selected.clear(),
            Act::Invert => self.selected = self.shown.iter().copied().filter(|i| !self.selected.contains(i)).collect(),
            Act::Properties => self.show_props(),
            Act::PinSelected => {
                let dirs: Vec<PathBuf> = self.targets().into_iter().filter(|p| p.is_dir()).collect();
                let dirs = if dirs.is_empty() { self.here().into_iter().collect() } else { dirs };
                for d in dirs {
                    self.apply(ctx, Act::Pin(d));
                }
            }
            Act::Pin(p) => {
                if !self.pins.contains(&p) {
                    self.pins.push(p);
                    save_pins(&self.pins);
                }
            }
            Act::Unpin(p) => {
                self.pins.retain(|x| *x != p);
                save_pins(&self.pins);
            }
            Act::Restore(all) => self.restore(all),
            Act::EmptyTrash => {
                if !self.entries.is_empty() || self.loc != Loc::Trash {
                    self.confirm = Some(Confirm::EmptyTrash(trash_entries().len()));
                }
            }
            Act::Zip => self.zip(),
            Act::Email => {
                let mut cmd = std::process::Command::new("xdg-email");
                for p in self.selected_paths().iter().filter(|p| p.is_file()) {
                    cmd.arg("--attach").arg(p);
                }
                if let Err(e) = spawn(&mut cmd) {
                    self.status = Some(format!("Couldn't start email: {e}"));
                }
            }
            Act::MoveTo(dir) => {
                let p = self.selected_paths();
                self.transfer(p, dir, true);
            }
            Act::CopyTo(dir) => {
                let p = self.targets();
                self.transfer(p, dir, false);
            }
            Act::SetView(v) => self.view = v,
            Act::Sort(by) => {
                self.sort = if self.sort.0 == by { (by, !self.sort.1) } else { (by, true) };
                self.sort_entries();
            }
            Act::SortAscending(asc) => {
                self.sort.1 = asc;
                self.sort_entries();
            }
            Act::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                self.refresh();
            }
            Act::ToggleExt => self.show_ext = !self.show_ext,
            Act::ToggleNav => self.nav_pane = !self.nav_pane,
            Act::Search => {
                let q = self.filter.trim().to_string();
                let root = match &self.loc {
                    Loc::Dir(p) | Loc::Search(p, _) => Some(p.clone()),
                    Loc::ThisPc => Some(home()),
                    Loc::Trash => None,
                };
                match root {
                    Some(root) if !q.is_empty() => {
                        if matches!(self.loc, Loc::Search(..)) {
                            // A new query replaces the results rather than piling up history.
                            self.loc = Loc::Search(root, q);
                            self.reload();
                        } else {
                            self.go(Loc::Search(root, q));
                        }
                    }
                    Some(root) if matches!(self.loc, Loc::Search(..)) => self.go(Loc::Dir(root)),
                    _ => {}
                }
            }
            Act::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    // ---- Ribbon ----

    fn ribbon(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        let in_trash = self.loc == Loc::Trash;
        if self.tab == Tab::Manage && !in_trash {
            self.tab = Tab::Home;
        }
        let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::hover());
        if self.ribbon_open {
            ui.painter().hline(strip.x_range(), strip.bottom() - 0.5, (1.0, RIBBON_LINE));
        }

        // The blue File tab opens the backstage-ish menu.
        let file = Rect::from_min_size(strip.min + vec2(0.0, 1.0), vec2(56.0, 24.0));
        let fr = ui.interact(file, Id::unique("file-tab"), Sense::click());
        ui.painter().rect_filled(file, 0.0, if fr.hovered() { FILE_BLUE_HOT } else { FILE_BLUE });
        ui.painter().text(file.center(), Align2::CENTER_CENTER, "File", FontId::proportional(12.5), WHITE);
        self.file_menu(&fr, acts);

        let mut x = file.right() + 2.0;
        let mut tabs = vec![(Tab::Home, "Home"), (Tab::Share, "Share"), (Tab::View, "View")];
        if in_trash {
            tabs.push((Tab::Manage, "Manage"));
        }
        for (t, label) in tabs {
            let w = text_w(ui, label, 12.5) + 24.0;
            let r = Rect::from_min_size(pos2(x, strip.top() + 1.0), vec2(w, 24.0));
            let resp = ui.interact(r, Id::unique(("ribbon-tab", label)), Sense::click());
            let active = self.tab == t && self.ribbon_open;
            if active {
                ui.painter().rect_filled(r, 0.0, WHITE);
                let s = Stroke::new(1.0, RIBBON_LINE);
                ui.painter().vline(r.left() + 0.5, r.y_range(), s);
                ui.painter().vline(r.right() - 0.5, r.y_range(), s);
                ui.painter().hline(r.x_range(), r.top() + 0.5, s);
            } else if resp.hovered() {
                ui.painter().rect_filled(r.shrink2(vec2(0.0, 1.0)), 0.0, Color32::from_rgb(0xf3, 0xf3, 0xf3));
            }
            let color = if t == Tab::Manage { Color32::from_rgb(0x8a, 0x5d, 0x00) } else { TEXT };
            ui.painter().text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.5), color);
            if resp.double_clicked() {
                self.ribbon_open = !self.ribbon_open;
            } else if resp.clicked() {
                self.tab = t;
                self.ribbon_open = true;
            }
            x += w;
        }

        // Collapse chevron and help at the far right.
        let help = Rect::from_center_size(pos2(strip.right() - 14.0, strip.center().y), vec2(20.0, 20.0));
        let hr = ui.interact(help, Id::unique("ribbon-help"), Sense::click());
        ui.painter().circle_filled(help.center(), 7.0, if hr.hovered() { FILE_BLUE_HOT } else { FILE_BLUE });
        ui.painter().text(help.center(), Align2::CENTER_CENTER, "?", FontId::proportional(11.0), WHITE);
        if hr.clicked() {
            let _ = spawn(std::process::Command::new("xdg-open").arg("https://www.bing.com/search?q=get+help+with+file+explorer+in+windows+10"));
        }
        let col = Rect::from_center_size(pos2(strip.right() - 38.0, strip.center().y), vec2(22.0, 20.0));
        let cr = ui.interact(col, Id::unique("ribbon-collapse"), Sense::click());
        if cr.hovered() {
            ui.painter().rect_filled(col, 0.0, HOVER);
        }
        chevron(ui.painter(), col.center(), if self.ribbon_open { Dir::Up } else { Dir::Down }, 3.5, DIM);
        if cr.on_hover_text(if self.ribbon_open { "Minimize the Ribbon (Ctrl+F1)" } else { "Expand the Ribbon (Ctrl+F1)" }).clicked() {
            self.ribbon_open = !self.ribbon_open;
        }

        if !self.ribbon_open {
            return;
        }

        let targets = self.targets();
        let has_sel = !targets.is_empty();
        let one = self.selected.len() == 1;
        let in_dir = self.here().is_some();
        let can_paste = in_dir && self.clipboard.is_some();
        let files_sel = self.selected_paths().iter().any(|p| p.is_file());
        let mut dests: Vec<(String, PathBuf)> = self.pins.iter().map(|p| (dir_name(p), p.clone())).collect();
        for (n, p, _) in user_folders() {
            if !dests.iter().any(|(_, d)| *d == p) {
                dests.push((n, p));
            }
        }
        dests.push(("Home".into(), home()));

        let body = ui.allocate_ui_with_layout(vec2(ui.available_width(), 92.0), egui::Layout::left_to_right(Align::Min), |ui| {
            ui.set_min_height(92.0);
            ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
            ui.add_space(4.0);
            match self.tab {
                Tab::Home => {
                    group(ui, "Clipboard", |ui| {
                        if big(ui, "pin", "Pin to Quick\naccess", has_sel || in_dir, false).clicked() {
                            acts.push(Act::PinSelected);
                        }
                        if big(ui, "edit-copy", "Copy", has_sel, false).clicked() {
                            acts.push(Act::Copy);
                        }
                        if big(ui, "edit-paste", "Paste", can_paste, false).clicked() {
                            acts.push(Act::Paste);
                        }
                        ui.vertical(|ui| {
                            ui.add_space(2.0);
                            if small(ui, "edit-cut", "Cut", has_sel && !in_trash).clicked() {
                                acts.push(Act::Cut);
                            }
                            if small(ui, "edit-copy-path", "Copy path", has_sel || in_dir).clicked() {
                                acts.push(Act::CopyPath);
                            }
                            if small(ui, "insert-link", "Paste shortcut", can_paste).clicked() {
                                acts.push(Act::PasteShortcut);
                            }
                        });
                    });
                    group(ui, "Organize", |ui| {
                        let r = big(ui, "edit-move", "Move\nto", has_sel && !in_trash && self.loc != Loc::ThisPc, true);
                        dest_menu(&r, &dests, true, acts);
                        let r = big(ui, "edit-copy", "Copy\nto", has_sel && !in_trash, true);
                        dest_menu(&r, &dests, false, acts);
                        let r = big(ui, "edit-delete", "Delete", has_sel && self.loc != Loc::ThisPc, true);
                        egui::Popup::menu(&r).show(|ui| {
                            if ui.button("Recycle").clicked() {
                                acts.push(Act::Delete(false));
                            }
                            if ui.button("Permanently delete").clicked() {
                                acts.push(Act::Delete(true));
                            }
                        });
                        if big(ui, "edit-rename", "Rename", one && !in_trash && self.loc != Loc::ThisPc, false).clicked() {
                            acts.push(Act::Rename);
                        }
                    });
                    group(ui, "New", |ui| {
                        if big(ui, "folder-new", "New\nfolder", in_dir, false).clicked() {
                            acts.push(Act::NewFolder);
                        }
                        ui.vertical(|ui| {
                            ui.add_space(2.0);
                            let r = small(ui, "document-new", "New item  ▾", in_dir);
                            egui::Popup::menu(&r).show(|ui| {
                                if ui.button("Folder").clicked() {
                                    acts.push(Act::NewFolder);
                                }
                                if ui.button("Text Document").clicked() {
                                    acts.push(Act::NewFile);
                                }
                            });
                            if small(ui, "window-pin", "Easy access", has_sel && !in_trash).on_hover_text("Create a shortcut").clicked() {
                                acts.push(Act::Shortcut);
                            }
                        });
                    });
                    group(ui, "Open", |ui| {
                        if big(ui, "document-properties", "Properties", has_sel || in_dir, false).clicked() {
                            acts.push(Act::Properties);
                        }
                        ui.vertical(|ui| {
                            ui.add_space(2.0);
                            if small(ui, "document-open", "Open", one && !in_trash).clicked()
                                && let Some(&i) = self.selected.first()
                            {
                                acts.push(Act::Open(i));
                            }
                            if small(ui, "document-edit", "Edit", one && files_sel && !in_trash).clicked()
                                && let Some(&i) = self.selected.first()
                            {
                                acts.push(Act::OpenDefault(i));
                            }
                            if small(ui, "utilities-terminal", "PowerSlop", in_dir).clicked()
                                && let Some(d) = self.here()
                            {
                                acts.push(Act::Terminal(d));
                            }
                        });
                    });
                    group(ui, "Select", |ui| {
                        ui.vertical(|ui| {
                            ui.add_space(2.0);
                            let any = !self.shown.is_empty();
                            if small(ui, "edit-select-all", "Select all", any).clicked() {
                                acts.push(Act::SelectAll);
                            }
                            if small(ui, "edit-select-none", "Select none", has_sel).clicked() {
                                acts.push(Act::SelectNone);
                            }
                            if small(ui, "edit-select-invert", "Invert selection", any).clicked() {
                                acts.push(Act::Invert);
                            }
                        });
                    });
                }
                Tab::Share => {
                    group(ui, "Send", |ui| {
                        if big(ui, "mail-message-new", "Email", files_sel, false).clicked() {
                            acts.push(Act::Email);
                        }
                        if big(ui, "archive-insert", "Zip", has_sel && in_dir, false).clicked() {
                            acts.push(Act::Zip);
                        }
                        ui.vertical(|ui| {
                            ui.add_space(2.0);
                            if small(ui, "edit-copy-path", "Copy path", has_sel || in_dir).clicked() {
                                acts.push(Act::CopyPath);
                            }
                            if small(ui, "utilities-terminal", "Open PowerSlop here", in_dir).clicked()
                                && let Some(d) = self.here()
                            {
                                acts.push(Act::Terminal(d));
                            }
                        });
                    });
                }
                Tab::View => {
                    let nav = self.nav_pane;
                    group(ui, "Panes", |ui| {
                        if toggle_big(ui, "view-sidetree", "Navigation\npane", nav).clicked() {
                            acts.push(Act::ToggleNav);
                        }
                    });
                    let view = self.view;
                    group(ui, "Layout", |ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(252.0, 70.0), Sense::hover());
                        ui.painter().rect_stroke(rect, 0.0, (1.0, RIBBON_LINE), StrokeKind::Inside);
                        for (k, (v, label, ic)) in VIEWS.iter().enumerate() {
                            let r = Rect::from_min_size(rect.min + vec2(2.0 + (k / 3) as f32 * 124.0, 2.0 + (k % 3) as f32 * 22.0), vec2(123.0, 22.0));
                            let resp = ui.interact(r, Id::unique(("layout", k)), Sense::click());
                            if *v == view {
                                ui.painter().rect_filled(r, 0.0, SELECTED);
                                ui.painter().rect_stroke(r, 0.0, (1.0, SELECTED_LINE), StrokeKind::Inside);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(r, 0.0, HOVER);
                            }
                            paint_icon(ui, ic, Rect::from_center_size(pos2(r.left() + 12.0, r.center().y), vec2(16.0, 16.0)), 1.0);
                            ui.painter().text(pos2(r.left() + 25.0, r.center().y), Align2::LEFT_CENTER, *label, FontId::proportional(12.0), TEXT);
                            if resp.clicked() {
                                acts.push(Act::SetView(*v));
                            }
                        }
                    });
                    let sort = self.sort;
                    let cols = self.columns();
                    group(ui, "Current view", |ui| {
                        let r = big(ui, "view-sort", "Sort\nby", self.loc != Loc::ThisPc, true);
                        egui::Popup::menu(&r).show(|ui| {
                            for (label, by) in cols {
                                if ui.radio(sort.0 == by, label).clicked() {
                                    acts.push(Act::Sort(by));
                                }
                            }
                            ui.separator();
                            if ui.radio(sort.1, "Ascending").clicked() {
                                acts.push(Act::SortAscending(true));
                            }
                            if ui.radio(!sort.1, "Descending").clicked() {
                                acts.push(Act::SortAscending(false));
                            }
                        });
                    });
                    let (mut ext, mut hidden) = (self.show_ext, self.show_hidden);
                    group(ui, "Show/hide", |ui| {
                        ui.vertical(|ui| {
                            ui.add_space(6.0);
                            ui.spacing_mut().item_spacing.y = 4.0;
                            if ui.checkbox(&mut ext, "File name extensions").changed() {
                                acts.push(Act::ToggleExt);
                            }
                            if ui.checkbox(&mut hidden, "Hidden items").changed() {
                                acts.push(Act::ToggleHidden);
                            }
                        });
                    });
                }
                Tab::Manage => {
                    let any = !self.entries.is_empty();
                    group(ui, "Manage", |ui| {
                        if big(ui, "user-trash", "Empty\nRecycle Bin", any, false).clicked() {
                            acts.push(Act::EmptyTrash);
                        }
                        if big(ui, "document-properties", "Properties", has_sel, false).clicked() {
                            acts.push(Act::Properties);
                        }
                    });
                    group(ui, "Restore", |ui| {
                        if big(ui, "edit-undo", "Restore all\nitems", any, false).clicked() {
                            acts.push(Act::Restore(true));
                        }
                        if big(ui, "edit-undo", "Restore the\nselected items", has_sel, false).clicked() {
                            acts.push(Act::Restore(false));
                        }
                    });
                }
            }
        });
        let r = body.response.rect;
        ui.painter().hline(strip.x_range(), r.bottom() + 0.5, (1.0, RIBBON_LINE));
    }

    fn file_menu(&self, tab: &egui::Response, acts: &mut Vec<Act>) {
        let here = self.here().unwrap_or_else(home);
        egui::Popup::menu(tab).show(|ui| {
            ui.set_min_width(420.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    if menu_item(ui, "window-new", "Open new window") {
                        acts.push(Act::NewWindow(here.clone()));
                        ui.close();
                    }
                    if menu_item(ui, "utilities-terminal", "Open PowerSlop") {
                        acts.push(Act::Terminal(here.clone()));
                        ui.close();
                    }
                    if menu_item(ui, "edit-delete", "Delete history") {
                        acts.push(Act::Go(Loc::Trash));
                        ui.close();
                    }
                    if menu_item(ui, "help-contents", "Help") {
                        let _ = spawn(std::process::Command::new("xdg-open").arg("https://www.bing.com/search?q=get+help+with+file+explorer+in+windows+10"));
                        ui.close();
                    }
                    if menu_item(ui, "window-close", "Close") {
                        acts.push(Act::Close);
                        ui.close();
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Frequent places").color(TEXT).size(13.0));
                    ui.add_space(4.0);
                    for (k, p) in self.pins.iter().enumerate().take(9) {
                        if ui.add(egui::Button::new(format!("{}   {}", k + 1, dir_name(p))).frame(false)).clicked() {
                            acts.push(Act::Go(Loc::Dir(p.clone())));
                            ui.close();
                        }
                    }
                });
            });
        });
    }

    // ---- Address bar ----

    fn address_bar(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
        ui.horizontal(|ui| {
            if nav_button(ui, "go-previous", !self.back.is_empty(), "Back (Alt+Left Arrow)").clicked() {
                acts.push(Act::Back);
            }
            if nav_button(ui, "go-next", !self.forward.is_empty(), "Forward (Alt+Right Arrow)").clicked() {
                acts.push(Act::Forward);
            }
            // Recent locations: the back history, newest first.
            let (r, resp) = ui.allocate_exact_size(vec2(14.0, 24.0), if self.back.is_empty() { Sense::hover() } else { Sense::click() });
            if resp.hovered() && !self.back.is_empty() {
                ui.painter().rect_filled(r, 0.0, HOVER);
            }
            chevron(ui.painter(), r.center(), Dir::Down, 3.0, if self.back.is_empty() { FAINT } else { DIM });
            let names: Vec<String> = self.back.iter().rev().take(9).map(|l| self.loc_name(l)).collect();
            egui::Popup::menu(&resp).show(|ui| {
                for (k, n) in names.iter().enumerate() {
                    if ui.button(n).clicked() {
                        acts.push(Act::BackTo(k + 1));
                    }
                }
            });
            let up = self.parent_loc().map(|l| self.loc_name(&l));
            if nav_button(ui, "go-up", up.is_some(), &format!("Up to \"{}\" (Alt+Up Arrow)", up.unwrap_or_default())).clicked() {
                acts.push(Act::Up);
            }
            ui.add_space(6.0);

            let search_w = (ui.available_width() * 0.28).clamp(150.0, 280.0);
            let addr_w = ui.available_width() - search_w - 8.0;
            let (rect, resp) = ui.allocate_exact_size(vec2(addr_w, 24.0), Sense::click());
            let editing = self.address.is_some();
            ui.painter().rect_filled(rect, 0.0, WHITE);
            ui.painter().rect_stroke(rect, 0.0, (1.0, if editing || resp.hovered() { SELECTED_LINE } else { RIBBON_LINE }), StrokeKind::Inside);
            if let Some(text) = &mut self.address {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 2.0))).layout(egui::Layout::left_to_right(Align::Center)));
                let te = child.add(egui::TextEdit::singleline(text).frame(egui::Frame::NONE).desired_width(rect.width() - 12.0).id(Id::unique("address")));
                if !te.has_focus() && !te.lost_focus() {
                    te.request_focus();
                }
                if te.lost_focus() {
                    let cancelled = ui.input(|i| i.key_pressed(Key::Escape));
                    let typed = shellexpand(text.trim());
                    self.address = None;
                    if !cancelled {
                        let p = PathBuf::from(&typed);
                        match typed.as_str() {
                            "This PC" | "Computer" => acts.push(Act::Go(Loc::ThisPc)),
                            "Recycle Bin" => acts.push(Act::Go(Loc::Trash)),
                            _ if p.is_dir() => acts.push(Act::Go(Loc::Dir(p))),
                            _ if p.exists() => acts.push(Act::OpenPath(p)),
                            _ => self.status = Some(format!("File Exploder can't find '{typed}'. Check the spelling and try again.")),
                        }
                    }
                }
            } else {
                let p = ui.painter();
                paint_icon(ui, self.loc_icon(&self.loc), Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(16.0, 16.0)), 1.0);
                // Crumbs, dropping the oldest ones when the bar is too narrow.
                let crumbs = self.crumbs();
                let refresh = Rect::from_center_size(pos2(rect.right() - 13.0, rect.center().y), vec2(22.0, 22.0));
                let room = refresh.left() - rect.left() - 30.0;
                let widths: Vec<f32> = crumbs.iter().map(|(n, _)| text_w(ui, n, 12.5) + 10.0 + 14.0).collect();
                let mut first = 0;
                while first + 1 < crumbs.len() && widths[first..].iter().sum::<f32>() > room {
                    first += 1;
                }
                let mut x = rect.left() + 24.0;
                if first > 0 {
                    p.text(pos2(x + 4.0, rect.center().y), Align2::LEFT_CENTER, "«", FontId::proportional(12.5), DIM);
                    x += 12.0;
                }
                let mut clicked_crumb = false;
                for (k, (name, loc)) in crumbs.iter().enumerate().skip(first) {
                    chevron(p, pos2(x + 6.0, rect.center().y), Dir::Right, 3.0, DIM);
                    x += 14.0;
                    let w = (widths[k] - 14.0).min(rect.right() - 30.0 - x).max(10.0);
                    let r = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(w, rect.height() - 4.0));
                    let cr = ui.interact(r, Id::unique(("crumb", k)), Sense::click());
                    if cr.hovered() {
                        p.rect_filled(r, 0.0, HOVER);
                        p.rect_stroke(r, 0.0, (1.0, HOVER_LINE), StrokeKind::Inside);
                    }
                    paint_text(ui, pos2(r.left() + 5.0, r.center().y), Align2::LEFT_CENTER, name, 12.5, TEXT, w - 8.0, 1);
                    if cr.clicked() {
                        clicked_crumb = true;
                        acts.push(Act::Go(loc.clone()));
                    }
                    x += w;
                }
                let rr = ui.interact(refresh, Id::unique("refresh"), Sense::click());
                if rr.hovered() {
                    p.rect_filled(refresh, 0.0, HOVER);
                }
                paint_icon(ui, "view-refresh", Rect::from_center_size(refresh.center(), vec2(14.0, 14.0)), 1.0);
                if rr.on_hover_text(format!("Refresh \"{}\" (F5)", self.loc_name(&self.loc))).clicked() {
                    acts.push(Act::Refresh);
                } else if resp.clicked() && !clicked_crumb {
                    self.address = Some(self.here().map(|p| p.display().to_string()).unwrap_or_else(|| self.loc_name(&self.loc)));
                }
            }

            ui.add_space(6.0);
            let (srect, _) = ui.allocate_exact_size(vec2(search_w, 24.0), Sense::hover());
            let sid = Id::unique("search");
            let focused = ui.memory(|m| m.has_focus(sid));
            ui.painter().rect_filled(srect, 0.0, WHITE);
            ui.painter().rect_stroke(srect, 0.0, (1.0, if focused { SELECTED_LINE } else { RIBBON_LINE }), StrokeKind::Inside);
            let place = match &self.loc {
                Loc::Search(root, _) => dir_name(root),
                l => self.loc_name(l),
            };
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(srect.shrink2(vec2(6.0, 2.0))).layout(egui::Layout::left_to_right(Align::Center)));
            let te = child.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text(format!("Search {place}"))
                    .frame(egui::Frame::NONE)
                    .desired_width(srect.width() - 34.0)
                    .id(sid),
            );
            if self.focus_search {
                te.request_focus();
                self.focus_search = false;
            }
            if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                acts.push(Act::Search);
            }
            let ir = Rect::from_center_size(pos2(srect.right() - 13.0, srect.center().y), vec2(20.0, 20.0));
            let resp = ui.interact(ir, Id::unique("search-go"), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(ir, 0.0, HOVER);
            }
            if self.filter.is_empty() {
                paint_icon(ui, "edit-find", Rect::from_center_size(ir.center(), vec2(14.0, 14.0)), 1.0);
            } else {
                paint_icon(ui, "edit-clear", Rect::from_center_size(ir.center(), vec2(14.0, 14.0)), 1.0);
                if resp.clicked() {
                    self.filter.clear();
                    acts.push(Act::Search);
                }
            }
        });
    }

    fn crumbs(&self) -> Vec<(String, Loc)> {
        let dir_crumbs = |p: &Path| {
            let mut out = vec![("This PC".to_string(), Loc::ThisPc)];
            let h = home();
            let (start, rest) = match p.strip_prefix(&h) {
                Ok(rest) => ((dir_name(&h), h.clone()), rest.to_path_buf()),
                Err(_) => (("Local Disk (/)".to_string(), PathBuf::from("/")), p.strip_prefix("/").unwrap_or(p).to_path_buf()),
            };
            let mut acc = start.1.clone();
            out.push((start.0, Loc::Dir(start.1)));
            for c in rest.components() {
                acc.push(c);
                out.push((c.as_os_str().to_string_lossy().into_owned(), Loc::Dir(acc.clone())));
            }
            out
        };
        match &self.loc {
            Loc::ThisPc => vec![("This PC".into(), Loc::ThisPc)],
            Loc::Trash => vec![("Recycle Bin".into(), Loc::Trash)],
            Loc::Dir(p) => dir_crumbs(p),
            Loc::Search(root, _) => {
                let mut c = dir_crumbs(root);
                c.push((format!("Search Results in {}", dir_name(root)), self.loc.clone()));
                c
            }
        }
    }

    fn loc_icon(&self, loc: &Loc) -> &'static str {
        match loc {
            Loc::ThisPc => "computer",
            Loc::Trash => {
                if trash_dir().join("files").read_dir().is_ok_and(|mut d| d.next().is_some()) {
                    "user-trash-full"
                } else {
                    "user-trash"
                }
            }
            Loc::Dir(p) => folder_icon(p),
            Loc::Search(..) => "edit-find",
        }
    }

    // ---- Navigation pane ----

    fn nav(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.add_space(6.0);
            let (r, toggle) = self.nav_row(ui, 0.0, Some(self.qa_open), "★", "Quick access", false, None);
            if toggle || r.clicked() {
                self.qa_open = !self.qa_open;
            }
            if self.qa_open {
                for p in self.pins.clone() {
                    let loc = Loc::Dir(p.clone());
                    let (r, _) = self.nav_row(ui, 1.0, None, folder_icon(&p), &dir_name(&p), true, Some(&loc));
                    if r.clicked() {
                        acts.push(Act::Go(loc));
                    }
                    r.context_menu(|ui| {
                        if ui.button("Open in new window").clicked() {
                            acts.push(Act::NewWindow(p.clone()));
                        }
                        if ui.button("Unpin from Quick access").clicked() {
                            acts.push(Act::Unpin(p.clone()));
                        }
                    });
                }
            }
            ui.add_space(6.0);
            let (r, toggle) = self.nav_row(ui, 0.0, Some(self.pc_open), "computer", "This PC", false, Some(&Loc::ThisPc));
            if toggle {
                self.pc_open = !self.pc_open;
            } else if r.clicked() {
                acts.push(Act::Go(Loc::ThisPc));
            }
            if self.pc_open {
                let mut items: Vec<(String, PathBuf, &'static str)> = user_folders();
                for d in &self.drives {
                    items.push((d.label.clone(), d.path.clone(), if d.removable { "drive-removable-media" } else { "drive-harddisk" }));
                }
                for (n, p, ic) in items {
                    let loc = Loc::Dir(p.clone());
                    let (r, _) = self.nav_row(ui, 1.0, None, ic, &n, false, Some(&loc));
                    if r.clicked() {
                        acts.push(Act::Go(loc));
                    }
                    r.context_menu(|ui| {
                        if ui.button("Open in new window").clicked() {
                            acts.push(Act::NewWindow(p.clone()));
                        }
                        if ui.button("Pin to Quick access").clicked() {
                            acts.push(Act::Pin(p.clone()));
                        }
                    });
                }
            }
            ui.add_space(6.0);
            let (r, _) = self.nav_row(ui, 0.0, None, self.loc_icon(&Loc::Trash), "Recycle Bin", false, Some(&Loc::Trash));
            if r.clicked() {
                acts.push(Act::Go(Loc::Trash));
            }
            r.context_menu(|ui| {
                if ui.button("Empty Recycle Bin").clicked() {
                    acts.push(Act::EmptyTrash);
                }
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn nav_row(&self, ui: &mut Ui, level: f32, expanded: Option<bool>, icon: &str, label: &str, pinned: bool, loc: Option<&Loc>) -> (egui::Response, bool) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
        let p = ui.painter();
        if loc.is_some_and(|l| *l == self.loc) {
            p.rect_filled(rect, 0.0, SELECTED);
        } else if resp.hovered() {
            p.rect_filled(rect, 0.0, HOVER);
        }
        let x0 = rect.left() + 4.0 + level * 16.0;
        let mut toggled = false;
        if let Some(open) = expanded {
            let chev = Rect::from_center_size(pos2(x0 + 6.0, rect.center().y), vec2(14.0, 22.0));
            let cr = ui.interact(chev, resp.id.with("chevron"), Sense::click());
            chevron(p, chev.center(), if open { Dir::Down } else { Dir::Right }, 3.5, if cr.hovered() { STAR_BLUE } else { DIM });
            toggled = cr.clicked();
        }
        let ir = Rect::from_center_size(pos2(x0 + 24.0, rect.center().y), vec2(16.0, 16.0));
        if icon == "★" {
            star(p, ir.center(), 7.5, STAR_BLUE);
        } else {
            paint_icon(ui, icon, ir, 1.0);
        }
        let max_w = rect.right() - ir.right() - if pinned { 28.0 } else { 8.0 };
        paint_text(ui, pos2(ir.right() + 6.0, rect.center().y), Align2::LEFT_CENTER, label, 12.5, TEXT, max_w, 1);
        if pinned {
            paint_icon(ui, "pin", Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(12.0, 12.0)), 0.5);
        }
        (resp, toggled)
    }

    // ---- Content ----

    fn columns(&self) -> [(&'static str, SortBy); 4] {
        match self.loc {
            Loc::Trash => [("Name", SortBy::Name), ("Original Location", SortBy::Location), ("Date Deleted", SortBy::Modified), ("Size", SortBy::Size)],
            Loc::Search(..) => [("Name", SortBy::Name), ("Folder", SortBy::Location), ("Date modified", SortBy::Modified), ("Size", SortBy::Size)],
            _ => [("Name", SortBy::Name), ("Date modified", SortBy::Modified), ("Type", SortBy::Kind), ("Size", SortBy::Size)],
        }
    }

    fn cell(&self, e: &Entry, by: SortBy) -> String {
        match by {
            SortBy::Name => self.display_name(e),
            SortBy::Modified => fmt_time(e.modified),
            SortBy::Kind => e.kind.clone(),
            SortBy::Size if e.is_dir => String::new(),
            SortBy::Size => human_size(e.size),
            SortBy::Location => e.location.clone(),
        }
    }

    fn content(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        if self.loc == Loc::ThisPc {
            self.this_pc(ui, acts);
            return;
        }
        let q = self.filter.to_lowercase();
        self.shown = (0..self.entries.len()).filter(|&i| q.is_empty() || self.entries[i].name.to_lowercase().contains(&q)).collect();
        let shown = self.shown.clone();

        if self.view == View::Details {
            self.details_header(ui, acts);
        }
        let column_major = self.view == View::List;
        let scroll = if column_major { egui::ScrollArea::horizontal() } else { egui::ScrollArea::vertical() };
        scroll.id_salt("items").auto_shrink(false).show(ui, |ui| {
            let avail = ui.available_size();
            let (cell, pad) = match self.view {
                View::LargeIcons => (vec2(112.0, 140.0), vec2(8.0, 8.0)),
                View::MediumIcons => (vec2(86.0, 96.0), vec2(8.0, 8.0)),
                View::SmallIcons => (vec2(230.0, ROW_H), vec2(8.0, 6.0)),
                View::List => (vec2(250.0, ROW_H), vec2(8.0, 6.0)),
                View::Details => (vec2(self.col_w.iter().sum::<f32>().max(avail.x - 8.0), ROW_H), vec2(4.0, 2.0)),
                View::Tiles => (vec2(272.0, 66.0), vec2(8.0, 8.0)),
            };
            let n = shown.len();
            let stride = if column_major {
                (((avail.y - pad.y * 2.0) / cell.y).floor() as usize).max(1)
            } else {
                (((avail.x - pad.x * 2.0) / cell.x).floor() as usize).max(1)
            };
            self.stride = stride;
            let lines = n.div_ceil(stride);
            let size = if column_major { vec2(pad.x * 2.0 + lines as f32 * cell.x, avail.y) } else { vec2(avail.x, pad.y * 2.0 + lines as f32 * cell.y) };
            let (area, bg) = ui.allocate_exact_size(size.max(avail), Sense::click());

            if n == 0 {
                let msg = if self.searching.load(Ordering::Relaxed) {
                    "Working on it..."
                } else if !q.is_empty() || matches!(self.loc, Loc::Search(..)) {
                    "No items match your search."
                } else if self.status.is_some() {
                    ""
                } else {
                    "This folder is empty."
                };
                ui.painter().text(pos2(area.center().x, area.top() + 30.0), Align2::CENTER_TOP, msg, FontId::proportional(12.5), DIM);
            }

            let clip = ui.clip_rect();
            for (k, &i) in shown.iter().enumerate() {
                let (c, r) = if column_major { (k / stride, k % stride) } else { (k % stride, k / stride) };
                let rect = Rect::from_min_size(area.min + pad + vec2(c as f32 * cell.x, r as f32 * cell.y), cell);
                if self.scroll_to == Some(i) {
                    ui.scroll_to_rect(rect, None);
                    self.scroll_to = None;
                }
                if !clip.intersects(rect) {
                    continue;
                }
                let item = if self.view == View::Details { rect } else { rect.shrink(1.0) };
                let resp = ui.interact(item, Id::unique(("item", &self.entries[i].path)), Sense::click());
                self.paint_item(ui, item, i, &resp, acts);
                self.item_input(ui, &resp, i, acts);
            }

            if bg.clicked() {
                self.selected.clear();
                if self.renaming.is_some() {
                    acts.push(Act::CommitRename);
                }
            }
            bg.context_menu(|ui| self.folder_menu(ui, acts));
        });
    }

    fn details_header(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::hover());
        let mut x = rect.left() + 4.0;
        for (k, (label, by)) in self.columns().into_iter().enumerate() {
            let w = self.col_w[k];
            let r = Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
            let resp = ui.interact(r, Id::unique(("col", k)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 0.0, HOVER);
            }
            let align = if by == SortBy::Size { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER };
            let tx = if by == SortBy::Size { r.right() - 8.0 } else { r.left() + 6.0 };
            paint_text(ui, pos2(tx, r.center().y + 2.0), align, label, 12.0, HEADER_TEXT, w - 12.0, 1);
            if self.sort.0 == by {
                chevron(ui.painter(), pos2(r.center().x, r.top() + 5.0), if self.sort.1 { Dir::Up } else { Dir::Down }, 3.0, FAINT);
            }
            if resp.clicked() {
                acts.push(Act::Sort(by));
            }
            ui.painter().vline(r.right() - 0.5, r.y_range().shrink(3.0), (1.0, BORDER));
            // Drag the divider to resize the column.
            let grip = Rect::from_center_size(pos2(r.right(), r.center().y), vec2(8.0, r.height()));
            let gr = ui.interact(grip, Id::unique(("col-grip", k)), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            if gr.dragged() {
                self.col_w[k] = (self.col_w[k] + gr.drag_delta().x).max(48.0);
            }
            x += w;
        }
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, (1.0, BORDER));
    }

    fn paint_item(&mut self, ui: &mut Ui, rect: Rect, i: usize, resp: &egui::Response, acts: &mut Vec<Act>) {
        let sel = self.selected.contains(&i);
        let p = ui.painter().clone();
        if sel {
            p.rect_filled(rect, 0.0, SELECTED);
            p.rect_stroke(rect, 0.0, (1.0, SELECTED_LINE), StrokeKind::Inside);
        } else if resp.hovered() {
            p.rect_filled(rect, 0.0, HOVER);
        }
        let e = self.entries[i].clone();
        let name = self.display_name(&e);
        // Cut and hidden items are drawn faded, like Explorer.
        let cut = matches!(&self.clipboard, Some(Clip::Cut(c)) if c.contains(&e.path));
        let alpha = if cut || e.name.starts_with('.') { 0.5 } else { 1.0 };
        let text_color = TEXT.gamma_multiply(alpha);
        let cy = rect.center().y;
        let name_rect = match self.view {
            View::Details => {
                self.entry_icon(ui, i, Rect::from_center_size(pos2(rect.left() + 14.0, cy), vec2(16.0, 16.0)), alpha);
                let nr = Rect::from_min_max(pos2(rect.left() + 26.0, rect.top()), pos2(rect.left() + 4.0 + self.col_w[0] - 4.0, rect.bottom()));
                if self.renaming.as_ref().is_none_or(|(ri, _)| *ri != i) {
                    paint_text(ui, pos2(nr.left() + 2.0, cy), Align2::LEFT_CENTER, &name, 12.5, text_color, nr.width() - 4.0, 1);
                }
                let mut x = rect.left() + 4.0 + self.col_w[0];
                for (k, (_, by)) in self.columns().into_iter().enumerate().skip(1) {
                    let w = self.col_w[k];
                    let s = self.cell(&e, by);
                    if by == SortBy::Size {
                        paint_text(ui, pos2(x + w - 8.0, cy), Align2::RIGHT_CENTER, &s, 12.5, DIM, w - 12.0, 1);
                    } else {
                        paint_text(ui, pos2(x + 6.0, cy), Align2::LEFT_CENTER, &s, 12.5, DIM, w - 12.0, 1);
                    }
                    x += w;
                }
                nr
            }
            View::SmallIcons | View::List => {
                self.entry_icon(ui, i, Rect::from_center_size(pos2(rect.left() + 12.0, cy), vec2(16.0, 16.0)), alpha);
                let nr = Rect::from_min_max(pos2(rect.left() + 24.0, rect.top()), rect.max);
                if self.renaming.as_ref().is_none_or(|(ri, _)| *ri != i) {
                    paint_text(ui, pos2(nr.left() + 2.0, cy), Align2::LEFT_CENTER, &name, 12.5, text_color, nr.width() - 6.0, 1);
                }
                nr
            }
            View::LargeIcons | View::MediumIcons => {
                let s = if self.view == View::LargeIcons { 96.0 } else { 48.0 };
                self.entry_icon(ui, i, Rect::from_min_size(pos2(rect.center().x - s / 2.0, rect.top() + 4.0), vec2(s, s)), alpha);
                let nr = Rect::from_min_max(pos2(rect.left() + 2.0, rect.top() + s + 8.0), pos2(rect.right() - 2.0, rect.bottom()));
                if self.renaming.as_ref().is_none_or(|(ri, _)| *ri != i) {
                    paint_text(ui, pos2(rect.center().x, nr.top()), Align2::CENTER_TOP, &name, 12.0, text_color, nr.width() - 4.0, if sel { 3 } else { 2 });
                }
                Rect::from_min_size(nr.min, vec2(nr.width(), 22.0))
            }
            View::Tiles => {
                self.entry_icon(ui, i, Rect::from_center_size(pos2(rect.left() + 32.0, cy), vec2(48.0, 48.0)), alpha);
                let x = rect.left() + 62.0;
                let w = rect.right() - x - 6.0;
                if self.renaming.as_ref().is_none_or(|(ri, _)| *ri != i) {
                    paint_text(ui, pos2(x, cy - 16.0), Align2::LEFT_CENTER, &name, 12.5, text_color, w, 1);
                }
                paint_text(ui, pos2(x, cy), Align2::LEFT_CENTER, &e.kind, 12.0, DIM, w, 1);
                if !e.is_dir {
                    paint_text(ui, pos2(x, cy + 16.0), Align2::LEFT_CENTER, &human_size(e.size), 12.0, DIM, w, 1);
                }
                Rect::from_min_size(pos2(x - 2.0, cy - 27.0), vec2(w, 22.0))
            }
        };

        if let Some((ri, text)) = &mut self.renaming
            && *ri == i
        {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name_rect).layout(egui::Layout::left_to_right(Align::Center)));
            let te = child.add(egui::TextEdit::singleline(text).desired_width(name_rect.width() - 4.0).id(Id::unique("rename")));
            if self.rename_focus {
                te.request_focus();
                self.rename_focus = false;
            } else if te.lost_focus() {
                acts.push(if ui.input(|i| i.key_pressed(Key::Escape)) { Act::CancelRename } else { Act::CommitRename });
            }
        }
    }

    /// An entry's icon, or its thumbnail for pictures in the bigger views.
    fn entry_icon(&mut self, ui: &Ui, i: usize, rect: Rect, alpha: f32) {
        let e = &self.entries[i];
        if rect.width() >= 40.0 && is_image(e) {
            match self.thumbs.get(&e.path) {
                Some(Some(tex)) => {
                    let s = tex.size_vec2();
                    let scale = (rect.width() / s.x).min(rect.height() / s.y).min(1.0);
                    let r = Rect::from_center_size(rect.center(), s * scale);
                    ui.painter().image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE.gamma_multiply(alpha));
                    ui.painter().rect_stroke(r, 0.0, (1.0, BORDER), StrokeKind::Outside);
                    return;
                }
                Some(None) => {}
                None => {
                    self.thumbs.insert(e.path.clone(), None);
                    let _ = self.thumb_tx.send((self.thumb_gen.load(Ordering::Relaxed), e.path.clone()));
                }
            }
        }
        paint_icon(ui, e.icon, rect, alpha);
        if e.is_link {
            let s = (rect.width() * 0.45).max(10.0);
            paint_icon(ui, "emblem-symbolic-link", Rect::from_min_size(pos2(rect.left(), rect.bottom() - s), vec2(s, s)), alpha);
        }
    }

    fn item_input(&mut self, ui: &Ui, resp: &egui::Response, i: usize, acts: &mut Vec<Act>) {
        if resp.clicked() {
            if self.renaming.as_ref().is_some_and(|(ri, _)| *ri != i) {
                acts.push(Act::CommitRename);
            }
            // A second click on the only selected item renames it, as in Explorer.
            let mods = ui.input(|inp| inp.modifiers);
            self.click(i, mods);
        }
        if resp.double_clicked() {
            acts.push(Act::Open(i));
        }
        if resp.secondary_clicked() && !self.selected.contains(&i) {
            self.selected = vec![i];
            self.anchor = Some(i);
        }
        resp.context_menu(|ui| self.item_menu(ui, i, acts));
    }

    fn item_menu(&self, ui: &mut Ui, i: usize, acts: &mut Vec<Act>) {
        let Some(e) = self.entries.get(i) else { return };
        ui.set_min_width(200.0);
        let item = |ui: &mut Ui, acts: &mut Vec<Act>, label: &str, act: Act| {
            if ui.button(label).clicked() {
                acts.push(act);
                ui.close();
            }
        };
        if self.loc == Loc::Trash {
            item(ui, acts, "Restore", Act::Restore(false));
            ui.separator();
            item(ui, acts, "Delete", Act::Delete(true));
            ui.separator();
            item(ui, acts, "Properties", Act::Properties);
            return;
        }
        let run = if e.is_dir { None } else { runnable(&e.path) };
        // The default action is bold, like Explorer's.
        if ui.button(egui::RichText::new(if run.is_some() { "Run" } else { "Open" }).strong()).clicked() {
            acts.push(Act::Open(i));
            ui.close();
        }
        if e.is_dir {
            item(ui, acts, "Open in new window", Act::NewWindow(e.path.clone()));
            item(ui, acts, "Pin to Quick access", Act::Pin(e.path.clone()));
            item(ui, acts, "Open PowerSlop window here", Act::Terminal(e.path.clone()));
        }
        match run {
            Some(Run::Script(_)) => item(ui, acts, "Edit", Act::OpenDefault(i)),
            Some(Run::Program) => item(ui, acts, "Run in PowerSlop", Act::RunInPowerSlop(i)),
            _ => {}
        }
        if let Loc::Search(..) = self.loc
            && let Some(parent) = e.path.parent()
        {
            item(ui, acts, "Open file location", Act::Go(Loc::Dir(parent.to_path_buf())));
        }
        ui.separator();
        if self.here().is_some() {
            item(ui, acts, "Add to zip file", Act::Zip);
        }
        if !e.is_dir {
            item(ui, acts, "Email", Act::Email);
        }
        ui.separator();
        item(ui, acts, "Cut", Act::Cut);
        item(ui, acts, "Copy", Act::Copy);
        ui.separator();
        item(ui, acts, "Create shortcut", Act::Shortcut);
        item(ui, acts, "Delete", Act::Delete(false));
        item(ui, acts, "Rename", Act::Rename);
        ui.separator();
        item(ui, acts, "Properties", Act::Properties);
    }

    fn folder_menu(&self, ui: &mut Ui, acts: &mut Vec<Act>) {
        ui.set_min_width(200.0);
        ui.menu_button("View", |ui| {
            for (v, label, _) in VIEWS {
                if ui.radio(self.view == v, label).clicked() {
                    acts.push(Act::SetView(v));
                    ui.close();
                }
            }
        });
        ui.menu_button("Sort by", |ui| {
            for (label, by) in self.columns() {
                if ui.radio(self.sort.0 == by, label).clicked() {
                    acts.push(Act::Sort(by));
                    ui.close();
                }
            }
            ui.separator();
            if ui.radio(self.sort.1, "Ascending").clicked() {
                acts.push(Act::SortAscending(true));
                ui.close();
            }
            if ui.radio(!self.sort.1, "Descending").clicked() {
                acts.push(Act::SortAscending(false));
                ui.close();
            }
        });
        if ui.button("Refresh").clicked() {
            acts.push(Act::Refresh);
            ui.close();
        }
        if self.loc == Loc::Trash {
            ui.separator();
            if ui.button("Empty Recycle Bin").clicked() {
                acts.push(Act::EmptyTrash);
                ui.close();
            }
            return;
        }
        let Some(dir) = self.here() else { return };
        ui.separator();
        let can_paste = self.clipboard.is_some();
        if ui.add_enabled(can_paste, egui::Button::new("Paste")).clicked() {
            acts.push(Act::Paste);
            ui.close();
        }
        if ui.add_enabled(can_paste, egui::Button::new("Paste shortcut")).clicked() {
            acts.push(Act::PasteShortcut);
            ui.close();
        }
        ui.separator();
        if ui.button("Open PowerSlop window here").clicked() {
            acts.push(Act::Terminal(dir.clone()));
            ui.close();
        }
        ui.separator();
        ui.menu_button("New", |ui| {
            if ui.button("Folder").clicked() {
                acts.push(Act::NewFolder);
                ui.close();
            }
            if ui.button("Text Document").clicked() {
                acts.push(Act::NewFile);
                ui.close();
            }
        });
        ui.separator();
        if ui.button("Properties").clicked() {
            acts.push(Act::Properties);
            ui.close();
        }
    }

    // ---- This PC ----

    fn this_pc(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        self.shown.clear();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let (bg, bg_resp) = (ui.max_rect(), ui.interact(ui.max_rect(), Id::unique("pc-bg"), Sense::click()));
            let _ = bg;
            if bg_resp.clicked() {
                self.pc_sel = None;
            }
            ui.add_space(6.0);
            let folders = user_folders();
            section(ui, &format!("Folders ({})", folders.len()));
            let tiles: Vec<(String, PathBuf, &'static str, Option<Drive>)> = folders.into_iter().map(|(n, p, i)| (n, p, i, None)).collect();
            self.pc_tiles(ui, tiles, vec2(250.0, 60.0), acts);
            ui.add_space(10.0);
            section(ui, &format!("Devices and drives ({})", self.drives.len()));
            let tiles = self.drives.iter().map(|d| (d.label.clone(), d.path.clone(), if d.removable { "drive-removable-media" } else { "drive-harddisk" }, Some(d.clone()))).collect();
            self.pc_tiles(ui, tiles, vec2(272.0, 66.0), acts);
        });
    }

    fn pc_tiles(&mut self, ui: &mut Ui, tiles: Vec<(String, PathBuf, &'static str, Option<Drive>)>, cell: Vec2, acts: &mut Vec<Act>) {
        let pad = vec2(18.0, 4.0);
        let stride = (((ui.available_width() - pad.x * 2.0) / cell.x).floor() as usize).max(1);
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), pad.y * 2.0 + tiles.len().div_ceil(stride) as f32 * cell.y), Sense::hover());
        for (k, (name, path, icon, drive)) in tiles.into_iter().enumerate() {
            let rect = Rect::from_min_size(area.min + pad + vec2((k % stride) as f32 * cell.x, (k / stride) as f32 * cell.y), cell).shrink(1.0);
            let resp = ui.interact(rect, Id::unique(("pc", &path)), Sense::click());
            let p = ui.painter();
            if self.pc_sel.as_ref() == Some(&path) {
                p.rect_filled(rect, 0.0, SELECTED);
                p.rect_stroke(rect, 0.0, (1.0, SELECTED_LINE), StrokeKind::Inside);
            } else if resp.hovered() {
                p.rect_filled(rect, 0.0, HOVER);
            }
            paint_icon(ui, icon, Rect::from_center_size(pos2(rect.left() + 30.0, rect.center().y), vec2(48.0, 48.0)), 1.0);
            let x = rect.left() + 60.0;
            let w = rect.right() - x - 8.0;
            match &drive {
                None => {
                    paint_text(ui, pos2(x, rect.center().y), Align2::LEFT_CENTER, &name, 12.5, TEXT, w, 1);
                }
                Some(d) => {
                    paint_text(ui, pos2(x, rect.top() + 14.0), Align2::LEFT_CENTER, &name, 12.5, TEXT, w, 1);
                    if let Some((free, total)) = d.usage {
                        let bar = Rect::from_min_size(pos2(x, rect.top() + 25.0), vec2(w.min(190.0), 14.0));
                        let used = if total > 0 { 1.0 - free as f32 / total as f32 } else { 0.0 };
                        p.rect_filled(bar, 0.0, BAR_BG);
                        p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * used, bar.height())), 0.0, if used > 0.9 { BAR_RED } else { BAR_BLUE });
                        p.rect_stroke(bar, 0.0, (1.0, BAR_LINE), StrokeKind::Inside);
                        paint_text(ui, pos2(x, rect.top() + 50.0), Align2::LEFT_CENTER, &format!("{} free of {}", disk_size(free), disk_size(total)), 12.0, DIM, w, 1);
                    }
                }
            }
            if resp.clicked() {
                self.pc_sel = Some(path.clone());
            }
            if resp.double_clicked() {
                acts.push(Act::Go(Loc::Dir(path.clone())));
            }
            if resp.secondary_clicked() {
                self.pc_sel = Some(path.clone());
            }
            resp.context_menu(|ui| {
                if ui.button(egui::RichText::new("Open").strong()).clicked() {
                    acts.push(Act::Go(Loc::Dir(path.clone())));
                    ui.close();
                }
                if ui.button("Open in new window").clicked() {
                    acts.push(Act::NewWindow(path.clone()));
                    ui.close();
                }
                if ui.button("Pin to Quick access").clicked() {
                    acts.push(Act::Pin(path.clone()));
                    ui.close();
                }
                ui.separator();
                if ui.button("Copy").clicked() {
                    acts.push(Act::Copy);
                    ui.close();
                }
                ui.separator();
                if ui.button("Properties").clicked() {
                    acts.push(Act::Properties);
                    ui.close();
                }
            });
        }
    }

    // ---- Dialogs ----

    fn dialogs(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        if let Some(c) = self.confirm.take() {
            let mut answer = None;
            egui::Modal::new(Id::unique("confirm")).show(ctx, |ui| {
                ui.set_width(400.0);
                let (title, body) = match &c {
                    Confirm::Delete(p) if p.len() == 1 => (
                        "Delete File",
                        format!("Are you sure you want to permanently delete \"{}\"?", p[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                    ),
                    Confirm::Delete(p) => ("Delete Multiple Items", format!("Are you sure you want to permanently delete these {} items?", p.len())),
                    Confirm::EmptyTrash(n) => ("Delete Multiple Items", format!("Are you sure you want to permanently delete these {n} items?")),
                    Confirm::Conflict { clashes, .. } => (
                        "Replace or Skip Files",
                        format!("The destination has {clashes} file{} with the same name{}.", if *clashes == 1 { "" } else { "s" }, if *clashes == 1 { "" } else { "s" }),
                    ),
                };
                ui.label(egui::RichText::new(title).size(15.0).color(GROUP_BLUE));
                ui.add_space(10.0);
                ui.label(egui::RichText::new(body).color(TEXT));
                ui.add_space(16.0);
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let b = |ui: &mut Ui, s: &str| ui.add(egui::Button::new(s).min_size(vec2(76.0, 24.0))).clicked();
                    match &c {
                        Confirm::Conflict { .. } => {
                            if b(ui, "Cancel") {
                                answer = Some(Answer::No);
                            }
                            if b(ui, "Keep both") {
                                answer = Some(Answer::KeepBoth);
                            }
                            if b(ui, "Skip these files") {
                                answer = Some(Answer::Skip);
                            }
                            if b(ui, "Replace the files") {
                                answer = Some(Answer::Replace);
                            }
                        }
                        _ => {
                            if b(ui, "No") {
                                answer = Some(Answer::No);
                            }
                            if b(ui, "Yes") {
                                answer = Some(Answer::Yes);
                            }
                        }
                    }
                });
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    answer = Some(Answer::No);
                }
            });
            match (c, answer) {
                (c, None) => self.confirm = Some(c),
                (_, Some(Answer::No)) => {}
                (Confirm::Delete(p), Some(_)) => self.delete_now(&p),
                (Confirm::EmptyTrash(_), Some(_)) => self.empty_trash(),
                (Confirm::Conflict { plan, cut, .. }, Some(a)) => self.resolve_conflict(plan, cut, a),
            }
        }

        if let Some(mut p) = self.props.take() {
            let mut open = true;
            let mut done = false;
            egui::Window::new(format!("{} Properties", p.title))
                .id(Id::unique("props"))
                .collapsible(false)
                .resizable(false)
                .default_width(380.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let r = ui.add(egui::Button::new("General").selected(true).frame(true));
                        let _ = r;
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                        paint_icon(ui, p.icon, r, 1.0);
                        ui.add_space(12.0);
                        let mut t = p.title.clone();
                        ui.add_enabled(false, egui::TextEdit::singleline(&mut t).desired_width(260.0));
                    });
                    ui.separator();
                    let (bytes, files, folders, finished) = *p.totals.lock().unwrap();
                    egui::Grid::new("props-grid").num_columns(2).spacing([28.0, 7.0]).show(ui, |ui| {
                        let row = |ui: &mut Ui, k: &str, v: String| {
                            ui.label(k);
                            ui.label(egui::RichText::new(v).color(TEXT));
                            ui.end_row();
                        };
                        row(ui, "Type:", p.kind.clone());
                        if let Some(d) = &p.drive {
                            row(ui, "File system:", d.fs.clone());
                        } else {
                            row(ui, "Location:", p.location.clone());
                            let more = if finished { "" } else { "…" };
                            row(ui, "Size:", format!("{} ({} bytes){more}", human_size_exact(bytes), commas(bytes)));
                            if p.md.as_ref().is_none_or(|m| m.is_dir()) {
                                row(ui, "Contains:", format!("{} Files, {} Folders{more}", commas(files), commas(folders)));
                            }
                        }
                    });
                    if let Some((free, total)) = p.drive.as_ref().and_then(|d| d.usage) {
                        ui.separator();
                        let used = total - free;
                        egui::Grid::new("props-drive").num_columns(3).spacing([16.0, 7.0]).show(ui, |ui| {
                            for (label, v, color) in [("Used space:", used, BAR_BLUE), ("Free space:", free, FREE_GREY)] {
                                let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                                ui.painter().rect_filled(r, 0.0, color);
                                ui.label(label);
                                ui.label(egui::RichText::new(format!("{} bytes    {}", commas(v), disk_size(v))).color(TEXT));
                                ui.end_row();
                            }
                            ui.label("");
                            ui.label("Capacity:");
                            ui.label(egui::RichText::new(format!("{} bytes    {}", commas(total), disk_size(total))).color(TEXT));
                            ui.end_row();
                        });
                        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
                        donut(ui.painter(), r.center(), 50.0, if total > 0 { used as f32 / total as f32 } else { 0.0 });
                    } else if let Some(md) = &p.md {
                        ui.separator();
                        egui::Grid::new("props-dates").num_columns(2).spacing([28.0, 7.0]).show(ui, |ui| {
                            for (k, t) in [("Created:", md.created().ok()), ("Modified:", md.modified().ok()), ("Accessed:", md.accessed().ok())] {
                                ui.label(k);
                                ui.label(egui::RichText::new(fmt_long(t)).color(TEXT));
                                ui.end_row();
                            }
                        });
                    }
                    if p.drive.is_none() {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Attributes:");
                            ui.add_space(14.0);
                            ui.checkbox(&mut p.readonly, "Read-only");
                            let mut hidden = p.paths.iter().all(|x| x.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')));
                            ui.add_enabled(false, egui::Checkbox::new(&mut hidden, "Hidden"));
                        });
                    }
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let b = |ui: &mut Ui, s: &str, on: bool| ui.add_enabled(on, egui::Button::new(s).min_size(vec2(72.0, 24.0))).clicked();
                        let changed = p.readonly != p.readonly_was;
                        if b(ui, "Apply", changed) {
                            set_readonly(&p.paths, p.readonly);
                            p.readonly_was = p.readonly;
                        }
                        if b(ui, "Cancel", true) {
                            done = true;
                        }
                        if b(ui, "OK", true) {
                            if changed {
                                set_readonly(&p.paths, p.readonly);
                            }
                            done = true;
                        }
                    });
                });
            if open && !done {
                if !p.totals.lock().unwrap().3 {
                    ctx.request_repaint_after(Duration::from_millis(200));
                }
                self.props = Some(p);
            } else {
                acts.push(Act::Refresh);
            }
        }

        if let Some((job, started)) = &mut self.job {
            let now = ctx.input(|i| i.time);
            if started.is_nan() {
                *started = now;
            }
            let job = job.clone();
            let mut p = job.lock().unwrap();
            if p.finished {
                if let Some(e) = p.error.take() {
                    self.status = Some(e);
                }
                drop(p);
                self.job = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
                // Quick copies finish before the dialog would just flash up.
                if now - *started > 0.4 {
                    let pct = if p.total > 0 { (p.done as f64 / p.total as f64 * 100.0) as u32 } else { 0 };
                    egui::Window::new(format!("{pct}% complete")).id(Id::unique("progress")).collapsible(false).resizable(false).default_width(420.0).show(ctx, |ui| {
                        ui.label(egui::RichText::new(format!(
                            "{} {} item{} from {} to {}",
                            if p.cut { "Moving" } else { "Copying" },
                            p.items,
                            if p.items == 1 { "" } else { "s" },
                            p.from,
                            p.to
                        )).color(TEXT));
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(format!("{pct}% complete")).size(17.0).color(GROUP_BLUE));
                        ui.add_space(6.0);
                        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
                        ui.painter().rect_filled(r, 0.0, BAR_BG);
                        ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width() * pct as f32 / 100.0, r.height())), 0.0, COPY_GREEN);
                        ui.painter().rect_stroke(r, 0.0, (1.0, BAR_LINE), StrokeKind::Inside);
                        ui.add_space(6.0);
                        ui.label(format!("Name: {}", p.current));
                        ui.label(format!("{} of {}", human_size_exact(p.done), human_size_exact(p.total)));
                        ui.add_space(6.0);
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(egui::Button::new("Cancel").min_size(vec2(72.0, 24.0))).clicked() {
                                p.cancel = true;
                            }
                        });
                    });
                }
            }
        }
    }
}

impl eframe::App for Exploder {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut v = egui::Visuals::light();
        v.selection.bg_fill = SELECTED;
        v.selection.stroke = Stroke::new(1.0, TEXT);
        v.panel_fill = WHITE;
        v.window_fill = Color32::from_rgb(0xf2, 0xf2, 0xf2);
        v.window_stroke = Stroke::new(1.0, Color32::from_rgb(0xcc, 0xcc, 0xcc));
        v.window_corner_radius = 0.into();
        v.menu_corner_radius = 0.into();
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(0x50, 0x50, 0x50);
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = 0.into();
        }
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x91, 0xc9, 0xf7);
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        ctx.set_visuals(v);

        // Results and messages from background threads.
        if self.dirty.swap(false, Ordering::Relaxed) {
            self.refresh();
        }
        if let Some(n) = self.notice.lock().unwrap().take() {
            self.status = Some(n);
        }
        if let Some(rx) = &self.search_rx {
            let new: Vec<Entry> = rx.try_iter().collect();
            if !new.is_empty() {
                self.entries.extend(new);
                self.sort_entries();
            }
        }
        if self.searching.load(Ordering::Relaxed) {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        while let Ok((path, img)) = self.thumb_rx.try_recv() {
            if let (Some(img), Some(slot)) = (img, self.thumbs.get_mut(&path)) {
                *slot = Some(ctx.load_texture(path.to_string_lossy(), img, egui::TextureOptions::LINEAR));
            }
        }
        // Explorer notices new files without F5; check the folder's mtime once a second.
        let now = ctx.input(|i| i.time);
        if now - self.last_poll > 1.0 && self.renaming.is_none() {
            self.last_poll = now;
            let m = match &self.loc {
                Loc::Dir(d) => Some(mtime(d)),
                Loc::Trash => Some(mtime(&trash_dir().join("files"))),
                _ => None,
            };
            if m.is_some_and(|m| m != self.dir_mtime) {
                self.refresh();
            }
        }
        ctx.request_repaint_after(Duration::from_secs(1));

        let mut acts = Vec::new();
        self.keys(&ctx, &mut acts);

        egui::Panel::top("ribbon").frame(egui::Frame::new().fill(WHITE)).show_separator_line(false).show(ui, |ui| self.ribbon(ui, &mut acts));
        egui::Panel::top("address")
            .frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin { left: 6, right: 8, top: 6, bottom: 6 }))
            .show_separator_line(false)
            .show(ui, |ui| self.address_bar(ui, &mut acts));
        egui::Panel::bottom("status").frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin::symmetric(10, 2))).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_height(22.0);
                let n = if self.loc == Loc::ThisPc { user_folders().len() + self.drives.len() } else { self.shown.len() };
                let small = |s: String| egui::RichText::new(s).color(TEXT).size(12.0);
                ui.label(small(format!("{n} item{}", if n == 1 { "" } else { "s" })));
                if !self.selected.is_empty() {
                    let bytes: u64 = self.selected.iter().filter_map(|&i| self.entries.get(i)).filter(|e| !e.is_dir).map(|e| e.size).sum();
                    let k = self.selected.len();
                    ui.add_space(16.0);
                    ui.label(small(format!("{k} item{} selected  {}", if k == 1 { "" } else { "s" }, if bytes > 0 { human_size_exact(bytes) } else { String::new() })));
                }
                if self.searching.load(Ordering::Relaxed) {
                    ui.add_space(16.0);
                    ui.label(small("Searching…".into()));
                }
                if let Some(s) = &self.status {
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new(s).color(ERROR).size(12.0));
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    for (v, icon, tip) in [(View::LargeIcons, "view-list-icons", "Display items by using large thumbnails"), (View::Details, "view-list-details", "Display information about each item in the window")] {
                        let (r, resp) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::click());
                        if self.view == v {
                            ui.painter().rect_filled(r, 0.0, SELECTED);
                            ui.painter().rect_stroke(r, 0.0, (1.0, SELECTED_LINE), StrokeKind::Inside);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 0.0, HOVER);
                        }
                        paint_icon(ui, icon, Rect::from_center_size(r.center(), vec2(16.0, 16.0)), 1.0);
                        if resp.on_hover_text(tip).clicked() {
                            acts.push(Act::SetView(v));
                        }
                    }
                });
            });
        });
        if self.nav_pane {
            egui::Panel::left("nav")
                .resizable(true)
                .default_size(210.0)
                .size_range(130.0..=420.0)
                .frame(egui::Frame::new().fill(WHITE))
                .show(ui, |ui| self.nav(ui, &mut acts));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(WHITE)).show(ui, |ui| self.content(ui, &mut acts));

        self.dialogs(&ctx, &mut acts);
        for a in acts {
            self.apply(&ctx, a);
        }

        let title = self.loc_name(&self.loc);
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

// ---- Ribbon widgets ----

fn group(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    let r = ui
        .vertical(|ui| {
            let row = ui.horizontal_top(|ui| {
                ui.set_min_height(70.0);
                add(ui);
            });
            let w = row.response.rect.width().max(text_w(ui, label, 11.5) + 12.0);
            let (lr, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
            ui.painter().text(pos2(lr.center().x, lr.center().y), Align2::CENTER_CENTER, label, FontId::proportional(11.5), DIM);
        })
        .response
        .rect;
    ui.add_space(4.0);
    ui.painter().vline(ui.cursor().left(), egui::Rangef::new(r.top() + 4.0, r.bottom() - 4.0), (1.0, RIBBON_LINE));
    ui.add_space(5.0);
}

fn button_bg(ui: &Ui, rect: Rect, resp: &egui::Response, on: bool) {
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.rect_filled(rect, 0.0, PRESSED);
        p.rect_stroke(rect, 0.0, (1.0, SELECTED_LINE), StrokeKind::Inside);
    } else if on {
        p.rect_filled(rect, 0.0, SELECTED);
        p.rect_stroke(rect, 0.0, (1.0, SELECTED_LINE), StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, HOVER);
        p.rect_stroke(rect, 0.0, (1.0, HOVER_LINE), StrokeKind::Inside);
    }
}

/// A large ribbon button: 32px icon over a (possibly two-line) label.
fn big(ui: &mut Ui, icon: &str, label: &str, enabled: bool, dropdown: bool) -> egui::Response {
    let color = if enabled { TEXT } else { FAINT };
    let mut job = egui::text::LayoutJob::simple(label.to_owned(), FontId::proportional(12.0), color, f32::INFINITY);
    job.halign = Align::Center;
    let g = ui.painter().layout_job(job);
    let w = (g.size().x + 12.0).max(44.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 70.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled {
        button_bg(ui, rect, &resp, false);
    }
    paint_icon(ui, icon, Rect::from_center_size(pos2(rect.center().x, rect.top() + 20.0), vec2(30.0, 30.0)), if enabled { 1.0 } else { 0.35 });
    let h = g.size().y;
    ui.painter().galley(pos2(rect.center().x, rect.top() + 39.0), g, color);
    if dropdown {
        tri_down(ui.painter(), pos2(rect.center().x, rect.top() + 43.0 + h), color);
    }
    resp
}

fn toggle_big(ui: &mut Ui, icon: &str, label: &str, on: bool) -> egui::Response {
    let mut job = egui::text::LayoutJob::simple(label.to_owned(), FontId::proportional(12.0), TEXT, f32::INFINITY);
    job.halign = Align::Center;
    let g = ui.painter().layout_job(job);
    let (rect, resp) = ui.allocate_exact_size(vec2((g.size().x + 12.0).max(44.0), 70.0), Sense::click());
    button_bg(ui, rect, &resp, on);
    paint_icon(ui, icon, Rect::from_center_size(pos2(rect.center().x, rect.top() + 20.0), vec2(30.0, 30.0)), 1.0);
    ui.painter().galley(pos2(rect.center().x, rect.top() + 39.0), g, TEXT);
    resp
}

/// A small ribbon button: 16px icon and a label on one 22px row.
fn small(ui: &mut Ui, icon: &str, label: &str, enabled: bool) -> egui::Response {
    let color = if enabled { TEXT } else { FAINT };
    let w = text_w(ui, label, 12.0) + 32.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled {
        button_bg(ui, rect, &resp, false);
    }
    paint_icon(ui, icon, Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(16.0, 16.0)), if enabled { 1.0 } else { 0.35 });
    ui.painter().text(pos2(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.0), color);
    resp
}

fn nav_button(ui: &mut Ui, icon: &str, enabled: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, HOVER);
    }
    paint_icon(ui, icon, Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), if enabled { 1.0 } else { 0.3 });
    if enabled { resp.on_hover_text(tip) } else { resp }
}

fn menu_item(ui: &mut Ui, icon: &str, label: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, HOVER);
    }
    paint_icon(ui, icon, Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(22.0, 22.0)), 1.0);
    ui.painter().text(pos2(rect.left() + 38.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.0), TEXT);
    resp.clicked()
}

fn dest_menu(resp: &egui::Response, dests: &[(String, PathBuf)], cut: bool, acts: &mut Vec<Act>) {
    egui::Popup::menu(resp).show(|ui| {
        ui.set_min_width(180.0);
        for (n, p) in dests {
            if ui.button(n).clicked() {
                acts.push(if cut { Act::MoveTo(p.clone()) } else { Act::CopyTo(p.clone()) });
            }
        }
    });
}

/// A This PC group header: blue title, then a rule to the right edge.
fn section(ui: &mut Ui, title: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    chevron(ui.painter(), pos2(r.left() + 14.0, r.center().y), Dir::Down, 3.5, GROUP_BLUE);
    let tr = paint_text(ui, pos2(r.left() + 26.0, r.center().y), Align2::LEFT_CENTER, title, 14.5, GROUP_BLUE, r.width() - 40.0, 1);
    ui.painter().hline(egui::Rangef::new(tr.right() + 8.0, r.right() - 12.0), r.center().y + 1.0, (1.0, Color32::from_rgb(0xe2, 0xe2, 0xe2)));
}

// ---- Painting helpers ----

#[derive(Clone, Copy)]
enum Dir {
    Up,
    Down,
    Right,
}

fn chevron(p: &egui::Painter, c: Pos2, dir: Dir, size: f32, color: Color32) {
    let (a, t, b) = match dir {
        Dir::Down => (vec2(-1.0, -0.5), vec2(0.0, 0.5), vec2(1.0, -0.5)),
        Dir::Up => (vec2(-1.0, 0.5), vec2(0.0, -0.5), vec2(1.0, 0.5)),
        Dir::Right => (vec2(-0.5, -1.0), vec2(0.5, 0.0), vec2(-0.5, 1.0)),
    };
    let s = Stroke::new(1.2, color);
    p.line_segment([c + a * size, c + t * size], s);
    p.line_segment([c + t * size, c + b * size], s);
}

fn tri_down(p: &egui::Painter, c: Pos2, color: Color32) {
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-3.0, -1.5), c + vec2(3.0, -1.5), c + vec2(0.0, 1.5)], color, Stroke::NONE));
}

/// Quick access's blue star.
fn star(p: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let pt = |k: usize, rad: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
        c + vec2(a.cos(), a.sin()) * rad
    };
    let inner = r * 0.42;
    p.add(egui::Shape::convex_polygon((0..5).map(|k| pt(2 * k + 1, inner)).collect(), color, Stroke::NONE));
    for k in 0..5 {
        p.add(egui::Shape::convex_polygon(vec![pt(2 * k, r), pt(2 * k + 1, inner), pt((2 * k + 9) % 10, inner)], color, Stroke::NONE));
    }
}

/// The drive Properties pie: used space blue, free grey, with a hole in the middle.
fn donut(p: &egui::Painter, c: Pos2, r: f32, used: f32) {
    let steps = 120;
    for k in 0..steps {
        let a0 = -std::f32::consts::FRAC_PI_2 + k as f32 / steps as f32 * std::f32::consts::TAU;
        let a1 = a0 + std::f32::consts::TAU / steps as f32 + 0.01;
        let color = if (k as f32) < used * steps as f32 { BAR_BLUE } else { FREE_GREY };
        p.add(egui::Shape::convex_polygon(vec![c, c + vec2(a0.cos(), a0.sin()) * r, c + vec2(a1.cos(), a1.sin()) * r], color, Stroke::NONE));
    }
    p.circle_filled(c, r * 0.45, Color32::from_rgb(0xf2, 0xf2, 0xf2));
}

fn text_w(ui: &Ui, s: &str, size: f32) -> f32 {
    ui.painter().layout_no_wrap(s.to_owned(), FontId::proportional(size), TEXT).size().x
}

/// Text cut to `max_w` with an ellipsis, at most `rows` lines, placed by `anchor` at `pos`.
#[allow(clippy::too_many_arguments)]
fn paint_text(ui: &Ui, pos: Pos2, anchor: Align2, s: &str, size: f32, color: Color32, max_w: f32, rows: usize) -> Rect {
    let mut job = egui::text::LayoutJob::simple(s.to_owned(), FontId::proportional(size), color, max_w.max(1.0));
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = rows > 1;
    job.wrap.overflow_character = Some('…');
    job.halign = anchor.x();
    let g = ui.painter().layout_job(job);
    let rect = anchor.anchor_size(pos, g.size());
    // Centred and right-aligned galleys are laid out around x = 0.
    let origin = match anchor.x() {
        Align::Center => pos2(rect.center().x, rect.top()),
        Align::Max => pos2(rect.right(), rect.top()),
        Align::Min => rect.min,
    };
    ui.painter().galley(origin, g, color);
    rect
}

/// A theme icon's file. Cached, since every visible row asks every frame.
fn icon_path(name: &str, size: u32) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<(String, bool), Option<String>>>> = OnceLock::new();
    let big = size > 32;
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    cache
        .entry((name.to_string(), big))
        .or_insert_with(|| {
            // Breeze's 16px places and mimetypes are monochrome; 32 and up are in colour, like Explorer's.
            let sizes: &[u32] = if big { &[96, 64, 48, 32, 24, 22, 16] } else { &[32, 48, 64, 24, 22, 16] };
            for theme in ["breeze", "Adwaita", "hicolor"] {
                for s in sizes {
                    for d in ["places", "mimetypes", "devices", "actions", "status", "emblems", "apps"] {
                        let p = format!("/usr/share/icons/{theme}/{d}/{s}/{name}.svg");
                        if Path::new(&p).exists() {
                            return Some(p);
                        }
                    }
                }
            }
            None
        })
        .clone()
}

fn paint_icon(ui: &Ui, name: &str, rect: Rect, alpha: f32) {
    if let Some(p) = icon_path(name, rect.width() as u32) {
        egui::Image::new(format!("file://{p}")).fit_to_exact_size(rect.size()).tint(Color32::WHITE.gamma_multiply(alpha)).paint_at(ui, rect);
    }
}

fn is_image(e: &Entry) -> bool {
    let ext = e.path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
    !e.is_dir && e.size < 64 << 20 && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
}

/// Decodes thumbnails one at a time off the UI thread. Requests from an older
/// folder (`gen` moved on) are skipped.
#[allow(clippy::type_complexity)]
fn thumb_worker(ctx: egui::Context, current: Arc<AtomicUsize>) -> (mpsc::Sender<(usize, PathBuf)>, mpsc::Receiver<(PathBuf, Option<egui::ColorImage>)>) {
    let (req_tx, req_rx) = mpsc::channel::<(usize, PathBuf)>();
    let (res_tx, res_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for (gen_, path) in req_rx {
            if gen_ != current.load(Ordering::Relaxed) {
                continue;
            }
            let img = image::open(&path).ok().map(|i| {
                let t = i.thumbnail(THUMB, THUMB).to_rgba8();
                egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw())
            });
            if res_tx.send((path, img)).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
    (req_tx, res_rx)
}

// ---- Files ----

fn entry(path: PathBuf, location: String) -> Option<Entry> {
    let lmd = std::fs::symlink_metadata(&path).ok()?;
    // Follow links for size and type; a broken link still shows, as itself.
    let md = std::fs::metadata(&path).unwrap_or_else(|_| lmd.clone());
    let is_dir = md.is_dir();
    let (kind, icon) = describe(&path, is_dir);
    Some(Entry {
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string()),
        path,
        is_dir,
        is_link: lmd.file_type().is_symlink(),
        size: md.len(),
        modified: md.modified().ok(),
        kind,
        icon,
        location,
    })
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).ok()?.modified().ok()
}

/// "Type" column text and a theme icon name, from the file's MIME type.
fn describe(path: &Path, is_dir: bool) -> (String, &'static str) {
    if is_dir {
        return ("File folder".into(), folder_icon(path));
    }
    let mime = mime_guess::from_path(path).first();
    let ext = path.extension().map(|e| e.to_string_lossy().to_uppercase());
    let ext_l = ext.as_deref().unwrap_or("").to_lowercase();
    let kind = match ext_l.as_str() {
        "txt" => "Text Document".into(),
        "desktop" => "Shortcut".into(),
        "zip" => "Compressed (zipped) Folder".into(),
        "sh" => "Shell Script".into(),
        _ => match &ext {
            Some(e) => format!("{e} File"),
            None => "File".into(),
        },
    };
    // A small fixed set of icons, like Explorer. One texture per distinct icon, and on
    // NVIDIA each costs far more GPU memory than its pixels, so per-MIME icons made
    // memory grow with every new file type browsed.
    let top = mime.as_ref().map(|m| m.type_().as_str().to_string()).unwrap_or_default();
    let icon = match (top.as_str(), ext_l.as_str()) {
        ("image", _) => "image-x-generic",
        ("video", _) => "video-x-generic",
        ("audio", _) => "audio-x-generic",
        (_, "pdf") => "application-pdf",
        (_, "zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "rar" | "bz2") => "package-x-generic",
        (_, "sh" | "bash" | "zsh" | "fish") => "application-x-shellscript",
        (_, "appimage" | "exe" | "bin" | "run") => "application-x-executable",
        (_, "rs" | "c" | "h" | "cpp" | "py" | "js" | "ts" | "go" | "java" | "html" | "css" | "json" | "toml" | "yaml" | "yml") => "text-x-script",
        _ => "text-x-generic",
    };
    (kind, icon)
}

fn folder_icon(p: &Path) -> &'static str {
    if p == home() {
        return "user-home";
    }
    if p == Path::new("/") {
        return "drive-harddisk";
    }
    user_folders().into_iter().find(|(_, f, _)| f == p).map(|(_, _, i)| i).unwrap_or("folder")
}

fn dir_name(p: &Path) -> String {
    match p.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => "Local Disk (/)".into(),
    }
}

/// Explorer sorts "file2" before "file10".
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (mt, nt) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let ord = mt.len().cmp(&nt.len()).then_with(|| mt.cmp(nt));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

/// `path`, or the first free variant of it built by `name(stem, ".ext", n)` for n = 2, 3...
fn unique(path: &Path, name: impl Fn(&str, &str, u32) -> String) -> PathBuf {
    if std::fs::symlink_metadata(path).is_err() {
        return path.to_path_buf();
    }
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = match file.rfind('.') {
        Some(i) if i > 0 && !path.is_dir() => (file[..i].to_string(), file[i..].to_string()),
        _ => (file.clone(), String::new()),
    };
    (2..)
        .map(|n| path.with_file_name(name(&stem, &ext, n)))
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .unwrap()
}

fn human_size(b: u64) -> String {
    // Explorer's Size column is always whole KB and up.
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut v = b as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{} KB", commas(v.ceil() as u64)) } else { format!("{:.0} {}", v.max(1.0), UNITS[u]) }
}

/// "1.25 MB" style, with bytes below 1 KB.
fn human_size_exact(b: u64) -> String {
    if b < 1024 {
        return format!("{b} bytes");
    }
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut v = b as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v < 10.0 { format!("{v:.2} {}", UNITS[u]) } else if v < 100.0 { format!("{v:.1} {}", UNITS[u]) } else { format!("{v:.0} {}", UNITS[u]) }
}

fn disk_size(b: u64) -> String {
    let gb = b as f64 / (1u64 << 30) as f64;
    if gb >= 1000.0 { format!("{:.1} TB", gb / 1024.0) } else if gb >= 100.0 { format!("{gb:.0} GB") } else { format!("{gb:.1} GB") }
}

fn commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn fmt_time(t: Option<SystemTime>) -> String {
    t.map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%-m/%-d/%Y %-I:%M %p").to_string()).unwrap_or_default()
}

fn fmt_long(t: Option<SystemTime>) -> String {
    t.map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%A, %B %-d, %Y, %-I:%M:%S %p").to_string()).unwrap_or_default()
}

fn tree_size(p: &Path) -> u64 {
    let Ok(md) = std::fs::symlink_metadata(p) else { return 0 };
    if md.is_dir() {
        std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| tree_size(&e.path())).sum()).unwrap_or(0)
    } else {
        md.len()
    }
}

fn transfer_job(plan: &[(PathBuf, PathBuf)], cut: bool, prog: &Mutex<Progress>) {
    let sizes: Vec<u64> = plan.iter().map(|(s, _)| tree_size(s)).collect();
    prog.lock().unwrap().total = sizes.iter().sum();
    for ((src, dst), size) in plan.iter().zip(sizes) {
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        {
            let mut p = prog.lock().unwrap();
            if p.cancel {
                return;
            }
            p.current = name.clone();
        }
        let res = (|| -> std::io::Result<()> {
            if cut && std::fs::rename(src, dst).is_ok() {
                prog.lock().unwrap().done += size;
                return Ok(());
            }
            // Replacing a folder with a file or the other way round: clear the old one.
            // Folder onto folder merges, as in Explorer.
            if let Ok(dm) = std::fs::symlink_metadata(dst)
                && dm.is_dir() != std::fs::symlink_metadata(src)?.is_dir()
            {
                remove_rec(dst)?;
            }
            copy_tree(src, dst, prog)?;
            if cut {
                remove_rec(src)?;
            }
            Ok(())
        })();
        if let Err(e) = res {
            let mut p = prog.lock().unwrap();
            if !p.cancel {
                p.error = Some(format!("Couldn't {} {name}: {e}", if cut { "move" } else { "copy" }));
            }
            return;
        }
    }
}

fn copy_tree(src: &Path, dst: &Path, prog: &Mutex<Progress>) -> std::io::Result<()> {
    use std::io::{Read, Write};
    let md = std::fs::symlink_metadata(src)?;
    if md.file_type().is_symlink() {
        let target = std::fs::read_link(src)?;
        let _ = std::fs::remove_file(dst);
        return std::os::unix::fs::symlink(target, dst);
    }
    if md.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)?.flatten() {
            copy_tree(&e.path(), &dst.join(e.file_name()), prog)?;
        }
        return Ok(());
    }
    let mut from = std::fs::File::open(src)?;
    let mut to = std::fs::File::create(dst)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        to.write_all(&buf[..n])?;
        let mut p = prog.lock().unwrap();
        p.done += n as u64;
        if p.cancel {
            drop(to);
            let _ = std::fs::remove_file(dst);
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
        }
    }
    let _ = std::fs::set_permissions(dst, md.permissions());
    Ok(())
}

fn remove_rec(p: &Path) -> std::io::Result<()> {
    // symlink_metadata, so a link to a folder loses the link, not the folder's contents.
    if std::fs::symlink_metadata(p)?.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }
}

fn set_readonly(paths: &[PathBuf], ro: bool) {
    use std::os::unix::fs::PermissionsExt;
    for p in paths {
        if let Ok(md) = std::fs::metadata(p) {
            let mut perm = md.permissions();
            perm.set_mode(if ro { perm.mode() & !0o222 } else { perm.mode() | 0o200 });
            let _ = std::fs::set_permissions(p, perm);
        }
    }
}

fn props_for(paths: Vec<PathBuf>, drive: Option<Drive>) -> Props {
    use std::os::unix::fs::PermissionsExt;
    let single = paths.len() == 1;
    let first = paths[0].clone();
    let md = if single { std::fs::metadata(&first).ok() } else { None };
    let is_dir = |p: &PathBuf| p.is_dir();
    let (title, kind, icon) = if let Some(d) = &drive {
        (d.label.clone(), if d.removable { "Removable Disk".into() } else { "Local Disk".into() }, if d.removable { "drive-removable-media" } else { "drive-harddisk" })
    } else if single {
        let (kind, icon) = describe(&first, first.is_dir());
        (dir_name(&first), kind, icon)
    } else {
        let folders = paths.iter().filter(|p| is_dir(p)).count();
        let files = paths.len() - folders;
        (format!("{files} Files, {folders} Folders"), "Multiple Types".into(), "folder")
    };
    let location = first.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let readonly = paths.iter().all(|p| std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o222 == 0));
    let totals = Arc::new(Mutex::new((0, 0, 0, false)));
    if drive.is_none() {
        let t = totals.clone();
        let roots = paths.clone();
        std::thread::spawn(move || {
            // Walk a stack so deep trees can't overflow, and publish as we go. A selected
            // folder isn't counted in "Contains", only what's inside it.
            let mut stack: Vec<(PathBuf, bool)> = roots.into_iter().map(|p| (p, true)).collect();
            while let Some((p, root)) = stack.pop() {
                let Ok(md) = std::fs::symlink_metadata(&p) else { continue };
                if md.is_dir() {
                    if !root {
                        t.lock().unwrap().2 += 1;
                    }
                    if let Ok(rd) = std::fs::read_dir(&p) {
                        stack.extend(rd.flatten().map(|e| (e.path(), false)));
                    }
                } else {
                    let mut tot = t.lock().unwrap();
                    tot.0 += md.len();
                    tot.1 += 1;
                }
            }
            t.lock().unwrap().3 = true;
        });
    }
    let location = if drive.is_some() { String::new() } else { location };
    Props { paths, title, icon, kind, location, md, drive, totals, readonly, readonly_was: readonly }
}

// ---- Places ----

/// User folders shown under This PC (only the ones that exist).
fn user_folders() -> Vec<(String, PathBuf, &'static str)> {
    let h = home();
    [
        ("Desktop", "user-desktop"),
        ("Documents", "folder-documents"),
        ("Downloads", "folder-download"),
        ("Music", "folder-music"),
        ("Pictures", "folder-pictures"),
        ("Videos", "folder-videos"),
    ]
    .into_iter()
    .map(|(n, i)| (n.to_string(), h.join(n), i))
    .filter(|(_, p, _)| p.exists())
    .collect()
}

/// Real disks from /proc/mounts: the root, a separate /home, and removable media.
fn drives() -> Vec<Drive> {
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut out = Vec::new();
    for line in mounts.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 || !f[0].starts_with("/dev/") {
            continue;
        }
        let mnt = f[1].replace("\\040", " ");
        let removable = mnt.starts_with("/run/media") || mnt.starts_with("/media") || mnt.starts_with("/mnt");
        let label = match mnt.as_str() {
            "/" | "/home" => format!("Local Disk ({mnt})"),
            _ if removable => mnt.rsplit('/').next().unwrap_or(&mnt).to_string(),
            _ => continue,
        };
        if out.iter().any(|d: &Drive| d.path == Path::new(&mnt)) {
            continue;
        }
        let path = PathBuf::from(&mnt);
        out.push(Drive { label, usage: disk_usage(&path), path, fs: f[2].to_string(), removable });
    }
    if !out.iter().any(|d| d.path == Path::new("/")) {
        out.insert(0, Drive { label: "Local Disk (/)".into(), path: "/".into(), fs: String::new(), removable: false, usage: disk_usage(Path::new("/")) });
    }
    out.sort_by_key(|d| (d.removable, d.path.clone()));
    out
}

/// Free and total bytes on the filesystem holding `p`.
fn disk_usage(p: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs only writes into the struct we pass, and `c` is a valid C string.
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let bs = s.f_frsize as u64;
    Some((s.f_bavail as u64 * bs, s.f_blocks as u64 * bs))
}

fn trash_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share"));
    data.join("Trash")
}

fn trash_info(trashed_name: &str) -> PathBuf {
    trash_dir().join("info").join(format!("{trashed_name}.trashinfo"))
}

/// What's in the (home) Recycle Bin, with original names, locations and deletion dates.
fn trash_entries() -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(trash_dir().join("files")) else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| {
            let tname = e.file_name().to_string_lossy().into_owned();
            let info = std::fs::read_to_string(trash_info(&tname)).unwrap_or_default();
            let orig = info.lines().find_map(|l| l.strip_prefix("Path=")).map(percent_decode).unwrap_or_default();
            let deleted = info
                .lines()
                .find_map(|l| l.strip_prefix("DeletionDate="))
                .and_then(|d| chrono::NaiveDateTime::parse_from_str(d.trim(), "%Y-%m-%dT%H:%M:%S").ok())
                .and_then(|d| d.and_local_timezone(chrono::Local).single())
                .map(SystemTime::from);
            let orig = Path::new(&orig);
            let mut en = entry(e.path(), orig.parent().map(|p| p.display().to_string()).unwrap_or_default())?;
            // The trash renames duplicates ("a.txt.2"); show the name it had.
            if let Some(n) = orig.file_name() {
                en.name = n.to_string_lossy().into_owned();
            }
            en.modified = deleted.or(en.modified);
            Some(en)
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"))
}

fn pins_file() -> PathBuf {
    config_dir().join("vide-de/quick-access")
}

/// Quick access pins, one path per line. New users get Explorer's default four.
fn load_pins() -> Vec<PathBuf> {
    match std::fs::read_to_string(pins_file()) {
        Ok(s) => s.lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).collect(),
        Err(_) => ["Desktop", "Downloads", "Documents", "Pictures"].iter().map(|n| home().join(n)).filter(|p| p.exists()).collect(),
    }
}

fn save_pins(pins: &[PathBuf]) {
    let f = pins_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, pins.iter().map(|p| format!("{}\n", p.display())).collect::<String>());
}

// ---- Running things ----

fn powerslop() -> Option<PathBuf> {
    let ours = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join("powerslop"))).filter(|p| p.exists());
    ours.or_else(|| std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join("powerslop")).find(|p| p.exists()))
}

/// Open our terminal in `dir` (PowerSlop next to us, else the user's).
fn spawn_terminal(dir: &Path) {
    let mut cmd = match powerslop() {
        Some(p) => std::process::Command::new(p),
        None => std::process::Command::new("konsole"),
    };
    let _ = spawn(cmd.current_dir(dir));
}

/// How double-click should run a file, if it's something runnable.
#[derive(Clone, Copy, PartialEq)]
enum Run {
    /// A script, shown in a console. The interpreter is set when the file isn't
    /// executable, since Explorer doesn't make you chmod a .bat first.
    Script(Option<&'static str>),
    /// A native program or AppImage.
    Program,
    /// A .desktop launcher.
    Launcher,
}

fn runnable(path: &Path) -> Option<Run> {
    use std::{io::Read, os::unix::fs::PermissionsExt};
    let md = std::fs::metadata(path).ok()?;
    if !md.is_file() {
        return None;
    }
    let exec = md.permissions().mode() & 0o111 != 0;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "desktop" {
        return Some(Run::Launcher);
    }
    let mut head = [0u8; 4];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let head = &head[..n];
    if head == b"\x7fELF" || ext == "appimage" {
        return Some(Run::Program);
    }
    let shebang = head.starts_with(b"#!");
    let by_ext = match ext.as_str() {
        "sh" | "bash" => Some("bash"),
        "zsh" => Some("zsh"),
        "fish" => Some("fish"),
        "py" => Some("python3"),
        "pl" => Some("perl"),
        "rb" => Some("ruby"),
        "js" | "mjs" if shebang => Some("node"),
        "run" => Some("sh"),
        _ => None,
    };
    match (exec, shebang, by_ext) {
        (true, true, _) => Some(Run::Script(None)),
        // Not executable: the kernel won't read the shebang, so name the interpreter.
        (false, true, i) => Some(Run::Script(Some(i.unwrap_or("sh")))),
        (_, false, Some(i)) => Some(Run::Script(Some(i))),
        _ => None,
    }
}

fn run_in_terminal(path: &Path, interp: Option<&str>) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("/"));
    let target = path.to_string_lossy().into_owned();
    let argv: Vec<String> = match interp {
        Some(i) => vec![i.to_string(), target],
        None => vec![target],
    };
    let mut cmd = match powerslop() {
        Some(p) => {
            let mut c = std::process::Command::new(p);
            c.arg("-e");
            c
        }
        None => {
            let mut c = std::process::Command::new("konsole");
            c.args(["--hold", "-e"]);
            c
        }
    };
    spawn(cmd.args(argv).current_dir(dir))
}

fn run_program(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path)?.permissions();
    if perm.mode() & 0o100 == 0 {
        // Downloaded AppImages and binaries arrive without +x; double-click means "run it".
        perm.set_mode(perm.mode() | 0o111);
        std::fs::set_permissions(path, perm)?;
    }
    spawn(std::process::Command::new(path).current_dir(path.parent().unwrap_or(Path::new("/"))))
}

/// Start a child in its own process group and reap it on a thread, so finished apps
/// don't linger as zombies while we're open.
fn spawn(cmd: &mut std::process::Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut child = cmd.process_group(0).stdin(std::process::Stdio::null()).spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn shellexpand(s: &str) -> String {
    match s.strip_prefix('~') {
        Some(rest) => format!("{}{rest}", home().display()),
        None => s.to_string(),
    }
}
