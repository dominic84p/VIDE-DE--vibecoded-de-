//! What the user pinned to Start and the taskbar, and how often they launch each app
//! (for ranking search). ~/.config/vide-de/pins.conf, by desktop-file id.

use std::{collections::HashMap, path::PathBuf};

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
};

use crate::{
    search::Entry,
    state::State,
    ui::{StartRow, TaskItem},
};

pub struct Pins {
    pub start: Vec<String>,
    pub taskbar: Vec<String>,
    pub launches: HashMap<String, u32>,
}

/// Next to settings.conf, so it follows XDG_CONFIG_HOME too (nested tests set it so they
/// never touch the real pins).
fn path() -> PathBuf {
    vide_config::Config::path().with_file_name("pins.conf")
}

impl Pins {
    pub fn load() -> Self {
        let Ok(text) = std::fs::read_to_string(path()) else {
            // First run: our own apps, like Windoze ships with its own pinned.
            return Pins {
                start: ["file-exploder", "vide-settings", "powerslop", "task-mangler", "snip-and-sketchy"].map(String::from).to_vec(),
                taskbar: ["file-exploder", "powerslop"].map(String::from).to_vec(),
                launches: HashMap::new(),
            };
        };
        let mut p = Pins { start: Vec::new(), taskbar: Vec::new(), launches: HashMap::new() };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k {
                "start" => p.start.push(v.to_string()),
                "taskbar" => p.taskbar.push(v.to_string()),
                "used" => {
                    if let Some((id, n)) = v.split_once('\t') {
                        p.launches.insert(id.to_string(), n.parse().unwrap_or(0));
                    }
                }
                _ => {}
            }
        }
        p
    }

    pub fn save(&self) {
        let mut out = String::new();
        for id in &self.start {
            out += &format!("start={id}\n");
        }
        for id in &self.taskbar {
            out += &format!("taskbar={id}\n");
        }
        for (id, n) in &self.launches {
            out += &format!("used={id}\t{n}\n");
        }
        let p = path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&p, out) {
            tracing::warn!("couldn't save pins: {e}");
        }
    }

    pub fn launched(&mut self, id: &str) {
        *self.launches.entry(id.to_string()).or_default() += 1;
        self.save();
    }

    pub fn toggle_start(&mut self, id: &str) {
        toggle(&mut self.start, id);
        self.save();
    }

    pub fn toggle_taskbar(&mut self, id: &str) {
        toggle(&mut self.taskbar, id);
        self.save();
    }
}

fn toggle(list: &mut Vec<String>, id: &str) {
    match list.iter().position(|x| x == id) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(id.to_string()),
    }
}

impl State {
    /// Start menu rows for the current results.
    pub fn start_rows(&self) -> Vec<StartRow> {
        self.start
            .results
            .iter()
            .map(|e| match e {
                Entry::Header(h) => StartRow::Header(h.clone()),
                Entry::App(i) => {
                    let a = &self.apps[*i];
                    let detail = if self.pins.start.contains(&a.id) && !self.start.query.is_empty() { "Pinned" } else { "" };
                    StartRow::Item(a.name.clone(), a.icon.clone(), detail.into())
                }
                Entry::Setting(title, _) => StartRow::Item((*title).into(), "preferences-system".into(), "Settings".into()),
                Entry::File(p) => {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let home = std::env::var("HOME").unwrap_or_default();
                    let parent = p.parent().map(|d| d.to_string_lossy().replacen(&home, "~", 1)).unwrap_or_default();
                    let icon = if p.is_dir() { "folder" } else { "text-x-generic" };
                    StartRow::Item(name, icon.into(), parent)
                }
                Entry::Calc(v) => StartRow::Item(format!("= {v}"), "accessories-calculator".into(), "Enter to copy".into()),
                Entry::Power(a) => {
                    let (label, icon) = match a {
                        crate::ui::PowerAction::ShutDown => ("Shut down", "system-shutdown"),
                        crate::ui::PowerAction::Restart => ("Restart", "system-reboot"),
                        crate::ui::PowerAction::SignOut => ("Sign out", "system-log-out"),
                    };
                    StartRow::Item(label.into(), icon.into(), "Command".into())
                }
                Entry::Web(q) => StartRow::Item(format!("{q} - See web results"), "internet-web-browser".into(), String::new()),
            })
            .collect()
    }

    /// Start's tiles: pinned apps under "Productivity" (group 0), then our own apps
    /// that aren't pinned under "Explore" (group 1), like Windoze' default layout.
    pub fn start_tiles(&self) -> Vec<(usize, u8)> {
        let find = |id: &str| self.apps.iter().position(|a| a.id == id);
        let mut out: Vec<(usize, u8)> = self.pins.start.iter().filter_map(|id| find(id)).map(|i| (i, 0)).collect();
        for id in ["vide-settings", "file-exploder", "task-mangler", "powerslop", "snip-and-sketchy", "vide-about"] {
            if let Some(i) = find(id)
                && !out.iter().any(|(j, _)| *j == i)
            {
                out.push((i, 1));
            }
        }
        out
    }

    /// What kind of thing a result is, for the search pane ("App", "File folder"...).
    pub fn result_kind(&self, e: &Entry) -> String {
        match e {
            Entry::Header(_) => String::new(),
            Entry::App(_) => "App".into(),
            Entry::Setting(..) => "System settings".into(),
            Entry::File(p) if p.is_dir() => "File folder".into(),
            Entry::File(p) => match p.extension().map(|x| x.to_string_lossy().to_lowercase()) {
                Some(x) if x == "txt" => "Text Document".into(),
                Some(x) => format!("{} File", x.to_uppercase()),
                None => "File".into(),
            },
            Entry::Calc(_) => "Calculator".into(),
            Entry::Power(_) => "Command".into(),
            Entry::Web(_) => "See web results".into(),
        }
    }

    /// The search pane's right side for the highlighted result: what it is and
    /// what you can do with it.
    pub fn start_preview(&self) -> Option<crate::ui::Preview> {
        let e = self.start.results.get(self.start.selected)?;
        let rows = self.start_rows();
        let crate::ui::StartRow::Item(name, icon, detail) = rows.get(self.start.selected)?.clone() else { return None };
        let a = |id: u8, label: &str, icon: &str| (id, label.to_string(), icon.to_string());
        let open = a(0, "Open", "document-open");
        let actions = match e {
            Entry::Header(_) => return None,
            Entry::App(i) => {
                let id = &self.apps[*i].id;
                vec![
                    open,
                    a(2, if self.pins.start.contains(id) { "Unpin from Start" } else { "Pin to Start" }, "window-pin"),
                    a(3, if self.pins.taskbar.contains(id) { "Unpin from taskbar" } else { "Pin to taskbar" }, "window-pin"),
                ]
            }
            Entry::File(_) => vec![open, a(1, "Open file location", "folder-open"), a(4, "Copy full path", "edit-copy")],
            Entry::Calc(_) => vec![a(0, "Copy", "edit-copy")],
            Entry::Web(_) => vec![a(0, "Open results in browser", "internet-web-browser")],
            Entry::Setting(..) | Entry::Power(_) => vec![open],
        };
        let location = match e {
            Entry::File(_) => format!("Location: {detail}"),
            _ => String::new(),
        };
        let name = match e {
            Entry::Web(q) => q.clone(),
            _ => name,
        };
        Some(crate::ui::Preview { name, icon, kind: self.result_kind(e), location, actions })
    }

    /// Taskbar buttons: pinned apps first (with their windows grouped under them),
    /// then a button per other window on this desktop.
    pub fn taskbar_items(&self, focused: Option<&WlSurface>) -> Vec<TaskItem> {
        let mut windows: Vec<_> =
            self.wm.windows.iter().filter(|w| w.mapped && w.workspace == self.wm.current_workspace).collect();
        windows.sort_by_key(|w| w.id);
        let app_of = |w: &&crate::wm::Window| crate::apps::match_app(&self.apps, &w.app_id()).map(|a| a.id.clone());
        let is_focused = |w: &&crate::wm::Window| !w.minimized && Some(w.surface()) == focused;
        let mut out = Vec::new();
        for id in &self.pins.taskbar {
            let Some(app) = self.apps.iter().find(|a| &a.id == id) else { continue };
            let mine: Vec<_> = windows.iter().filter(|w| app_of(w).as_ref() == Some(id)).collect();
            out.push(TaskItem {
                windows: mine.iter().map(|w| w.surface().clone()).collect(),
                title: app.name.clone(),
                icon: app.icon.clone(),
                pinned: Some(id.clone()),
                focused: mine.iter().any(|w| is_focused(w)),
            });
        }
        for w in &windows {
            if app_of(w).is_some_and(|id| self.pins.taskbar.contains(&id)) {
                continue;
            }
            let app_id = w.app_id();
            let icon = if app_id.is_empty() {
                self.client_proc_name(w.surface())
                    .map(|p| crate::apps::icon_for_app_id(&self.apps, &p))
                    .unwrap_or_default()
            } else {
                crate::apps::icon_for_app_id(&self.apps, &app_id)
            };
            out.push(TaskItem {
                windows: vec![w.surface().clone()],
                title: w.title(),
                icon,
                pinned: None,
                focused: is_focused(w),
            });
        }
        out
    }

    fn client_proc_name(&self, surface: &WlSurface) -> Option<String> {
        use smithay::reexports::wayland_server::Resource as _;
        let client = self.dh.get_client(surface.id()).ok()?;
        let pid = client.get_credentials(&self.dh).ok()?.pid;
        if pid <= 0 {
            return None;
        }
        let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        exe.file_name().map(|n| n.to_string_lossy().into_owned())
    }

    /// Left press on a taskbar button. Pinned buttons wait for the release, since
    /// the press may turn into a drag that reorders them.
    pub fn press_task_button(&mut self, i: usize, x: f64) {
        let pinned = self.taskbar.as_ref().and_then(|t| t.key.items.get(i)).and_then(|it| it.pinned.clone());
        match pinned {
            Some(id) => self.task_drag = Some(TaskDrag { id, start_x: x, moved: false, index: i }),
            None => self.click_task_button(i),
        }
    }

    /// Pointer moved while a pinned button is held: past a few pixels, drag it along.
    pub fn task_drag_motion(&mut self, x: f64) {
        let Some(d) = self.task_drag.as_mut() else { return };
        if !d.moved && (x - d.start_x).abs() < 12.0 {
            return;
        }
        d.moved = true;
        let slot = ((x as i32 - crate::ui::TASK_BUTTONS_X) / crate::ui::TASK_W).max(0) as usize;
        let slot = slot.min(self.pins.taskbar.len().saturating_sub(1));
        if let Some(from) = self.pins.taskbar.iter().position(|p| *p == d.id) {
            if from != slot {
                let id = self.pins.taskbar.remove(from);
                self.pins.taskbar.insert(slot, id);
                self.needs_redraw = true;
            }
        }
    }

    pub fn task_drag_release(&mut self) {
        let Some(d) = self.task_drag.take() else { return };
        if d.moved {
            self.pins.save();
        } else {
            self.click_task_button(d.index);
        }
    }

    /// Not running: launch it. One window: focus or minimize it. Several: step through them.
    pub fn click_task_button(&mut self, i: usize) {
        let Some(item) = self.taskbar.as_ref().and_then(|t| t.key.items.get(i)).cloned() else { return };
        match item.windows.len() {
            0 => {
                if let Some(app) = item.pinned.and_then(|id| self.apps.iter().find(|a| a.id == id).cloned()) {
                    self.launch(&app);
                }
            }
            1 => self.activate_from_taskbar(&item.windows[0]),
            n => {
                let focused = self.focused();
                let next = match item.windows.iter().position(|w| Some(w) == focused.as_ref()) {
                    Some(at) => item.windows[(at + 1) % n].clone(),
                    // The one highest in the stack, i.e. the one you used last.
                    None => self
                        .wm
                        .windows
                        .iter()
                        .rev()
                        .map(|w| w.surface())
                        .find(|s| item.windows.contains(s))
                        .cloned()
                        .unwrap_or_else(|| item.windows[0].clone()),
                };
                self.focus(Some(next), smithay::utils::SERIAL_COUNTER.next_serial());
            }
        }
    }

    pub fn open_task_button_menu(&mut self, i: usize, x: i32) {
        let Some(item) = self.taskbar.as_ref().and_then(|t| t.key.items.get(i)).cloned() else { return };
        if !item.windows.is_empty() {
            self.open_taskbar_menu(&item.windows, x);
        } else if let Some(idx) = item.pinned.and_then(|id| self.apps.iter().position(|a| a.id == id)) {
            self.open_pinned_jump_list(idx, x);
        }
    }
}

/// An app held down in Start (list or tile): a click on release, or dragged onto the taskbar
/// to pin it there, as in Windoze 10.
pub struct StartDrag {
    pub app: usize,
    pub start: Point<f64, Logical>,
    pub moved: bool,
    /// What a plain click does instead.
    pub click: StartClick,
    /// The icon that follows the pointer, plain and with "Pin to taskbar" for over the taskbar.
    pub ghost: Option<(MemoryRenderBuffer, MemoryRenderBuffer)>,
}

#[derive(Clone, Copy)]
pub enum StartClick {
    Entry(usize),
    Tile(usize),
}

impl State {
    pub fn press_start_app(&mut self, app: usize, click: StartClick, at: Point<f64, Logical>) {
        self.start_drag = Some(StartDrag { app, start: at, moved: false, click, ghost: None });
    }

    pub fn start_drag_motion(&mut self, at: Point<f64, Logical>) {
        let Some(d) = self.start_drag.as_mut() else { return };
        let moved = at - d.start;
        if !d.moved && moved.x.hypot(moved.y) < 8.0 {
            return;
        }
        if !d.moved {
            d.moved = true;
            let icon = crate::ui::app_icon(&mut self.icons, &self.apps[d.app].icon, 32);
            let ghost = |label: bool| {
                // The label sits above the icon, as on Windoze: below would be off the screen.
                let mut c = crate::canvas::Canvas::new(150, 58);
                match &icon {
                    Some(i) => c.icon(i, 0, 26),
                    None => c.round_rect(0, 26, 32, 32, 4, crate::theme::ACCENT),
                }
                if let (true, Some(f)) = (label, self.font.as_ref()) {
                    let t = "Pin to taskbar";
                    let w = crate::canvas::measure(f, 12.0, t) + 12;
                    c.rect(0, 0, w, 22, crate::theme::th().menu);
                    c.outline(0, 0, w, 22, crate::theme::th().border);
                    c.text(f, 12.0, 6, 0, 22, crate::theme::th().text, t);
                }
                c.into_buffer()
            };
            let pair = (ghost(false), ghost(true));
            if let Some(d) = self.start_drag.as_mut() {
                d.ghost = Some(pair);
            }
        }
        self.needs_redraw = true;
    }

    /// Let go: a click does what it always did; a drag that ends on the taskbar pins the app at
    /// that spot (moving it there if it was pinned already).
    pub fn start_drag_release(&mut self, at: Point<f64, Logical>) {
        let Some(d) = self.start_drag.take() else { return };
        self.needs_redraw = true;
        if !d.moved {
            match d.click {
                StartClick::Entry(i) => self.activate_start_entry(i),
                StartClick::Tile(_) => {
                    let app = self.apps[d.app].clone();
                    self.launch(&app);
                    self.toggle_start();
                }
            }
            return;
        }
        if at.y < (self.output_size.h - crate::ui::TASKBAR_H) as f64 {
            return;
        }
        let id = self.apps[d.app].id.clone();
        let slot = ((at.x as i32 - crate::ui::TASK_BUTTONS_X + crate::ui::TASK_W / 2) / crate::ui::TASK_W).max(0) as usize;
        self.pins.taskbar.retain(|p| *p != id);
        let slot = slot.min(self.pins.taskbar.len());
        self.pins.taskbar.insert(slot, id);
        self.pins.save();
        self.taskbar = None;
        if self.start.open {
            self.toggle_start();
        }
    }

    /// The icon following the pointer while dragging from Start.
    pub fn start_drag_ghost(&self) -> Option<&MemoryRenderBuffer> {
        let d = self.start_drag.as_ref().filter(|d| d.moved)?;
        let (plain, pin) = d.ghost.as_ref()?;
        let over_taskbar = self.pointer_loc.y >= (self.output_size.h - crate::ui::TASKBAR_H) as f64;
        Some(if over_taskbar { pin } else { plain })
    }
}

/// A pinned taskbar button being held (maybe dragged).
pub struct TaskDrag {
    pub id: String,
    pub start_x: f64,
    pub moved: bool,
    /// Its button index when pressed, for a plain click.
    pub index: usize,
}
