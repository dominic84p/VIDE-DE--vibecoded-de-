//! File Exploder: VIDE-DE's file manager, modelled on Windows 10 File Explorer.
//! Light theme, navigation pane, breadcrumb address bar, details view, status bar.
//!
//! Double-click opens (folders here, files with xdg-open). F2 rename, Delete to
//! trash, Ctrl+C/X/V, Ctrl+Shift+N new folder, Alt+Left/Right/Up, Backspace, F5.

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use eframe::egui::{self, Color32, Key, Modifiers, RichText, Sense, Vec2};

const WHITE: Color32 = Color32::WHITE;
const NAV_BG: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const BORDER: Color32 = Color32::from_rgb(0xe5, 0xe5, 0xe5);
const HOVER: Color32 = Color32::from_rgb(0xe5, 0xf3, 0xff);
const SELECTED: Color32 = Color32::from_rgb(0xcc, 0xe8, 0xff);
const TEXT: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
const DIM: Color32 = Color32::from_rgb(0x6d, 0x6d, 0x6d);
const ROW_H: f32 = 22.0;

fn main() -> eframe::Result {
    let start = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(home);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("File Exploder")
            .with_app_id("file-exploder")
            .with_inner_size([1000.0, 620.0])
            .with_min_inner_size([520.0, 320.0]),
        ..Default::default()
    };
    eframe::run_native(
        "File Exploder",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(Exploder::new(start)))
        }),
    )
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    size: u64,
    modified: Option<SystemTime>,
    kind: String,
    icon: String,
}

#[derive(Clone, Copy, PartialEq)]
enum SortBy {
    Name,
    Modified,
    Kind,
    Size,
}

#[derive(Clone)]
enum Clip {
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
}

struct Exploder {
    cwd: PathBuf,
    entries: Vec<Entry>,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    selected: Vec<usize>,
    sort: (SortBy, bool),
    search: String,
    address: Option<String>,
    renaming: Option<(usize, String)>,
    clipboard: Option<Clip>,
    status: Option<String>,
    show_hidden: bool,
}

impl Exploder {
    fn new(start: PathBuf) -> Self {
        let mut s = Self {
            cwd: start,
            entries: Vec::new(),
            back: Vec::new(),
            forward: Vec::new(),
            selected: Vec::new(),
            sort: (SortBy::Name, true),
            search: String::new(),
            address: None,
            renaming: None,
            clipboard: None,
            status: None,
            show_hidden: false,
        };
        s.reload();
        s
    }

    fn reload(&mut self) {
        self.entries.clear();
        self.selected.clear();
        self.renaming = None;
        match std::fs::read_dir(&self.cwd) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') && !self.show_hidden {
                        continue;
                    }
                    let md = std::fs::metadata(e.path()).ok();
                    let is_dir = md.as_ref().is_some_and(|m| m.is_dir());
                    let (kind, icon) = describe(&e.path(), is_dir);
                    self.entries.push(Entry {
                        path: e.path(),
                        name,
                        is_dir,
                        size: md.as_ref().map(|m| m.len()).unwrap_or(0),
                        modified: md.and_then(|m| m.modified().ok()),
                        kind,
                        icon,
                    });
                }
                self.status = None;
            }
            Err(e) => self.status = Some(format!("Can't open {}: {e}", self.cwd.display())),
        }
        self.sort_entries();
    }

    fn sort_entries(&mut self) {
        let (by, asc) = self.sort;
        self.entries.sort_by(|a, b| {
            // Folders first, like Explorer.
            let ord = b.is_dir.cmp(&a.is_dir).then_with(|| match by {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Modified => a.modified.cmp(&b.modified),
                SortBy::Kind => a.kind.cmp(&b.kind),
                SortBy::Size => a.size.cmp(&b.size),
            });
            if asc || a.is_dir != b.is_dir { ord } else { ord.reverse() }
        });
    }

    fn go(&mut self, path: PathBuf) {
        if path == self.cwd {
            return;
        }
        self.back.push(std::mem::replace(&mut self.cwd, path));
        self.forward.clear();
        self.search.clear();
        self.reload();
    }

    fn go_back(&mut self) {
        if let Some(p) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.cwd, p));
            self.reload();
        }
    }

    fn go_forward(&mut self) {
        if let Some(p) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.cwd, p));
            self.reload();
        }
    }

    fn go_up(&mut self) {
        if let Some(parent) = self.cwd.parent().map(Path::to_path_buf) {
            self.go(parent);
        }
    }

    fn open(&mut self, i: usize) {
        let Some(e) = self.entries.get(i).cloned() else { return };
        if e.is_dir {
            self.go(e.path);
        } else if let Err(err) = std::process::Command::new("xdg-open").arg(&e.path).spawn() {
            self.status = Some(format!("Couldn't open {}: {err}", e.name));
        }
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected.iter().filter_map(|&i| self.entries.get(i)).map(|e| e.path.clone()).collect()
    }

    fn delete_selected(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        // To the trash, never a hard delete.
        let res = std::process::Command::new("gio").arg("trash").args(&paths).status();
        if !res.is_ok_and(|s| s.success()) {
            self.status = Some("Couldn't move to Recycle Bin (is gio installed?)".into());
        }
        self.reload();
    }

    fn new_folder(&mut self) {
        let mut name = "New folder".to_string();
        let mut n = 2;
        while self.cwd.join(&name).exists() {
            name = format!("New folder ({n})");
            n += 1;
        }
        if let Err(e) = std::fs::create_dir(self.cwd.join(&name)) {
            self.status = Some(format!("Couldn't create folder: {e}"));
            return;
        }
        self.reload();
        if let Some(i) = self.entries.iter().position(|e| e.name == name) {
            self.selected = vec![i];
            self.renaming = Some((i, name));
        }
    }

    fn paste(&mut self) {
        let Some(clip) = self.clipboard.clone() else { return };
        let (paths, cut) = match clip {
            Clip::Copy(p) => (p, false),
            Clip::Cut(p) => (p, true),
        };
        for src in paths {
            let Some(name) = src.file_name() else { continue };
            let dst = self.cwd.join(name);
            if dst == src {
                continue;
            }
            let res = if cut {
                std::fs::rename(&src, &dst).or_else(|_| copy_rec(&src, &dst).and_then(|_| remove_rec(&src)))
            } else {
                copy_rec(&src, &dst)
            };
            if let Err(e) = res {
                self.status = Some(format!("Couldn't paste {}: {e}", name.to_string_lossy()));
            }
        }
        if cut {
            self.clipboard = None;
        }
        self.reload();
    }

    fn commit_rename(&mut self) {
        let Some((i, new)) = self.renaming.take() else { return };
        let Some(e) = self.entries.get(i) else { return };
        if new.trim().is_empty() || new == e.name {
            return;
        }
        if let Err(err) = std::fs::rename(&e.path, self.cwd.join(new.trim())) {
            self.status = Some(format!("Couldn't rename: {err}"));
        }
        self.reload();
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if self.renaming.is_some() || self.address.is_some() {
            return;
        }
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if pressed(Modifiers::ALT, Key::ArrowLeft) || pressed(Modifiers::NONE, Key::Backspace) {
            self.go_back();
        }
        if pressed(Modifiers::ALT, Key::ArrowRight) {
            self.go_forward();
        }
        if pressed(Modifiers::ALT, Key::ArrowUp) {
            self.go_up();
        }
        if pressed(Modifiers::NONE, Key::F5) {
            self.reload();
        }
        if pressed(Modifiers::NONE, Key::F2) {
            if let Some(&i) = self.selected.first() {
                self.renaming = Some((i, self.entries[i].name.clone()));
            }
        }
        if pressed(Modifiers::NONE, Key::Delete) {
            self.delete_selected();
        }
        if pressed(Modifiers::NONE, Key::Enter) {
            if let Some(&i) = self.selected.first() {
                self.open(i);
            }
        }
        if pressed(Modifiers::CTRL, Key::A) {
            self.selected = (0..self.entries.len()).collect();
        }
        if pressed(Modifiers::CTRL | Modifiers::SHIFT, Key::N) {
            self.new_folder();
        }
        if pressed(Modifiers::CTRL, Key::L) {
            self.address = Some(self.cwd.display().to_string());
        }
        if pressed(Modifiers::CTRL, Key::H) {
            self.show_hidden = !self.show_hidden;
            self.reload();
        }
        // egui turns Ctrl+C/X/V into events rather than key presses.
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Copy if !self.selected.is_empty() => self.clipboard = Some(Clip::Copy(self.selected_paths())),
                egui::Event::Cut if !self.selected.is_empty() => self.clipboard = Some(Clip::Cut(self.selected_paths())),
                egui::Event::Paste(_) => self.paste(),
                _ => {}
            }
        }
    }
}

/// "Type" column text and a theme icon name, from the file's MIME type.
fn describe(path: &Path, is_dir: bool) -> (String, String) {
    if is_dir {
        return ("File folder".into(), "folder".into());
    }
    let mime = mime_guess::from_path(path).first();
    let ext = path.extension().map(|e| e.to_string_lossy().to_uppercase());
    let kind = match &ext {
        Some(e) => format!("{e} File"),
        None => "File".into(),
    };
    let icon = mime.map(|m| m.essence_str().replace('/', "-")).unwrap_or_else(|| "text-x-generic".into());
    (kind, icon)
}

/// A theme icon's file, from Breeze or hicolor.
fn icon_path(name: &str, size: u32) -> Option<String> {
    let dirs = ["mimetypes", "places", "actions", "devices"];
    for theme in ["breeze", "Adwaita", "hicolor"] {
        for d in dirs {
            for s in [size, 32, 22, 16] {
                let p = format!("/usr/share/icons/{theme}/{d}/{s}/{name}.svg");
                if Path::new(&p).exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn icon(ui: &mut egui::Ui, name: &str, size: f32) {
    let fallback = if name == "folder" { "folder" } else { "text-x-generic" };
    // Breeze's 16px icons are monochrome; the 32px ones are in colour, like Explorer's.
    match icon_path(name, 32).or_else(|| icon_path(fallback, 32)) {
        Some(p) => {
            ui.add(egui::Image::new(format!("file://{p}")).fit_to_exact_size(Vec2::splat(size)));
        }
        None => {
            ui.add_space(size);
        }
    }
}

fn human_size(b: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut v = b as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{:.0} {}", v.max(1.0), UNITS[u])
}

fn fmt_time(t: Option<SystemTime>) -> String {
    t.map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%-m/%-d/%Y %-I:%M %p").to_string())
        .unwrap_or_default()
}

fn copy_rec(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)?.flatten() {
            copy_rec(&e.path(), &dst.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

fn remove_rec(p: &Path) -> std::io::Result<()> {
    if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }
}

/// User folders shown under "This PC" (only the ones that exist).
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

/// Real disks from /proc/mounts.
fn drives() -> Vec<(String, PathBuf)> {
    let Ok(m) = std::fs::read_to_string("/proc/mounts") else { return Vec::new() };
    let mut out = vec![("Local Disk (/)".to_string(), PathBuf::from("/"))];
    for line in m.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() > 2 && f[0].starts_with("/dev/") && (f[1].starts_with("/run/media") || f[1].starts_with("/mnt") || f[1].starts_with("/media")) {
            let label = f[1].rsplit('/').next().unwrap_or(f[1]).replace("\\040", " ");
            out.push((label, PathBuf::from(f[1])));
        }
    }
    out
}

impl eframe::App for Exploder {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut v = egui::Visuals::light();
        v.selection.bg_fill = SELECTED;
        v.panel_fill = WHITE;
        ctx.set_visuals(v);
        self.keys(&ctx);

        // Toolbar: back / forward / up, address bar, search.
        egui::Panel::top("toolbar").frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin::symmetric(8, 6))).show(ui, |ui| {
            ui.horizontal(|ui| {
                let nav = |ui: &mut egui::Ui, name: &str, enabled: bool, tip: &str| {
                    let img = icon_path(name, 16).map(|p| egui::Image::new(format!("file://{p}")).fit_to_exact_size(Vec2::splat(16.0)));
                    match img {
                        Some(img) => ui.add_enabled(enabled, egui::Button::image(img).frame(false)).on_hover_text(tip).clicked(),
                        None => ui.add_enabled(enabled, egui::Button::new(tip)).clicked(),
                    }
                };
                if nav(ui, "go-previous", !self.back.is_empty(), "Back (Alt+Left)") {
                    self.go_back();
                }
                if nav(ui, "go-next", !self.forward.is_empty(), "Forward (Alt+Right)") {
                    self.go_forward();
                }
                if nav(ui, "go-up", self.cwd.parent().is_some(), "Up (Alt+Up)") {
                    self.go_up();
                }
                ui.add_space(6.0);

                let search_w = 220.0;
                let addr_w = ui.available_width() - search_w - 16.0;
                egui::Frame::new().stroke((1.0, BORDER)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                    ui.set_width(addr_w);
                    if let Some(text) = &mut self.address {
                        let r = ui.add(egui::TextEdit::singleline(text).frame(egui::Frame::NONE).desired_width(addr_w));
                        r.request_focus();
                        if r.lost_focus() {
                            let target = PathBuf::from(shellexpand(text));
                            self.address = None;
                            if target.is_dir() {
                                self.go(target);
                            }
                        }
                    } else {
                        // Breadcrumbs: click a segment to jump there, empty space to type a path.
                        let r = ui.horizontal(|ui| {
                            let mut acc = PathBuf::from("/");
                            let mut jump = None;
                            if ui.add(egui::Label::new(RichText::new("This PC").color(TEXT)).sense(Sense::click())).clicked() {
                                jump = Some(PathBuf::from("/"));
                            }
                            for comp in self.cwd.components().skip(1) {
                                acc.push(comp);
                                ui.label(RichText::new("›").color(DIM));
                                let label = comp.as_os_str().to_string_lossy();
                                if ui.add(egui::Label::new(RichText::new(label.as_ref()).color(TEXT)).sense(Sense::click())).clicked() {
                                    jump = Some(acc.clone());
                                }
                            }
                            ui.allocate_space(Vec2::new(ui.available_width(), 16.0));
                            jump
                        });
                        if let Some(p) = r.inner {
                            self.go(p);
                        } else if r.response.interact(Sense::click()).clicked() {
                            self.address = Some(self.cwd.display().to_string());
                        }
                    }
                });
                egui::Frame::new().stroke((1.0, BORDER)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                    let folder = self.cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "This PC".into());
                    ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(format!("Search {folder}")).frame(egui::Frame::NONE).desired_width(search_w - 16.0));
                });
            });
        });

        // Status bar.
        egui::Panel::bottom("status").frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin::symmetric(10, 3))).show(ui, |ui| {
            ui.horizontal(|ui| {
                let n = self.entries.len();
                ui.label(RichText::new(format!("{n} item{}", if n == 1 { "" } else { "s" })).color(TEXT).size(12.0));
                if !self.selected.is_empty() {
                    let bytes: u64 = self.selected.iter().filter_map(|&i| self.entries.get(i)).filter(|e| !e.is_dir).map(|e| e.size).sum();
                    ui.label(RichText::new(format!("   {} item{} selected  {}", self.selected.len(), if self.selected.len() == 1 { "" } else { "s" }, if bytes > 0 { human_size(bytes) } else { String::new() })).color(TEXT).size(12.0));
                }
                if let Some(s) = &self.status {
                    ui.label(RichText::new(format!("   {s}")).color(Color32::from_rgb(0xc4, 0x2b, 0x1c)).size(12.0));
                }
            });
        });

        // Navigation pane.
        egui::Panel::left("nav").exact_size(210.0).frame(egui::Frame::new().fill(NAV_BG).inner_margin(egui::Margin::symmetric(4, 8))).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut jump = None;
                let mut item = |ui: &mut egui::Ui, label: &str, path: &Path, ic: &str, indent: f32| {
                    let active = self.cwd == path;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
                    if active {
                        ui.painter().rect_filled(rect, 0.0, SELECTED);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, HOVER);
                    }
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0 + indent, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    icon(&mut child, ic, 16.0);
                    child.label(RichText::new(label).color(TEXT).size(13.0));
                    if resp.clicked() {
                        jump = Some(path.to_path_buf());
                    }
                };
                ui.label(RichText::new("  Quick access").color(DIM).size(12.0));
                item(ui, "Home", &home(), "user-home", 8.0);
                for (n, p, i) in user_folders() {
                    item(ui, &n, &p, i, 8.0);
                }
                ui.add_space(8.0);
                ui.label(RichText::new("  This PC").color(DIM).size(12.0));
                for (n, p) in drives() {
                    item(ui, &n, &p, "drive-harddisk", 8.0);
                }
                if let Some(p) = jump {
                    self.go(p);
                }
            });
        });

        // Details view.
        egui::CentralPanel::default().frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            let widths = [ui.available_width() * 0.45, ui.available_width() * 0.22, ui.available_width() * 0.18];
            // Column headers; click to sort.
            ui.horizontal(|ui| {
                for (i, (label, by)) in [("Name", SortBy::Name), ("Date modified", SortBy::Modified), ("Type", SortBy::Kind), ("Size", SortBy::Size)].into_iter().enumerate() {
                    let w = widths.get(i).copied().unwrap_or(ui.available_width());
                    let arrow = if self.sort.0 == by { if self.sort.1 { " ˄" } else { " ˅" } } else { "" };
                    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, 24.0), Sense::click());
                    if r.hovered() {
                        ui.painter().rect_filled(rect, 0.0, HOVER);
                    }
                    ui.painter().text(egui::pos2(rect.left() + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, format!("{label}{arrow}"), egui::FontId::proportional(12.5), DIM);
                    if r.clicked() {
                        self.sort = if self.sort.0 == by { (by, !self.sort.1) } else { (by, true) };
                        self.sort_entries();
                    }
                }
            });
            ui.painter().hline(ui.min_rect().x_range(), ui.cursor().top(), (1.0, BORDER));

            let query = self.search.to_lowercase();
            let shown: Vec<usize> = (0..self.entries.len()).filter(|&i| query.is_empty() || self.entries[i].name.to_lowercase().contains(&query)).collect();
            let mut open = None;
            let mut context = None;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, ROW_H, shown.len(), |ui, range| {
                for &i in &shown[range] {
                    let e = self.entries[i].clone();
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                    let sel = self.selected.contains(&i);
                    if sel {
                        ui.painter().rect_filled(rect, 0.0, SELECTED);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, HOVER);
                    }
                    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(4.0, 1.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    row.allocate_ui_with_layout(Vec2::new(widths[0], ROW_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_width(widths[0]);
                        icon(ui, &e.icon, 16.0);
                        match &mut self.renaming {
                            Some((ri, text)) if *ri == i => {
                                let r = ui.add(egui::TextEdit::singleline(text).desired_width(widths[0] - 30.0));
                                r.request_focus();
                                if r.lost_focus() {
                                    open = Some(usize::MAX); // commit below
                                }
                            }
                            _ => {
                                ui.add(egui::Label::new(RichText::new(&e.name).color(TEXT).size(13.0)).truncate());
                            }
                        }
                    });
                    // The other columns at fixed x positions, so they line up with the headers.
                    let font = egui::FontId::proportional(12.5);
                    let mut x = rect.left() + 4.0 + widths[0];
                    let p = ui.painter();
                    p.text(egui::pos2(x + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, fmt_time(e.modified), font.clone(), DIM);
                    x += widths[1];
                    p.text(egui::pos2(x + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, &e.kind, font.clone(), DIM);
                    x += widths[2];
                    if !e.is_dir {
                        p.text(egui::pos2(x + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, human_size(e.size), font, DIM);
                    }

                    if resp.clicked() {
                        let ctrl = ui.input(|inp| inp.modifiers.ctrl);
                        if ctrl {
                            if let Some(p) = self.selected.iter().position(|&s| s == i) {
                                self.selected.remove(p);
                            } else {
                                self.selected.push(i);
                            }
                        } else {
                            self.selected = vec![i];
                        }
                    }
                    if resp.double_clicked() {
                        open = Some(i);
                    }
                    if resp.secondary_clicked() {
                        if !self.selected.contains(&i) {
                            self.selected = vec![i];
                        }
                        context = Some(i);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Open").clicked() {
                            open = Some(i);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Cut").clicked() {
                            self.clipboard = Some(Clip::Cut(self.selected_paths()));
                            ui.close();
                        }
                        if ui.button("Copy").clicked() {
                            self.clipboard = Some(Clip::Copy(self.selected_paths()));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Delete").clicked() {
                            context = Some(usize::MAX - 1);
                            ui.close();
                        }
                        if ui.button("Rename").clicked() {
                            self.renaming = Some((i, e.name.clone()));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Open in PowerSlop").clicked() {
                            let dir = if e.is_dir { e.path.clone() } else { self.cwd.clone() };
                            spawn_terminal(&dir);
                            ui.close();
                        }
                    });
                }
                // Empty space: clear selection, right-click for folder actions.
                let (_, rest) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ui.available_height().max(40.0)), Sense::click());
                if rest.clicked() {
                    self.selected.clear();
                }
                rest.context_menu(|ui| {
                    if ui.button("Refresh").clicked() {
                        self.reload();
                        ui.close();
                    }
                    if ui.add_enabled(self.clipboard.is_some(), egui::Button::new("Paste")).clicked() {
                        self.paste();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("New folder").clicked() {
                        self.new_folder();
                        ui.close();
                    }
                    if ui.button(if self.show_hidden { "Hide hidden items" } else { "Show hidden items" }).clicked() {
                        self.show_hidden = !self.show_hidden;
                        self.reload();
                        ui.close();
                    }
                    if ui.button("Open in PowerSlop").clicked() {
                        spawn_terminal(&self.cwd);
                        ui.close();
                    }
                });
            });
            let _ = context;
            match open {
                Some(usize::MAX) => self.commit_rename(),
                Some(i) => self.open(i),
                None => {}
            }
            if context == Some(usize::MAX - 1) {
                self.delete_selected();
            }
        });
    }
}

/// Open our terminal in `dir` (PowerSlop next to us, else the user's).
fn spawn_terminal(dir: &Path) {
    let ours = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join("powerslop"))).filter(|p| p.exists());
    let mut cmd = match ours {
        Some(p) => std::process::Command::new(p),
        None => std::process::Command::new("konsole"),
    };
    let _ = cmd.current_dir(dir).spawn();
}

fn shellexpand(s: &str) -> String {
    match s.strip_prefix('~') {
        Some(rest) => format!("{}{rest}", home().display()),
        None => s.to_string(),
    }
}
