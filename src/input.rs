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
    ui::{self, PowerAction, StartHit, TaskbarHit, TitleButton, MENU_W, TASKBAR_H},
    wm::{edge, Hit},
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
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
    WindowMenu,
    CloseMenu,
    CancelSnip,
    Volume(f32),
    Mute,
    Brightness(f32),
    Screenshot { region: bool },
    Quit,
    Vt(i32),
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
    if mods.alt && !mods.ctrl && sym == Keysym::F4 {
        return Some(Action::Close);
    }
    if super_mod && mods.alt && !mods.logo && sym == Keysym::space {
        return Some(Action::WindowMenu);
    }
    if sym == Keysym::Print {
        return Some(Action::Screenshot { region: mods.shift });
    }
    if mods.logo && mods.shift && sym == Keysym::s {
        return Some(Action::Screenshot { region: true });
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
                let over_volume = matches!(self.target(self.pointer_loc), Target::Taskbar(TaskbarHit::Volume))
                    || (self.tray.flyout == Some(crate::tray::Flyout::Volume) && self.flyout_rect().to_f64().contains(self.pointer_loc));
                if over_volume {
                    let v = event.amount(Axis::Vertical).unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) / 8.0);
                    if v != 0.0 {
                        self.set_volume(self.tray.volume - v.signum() as f32 * 0.05);
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
                return FilterResult::Intercept(if pressed { Action::CancelSnip } else { Action::CloseMenu });
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
            Action::WindowMenu => {
                if let Some(s) = focused {
                    let at = self.wm.find(&s).map(|w| w.frame().loc).unwrap_or_default();
                    self.open_window_menu(&s, at);
                }
            }
            Action::CloseMenu => self.close_menu(),
            Action::CancelSnip => self.cancel_snip(),
            Action::Volume(d) => self.set_volume(self.tray.volume + d),
            Action::Mute => self.toggle_mute(),
            Action::Brightness(d) => self.set_brightness(self.tray.brightness + d),
            Action::Screenshot { region } => self.start_screenshot(region),
            Action::Quit => self.running = false,
            Action::Vt(n) => self.pending_vt = Some(n),
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
        self.start.open || !self.wm.top().is_some_and(|w| w.fullscreen.is_some() && Some(w.surface()) == focused.as_ref())
    }

    fn target(&self, pos: Point<f64, Logical>) -> Target {
        if let Some(m) = &self.menu {
            if m.rect().to_f64().contains(pos) {
                return Target::Menu(m.item_at(pos));
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
        match self.wm.hit(pos) {
            Some(h) => Target::Window(h),
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

        let mut focus = None;
        if self.wm.grab.is_some() {
            self.wm.update_grab(pos, 0);
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
            self.hover = match target {
                Target::Menu(_) => Hover::None,
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

        if state == ButtonState::Released && button == BTN_LEFT && self.tray.dragging {
            self.tray.dragging = false;
            return;
        }

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
            if !matches!(target, Target::Taskbar(TaskbarHit::Volume | TaskbarHit::Brightness)) {
                self.close_flyout();
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
                if left {
                    match hit {
                        StartHit::Item(i) => {
                            if let Some(&idx) = self.filtered_apps().get(i) {
                                let app = self.apps[idx].clone();
                                self.launch(&app);
                                self.toggle_start();
                            }
                        }
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
                            }
                        }
                        TaskbarHit::Window(s) => self.activate_from_taskbar(&s),
                        TaskbarHit::ShowDesktop => self.show_desktop(),
                        TaskbarHit::Volume => self.toggle_flyout(crate::tray::Flyout::Volume),
                        TaskbarHit::Brightness => self.toggle_flyout(crate::tray::Flyout::Brightness),
                        TaskbarHit::Nothing => {}
                    }
                } else if let (true, TaskbarHit::Window(s)) = (right, hit) {
                    self.open_taskbar_menu(&s, pos.x as i32);
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
            Target::Menu(_) => {}
        }
    }

    fn power(&mut self, a: PowerAction) {
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
