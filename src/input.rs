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
        pointer::{AxisFrame, ButtonEvent, CursorIcon, CursorImageStatus, MotionEvent},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
    wayland::compositor::get_parent,
};

use crate::{
    state::{Hover, State},
    ui::{self, StartHit, TaskbarHit, TitleButton, MENU_W, TASKBAR_H},
    wm::{edge, Hit},
};

const BTN_LEFT: u32 = 0x110;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

pub const TERMINAL: &str = "konsole";

enum Action {
    Spawn(&'static str),
    Close,
    Cycle,
    Maximize,
    Minimize,
    Fullscreen,
    StartMenu,
    Quit,
    Vt(i32),
    Workspace(usize),
    WorkspaceNext,
    WorkspacePrev,
    MoveToWorkspace(usize),
    /// Swallowed by the start menu.
    Menu(MenuKey),
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
    if !(if super_mod { mods.logo } else { mods.alt }) {
        return None;
    }
    match sym {
        Keysym::Return => Some(Action::Spawn(TERMINAL)),
        Keysym::q => Some(Action::Close),
        Keysym::Tab => Some(Action::Cycle),
        Keysym::Up => Some(Action::Maximize),
        Keysym::Down => Some(Action::Minimize),
        Keysym::f => Some(Action::Fullscreen),
        Keysym::space => Some(Action::StartMenu),
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
                let size = self.output_size.to_f64();
                let mut pos = self.pointer_loc + event.delta();
                pos.x = pos.x.clamp(0.0, size.w - 1.0);
                pos.y = pos.y.clamp(0.0, size.h - 1.0);
                self.pointer_to(pos, event.time_msec());
            }

            InputEvent::PointerButton { event } => {
                self.on_button(event.button_code(), event.state(), event.time_msec());
            }

            InputEvent::PointerAxis { event } => {
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
            Action::Spawn(cmd) => self.spawn(cmd),
            Action::Close => {
                if let Some(w) = focused.and_then(|s| self.wm.find(&s)) {
                    w.close();
                }
            }
            Action::Cycle => {
                let s = self.wm.cycle();
                self.focus(s, SERIAL_COUNTER.next_serial());
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
                }
            }
            Action::Menu(k) => self.menu_key(k),
        }
        self.needs_redraw = true;
    }

    fn menu_key(&mut self, k: MenuKey) {
        let n = self.filtered_apps().len();
        match k {
            MenuKey::Close => self.toggle_start(),
            MenuKey::Launch => {
                if let Some(&i) = self.filtered_apps().get(self.start.selected) {
                    let app = self.apps[i].clone();
                    self.launch(&app);
                    self.toggle_start();
                }
            }
            MenuKey::Up => self.select(self.start.selected.saturating_sub(1)),
            MenuKey::Down => self.select((self.start.selected + 1).min(n.saturating_sub(1))),
            MenuKey::Backspace => {
                self.start.query.pop();
                self.select(0);
            }
            MenuKey::Char(c) => {
                self.start.query.push(c);
                self.select(0);
            }
            MenuKey::Ignore => {}
        }
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
        let n = self.filtered_apps().len();
        let visible = ui::start_menu_rows(self.start_menu_rect().size.h);
        let max = n.saturating_sub(visible) as i32;
        self.start.scroll = (self.start.scroll as i32 + rows).clamp(0, max) as usize;
        self.needs_redraw = true;
    }

    // ------------------------------------------------------------ pointer

    pub fn start_menu_rect(&self) -> Rectangle<i32, Logical> {
        let h = (self.output_size.h - TASKBAR_H).min(620);
        Rectangle::new((0, self.output_size.h - TASKBAR_H - h).into(), (MENU_W, h).into())
    }

    fn taskbar_visible(&self) -> bool {
        let focused = self.focused();
        !self.wm.top().is_some_and(|w| w.fullscreen.is_some() && Some(w.surface()) == focused.as_ref())
    }

    fn target(&self, pos: Point<f64, Logical>) -> Target {
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
        match self.wm.hit(pos) {
            Some(h) => Target::Window(h),
            None => Target::Desktop,
        }
    }

    fn pointer_to(&mut self, pos: Point<f64, Logical>, time: u32) {
        self.pointer_loc = pos;
        self.needs_redraw = true;

        let mut focus = None;
        if self.wm.grab.is_some() {
            self.wm.update_grab(pos, 0);
        } else {
            let mut cursor = CursorIcon::Default;
            self.hover = match self.target(pos) {
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

        let ptr = self.pointer.clone();
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

        if state == ButtonState::Released {
            if self.wm.grab.is_some() && button == BTN_LEFT {
                self.wm.end_grab();
            }
            return forward(self);
        }

        // A client grab (open popup menu, or a held button) owns the pointer.
        if self.pointer.is_grabbed() {
            return forward(self);
        }

        let pos = self.pointer_loc;
        let left = button == BTN_LEFT;
        let target = self.target(pos);
        self.needs_redraw = true;

        // Any click outside the start menu (except its own buttons) closes it.
        if self.start.open && !matches!(target, Target::Start(_) | Target::Taskbar(TaskbarHit::Start | TaskbarHit::Search)) {
            self.toggle_start();
            if !matches!(target, Target::Taskbar(_)) {
                return;
            }
        }

        match target {
            Target::Start(hit) => {
                if left {
                    match hit {
                        StartHit::Item(i) => {
                            if let Some(&idx) = self.filtered_apps().get(i) {
                                let app = self.apps[idx].clone();
                                self.launch(&app);
                                self.toggle_start();
                            }
                        }
                        StartHit::Power => self.running = false,
                        StartHit::Nothing => {}
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
                            }
                        }
                        TaskbarHit::Window(s) => self.activate_from_taskbar(&s),
                        TaskbarHit::Workspace(ws) => {
                            self.wm.set_workspace(ws);
                            self.focus_top();
                        }
                        TaskbarHit::ShowDesktop => self.show_desktop(),
                        TaskbarHit::Nothing => {}
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
                if left {
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
            Target::Desktop => forward(self),
        }
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
