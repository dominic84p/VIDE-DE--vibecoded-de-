//! Input routing and keybinds.
//!
//! The modifier is Super on a real TTY. Nested inside another desktop the host
//! eats Super, so it's Alt there. Ctrl+Alt+Backspace always quits.
//!
//! Mod+Enter terminal · Mod+Q close · Mod+Tab cycle · Mod+Up maximize ·
//! Mod+Down minimize · Mod+F fullscreen · Mod+Space or Super tap: start menu ·
//! Mod+Shift+E quit · Mod+drag move.

use std::time::{Duration, Instant};

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, ButtonState, Event, InputBackend, InputEvent, KeyState, KeyboardKeyEvent,
        PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    desktop::{find_popup_root_surface, utils::under_from_surface_tree, WindowSurfaceType},
    input::{
        keyboard::{keysyms::KEY_NoSymbol, FilterResult, Keysym, ModifiersState},
        pointer::{AxisFrame, ButtonEvent, CursorIcon, CursorImageStatus, MotionEvent, RelativeMotionEvent},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
    wayland::{
        compositor::get_parent,
        pointer_constraints::{with_pointer_constraint, PointerConstraint},
    },
};

use smithay::wayland::shell::wlr_layer::Layer;

use crate::{
    state::{Hover, State},
    ui::{self, PowerAction, StartHit, TaskbarHit, TitleButton, MENU_W, TASKBAR_H},
    wm::{edge, Hit},
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Our own app binaries live next to the compositor's.
pub fn own_app(name: &str) -> Option<String> {
    let p = std::env::current_exe().ok()?.parent()?.join(name);
    p.exists().then(|| format!("'{}'", p.display()))
}

/// PowerSlop if it's built, else Konsole.
pub fn terminal() -> String {
    own_app("powerslop").unwrap_or_else(|| "konsole".into())
}

enum Action {
    Terminal,
    Files,
    TaskManager,
    Close,
    SwitcherStep { back: bool },
    SwitcherCommit,
    SwitcherCancel,
    TaskView,
    Snap { right: bool },
    Maximize,
    Minimize,
    Fullscreen,
    StartMenu,
    WindowMenu,
    CloseMenu,
    CancelSnip,
    CloseDesktops,
    ToggleActionCenter,
    CloseActionCenter,
    Volume(f32),
    Mute,
    Brightness(f32),
    Screenshot { region: bool },
    Quit,
    Vt(i32),
    Workspace(usize),
    WorkspaceNext,
    WorkspacePrev,
    MoveToWorkspace(usize),
    /// Swallowed by the start menu.
    Menu(MenuKey),
    /// Typing into the network flyout's security key box.
    FlyoutKey(MenuKey),
    CloseFlyout,
}

enum MenuKey {
    Close,
    Launch,
    Up,
    Down,
    Backspace,
    Char(char),
    Ignore,
}

fn bind(super_mod: bool, mods: &ModifiersState, sym: Keysym) -> Option<Action> {
    let raw = sym.raw();
    if (Keysym::XF86_Switch_VT_1.raw()..=Keysym::XF86_Switch_VT_12.raw()).contains(&raw) {
        return Some(Action::Vt((raw - Keysym::XF86_Switch_VT_1.raw()) as i32 + 1));
    }
    if mods.ctrl && mods.alt && sym == Keysym::BackSpace {
        return Some(Action::Quit);
    }
    // Media keys.
    match sym {
        Keysym::XF86_AudioRaiseVolume => return Some(Action::Volume(0.05)),
        Keysym::XF86_AudioLowerVolume => return Some(Action::Volume(-0.05)),
        Keysym::XF86_AudioMute => return Some(Action::Mute),
        Keysym::XF86_MonBrightnessUp => return Some(Action::Brightness(0.1)),
        Keysym::XF86_MonBrightnessDown => return Some(Action::Brightness(-0.1)),
        _ => {}
    }
    // Windows-style keys that don't use our modifier.
    if mods.ctrl && mods.shift && sym == Keysym::Escape {
        return Some(Action::TaskManager);
    }
    if mods.alt && !mods.ctrl && sym == Keysym::F4 {
        return Some(Action::Close);
    }
    // Alt+Tab is always the window switcher, whatever our modifier is.
    if mods.alt && !mods.ctrl && matches!(sym, Keysym::Tab | Keysym::ISO_Left_Tab) {
        return Some(Action::SwitcherStep { back: mods.shift });
    }
    if super_mod && mods.alt && !mods.logo && sym == Keysym::space {
        return Some(Action::WindowMenu);
    }
    if sym == Keysym::Print {
        return Some(Action::Screenshot { region: mods.shift });
    }
    if mods.logo && mods.shift && matches!(sym, Keysym::s | Keysym::S) {
        return Some(Action::Screenshot { region: true });
    }
    if !(if super_mod { mods.logo } else { mods.alt }) {
        return None;
    }
    match sym {
        Keysym::Return => Some(Action::Terminal),
        Keysym::e if !mods.shift => Some(Action::Files),
        Keysym::q => Some(Action::Close),
        // Win+Tab: Task View (on the TTY; nested, Alt+Tab above wins).
        Keysym::Tab => Some(Action::TaskView),
        Keysym::Up => Some(Action::Maximize),
        Keysym::Down => Some(Action::Minimize),
        Keysym::Left if !mods.ctrl => Some(Action::Snap { right: false }),
        Keysym::Right if !mods.ctrl => Some(Action::Snap { right: true }),
        Keysym::f => Some(Action::Fullscreen),
        Keysym::space => Some(Action::StartMenu),
        Keysym::a | Keysym::n => Some(Action::ToggleActionCenter),
        Keysym::e if mods.shift => Some(Action::Quit),
        // Workspace switching: Mod+1-9,0 (0 = workspace 10)
        // Mod+Shift+1-9,0 to move focused window to workspace (must come first)
        Keysym::_1 if mods.shift => Some(Action::MoveToWorkspace(0)),
        Keysym::_2 if mods.shift => Some(Action::MoveToWorkspace(1)),
        Keysym::_3 if mods.shift => Some(Action::MoveToWorkspace(2)),
        Keysym::_4 if mods.shift => Some(Action::MoveToWorkspace(3)),
        Keysym::_5 if mods.shift => Some(Action::MoveToWorkspace(4)),
        Keysym::_6 if mods.shift => Some(Action::MoveToWorkspace(5)),
        Keysym::_7 if mods.shift => Some(Action::MoveToWorkspace(6)),
        Keysym::_8 if mods.shift => Some(Action::MoveToWorkspace(7)),
        Keysym::_9 if mods.shift => Some(Action::MoveToWorkspace(8)),
        Keysym::_0 if mods.shift => Some(Action::MoveToWorkspace(9)),
        Keysym::_1 => Some(Action::Workspace(0)),
        Keysym::_2 => Some(Action::Workspace(1)),
        Keysym::_3 => Some(Action::Workspace(2)),
        Keysym::_4 => Some(Action::Workspace(3)),
        Keysym::_5 => Some(Action::Workspace(4)),
        Keysym::_6 => Some(Action::Workspace(5)),
        Keysym::_7 => Some(Action::Workspace(6)),
        Keysym::_8 => Some(Action::Workspace(7)),
        Keysym::_9 => Some(Action::Workspace(8)),
        Keysym::_0 => Some(Action::Workspace(9)),
        // Mod+Ctrl+Left/Right for next/prev workspace
        Keysym::Left if mods.ctrl => Some(Action::WorkspacePrev),
        Keysym::Right if mods.ctrl => Some(Action::WorkspaceNext),
        _ => None,
    }
}

fn menu_key(sym: Keysym, ch: Option<char>) -> MenuKey {
    match sym {
        Keysym::Escape => MenuKey::Close,
        Keysym::Return | Keysym::KP_Enter => MenuKey::Launch,
        Keysym::Up => MenuKey::Up,
        Keysym::Down => MenuKey::Down,
        Keysym::BackSpace => MenuKey::Backspace,
        _ => match ch {
            Some(c) if !c.is_control() => MenuKey::Char(c),
            _ => MenuKey::Ignore,
        },
    }
}

/// Where the pointer is, in terms of our UI.
enum Target {
    /// Over our right-click menu, maybe on an item.
    Menu(Option<usize>),
    /// Over the Task View panel, maybe on a desktop tile.
    Desktops(Option<usize>),
    /// Over the Action Center (notifications).
    ActionCenter(crate::notifications::ActionCenterHit),
    Start(StartHit),
    Taskbar(TaskbarHit),
    Window(Hit),
    Desktop,
}

impl State {
    pub fn handle_input<B: InputBackend>(&mut self, event: InputEvent<B>) {
        match event {
            InputEvent::Keyboard { event } => self.on_key(event.key_code(), event.state(), event.time_msec()),

            InputEvent::PointerMotionAbsolute { event } => {
                let pos = event.position_transformed(self.output_size);
                self.pointer_to(pos, event.time_msec());
            }

            InputEvent::PointerMotion { event } => {
                if !self.seen_pointer {
                    self.seen_pointer = true;
                    tracing::info!("first relative pointer motion");
                }
                let delta = event.delta();

                // Raw movement for games (mouse-look), whether or not the cursor can move.
                let ptr = self.pointer.clone();
                let focus = self.pointer_focus.clone();
                ptr.relative_motion(
                    self,
                    focus.clone(),
                    &RelativeMotionEvent { delta, delta_unaccel: event.delta_unaccel(), utime: event.time() },
                );

                // A game holding a lock keeps the cursor still; a confine keeps it in its region.
                let constraint = focus.as_ref().and_then(|(s, origin)| {
                    with_pointer_constraint(s, &ptr, |c| match c.as_deref() {
                        Some(PointerConstraint::Locked(_)) if c.as_ref().unwrap().is_active() => Some(None),
                        Some(PointerConstraint::Confined(conf)) if c.as_ref().unwrap().is_active() => {
                            Some(Some((s.clone(), *origin, conf.region().cloned())))
                        }
                        _ => None,
                    })
                });
                let size = self.output_size.to_f64();
                let mut pos = self.pointer_loc + delta;
                pos.x = pos.x.clamp(0.0, size.w - 1.0);
                pos.y = pos.y.clamp(0.0, size.h - 1.0);
                match constraint {
                    Some(None) => {
                        ptr.frame(self);
                        return;
                    }
                    Some(Some((s, origin, region))) => {
                        let inside = |p: Point<f64, Logical>| {
                            let local = (p - origin).to_i32_round::<i32>();
                            match &region {
                                Some(r) => r.contains(local),
                                None => self.wm.hit(p).is_some_and(|h| matches!(h, Hit::Client(ref hs, _) if *hs == s)),
                            }
                        };
                        if !inside(pos) {
                            // Slide along whichever axis still fits, else stay.
                            let x_only = Point::from((pos.x, self.pointer_loc.y));
                            let y_only = Point::from((self.pointer_loc.x, pos.y));
                            pos = if inside(x_only) { x_only } else if inside(y_only) { y_only } else { self.pointer_loc };
                        }
                    }
                    None => {}
                }
                self.pointer_to(pos, event.time_msec());
            }

            InputEvent::PointerButton { event } => {
                self.on_button(event.button_code(), event.state(), event.time_msec());
            }

            InputEvent::PointerAxis { event } => {
                let over_volume = matches!(self.target(self.pointer_loc), Target::Taskbar(TaskbarHit::Volume))
                    || (self.tray.flyout == Some(crate::tray::Flyout::Volume) && self.flyout_rect().to_f64().contains(self.pointer_loc));
                if matches!(self.target(self.pointer_loc), Target::Taskbar(TaskbarHit::TaskView)) {
                    let v = event.amount(Axis::Vertical).unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) / 8.0);
                    if v > 0.0 {
                        self.wm.next_workspace();
                    } else if v < 0.0 {
                        self.wm.prev_workspace();
                    }
                    self.focus_top();
                    return;
                }
                if over_volume {
                    let v = event.amount(Axis::Vertical).unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) / 8.0);
                    if v != 0.0 {
                        self.set_volume(self.tray.volume - v.signum() as f32 * 0.05);
                    }
                    return;
                }
                if self.tray.flyout == Some(crate::tray::Flyout::Network) && self.flyout_rect().to_f64().contains(self.pointer_loc) {
                    let v = event.amount(Axis::Vertical).unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) / 8.0);
                    if v != 0.0 {
                        self.flyout_scroll(v.signum() as i32);
                    }
                    return;
                }
                if self.start.open {
                    if let Target::Start(_) = self.target(self.pointer_loc) {
                        let v = event
                            .amount(Axis::Vertical)
                            .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) / 8.0);
                        self.scroll_menu(v.signum() as i32 * 3);
                        return;
                    }
                }
                let mut frame = AxisFrame::new(event.time_msec()).source(event.source());
                for axis in [Axis::Horizontal, Axis::Vertical] {
                    let v120 = event.amount_v120(axis);
                    let amount = event.amount(axis).unwrap_or_else(|| v120.unwrap_or(0.0) * 15.0 / 120.0);
                    if amount != 0.0 {
                        frame = frame.value(axis, amount);
                        if let Some(d) = v120 {
                            frame = frame.v120(axis, d as i32);
                        }
                    }
                }
                let ptr = self.pointer.clone();
                ptr.axis(self, frame);
                ptr.frame(self);
            }

            _ => {}
        }
    }

    // ------------------------------------------------------------ keyboard

    fn on_key(&mut self, code: smithay::input::keyboard::Keycode, key_state: KeyState, time: u32) {
        let serial = SERIAL_COUNTER.next_serial();
        let pressed = key_state == KeyState::Pressed;
        let super_mod = self.tty;
        let menu_open = self.start.open;
        let kb = self.keyboard.clone();
        let action = kb.input::<Action, _>(self, code, key_state, serial, time, |state, mods, handle| {
            // VT switch keysyms only exist after Ctrl+Alt is applied, so check the modified sym too.
            let modified = handle.modified_sym();
            let raw = handle.raw_syms().first().copied().unwrap_or(Keysym::from(KEY_NoSymbol));

            // Super tap (press + release with nothing in between) toggles the start menu.
            // Alt+Tab in progress: letting go of Alt picks, Esc backs out.
            if state.switcher.is_some() {
                if !pressed && matches!(raw, Keysym::Alt_L | Keysym::Alt_R) {
                    return FilterResult::Intercept(Action::SwitcherCommit);
                }
                if pressed && modified == Keysym::Escape {
                    return FilterResult::Intercept(Action::SwitcherCancel);
                }
            }
            if matches!(raw, Keysym::Super_L | Keysym::Super_R) {
                if pressed {
                    state.super_tap = true;
                } else if std::mem::take(&mut state.super_tap) && super_mod {
                    return FilterResult::Intercept(Action::StartMenu);
                }
                return FilterResult::Forward;
            }
            if pressed {
                state.super_tap = false;
            }

            if let Some(a) = bind(super_mod, mods, modified).or_else(|| bind(super_mod, mods, raw)) {
                return if pressed { FilterResult::Intercept(a) } else { FilterResult::Forward };
            }
            if state.snip.is_some() && modified == Keysym::Escape {
                return FilterResult::Intercept(if pressed { Action::CancelSnip } else { Action::Menu(MenuKey::Ignore) });
            }
            if state.tray.password.is_some() && state.tray.flyout == Some(crate::tray::Flyout::Network) {
                let k = if pressed { menu_key(modified, modified.key_char()) } else { MenuKey::Ignore };
                return FilterResult::Intercept(Action::FlyoutKey(k));
            }
            if state.tray.flyout.is_some() && pressed && modified == Keysym::Escape {
                return FilterResult::Intercept(Action::CloseFlyout);
            }
            if state.action_center.open && pressed && modified == Keysym::Escape {
                return FilterResult::Intercept(Action::CloseActionCenter);
            }
            if state.desktops.open && pressed && modified == Keysym::Escape {
                return FilterResult::Intercept(Action::CloseDesktops);
            }
            if state.menu.is_some() && pressed && modified == Keysym::Escape {
                return FilterResult::Intercept(Action::CloseMenu);
            }
            if menu_open {
                // The start menu takes all typing while open.
                let k = if pressed { menu_key(modified, modified.key_char()) } else { MenuKey::Ignore };
                return FilterResult::Intercept(Action::Menu(k));
            }
            FilterResult::Forward
        });
        if let Some(a) = action {
            self.run(a);
        }
    }

    fn run(&mut self, action: Action) {
        let focused = self.focused().filter(|s| self.wm.find(s).is_some());
        match action {
            Action::Terminal => self.spawn(&terminal()),
            Action::TaskManager => self.spawn(&own_app("task-mangler").unwrap_or_else(|| "plasma-systemmonitor".into())),
            Action::Files => self.spawn(&own_app("file-exploder").unwrap_or_else(|| "dolphin".into())),
            Action::Close => {
                if let Some(w) = focused.and_then(|s| self.wm.find(&s)) {
                    w.close();
                }
            }
            Action::SwitcherStep { back } => self.switcher_step(back),
            Action::SwitcherCommit => self.switcher_commit(),
            Action::SwitcherCancel => self.switcher_cancel(),
            Action::TaskView => self.toggle_desktops(),
            Action::Snap { right } => {
                if let Some(s) = focused {
                    let a = self.work_area();
                    let half = a.size.w / 2;
                    let x = if right { a.loc.x + a.size.w - half } else { a.loc.x };
                    let zone = Rectangle::new((x, a.loc.y).into(), (half, a.size.h).into());
                    self.wm.snap(&s, zone, a);
                }
            }
            Action::Maximize => {
                if let Some(s) = focused {
                    let area = self.work_area();
                    self.wm.toggle_maximize(&s, area);
                }
            }
            Action::Minimize => {
                if let Some(s) = focused {
                    if self.wm.find(&s).is_some_and(|w| w.maximized.is_some()) {
                        self.wm.unmaximize(&s);
                    } else if self.wm.find(&s).is_some_and(|w| w.snapped.is_some()) {
                        self.wm.unsnap(&s);
                    } else {
                        self.minimize(&s);
                    }
                }
            }
            Action::Fullscreen => {
                if let Some(s) = focused {
                    if self.wm.find(&s).is_some_and(|w| w.fullscreen.is_some()) {
                        self.wm.unfullscreen(&s);
                    } else {
                        let rect = self.output_rect();
                        self.wm.fullscreen(&s, rect);
                    }
                }
            }
            Action::StartMenu => self.toggle_start(),
            Action::WindowMenu => {
                if let Some(s) = focused {
                    let at = self.wm.find(&s).map(|w| w.frame().loc).unwrap_or_default();
                    self.open_window_menu(&s, at);
                }
            }
            Action::CloseMenu => self.close_menu(),
            Action::CancelSnip => self.cancel_snip(),
            Action::CloseDesktops => self.close_desktops(),
            Action::ToggleActionCenter => self.toggle_action_center(),
            Action::CloseActionCenter => self.close_action_center(),
            Action::Volume(d) => self.set_volume(self.tray.volume + d),
            Action::Mute => self.toggle_mute(),
            Action::Brightness(d) => self.set_brightness(self.tray.brightness + d),
            Action::Screenshot { region } => self.start_screenshot(region),
            Action::Quit => self.running = false,
            Action::Vt(n) => self.pending_vt = Some(n),
            Action::Workspace(ws) => {
                self.wm.set_workspace(ws);
                self.focus_top();
            }
            Action::WorkspaceNext => {
                self.wm.next_workspace();
                self.focus_top();
            }
            Action::WorkspacePrev => {
                self.wm.prev_workspace();
                self.focus_top();
            }
            Action::MoveToWorkspace(ws) => {
                if let Some(s) = focused {
                    self.wm.move_window_to_workspace(&s, ws);
                    // It just left the screen; don't keep typing into it.
                    if ws != self.wm.current_workspace {
                        self.focus_top();
                    }
                }
            }
            Action::Menu(k) => self.menu_key(k),
            Action::FlyoutKey(k) => match k {
                MenuKey::Char(c) => self.flyout_type(Some(c), false),
                MenuKey::Backspace => self.flyout_type(None, true),
                MenuKey::Launch => self.run_flyout_action(crate::tray::FlyoutAction::PasswordNext),
                MenuKey::Close => self.run_flyout_action(crate::tray::FlyoutAction::PasswordCancel),
                _ => {}
            },
            Action::CloseFlyout => self.close_flyout(),
        }
        self.needs_redraw = true;
    }

    fn menu_key(&mut self, k: MenuKey) {
        match k {
            MenuKey::Close => self.toggle_start(),
            MenuKey::Launch => self.activate_start_entry(self.start.selected),
            MenuKey::Up => self.step_selection(-1),
            MenuKey::Down => self.step_selection(1),
            MenuKey::Backspace => {
                self.start.query.pop();
                self.refresh_start();
            }
            MenuKey::Char(c) => {
                self.start.query.push(c);
                self.refresh_start();
            }
            MenuKey::Ignore => {}
        }
    }

    /// Move the highlight up or down, hopping over section headers.
    fn step_selection(&mut self, dir: i32) {
        let n = self.start.results.len() as i32;
        let mut i = self.start.selected as i32 + dir;
        while i >= 0 && i < n && !self.start.results[i as usize].selectable() {
            i += dir;
        }
        if i >= 0 && i < n {
            self.select(i as usize);
        } else if dir < 0 {
            // At the top: show the header above too.
            self.start.scroll = 0;
        }
    }

    /// Run whatever a start menu row stands for.
    fn activate_start_entry(&mut self, i: usize) {
        use crate::search::Entry;
        let Some(entry) = self.start.results.get(i).cloned() else { return };
        let quote = |p: &std::path::Path| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"));
        let exec_of = |s: &Self, id: &str| s.apps.iter().find(|a| a.id == id).map(|a| a.exec.clone());
        match entry {
            Entry::Header(_) => return,
            Entry::App(idx) => {
                let app = self.apps[idx].clone();
                self.launch(&app);
            }
            Entry::Setting(_, page) => {
                if let Some(exec) = exec_of(self, "vide-settings") {
                    self.spawn(&format!("{exec} --page {page}"));
                }
            }
            Entry::File(p) => match (p.is_dir(), exec_of(self, "file-exploder")) {
                (true, Some(fe)) => self.spawn(&format!("{fe} {}", quote(&p))),
                _ => self.spawn(&format!("xdg-open {}", quote(&p))),
            },
            Entry::Calc(v) => {
                let mimes = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"].map(String::from).to_vec();
                smithay::wayland::selection::data_device::set_data_device_selection(
                    &self.dh,
                    &self.seat,
                    mimes,
                    std::sync::Arc::new(v.into_bytes()),
                );
            }
            Entry::Power(a) => return self.power(a),
            Entry::Web(q) => {
                let q: String = q.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_string() } else if c == ' ' { "+".into() } else { format!("%{:02X}", c as u32 & 0xff) }).collect();
                self.spawn(&format!("xdg-open 'https://www.google.com/search?q={q}'"));
            }
        }
        self.toggle_start();
    }

    /// Right-click on a start menu app: pin or unpin it.
    fn open_start_item_menu(&mut self, i: usize, at: Point<i32, Logical>) {
        let Some(crate::search::Entry::App(idx)) = self.start.results.get(i) else { return };
        let id = self.apps[*idx].id.clone();
        let start = if self.pins.start.contains(&id) { "Unpin from Start" } else { "Pin to Start" };
        let bar = if self.pins.taskbar.contains(&id) { "Unpin from taskbar" } else { "Pin to taskbar" };
        let items = vec![
            ("Open".into(), crate::menu::Action::Launch(*idx)),
            (String::new(), crate::menu::Action::Separator),
            (start.into(), crate::menu::Action::PinStart(id.clone())),
            (bar.into(), crate::menu::Action::PinTaskbar(id)),
        ];
        self.open_menu(at, items);
    }

    /// Select row `i` of the filtered list, scrolling it into view.
    fn select(&mut self, i: usize) {
        self.start.selected = i;
        let rows = ui::start_menu_rows(self.start_menu_rect().size.h);
        if i < self.start.scroll {
            self.start.scroll = i;
        } else if i >= self.start.scroll + rows {
            self.start.scroll = i + 1 - rows;
        }
    }

    fn scroll_menu(&mut self, rows: i32) {
        let n = self.start.results.len();
        let visible = ui::start_menu_rows(self.start_menu_rect().size.h);
        let max = n.saturating_sub(visible) as i32;
        self.start.scroll = (self.start.scroll as i32 + rows).clamp(0, max) as usize;
        self.needs_redraw = true;
    }

    // ------------------------------------------------------------ pointer

    pub fn start_menu_rect(&self) -> Rectangle<i32, Logical> {
        let h = (self.output_size.h - TASKBAR_H).min(620);
        let w = if self.start.searching() { ui::SEARCH_PANEL_W.min(self.output_size.w) } else { MENU_W };
        Rectangle::new((0, self.output_size.h - TASKBAR_H - h).into(), (w, h).into())
    }

    /// Right-click on a Start tile, as on Windows: unpin, or pin to the taskbar.
    fn open_tile_menu(&mut self, t: usize, at: Point<i32, Logical>) {
        let Some(&(idx, group)) = self.start_tiles().get(t) else { return };
        let id = self.apps[idx].id.clone();
        let start = if group == 0 { "Unpin from Start" } else { "Pin to Start" };
        let bar = if self.pins.taskbar.contains(&id) { "Unpin from taskbar" } else { "Pin to taskbar" };
        let items = vec![
            ("Open".into(), crate::menu::Action::Launch(idx)),
            (String::new(), crate::menu::Action::Separator),
            (start.into(), crate::menu::Action::PinStart(id.clone())),
            (bar.into(), crate::menu::Action::PinTaskbar(id)),
        ];
        self.open_menu(at, items);
    }

    fn window_for_tray(&self, id: &str) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
        let item = self.sni_items.iter().find(|i| i.id == id)?;
        let needles: Vec<String> = [
            item.proc_name.as_str(),
            item.title.as_str(),
            item.icon_name.as_str(),
            id.split('/').next().unwrap_or(id),
        ]
            .into_iter()
            .filter(|s| !s.is_empty() && !s.starts_with(':'))
            .map(norm)
            .filter(|s| s.len() >= 3)
            .collect();
        self.wm
            .windows
            .iter()
            .rev()
            .find(|w| {
                let app = norm(&w.app_id());
                let title = norm(&w.title());
                needles.iter().any(|n| app == *n || title == *n || (!app.is_empty() && (app.contains(n) || n.contains(&app))))
            })
            .map(|w| w.surface().clone())
    }

    fn rail_click(&mut self, r: ui::RailItem, at: Point<i32, Logical>) {
        use ui::RailItem;
        let home = std::env::var("HOME").unwrap_or_default();
        let folder = |state: &mut Self, sub: &str| {
            let fe = own_app("file-exploder").unwrap_or_else(|| "xdg-open".into());
            state.spawn(&format!("{fe} '{home}/{sub}'"));
            state.toggle_start();
        };
        match r {
            RailItem::Expand => self.start.rail_open = !self.start.rail_open,
            RailItem::Account => {
                let settings = own_app("vide-settings").unwrap_or_default();
                let items = vec![
                    ("Change account settings".into(), crate::menu::Action::Spawn(format!("{settings} --page about"))),
                    ("Sign out".into(), crate::menu::Action::SignOut),
                ];
                self.open_menu(at, items);
            }
            RailItem::Documents => folder(self, "Documents"),
            RailItem::Pictures => folder(self, "Pictures"),
            RailItem::Settings => {
                if let Some(s) = own_app("vide-settings") {
                    self.spawn(&s);
                }
                self.toggle_start();
            }
        }
        self.needs_redraw = true;
    }

    /// A click on an action in the search pane's preview.
    fn start_action(&mut self, id: u8) {
        use crate::search::Entry;
        let i = self.start.selected;
        let Some(entry) = self.start.results.get(i).cloned() else { return };
        match (id, entry) {
            (0, _) => self.activate_start_entry(i),
            (1, Entry::File(p)) => {
                let dir = p.parent().unwrap_or(std::path::Path::new("/")).to_string_lossy().replace('\'', "'\\''");
                let fe = own_app("file-exploder").unwrap_or_else(|| "xdg-open".into());
                self.spawn(&format!("{fe} '{dir}'"));
                self.toggle_start();
            }
            (2, Entry::App(a)) => {
                let id = self.apps[a].id.clone();
                self.pins.toggle_start(&id);
            }
            (3, Entry::App(a)) => {
                let id = self.apps[a].id.clone();
                self.pins.toggle_taskbar(&id);
                self.taskbar = None;
            }
            (4, Entry::File(p)) => {
                let mimes = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"].map(String::from).to_vec();
                let bytes = p.to_string_lossy().into_owned().into_bytes();
                smithay::wayland::selection::data_device::set_data_device_selection(&self.dh, &self.seat, mimes, std::sync::Arc::new(bytes));
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    fn taskbar_visible(&self) -> bool {
        let focused = self.focused();
        self.start.open || !self.wm.top().is_some_and(|w| w.fullscreen.is_some() && Some(w.surface()) == focused.as_ref())
    }

    fn target(&self, pos: Point<f64, Logical>) -> Target {
        if let Some(m) = &self.menu {
            if m.rect().to_f64().contains(pos) {
                return Target::Menu(m.item_at(pos));
            }
        }
        if self.desktops.open && self.desktops_rect().to_f64().contains(pos) {
            return Target::Desktops(self.desktop_at(pos));
        }
        if self.action_center.open {
            let r = self.action_center_rect();
            if r.to_f64().contains(pos) {
                let local = (pos - r.loc.to_f64()).to_i32_round();
                let hit = self
                    .action_center
                    .panel
                    .as_ref()
                    .and_then(|p| p.hits.iter().find(|(hr, _)| hr.contains(local)).map(|(_, h)| h.clone()))
                    .unwrap_or(crate::notifications::ActionCenterHit::Nothing);
                return Target::ActionCenter(hit);
            }
        }
        for x in self.x11_overrides.iter().rev() {
            if let Some(s) = x.wl_surface() {
                if let Some((hit, o)) = under_from_surface_tree(&s, pos, x.geometry().loc, WindowSurfaceType::ALL) {
                    return Target::Window(Hit::Client(hit, o.to_f64()));
                }
            }
        }
        if self.start.open {
            let r = self.start_menu_rect();
            if r.to_f64().contains(pos) {
                let local = (pos - r.loc.to_f64()).to_i32_round();
                let hit = self
                    .start
                    .menu
                    .as_ref()
                    .and_then(|m| m.hits.iter().find(|(r, _)| r.contains(local)).map(|(_, h)| h.clone()))
                    .unwrap_or(StartHit::Nothing);
                return Target::Start(hit);
            }
        }
        let bar_y = self.output_size.h - TASKBAR_H;
        if self.taskbar_visible() && pos.y >= bar_y as f64 {
            let local = Point::<i32, Logical>::from((pos.x as i32, pos.y as i32 - bar_y));
            let hit = self
                .taskbar
                .as_ref()
                .and_then(|t| t.hits.iter().find(|(r, _)| r.contains(local)).map(|(_, h)| h.clone()))
                .unwrap_or(TaskbarHit::Nothing);
            return Target::Taskbar(hit);
        }
        // Layer clients: panels/overlays above windows, the desktop program below them.
        if let Some((s, o)) = self.layer_hit(&[Layer::Overlay, Layer::Top], pos) {
            return Target::Window(Hit::Client(s, o));
        }
        if let Some(h) = self.wm.hit(pos) {
            return Target::Window(h);
        }
        match self.layer_hit(&[Layer::Bottom, Layer::Background], pos) {
            Some((s, o)) => Target::Window(Hit::Client(s, o)),
            None => Target::Desktop,
        }
    }

    fn pointer_to(&mut self, pos: Point<f64, Logical>, time: u32) {
        self.pointer_loc = pos;
        self.needs_redraw = true;

        // Snipping: the screen is ours until a region is picked.
        if self.snip.is_some() {
            let ptr = self.pointer.clone();
            ptr.motion(self, None, &MotionEvent { location: pos, serial: SERIAL_COUNTER.next_serial(), time });
            ptr.frame(self);
            return;
        }

        if self.tray.dragging {
            self.flyout_drag(pos);
            return;
        }
        self.task_drag_motion(pos.x);

        let mut focus = None;
        if self.wm.grab.is_some() {
            let area = self.work_area();
            self.wm.update_grab(pos, 0, area);
        } else {
            let mut cursor = CursorIcon::Default;
            let target = self.target(pos);
            let menu_hover = match target {
                Target::Menu(i) => i,
                _ => None,
            };
            if let Some(m) = self.menu.as_mut() {
                m.hover = menu_hover;
            }
            self.desktops.hover = match target {
                Target::Desktops(d) => d,
                _ => None,
            };
            let ac_hover = match &target {
                Target::ActionCenter(h) => h.clone(),
                _ => crate::notifications::ActionCenterHit::Nothing,
            };
            let fly_hover = if self.flyout_rect().to_f64().contains(pos) { self.flyout_action(pos) } else { None };
            if self.tray.hover != fly_hover {
                self.tray.hover = fly_hover;
            }
            if self.action_center.hover != ac_hover {
                self.action_center.hover = ac_hover;
                self.action_center.panel = None;
            }
            self.hover = match target {
                Target::Menu(_) | Target::Desktops(_) | Target::ActionCenter(_) => Hover::None,
                Target::Start(h) => Hover::Start(h),
                Target::Taskbar(h) => Hover::Taskbar(h),
                Target::Window(Hit::Client(s, origin)) => {
                    focus = Some((s, origin));
                    Hover::None
                }
                Target::Window(Hit::Button(s, b)) => Hover::Title(s, b),
                Target::Window(Hit::Edge(_, e)) => {
                    cursor = resize_cursor(e);
                    Hover::None
                }
                Target::Window(Hit::Titlebar(_)) | Target::Desktop => Hover::None,
            };
            // Over our own chrome we choose the cursor; over a client, the client does.
            if focus.is_none() {
                self.cursor_status = CursorImageStatus::Named(cursor);
            }
        }

        // Pointer constraints follow focus: deactivate on the old surface, activate on the new.
        let ptr = self.pointer.clone();
        let old = self.pointer_focus.as_ref().map(|(s, _)| s.clone());
        let new = focus.as_ref().map(|(s, _)| s.clone());
        if old != new {
            if let Some(s) = &old {
                with_pointer_constraint(s, &ptr, |c| {
                    if let Some(c) = c.filter(|c| c.is_active()) {
                        c.deactivate();
                    }
                });
            }
            if let Some(s) = &new {
                with_pointer_constraint(s, &ptr, |c| {
                    if let Some(c) = c.filter(|c| !c.is_active()) {
                        c.activate();
                    }
                });
            }
        }
        self.pointer_focus = focus.clone();

        ptr.motion(self, focus, &MotionEvent { location: pos, serial: SERIAL_COUNTER.next_serial(), time });
        ptr.frame(self);
    }

    fn on_button(&mut self, button: u32, state: ButtonState, time: u32) {
        let serial = SERIAL_COUNTER.next_serial();
        let forward = |s: &mut Self| {
            let ptr = s.pointer.clone();
            ptr.button(s, &ButtonEvent { button, state: state.into(), serial, time });
            ptr.frame(s);
        };

        if let Some(snip) = self.snip.as_mut() {
            match (button, state) {
                (BTN_LEFT, ButtonState::Pressed) => snip.start = Some(self.pointer_loc),
                (BTN_LEFT, ButtonState::Released) => {
                    let sel = snip.selection(self.pointer_loc).filter(|r| r.size.w > 2 && r.size.h > 2);
                    self.cancel_snip();
                    if let Some(r) = sel {
                        self.screenshot_requested = Some(Some(r));
                    }
                }
                (_, ButtonState::Pressed) => self.cancel_snip(),
                _ => {}
            }
            self.needs_redraw = true;
            return;
        }

        if state == ButtonState::Released && button == BTN_LEFT && self.task_drag.is_some() {
            self.task_drag_release();
            self.needs_redraw = true;
            return;
        }

        if state == ButtonState::Released && button == BTN_LEFT && self.tray.dragging {
            self.tray.dragging = false;
            return;
        }

        if state == ButtonState::Released {
            if self.wm.grab.is_some() && button == BTN_LEFT {
                let area = self.work_area();
                self.wm.end_grab(area);
            }
            return forward(self);
        }

        // A client grab (open popup menu, or a held button) owns the pointer.
        if self.pointer.is_grabbed() {
            return forward(self);
        }

        let pos = self.pointer_loc;
        let left = button == BTN_LEFT;
        let right = button == BTN_RIGHT;
        let target = self.target(pos);
        self.needs_redraw = true;

        // Tray flyout: clicks on it drive the slider; clicks elsewhere close it
        // (except on the tray buttons themselves, which toggle).
        if self.tray.flyout.is_some() {
            if self.flyout_rect().to_f64().contains(pos) {
                if left {
                    self.flyout_press(pos);
                }
                return;
            }
            if !matches!(
                target,
                Target::Taskbar(
                    TaskbarHit::Volume | TaskbarHit::Brightness | TaskbarHit::Network
                )
            ) {
                self.close_flyout();
            }
        }

        // Task View panel: a click on a tile switches desktop; anywhere else closes it
        // (the Task View button itself toggles it below).
        if self.desktops.open {
            if let Target::Desktops(tile) = target {
                if let (true, Some(ws)) = (left, tile) {
                    self.switch_desktop(ws);
                }
                return;
            }
            if !matches!(target, Target::Taskbar(TaskbarHit::TaskView)) {
                self.close_desktops();
            }
        }

        // Action Center: clicks inside interact with notifications; clicks outside close it.
        if self.action_center.open {
            if let Target::ActionCenter(hit) = &target {
                if left {
                    self.action_center_click(hit.clone());
                }
                return;
            }
            if !matches!(target, Target::Taskbar(TaskbarHit::Clock)) {
                self.close_action_center();
            }
        }

        // An open right-click menu eats the next click: run the item, or just close it.
        if self.menu.is_some() {
            if let Target::Menu(item) = target {
                if let Some(i) = item {
                    let action = self.menu.as_ref().unwrap().items[i].1.clone();
                    self.run_menu_action(action);
                }
                return;
            }
            self.close_menu();
            return;
        }

        // Any click outside the start menu (except its own buttons) closes it.
        if self.start.open && !matches!(target, Target::Start(_) | Target::Taskbar(TaskbarHit::Start | TaskbarHit::Search)) {
            self.toggle_start();
            if !matches!(target, Target::Taskbar(_)) {
                return;
            }
        }

        match target {
            Target::Start(hit) => {
                if right {
                    match hit {
                        StartHit::Item(i) => self.open_start_item_menu(i, pos.to_i32_round()),
                        StartHit::Tile(t) => self.open_tile_menu(t, pos.to_i32_round()),
                        _ => {}
                    }
                } else if left {
                    match hit {
                        StartHit::Item(i) => self.activate_start_entry(i),
                        StartHit::Filter(f) => {
                            self.start.filter = f;
                            self.refresh_start();
                        }
                        StartHit::Action(a) => self.start_action(a),
                        StartHit::Tile(t) => {
                            if let Some(&(i, _)) = self.start_tiles().get(t) {
                                let app = self.apps[i].clone();
                                self.launch(&app);
                                self.toggle_start();
                            }
                        }
                        StartHit::Rail(r) => self.rail_click(r, pos.to_i32_round()),
                        StartHit::Power => self.start.power_open = !self.start.power_open,
                        StartHit::PowerAction(a) => self.power(a),
                        StartHit::Nothing => self.start.power_open = false,
                    }
                }
            }
            Target::Taskbar(hit) => {
                if left {
                    match hit {
                        TaskbarHit::Start => self.toggle_start(),
                        TaskbarHit::Search => {
                            if !self.start.open {
                                self.toggle_start();
                                self.start.search_mode = true;
                                self.refresh_start();
                            }
                        }
                        TaskbarHit::Button(i) => self.press_task_button(i, pos.x),
                        TaskbarHit::TaskView => self.toggle_desktops(),
                        TaskbarHit::ShowDesktop => self.show_desktop(),
                        TaskbarHit::Volume => self.toggle_flyout(crate::tray::Flyout::Volume),
                        TaskbarHit::Brightness => self.toggle_flyout(crate::tray::Flyout::Brightness),
                        TaskbarHit::Network => self.toggle_flyout(crate::tray::Flyout::Network),
                        TaskbarHit::Clock => self.toggle_action_center(),
                        TaskbarHit::TrayApp(id) => {
                            self.tray_menu_x = pos.x as i32;
                            if let Some(surface) = self.window_for_tray(&id) {
                                self.focus(Some(surface), SERIAL_COUNTER.next_serial());
                            }
                            if let Some(tx) = &self.sni_tx {
                                let _ = tx.send(crate::sni::Cmd::Activate { id, x: pos.x as i32, y: pos.y as i32 });
                            }
                        }
                        TaskbarHit::Nothing => {}
                    }
                } else if right {
                    match hit {
                        TaskbarHit::Button(i) => self.open_task_button_menu(i, pos.x as i32),
                        TaskbarHit::TrayApp(id) => {
                            self.tray_menu_x = pos.x as i32;
                            if let Some(tx) = &self.sni_tx {
                                let _ = tx.send(crate::sni::Cmd::OpenMenu { id, x: pos.x as i32, y: pos.y as i32 });
                            }
                        }
                        _ => {}
                    }
                }
            }
            Target::Window(Hit::Button(s, b)) => {
                if left {
                    match b {
                        TitleButton::Minimize => self.minimize(&s),
                        TitleButton::Maximize => {
                            let area = self.work_area();
                            self.wm.toggle_maximize(&s, area);
                            self.focus(Some(s), serial);
                        }
                        TitleButton::Close => {
                            if let Some(w) = self.wm.find(&s) {
                                w.close();
                            }
                        }
                    }
                }
            }
            Target::Window(Hit::Titlebar(s)) => {
                self.focus(Some(s.clone()), serial);
                if right {
                    self.open_window_menu(&s, pos.to_i32_round());
                } else if left {
                    let now = Instant::now();
                    let double = matches!(&self.last_title_click, Some((ls, t)) if *ls == s && now - *t < DOUBLE_CLICK);
                    if double {
                        self.last_title_click = None;
                        let area = self.work_area();
                        self.wm.toggle_maximize(&s, area);
                    } else {
                        self.last_title_click = Some((s.clone(), now));
                        self.begin_move(&s);
                    }
                }
            }
            Target::Window(Hit::Edge(s, e)) => {
                self.focus(Some(s.clone()), serial);
                if left {
                    self.wm.start_resize(&s, e, pos);
                }
            }
            Target::Window(Hit::Client(s, _)) => {
                let root = self.root_of(&s);
                self.focus(Some(root.clone()), serial);
                let mods = self.keyboard.modifier_state();
                if left && if self.tty { mods.logo } else { mods.alt } {
                    // Mod+drag moves from anywhere; the client never sees this click.
                    self.begin_move(&root);
                    return;
                }
                forward(self);
            }
            Target::Desktop => {
                if right {
                    self.open_desktop_menu(pos.to_i32_round());
                }
                forward(self);
            }
            Target::Menu(_) | Target::Desktops(_) | Target::ActionCenter(_) => {}
        }
    }

    pub fn power(&mut self, a: PowerAction) {
        match a {
            PowerAction::SignOut => self.running = false,
            PowerAction::Restart => self.spawn("systemctl reboot"),
            PowerAction::ShutDown => self.spawn("systemctl poweroff"),
        }
        self.toggle_start();
    }

    /// The toplevel surface that `s` (a subsurface or popup) belongs to.
    fn root_of(&self, s: &WlSurface) -> WlSurface {
        let mut root = s.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        if let Some(popup) = self.popups.find_popup(&root) {
            if let Ok(r) = find_popup_root_surface(&popup) {
                return r;
            }
        }
        root
    }
}

fn resize_cursor(e: u32) -> CursorIcon {
    match e {
        x if x == edge::TOP | edge::LEFT => CursorIcon::NwResize,
        x if x == edge::TOP | edge::RIGHT => CursorIcon::NeResize,
        x if x == edge::BOTTOM | edge::LEFT => CursorIcon::SwResize,
        x if x == edge::BOTTOM | edge::RIGHT => CursorIcon::SeResize,
        edge::TOP => CursorIcon::NResize,
        edge::BOTTOM => CursorIcon::SResize,
        edge::LEFT => CursorIcon::WResize,
        edge::RIGHT => CursorIcon::EResize,
        _ => CursorIcon::Default,
    }
}
