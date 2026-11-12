//! What the user pinned to Start and the taskbar, and how often they launch each app
//! (for ranking search). ~/.config/vide-de/pins.conf, by desktop-file id.

use std::{collections::HashMap, path::PathBuf};

use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;

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

fn path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/vide-de/pins.conf")
}

impl Pins {
    pub fn load() -> Self {
        let Ok(text) = std::fs::read_to_string(path()) else {
            // First run: our own apps, like Windows ships with its own pinned.
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
            out.push(TaskItem {
                windows: vec![w.surface().clone()],
                title: w.title(),
                icon: crate::apps::icon_for_app_id(&self.apps, &w.app_id()),
                pinned: None,
                focused: is_focused(w),
            });
        }
        out
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
        if !d.moved && (x - d.start_x).abs() < 8.0 {
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
        match item.windows.last() {
            Some(w) => self.open_taskbar_menu(w, x),
            None => {
                let Some(id) = item.pinned else { return };
                let Some(idx) = self.apps.iter().position(|a| a.id == id) else { return };
                use crate::menu::Action;
                let items = vec![
                    (self.apps[idx].name.clone(), Action::Launch(idx)),
                    (String::new(), Action::Separator),
                    ("Unpin from taskbar".into(), Action::PinTaskbar(id)),
                ];
                self.open_menu_above_taskbar(x, items);
            }
        }
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
